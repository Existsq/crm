import { invokeTauri } from "@/shared/api/tauri";
import type { CrmVatRate, CrmWriteResult } from "@/shared/api/tauriCrm";

// ── Domain types ──────────────────────────────────────────────────────────

/** Unit of measure (`buzz_crm::Unit`). */
export type CrmUnit =
  | "piece"
  | "kilogram"
  | "gram"
  | "liter"
  | "milliliter"
  | "meter"
  | "square_meter"
  | "hour"
  | "service_unit";

/** Fiscal item type, FFD tag 1212 (`buzz_crm::PaymentObject`). */
export type CrmPaymentObject =
  | "commodity"
  | "excise"
  | "job"
  | "service"
  | "another";

/** Editable product fields (kind 42102 payload without the schema version). */
export type CrmProductFields = {
  name: string;
  sku: string | null;
  barcodes: string[];
  unit: CrmUnit;
  /** Price per unit in kopecks (AD-004). */
  priceMinor: number;
  vat: CrmVatRate;
  paymentObject: CrmPaymentObject;
  category: string | null;
  /** Low-stock threshold in thousandths of a unit (AD-004). */
  minStockMilli: number;
  requiresMarking: boolean;
  archived: boolean;
  /** Booking duration in minutes; services and jobs only. */
  durationMin: number | null;
};

/** Services (`service`, `job`) are booked; everything else is goods. */
export type CrmProductKind = "services" | "goods";

export type CrmProduct = CrmProductFields & {
  id: string;
  revision: string;
  updatedAt: string;
};

export type CrmProductPage = {
  products: CrmProduct[];
  /** Opaque cursor for the next page, `null` on the last page. */
  next: string | null;
};

export type CrmProductListFilter = {
  query?: string;
  barcode?: string;
  /** Include archived products. */
  archived?: boolean;
  kind?: CrmProductKind;
  limit?: number;
  after?: string | null;
};

export type CrmProductWriteInput = CrmProductFields & {
  channelId: string;
  /**
   * Product id. For a create, pick it once when the dialog opens
   * (`crypto.randomUUID()`) so a retry cannot create a duplicate.
   */
  productId: string;
  /** Current revision; required for updates, `null` for creation. */
  expectedRevision: string | null;
};

// ── Raw types (snake_case from backend) ───────────────────────────────────

export type RawCrmProduct = {
  id: string;
  name: string;
  sku: string | null;
  barcodes: string[];
  unit: CrmUnit;
  price_minor: number;
  vat: CrmVatRate;
  payment_object: CrmPaymentObject;
  category: string | null;
  min_stock_milli: number;
  requires_marking: boolean;
  archived: boolean;
  duration_min?: number | null;
  revision: string;
  updated_at: string;
};

export type RawCrmProductPage = {
  products: RawCrmProduct[];
  next: string | null;
};

type RawCrmWriteResult = {
  event_id: string;
  entity_id?: string | null;
};

// ── Mappers ───────────────────────────────────────────────────────────────

export function fromRawCrmProduct(raw: RawCrmProduct): CrmProduct {
  return {
    id: raw.id,
    name: raw.name,
    sku: raw.sku ?? null,
    barcodes: raw.barcodes ?? [],
    unit: raw.unit,
    priceMinor: raw.price_minor,
    vat: raw.vat,
    paymentObject: raw.payment_object,
    category: raw.category ?? null,
    minStockMilli: raw.min_stock_milli,
    requiresMarking: raw.requires_marking,
    archived: raw.archived,
    durationMin: raw.duration_min ?? null,
    revision: raw.revision,
    updatedAt: raw.updated_at,
  };
}

export function toRawCrmProductWrite(input: CrmProductWriteInput) {
  return {
    channel_id: input.channelId,
    product_id: input.productId,
    expected_revision: input.expectedRevision,
    name: input.name,
    sku: input.sku,
    barcodes: input.barcodes,
    unit: input.unit,
    price_minor: input.priceMinor,
    vat: input.vat,
    payment_object: input.paymentObject,
    category: input.category,
    min_stock_milli: input.minStockMilli,
    requires_marking: input.requiresMarking,
    archived: input.archived,
    duration_min: input.durationMin,
  };
}

// ── Commands ──────────────────────────────────────────────────────────────

export async function listCrmProducts(
  filter: CrmProductListFilter = {},
): Promise<CrmProductPage> {
  const raw = await invokeTauri<RawCrmProductPage>("crm_list_products", {
    query: filter.query ?? null,
    barcode: filter.barcode ?? null,
    archived: filter.archived ?? false,
    limit: filter.limit ?? null,
    after: filter.after ?? null,
    kind: filter.kind ?? null,
  });
  return {
    products: raw.products.map(fromRawCrmProduct),
    next: raw.next ?? null,
  };
}

export async function getCrmProduct(productId: string): Promise<CrmProduct> {
  return fromRawCrmProduct(
    await invokeTauri<RawCrmProduct>("crm_get_product", { productId }),
  );
}

export async function writeCrmProduct(
  input: CrmProductWriteInput,
): Promise<CrmWriteResult> {
  const raw = await invokeTauri<RawCrmWriteResult>("crm_write_product", {
    input: toRawCrmProductWrite(input),
  });
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}
