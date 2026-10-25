/**
 * E2E mock for the stock commands (`crm_*_stock*`, `crm_*_tech_card*`).
 *
 * Mirrors the relay contract for kinds 42106–42108: moving-average levels per
 * warehouse and product (value 0 whenever qty <= 0), receipts, write-offs,
 * inventory counts and transfers posted once with per-space numbers, voids
 * that append reversal moves newest first, goods sales that need stock unless
 * `allow_negative_stock` is on, and tech-card consumables that may go
 * negative. Seeded levels are opening balances without documents.
 */

import type { CrmCapability } from "@/shared/api/tauriCrm";
import type { RawCrmProduct } from "@/shared/api/tauriCrmProducts";

import { mockCrmProductById, mockCrmProducts } from "./e2eBridgeCrmProducts.ts";
import { mockCrmPartyById, mockCrmSaleById } from "./e2eBridgeCrmSales.ts";

export type MockCrmStockSeed = {
  productId: string;
  qtyMilli: number;
  /** Cost of one unit; defaults to half the product price. */
  costMinor?: number;
  /** Defaults to the first seeded warehouse. */
  warehouseId?: string;
};

export type MockCrmTechCardSeed = {
  serviceId: string;
  items: Array<{ productId: string; qtyMilli: number }>;
};

type MockStockAccess = {
  channelId: string | null;
  warehouses: Array<{ id: string; name: string; archived: boolean }>;
  allowNegative: boolean;
  requireCapability: (capability: CrmCapability) => void;
};

type DocType = "receipt" | "write_off" | "inventory" | "transfer";
type Source = DocType | "sale" | "consumable";
type Level = { qty: number; value: number };
type Delta = Level;

type MockMove = {
  id: number;
  warehouse_id: string;
  product_id: string;
  source: Source;
  doc_id: string;
  reversal: boolean;
  qty_delta_milli: number;
  value_delta_minor: number;
  qty_after_milli: number;
  moved_at: string;
};

type MockDocLine = {
  product_id: string;
  product_name: string;
  unit: string;
  qty_milli: number;
  book_qty_milli: number | null;
  cost_minor: number | null;
  amount_minor: number;
};

type MockDoc = {
  id: string;
  number: number;
  type: DocType;
  warehouse_id: string;
  to_warehouse_id: string | null;
  supplier_id: string | null;
  reference: string | null;
  note: string | null;
  total_minor: number;
  posted_at: string;
  void: { voided_at: string; reason: string } | null;
  lines: MockDocLine[];
};

type MockCard = {
  items: Array<{ product_id: string; qty_milli: number }>;
  revision: string;
  updated_at: string;
};

const PAGE = 50;
const QTY_SCALE = 1000;

let levels = new Map<string, Level>();
let moves: MockMove[] = [];
let docs: MockDoc[] = [];
let cards = new Map<string, MockCard>();

function eventId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

const key = (warehouseId: string, productId: string) =>
  `${warehouseId}:${productId}`;

function invalid(message: string): never {
  throw `relay error 400: invalid: ${message}`;
}

function conflict(message: string): never {
  throw `relay error 409: conflict: ${message}`;
}

function isStocked(product: RawCrmProduct): boolean {
  return (
    product.payment_object === "commodity" ||
    product.payment_object === "excise"
  );
}

/** `buzz_crm::Unit::check_qty`: pieces and service units are whole. */
function checkUnit(product: RawCrmProduct, qty: number, field: string) {
  const whole = product.unit === "piece" || product.unit === "service_unit";
  if (whole && qty % QTY_SCALE !== 0) {
    invalid(
      `${field}: ${product.unit} is sold in whole units (${product.name})`,
    );
  }
}

/** `buzz_crm::line_amount`: half-up rounding of price × qty / 1000. */
function lineAmount(price: number, qty: number): number {
  return Math.round((price * qty) / QTY_SCALE);
}

// ── StockLevel arithmetic (`buzz_crm::stock::StockLevel`) ────────────────

function deltaTo(level: Level, qty: number, value: number): Delta {
  const kept = qty <= 0 ? 0 : Math.max(value, 0);
  return { qty: qty - level.qty, value: kept - level.value };
}

function receive(level: Level, qty: number, value: number): Delta {
  const after = level.qty + qty;
  const kept =
    level.qty >= 0
      ? level.value + value
      : after > 0
        ? Math.trunc((value * after) / Math.max(qty, 1))
        : 0;
  return deltaTo(level, after, kept);
}

/** `null` when the level may not go negative and would. */
function issue(
  level: Level,
  qty: number,
  allowNegative: boolean,
): Delta | null {
  const after = level.qty - qty;
  if (after < 0 && !allowNegative) return null;
  const cost =
    after <= 0 ? level.value : Math.round((level.value * qty) / level.qty);
  return deltaTo(level, after, level.value - cost);
}

function averageCost(level: Level): number | null {
  return level.qty <= 0
    ? null
    : Math.round((level.value * QTY_SCALE) / level.qty);
}

function count(level: Level, counted: number, unitCost: number | null): Delta {
  const diff = counted - level.qty;
  if (diff === 0) return { qty: 0, value: 0 };
  if (diff < 0) return issue(level, -diff, true) as Delta;
  const cost = unitCost ?? averageCost(level) ?? 0;
  return receive(level, diff, lineAmount(cost, diff));
}

/**
 * Moves applied to a working copy and committed together, so a rejected
 * line leaves no partial effect (the relay's transaction).
 */
class Ledger {
  private work = new Map<string, Level>();
  private pending: Omit<MockMove, "id" | "moved_at">[] = [];

  level(warehouseId: string, productId: string): Level {
    const k = key(warehouseId, productId);
    return this.work.get(k) ?? levels.get(k) ?? { qty: 0, value: 0 };
  }

  push(
    warehouseId: string,
    productId: string,
    source: Source,
    docId: string,
    reversal: boolean,
    delta: Delta,
  ) {
    const level = this.level(warehouseId, productId);
    const after = {
      qty: level.qty + delta.qty,
      value: level.value + delta.value,
    };
    this.work.set(key(warehouseId, productId), after);
    this.pending.push({
      warehouse_id: warehouseId,
      product_id: productId,
      source,
      doc_id: docId,
      reversal,
      qty_delta_milli: delta.qty,
      value_delta_minor: delta.value,
      qty_after_milli: after.qty,
    });
  }

  commit() {
    for (const [k, level] of this.work) levels.set(k, level);
    const movedAt = new Date().toISOString();
    for (const move of this.pending) {
      moves.push({ ...move, id: moves.length + 1, moved_at: movedAt });
    }
  }
}

/** Undo every move of `docId`, newest first; `null` names a short product. */
function revertMoves(
  ledger: Ledger,
  docId: string,
  allowNegative: boolean,
): string | null {
  const own = moves.filter((move) => move.doc_id === docId && !move.reversal);
  for (const move of [...own].reverse()) {
    const level = ledger.level(move.warehouse_id, move.product_id);
    const after = level.qty - move.qty_delta_milli;
    if (after < 0 && after < level.qty && !allowNegative) {
      return mockCrmProductById(move.product_id)?.name ?? move.product_id;
    }
    ledger.push(
      move.warehouse_id,
      move.product_id,
      move.source,
      docId,
      true,
      deltaTo(level, after, level.value - move.value_delta_minor),
    );
  }
  return null;
}

export function seedMockCrmStock(
  seeds: MockCrmStockSeed[] = [],
  cardSeeds: MockCrmTechCardSeed[] = [],
  defaultWarehouseId: string | null = null,
): void {
  levels = new Map();
  moves = [];
  docs = [];
  cards = new Map();
  for (const seed of seeds) {
    const warehouseId = seed.warehouseId ?? defaultWarehouseId;
    const product = mockCrmProductById(seed.productId);
    if (!warehouseId || !product) continue;
    const cost = seed.costMinor ?? Math.round(product.price_minor / 2);
    levels.set(key(warehouseId, seed.productId), {
      qty: seed.qtyMilli,
      value: seed.qtyMilli > 0 ? lineAmount(cost, seed.qtyMilli) : 0,
    });
  }
  const now = new Date().toISOString();
  for (const card of cardSeeds) {
    cards.set(card.serviceId, {
      items: card.items.map((item) => ({
        product_id: item.productId,
        qty_milli: item.qtyMilli,
      })),
      revision: eventId(),
      updated_at: now,
    });
  }
}

/** Opening balance when a spec seeds no stock: 100 of each stocked product. */
export function defaultMockCrmStock(): MockCrmStockSeed[] {
  return mockCrmProducts()
    .filter(isStocked)
    .map((product) => ({ productId: product.id, qtyMilli: 100 * QTY_SCALE }));
}

/**
 * Issue a sale's goods and its services' consumables. Throws the relay's
 * rejection, with nothing applied, when goods are short.
 */
export function postMockSaleStock(
  saleId: string,
  warehouseId: string,
  lines: ReadonlyArray<{ product_id: string; qty_milli: number }>,
  allowNegative: boolean,
): void {
  const ledger = new Ledger();
  for (const line of lines) {
    const product = mockCrmProductById(line.product_id);
    if (!product) continue;
    if (isStocked(product)) {
      const level = ledger.level(warehouseId, line.product_id);
      const delta = issue(level, line.qty_milli, allowNegative);
      if (!delta) {
        invalid(
          `lines: not enough stock of "${product.name}": ${level.qty} on hand, ${line.qty_milli} requested`,
        );
      }
      ledger.push(warehouseId, line.product_id, "sale", saleId, false, delta);
      continue;
    }
    for (const item of cards.get(line.product_id)?.items ?? []) {
      const qty = Math.round((item.qty_milli * line.qty_milli) / QTY_SCALE);
      if (qty <= 0) continue;
      const level = ledger.level(warehouseId, item.product_id);
      const delta = issue(level, qty, true) as Delta;
      ledger.push(
        warehouseId,
        item.product_id,
        "consumable",
        saleId,
        false,
        delta,
      );
    }
  }
  ledger.commit();
}

/** Put a voided sale's goods and consumables back. */
export function voidMockSaleStock(saleId: string): void {
  const ledger = new Ledger();
  revertMoves(ledger, saleId, true);
  ledger.commit();
}

function requireMember(access: MockStockAccess): string {
  if (!access.channelId) throw "relay error 403: not a member of the CRM space";
  return access.channelId;
}

function warehouseName(access: MockStockAccess, id: string | null) {
  if (id === null) return null;
  return access.warehouses.find((w) => w.id === id)?.name ?? "";
}

function liveWarehouse(access: MockStockAccess, id: string, field: string) {
  const warehouse = access.warehouses.find((w) => w.id === id);
  if (!warehouse) invalid(`${field}: not a warehouse of this CRM space`);
  if (warehouse.archived) invalid(`${field}: the warehouse is archived`);
}

function docView(doc: MockDoc, access: MockStockAccess, withLines: boolean) {
  const { lines, ...rest } = doc;
  return {
    ...rest,
    warehouse_name: warehouseName(access, doc.warehouse_id) ?? "",
    to_warehouse_name: warehouseName(access, doc.to_warehouse_id),
    supplier_name: doc.supplier_id
      ? (mockCrmPartyById(doc.supplier_id)?.name ?? null)
      : null,
    line_count: lines.length,
    ...(withLines ? { lines } : {}),
  };
}

// biome-ignore lint/suspicious/noExplicitAny: mock payloads are untyped IPC.
function listStock(p: any, access: MockStockAccess) {
  requireMember(access);
  const warehouseId: string | null = p.warehouseId ?? null;
  const q = String(p.query ?? "")
    .trim()
    .toLowerCase();
  const items = mockCrmProducts()
    .map((product) => {
      let qty = 0;
      let value = 0;
      for (const [k, level] of levels) {
        const [wh, pid] = k.split(":");
        if (pid !== product.id || (warehouseId && wh !== warehouseId)) continue;
        qty += level.qty;
        value += level.value;
      }
      return { product, qty, value };
    })
    .filter(
      ({ product, qty }) =>
        (isStocked(product) && !product.archived) || qty !== 0,
    )
    .filter(
      ({ product }) =>
        q === "" ||
        product.name.toLowerCase().includes(q) ||
        (product.sku ?? "").toLowerCase().startsWith(q),
    )
    .filter(
      ({ product, qty }) =>
        !p.low ||
        (product.min_stock_milli > 0 && qty <= product.min_stock_milli),
    )
    .sort((a, b) =>
      a.product.name.toLowerCase().localeCompare(b.product.name.toLowerCase()),
    )
    .map(({ product, qty, value }) => ({
      product_id: product.id,
      name: product.name,
      sku: product.sku,
      unit: product.unit,
      category: product.category,
      price_minor: product.price_minor,
      min_stock_milli: product.min_stock_milli,
      qty_milli: qty,
      value_minor: value,
      archived: product.archived,
    }));
  return {
    warehouse_id: warehouseId,
    items,
    totals: {
      value_minor: items.reduce((sum, item) => sum + item.value_minor, 0),
      low_count: items.filter(
        (item) =>
          item.min_stock_milli > 0 && item.qty_milli <= item.min_stock_milli,
      ).length,
    },
  };
}

// biome-ignore lint/suspicious/noExplicitAny: mock payloads are untyped IPC.
function listMoves(p: any, access: MockStockAccess) {
  requireMember(access);
  const rows = moves
    .filter((move) => !p.warehouseId || move.warehouse_id === p.warehouseId)
    .filter((move) => !p.productId || move.product_id === p.productId)
    .filter((move) => p.before == null || move.id < p.before)
    .sort((a, b) => b.id - a.id);
  const page = rows.slice(0, PAGE);
  return {
    moves: page.map((move) => {
      const product = mockCrmProductById(move.product_id);
      const sale = mockCrmSaleById(move.doc_id);
      return {
        ...move,
        warehouse_name: warehouseName(access, move.warehouse_id) ?? "",
        product_name: product?.name ?? "",
        unit: product?.unit ?? "piece",
        doc_number:
          docs.find((doc) => doc.id === move.doc_id)?.number ??
          sale?.number ??
          null,
      };
    }),
    next: rows.length > PAGE ? (page[page.length - 1]?.id ?? null) : null,
  };
}

// biome-ignore lint/suspicious/noExplicitAny: mock payloads are untyped IPC.
function postDoc({ input }: any, access: MockStockAccess) {
  requireMember(access);
  access.requireCapability("post_documents");
  const type = input.doc_type as DocType;
  if (docs.some((doc) => doc.id === input.doc_id)) {
    conflict("stock document already exists");
  }
  const lines: Array<{
    product_id: string;
    qty_milli: number;
    cost_minor?: number;
  }> = input.lines ?? [];
  if (lines.length === 0 || lines.length > 500) {
    invalid("lines: a document has 1 to 500 lines");
  }
  if (new Set(lines.map((line) => line.product_id)).size !== lines.length) {
    invalid("lines: each product appears once");
  }
  liveWarehouse(access, input.warehouse_id, "warehouse_id");
  if (type === "transfer") {
    if (
      !input.to_warehouse_id ||
      input.to_warehouse_id === input.warehouse_id
    ) {
      invalid("to_warehouse_id: must name another warehouse");
    }
    liveWarehouse(access, input.to_warehouse_id, "to_warehouse_id");
  }
  if (input.supplier_id) {
    const party = mockCrmPartyById(input.supplier_id);
    if (!party) invalid("supplier_id: not a party of this CRM space");
    if (party.archived) invalid("supplier_id: the supplier is archived");
    if (party.type === "customer") {
      invalid("supplier_id: the party is not a supplier");
    }
  }

  const ledger = new Ledger();
  const wh: string = input.warehouse_id;
  const docLines: MockDocLine[] = [];
  let total = 0;
  for (const line of lines) {
    const product = mockCrmProductById(line.product_id);
    if (!product)
      invalid(`lines: product ${line.product_id} is not in the catalog`);
    if (!isStocked(product)) {
      invalid(`lines: "${product.name}" is a service, not stocked goods`);
    }
    if (type === "receipt" && product.archived) {
      invalid(`lines: product "${product.name}" is archived`);
    }
    if (line.qty_milli < 0 || (type !== "inventory" && line.qty_milli === 0)) {
      invalid("lines.qty_milli: must be positive");
    }
    checkUnit(product, line.qty_milli, "lines.qty_milli");
    const cost = line.cost_minor ?? null;
    if (type === "receipt" && cost === null) {
      invalid("lines.cost_minor: required on a receipt");
    }
    const level = ledger.level(wh, line.product_id);
    let amount = 0;
    let book: number | null = null;
    const short = () => invalid(`lines: not enough stock of "${product.name}"`);
    switch (type) {
      case "receipt": {
        amount = lineAmount(cost ?? 0, line.qty_milli);
        ledger.push(
          wh,
          line.product_id,
          type,
          input.doc_id,
          false,
          receive(level, line.qty_milli, amount),
        );
        break;
      }
      case "write_off": {
        const delta =
          issue(level, line.qty_milli, access.allowNegative) ?? short();
        ledger.push(wh, line.product_id, type, input.doc_id, false, delta);
        amount = -delta.value;
        break;
      }
      case "inventory": {
        const delta = count(level, line.qty_milli, cost);
        ledger.push(wh, line.product_id, type, input.doc_id, false, delta);
        amount = Math.abs(delta.value);
        book = level.qty;
        break;
      }
      case "transfer": {
        const out =
          issue(level, line.qty_milli, access.allowNegative) ?? short();
        ledger.push(wh, line.product_id, type, input.doc_id, false, out);
        amount = -out.value;
        const to: string = input.to_warehouse_id;
        ledger.push(
          to,
          line.product_id,
          type,
          input.doc_id,
          false,
          receive(ledger.level(to, line.product_id), line.qty_milli, amount),
        );
        break;
      }
    }
    total += amount;
    docLines.push({
      product_id: line.product_id,
      product_name: product.name,
      unit: product.unit,
      qty_milli: line.qty_milli,
      book_qty_milli: book,
      cost_minor: cost,
      amount_minor: amount,
    });
  }
  ledger.commit();
  docs.push({
    id: input.doc_id,
    number: docs.length + 1,
    type,
    warehouse_id: wh,
    to_warehouse_id: type === "transfer" ? input.to_warehouse_id : null,
    supplier_id: input.supplier_id ?? null,
    reference: input.reference ?? null,
    note: input.note ?? null,
    total_minor: total,
    posted_at: new Date().toISOString(),
    void: null,
    lines: docLines,
  });
  return { event_id: eventId(), entity_id: input.doc_id };
}

function voidDoc(
  payload: { docId: string; reason: string },
  access: MockStockAccess,
) {
  requireMember(access);
  access.requireCapability("void_documents");
  const doc = docs.find((row) => row.id === payload.docId);
  if (!doc) conflict("stock document does not exist");
  if (doc.void) conflict("stock document is already voided");
  if (payload.reason.trim() === "") invalid("reason: must not be empty");
  const ledger = new Ledger();
  const short = revertMoves(ledger, doc.id, access.allowNegative);
  if (short !== null) {
    invalid(
      `not enough stock of "${short}" to void the document: the goods were already used`,
    );
  }
  ledger.commit();
  doc.void = {
    voided_at: new Date().toISOString(),
    reason: payload.reason.trim(),
  };
  return { event_id: eventId(), entity_id: doc.id };
}

// biome-ignore lint/suspicious/noExplicitAny: mock payloads are untyped IPC.
function writeCard({ input }: any, access: MockStockAccess) {
  requireMember(access);
  access.requireCapability("edit_catalog");
  const service = mockCrmProductById(input.service_id);
  if (!service) invalid("crm-id: not a product of this CRM space");
  if (
    service.payment_object !== "service" &&
    service.payment_object !== "job"
  ) {
    invalid(`crm-id: "${service.name}" is not a service`);
  }
  const items: Array<{ product_id: string; qty_milli: number }> = input.items;
  if (items.length > 50)
    invalid("items: a tech card has at most 50 consumables");
  for (const item of items) {
    const product = mockCrmProductById(item.product_id);
    if (!product || !isStocked(product) || product.archived) {
      invalid(`items: ${item.product_id} is not live stocked goods`);
    }
    if (item.qty_milli <= 0) invalid("items.qty_milli: must be positive");
    checkUnit(product, item.qty_milli, "items.qty_milli");
  }
  const current = cards.get(input.service_id);
  if (input.expected_revision === null && current) {
    conflict(
      "the service already has a tech card; send expected-revision to replace it",
    );
  }
  if (input.expected_revision !== null && !current) {
    conflict("the service has no tech card");
  }
  if (current && current.revision !== input.expected_revision) {
    conflict("tech card changed since it was loaded");
  }
  const revision = eventId();
  cards.set(input.service_id, {
    items: items.map((item) => ({
      product_id: item.product_id,
      qty_milli: item.qty_milli,
    })),
    revision,
    updated_at: new Date().toISOString(),
  });
  return { event_id: revision, entity_id: input.service_id };
}

/** Handle a stock or tech-card command, or `undefined` otherwise. */
export function handleCrmStockMockCommand(
  command: string,
  payload: unknown,
  access: MockStockAccess,
): { value: unknown } | undefined {
  // biome-ignore lint/suspicious/noExplicitAny: mock payloads are untyped IPC.
  const p = payload as any;
  switch (command) {
    case "crm_list_stock":
      return { value: listStock(p, access) };
    case "crm_list_stock_moves":
      return { value: listMoves(p, access) };
    case "crm_list_stock_docs": {
      requireMember(access);
      const rows = docs
        .filter((doc) => !p.docType || doc.type === p.docType)
        .filter((doc) => p.before == null || doc.number < p.before)
        .sort((a, b) => b.number - a.number);
      const page = rows.slice(0, PAGE);
      return {
        value: {
          docs: page.map((doc) => docView(doc, access, false)),
          next:
            rows.length > PAGE ? (page[page.length - 1]?.number ?? null) : null,
        },
      };
    }
    case "crm_get_stock_doc": {
      requireMember(access);
      const doc = docs.find((row) => row.id === p.docId);
      if (!doc) throw "relay error 404: stock document not found";
      return { value: docView(doc, access, true) };
    }
    case "crm_list_tech_cards": {
      requireMember(access);
      return {
        value: {
          cards: [...cards].map(([serviceId, card]) => ({
            service_id: serviceId,
            service_name: mockCrmProductById(serviceId)?.name ?? "",
            items: card.items.map((item) => {
              const product = mockCrmProductById(item.product_id);
              return {
                ...item,
                product_name: product?.name ?? "",
                unit: product?.unit ?? "piece",
              };
            }),
            revision: card.revision,
            updated_at: card.updated_at,
          })),
        },
      };
    }
    case "crm_post_stock_doc":
      return { value: postDoc(p, access) };
    case "crm_void_stock_doc":
      return { value: voidDoc(p, access) };
    case "crm_write_tech_card":
      return { value: writeCard(p, access) };
    default:
      return undefined;
  }
}
