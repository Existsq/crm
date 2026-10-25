import { invokeTauri } from "@/shared/api/tauri";
import type { CrmWriteResult } from "@/shared/api/tauriCrm";
import type { CrmUnit } from "@/shared/api/tauriCrmProducts";

// ── Domain types ──────────────────────────────────────────────────────────

/** `buzz_crm::StockDocType`. */
export type CrmStockDocType =
  | "receipt"
  | "write_off"
  | "inventory"
  | "transfer";

/** Where a stock movement came from. */
export type CrmStockMoveSource = CrmStockDocType | "sale" | "consumable";

/** One product's stock on a warehouse (or summed over all of them). */
export type CrmStockItem = {
  productId: string;
  name: string;
  sku: string | null;
  unit: CrmUnit;
  category: string | null;
  priceMinor: number;
  minStockMilli: number;
  qtyMilli: number;
  /** Cost value of the quantity on hand, kopecks. */
  valueMinor: number;
  archived: boolean;
};

export type CrmStockLevels = {
  warehouseId: string | null;
  items: CrmStockItem[];
  totalValueMinor: number;
  lowCount: number;
};

export type CrmStockMove = {
  id: number;
  warehouseId: string;
  warehouseName: string;
  productId: string;
  productName: string;
  unit: CrmUnit;
  source: CrmStockMoveSource;
  docId: string;
  docNumber: number | null;
  reversal: boolean;
  qtyDeltaMilli: number;
  valueDeltaMinor: number;
  qtyAfterMilli: number;
  movedAt: string;
};

export type CrmStockMovePage = { moves: CrmStockMove[]; next: number | null };

export type CrmStockDocLine = {
  productId: string;
  productName: string;
  unit: CrmUnit;
  qtyMilli: number;
  bookQtyMilli: number | null;
  costMinor: number | null;
  amountMinor: number;
};

export type CrmStockDoc = {
  id: string;
  number: number;
  type: CrmStockDocType;
  warehouseId: string;
  warehouseName: string;
  toWarehouseId: string | null;
  toWarehouseName: string | null;
  supplierId: string | null;
  supplierName: string | null;
  reference: string | null;
  note: string | null;
  lineCount: number;
  totalMinor: number;
  postedAt: string;
  void: { voidedAt: string; reason: string } | null;
  lines: CrmStockDocLine[] | null;
};

export type CrmStockDocPage = { docs: CrmStockDoc[]; next: number | null };

/** A stock document line as submitted (kind 42106). */
export type CrmStockDocLineInput = {
  productId: string;
  qtyMilli: number;
  costMinor: number | null;
};

export type CrmStockDocInput = {
  channelId: string;
  /** Picked once per dialog, so a retried submit cannot post twice. */
  docId: string;
  type: CrmStockDocType;
  warehouseId: string;
  toWarehouseId: string | null;
  supplierId: string | null;
  reference: string | null;
  lines: CrmStockDocLineInput[];
  note: string | null;
};

export type CrmTechCardItem = {
  productId: string;
  productName: string;
  unit: CrmUnit;
  /** Quantity per service unit, thousandths. */
  qtyMilli: number;
};

export type CrmTechCard = {
  serviceId: string;
  serviceName: string;
  items: CrmTechCardItem[];
  revision: string;
  updatedAt: string;
};

export type CrmTechCardInput = {
  channelId: string;
  serviceId: string;
  /** Current revision to replace a card; `null` creates it. */
  expectedRevision: string | null;
  items: { productId: string; qtyMilli: number }[];
};

// ── Raw types (snake_case from backend) ───────────────────────────────────

export type RawCrmStockItem = {
  product_id: string;
  name: string;
  sku: string | null;
  unit: CrmUnit;
  category: string | null;
  price_minor: number;
  min_stock_milli: number;
  qty_milli: number;
  value_minor: number;
  archived: boolean;
};

export type RawCrmStockLevels = {
  warehouse_id: string | null;
  items: RawCrmStockItem[];
  totals: { value_minor: number; low_count: number };
};

export type RawCrmStockMove = {
  id: number;
  warehouse_id: string;
  warehouse_name: string;
  product_id: string;
  product_name: string;
  unit: CrmUnit;
  source: CrmStockMoveSource;
  doc_id: string;
  doc_number: number | null;
  reversal: boolean;
  qty_delta_milli: number;
  value_delta_minor: number;
  qty_after_milli: number;
  moved_at: string;
};

export type RawCrmStockDocLine = {
  product_id: string;
  product_name: string;
  unit: CrmUnit;
  qty_milli: number;
  book_qty_milli: number | null;
  cost_minor: number | null;
  amount_minor: number;
};

export type RawCrmStockDoc = {
  id: string;
  number: number;
  type: CrmStockDocType;
  warehouse_id: string;
  warehouse_name: string;
  to_warehouse_id: string | null;
  to_warehouse_name: string | null;
  supplier_id: string | null;
  supplier_name: string | null;
  reference: string | null;
  note: string | null;
  line_count: number;
  total_minor: number;
  posted_at: string;
  void: { voided_at: string; reason: string } | null;
  lines?: RawCrmStockDocLine[];
};

export type RawCrmTechCard = {
  service_id: string;
  service_name: string;
  items: Array<{
    product_id: string;
    product_name: string;
    unit: CrmUnit;
    qty_milli: number;
  }>;
  revision: string;
  updated_at: string;
};

type RawCrmWriteResult = { event_id: string; entity_id?: string | null };

// ── Mappers ───────────────────────────────────────────────────────────────

export function fromRawCrmStockLevels(raw: RawCrmStockLevels): CrmStockLevels {
  return {
    warehouseId: raw.warehouse_id ?? null,
    items: raw.items.map((item) => ({
      productId: item.product_id,
      name: item.name,
      sku: item.sku ?? null,
      unit: item.unit,
      category: item.category ?? null,
      priceMinor: item.price_minor,
      minStockMilli: item.min_stock_milli,
      qtyMilli: item.qty_milli,
      valueMinor: item.value_minor,
      archived: item.archived,
    })),
    totalValueMinor: raw.totals.value_minor,
    lowCount: raw.totals.low_count,
  };
}

export function fromRawCrmStockMove(raw: RawCrmStockMove): CrmStockMove {
  return {
    id: raw.id,
    warehouseId: raw.warehouse_id,
    warehouseName: raw.warehouse_name,
    productId: raw.product_id,
    productName: raw.product_name,
    unit: raw.unit,
    source: raw.source,
    docId: raw.doc_id,
    docNumber: raw.doc_number ?? null,
    reversal: raw.reversal,
    qtyDeltaMilli: raw.qty_delta_milli,
    valueDeltaMinor: raw.value_delta_minor,
    qtyAfterMilli: raw.qty_after_milli,
    movedAt: raw.moved_at,
  };
}

export function fromRawCrmStockDoc(raw: RawCrmStockDoc): CrmStockDoc {
  return {
    id: raw.id,
    number: raw.number,
    type: raw.type,
    warehouseId: raw.warehouse_id,
    warehouseName: raw.warehouse_name,
    toWarehouseId: raw.to_warehouse_id ?? null,
    toWarehouseName: raw.to_warehouse_name ?? null,
    supplierId: raw.supplier_id ?? null,
    supplierName: raw.supplier_name ?? null,
    reference: raw.reference ?? null,
    note: raw.note ?? null,
    lineCount: raw.line_count,
    totalMinor: raw.total_minor,
    postedAt: raw.posted_at,
    void: raw.void
      ? { voidedAt: raw.void.voided_at, reason: raw.void.reason }
      : null,
    lines:
      raw.lines?.map((line) => ({
        productId: line.product_id,
        productName: line.product_name,
        unit: line.unit,
        qtyMilli: line.qty_milli,
        bookQtyMilli: line.book_qty_milli ?? null,
        costMinor: line.cost_minor ?? null,
        amountMinor: line.amount_minor,
      })) ?? null,
  };
}

export function fromRawCrmTechCard(raw: RawCrmTechCard): CrmTechCard {
  return {
    serviceId: raw.service_id,
    serviceName: raw.service_name,
    items: raw.items.map((item) => ({
      productId: item.product_id,
      productName: item.product_name,
      unit: item.unit,
      qtyMilli: item.qty_milli,
    })),
    revision: raw.revision,
    updatedAt: raw.updated_at,
  };
}

export function toRawCrmStockDoc(input: CrmStockDocInput) {
  return {
    channel_id: input.channelId,
    doc_id: input.docId,
    doc_type: input.type,
    warehouse_id: input.warehouseId,
    to_warehouse_id: input.toWarehouseId,
    supplier_id: input.supplierId,
    reference: input.reference,
    lines: input.lines.map((line) => ({
      product_id: line.productId,
      qty_milli: line.qtyMilli,
      ...(line.costMinor === null ? {} : { cost_minor: line.costMinor }),
    })),
    note: input.note,
  };
}

function fromRawWrite(raw: RawCrmWriteResult): CrmWriteResult {
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}

// ── Commands ──────────────────────────────────────────────────────────────

export async function listCrmStock(
  filter: { warehouseId?: string | null; query?: string; low?: boolean } = {},
): Promise<CrmStockLevels> {
  return fromRawCrmStockLevels(
    await invokeTauri<RawCrmStockLevels>("crm_list_stock", {
      warehouseId: filter.warehouseId ?? null,
      query: filter.query ?? null,
      low: filter.low ?? false,
    }),
  );
}

export async function listCrmStockMoves(
  filter: {
    warehouseId?: string | null;
    productId?: string | null;
    before?: number | null;
  } = {},
): Promise<CrmStockMovePage> {
  const raw = await invokeTauri<{
    moves: RawCrmStockMove[];
    next: number | null;
  }>("crm_list_stock_moves", {
    warehouseId: filter.warehouseId ?? null,
    productId: filter.productId ?? null,
    before: filter.before ?? null,
  });
  return { moves: raw.moves.map(fromRawCrmStockMove), next: raw.next ?? null };
}

export async function listCrmStockDocs(
  filter: { type?: CrmStockDocType | null; before?: number | null } = {},
): Promise<CrmStockDocPage> {
  const raw = await invokeTauri<{
    docs: RawCrmStockDoc[];
    next: number | null;
  }>("crm_list_stock_docs", {
    docType: filter.type ?? null,
    before: filter.before ?? null,
  });
  return { docs: raw.docs.map(fromRawCrmStockDoc), next: raw.next ?? null };
}

export async function getCrmStockDoc(docId: string): Promise<CrmStockDoc> {
  return fromRawCrmStockDoc(
    await invokeTauri<RawCrmStockDoc>("crm_get_stock_doc", { docId }),
  );
}

export async function listCrmTechCards(): Promise<CrmTechCard[]> {
  const raw = await invokeTauri<{ cards: RawCrmTechCard[] }>(
    "crm_list_tech_cards",
  );
  return raw.cards.map(fromRawCrmTechCard);
}

export async function postCrmStockDoc(
  input: CrmStockDocInput,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_post_stock_doc", {
      input: toRawCrmStockDoc(input),
    }),
  );
}

export async function voidCrmStockDoc(input: {
  channelId: string;
  docId: string;
  reason: string;
}): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_void_stock_doc", input),
  );
}

export async function writeCrmTechCard(
  input: CrmTechCardInput,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_write_tech_card", {
      input: {
        channel_id: input.channelId,
        service_id: input.serviceId,
        expected_revision: input.expectedRevision,
        items: input.items.map((item) => ({
          product_id: item.productId,
          qty_milli: item.qtyMilli,
        })),
      },
    }),
  );
}
