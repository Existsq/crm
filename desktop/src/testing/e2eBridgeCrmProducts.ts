/**
 * E2E mock for the CRM catalog commands (`crm_*_product*`).
 *
 * Mirrors the relay contract for kind 42102 and `GET /crm/products`: name
 * substring / SKU prefix / exact barcode search, archived filter, keyset
 * pages, compare-and-swap revisions, unique SKU and barcodes per space, and
 * the relay's `restricted:` / `conflict:` / `invalid:` messages.
 */

import type { CrmCapability, CrmVatRate } from "@/shared/api/tauriCrm";
import type {
  CrmPaymentObject,
  CrmUnit,
  RawCrmProduct,
  RawCrmProductPage,
} from "@/shared/api/tauriCrmProducts";

export type MockCrmProductSeed = {
  id?: string;
  name: string;
  sku?: string | null;
  barcodes?: string[];
  unit?: CrmUnit;
  price_minor: number;
  vat?: CrmVatRate;
  payment_object?: CrmPaymentObject;
  category?: string | null;
  min_stock_milli?: number;
  requires_marking?: boolean;
  archived?: boolean;
};

/** What the product mock needs to know about the seeded CRM space. */
export type MockCrmAccess = {
  /** `null` while there is no space or the identity is not a member. */
  channelId: string | null;
  requireCapability: (capability: CrmCapability) => void;
};

type ListPayload = {
  query?: string | null;
  barcode?: string | null;
  archived?: boolean | null;
  limit?: number | null;
  after?: string | null;
};

type WritePayload = {
  input: Omit<RawCrmProduct, "id" | "revision" | "updated_at"> & {
    channel_id: string;
    product_id: string;
    expected_revision: string | null;
  };
};

const DEFAULT_PAGE = 50;
const MAX_PAGE = 200;

let products: RawCrmProduct[] = [];

function eventId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function seedMockCrmProducts(seeds: MockCrmProductSeed[] = []): void {
  const now = new Date().toISOString();
  products = seeds.map((seed) => ({
    id: seed.id ?? crypto.randomUUID(),
    name: seed.name,
    sku: seed.sku ?? null,
    barcodes: seed.barcodes ?? [],
    unit: seed.unit ?? "piece",
    price_minor: seed.price_minor,
    vat: seed.vat ?? "vat22",
    payment_object: seed.payment_object ?? "commodity",
    category: seed.category ?? null,
    min_stock_milli: seed.min_stock_milli ?? 0,
    requires_marking: seed.requires_marking ?? false,
    archived: seed.archived ?? false,
    revision: eventId(),
    updated_at: now,
  }));
}

/** Keyset order: `lower(name), id`, compared by code point like the cursor. */
function sortKey(product: RawCrmProduct): [string, string] {
  return [product.name.toLowerCase(), product.id];
}

function after(a: [string, string], b: [string, string]): boolean {
  return a[0] > b[0] || (a[0] === b[0] && a[1] > b[1]);
}

function requireMember(access: MockCrmAccess): void {
  if (!access.channelId) {
    throw "relay error 403: restricted: not a member of the CRM space";
  }
}

function listProducts(
  payload: ListPayload,
  access: MockCrmAccess,
): RawCrmProductPage {
  requireMember(access);
  const q = payload.query?.trim().toLowerCase() ?? "";
  const barcode = payload.barcode?.trim() ?? "";
  const limit = Math.min(Math.max(payload.limit ?? DEFAULT_PAGE, 1), MAX_PAGE);
  const cursor = payload.after
    ? (JSON.parse(payload.after) as [string, string])
    : null;
  const matching = products
    .filter((p) => payload.archived || !p.archived)
    .filter((p) => !barcode || p.barcodes.includes(barcode))
    .filter(
      (p) =>
        !q ||
        p.name.toLowerCase().includes(q) ||
        (p.sku?.toLowerCase().startsWith(q) ?? false),
    )
    .sort((a, b) => (after(sortKey(a), sortKey(b)) ? 1 : -1))
    .filter((p) => !cursor || after(sortKey(p), cursor));
  const page = matching.slice(0, limit);
  const last = page.at(-1);
  return {
    products: page.map((p) => ({ ...p, barcodes: [...p.barcodes] })),
    next:
      matching.length > limit && last ? JSON.stringify(sortKey(last)) : null,
  };
}

function getProduct(
  { productId }: { productId: string },
  access: MockCrmAccess,
): RawCrmProduct {
  requireMember(access);
  const product = products.find((p) => p.id === productId);
  if (!product) throw "relay error 404: product not found";
  return product;
}

/** The subset of `ProductPayload::parse` the UI can reach past its own checks. */
function normalize(input: WritePayload["input"]) {
  const name = input.name.trim();
  if (!name) throw "relay rejected event: invalid: name must not be empty";
  if (input.price_minor < 0) {
    throw "relay rejected event: invalid: price_minor must not be negative";
  }
  if (input.vat === "vat20" || input.vat === "vat120") {
    throw `relay rejected event: invalid: vat: ${input.vat} is historical; use the current rate`;
  }
  const wholeOnly = input.unit === "piece" || input.unit === "service_unit";
  if (wholeOnly && input.min_stock_milli % 1000 !== 0) {
    throw `relay rejected event: invalid: min_stock_milli: ${input.unit} is sold in whole units`;
  }
  return {
    ...input,
    name,
    sku: input.sku?.trim() || null,
    category: input.category?.trim() || null,
    barcodes: [...new Set(input.barcodes.map((code) => code.trim()))],
  };
}

function writeProduct({ input }: WritePayload, access: MockCrmAccess) {
  access.requireCapability("edit_catalog");
  if (input.channel_id !== access.channelId) {
    throw "relay error 400: invalid: this channel is not the CRM space";
  }
  const fields = normalize(input);
  const id = input.product_id;
  const current = products.find((p) => p.id === id);
  if (input.expected_revision === null) {
    if (current) {
      throw "relay error 409: conflict: product already exists; send expected-revision to update it";
    }
  } else {
    if (!current) throw "relay error 409: conflict: product does not exist";
    if (current.revision !== input.expected_revision) {
      throw "relay error 409: conflict: product changed since it was loaded";
    }
  }
  const others = products.filter((p) => p.id !== id);
  const sku = fields.sku?.toLowerCase();
  if (sku && others.some((p) => p.sku?.toLowerCase() === sku)) {
    throw `relay error 409: conflict: SKU "${fields.sku}" is already used by another product`;
  }
  for (const code of fields.barcodes) {
    if (others.some((p) => p.barcodes.includes(code))) {
      throw `relay error 409: conflict: barcode ${code} is already used by another product`;
    }
  }
  const revision = eventId();
  const next: RawCrmProduct = {
    id,
    name: fields.name,
    sku: fields.sku,
    barcodes: fields.barcodes,
    unit: fields.unit,
    price_minor: fields.price_minor,
    vat: fields.vat,
    payment_object: fields.payment_object,
    category: fields.category,
    min_stock_milli: fields.min_stock_milli,
    requires_marking: fields.requires_marking,
    archived: fields.archived,
    revision,
    updated_at: new Date().toISOString(),
  };
  products = current
    ? products.map((p) => (p.id === id ? next : p))
    : [...products, next];
  return { event_id: revision, entity_id: id };
}

/**
 * Simulate another client saving `productId`: apply `patch` and move the
 * revision. Returns whether the product exists.
 */
export function editMockCrmProductElsewhere(
  productId: string,
  patch: Partial<Omit<RawCrmProduct, "id" | "revision">>,
): boolean {
  const current = products.find((p) => p.id === productId);
  if (!current) return false;
  Object.assign(current, patch, {
    revision: eventId(),
    updated_at: new Date().toISOString(),
  });
  return true;
}

/** Handle a catalog command, or `undefined` for anything else. */
export function handleCrmProductMockCommand(
  command: string,
  payload: unknown,
  access: MockCrmAccess,
): { value: unknown } | undefined {
  switch (command) {
    case "crm_list_products":
      return { value: listProducts(payload as ListPayload, access) };
    case "crm_get_product":
      return {
        value: getProduct(payload as { productId: string }, access),
      };
    case "crm_write_product":
      return { value: writeProduct(payload as WritePayload, access) };
    default:
      return undefined;
  }
}
