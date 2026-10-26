import { invokeTauri } from "@/shared/api/tauri";
import type { CrmVatRate, CrmWriteResult } from "@/shared/api/tauriCrm";
import type { CrmUnit } from "@/shared/api/tauriCrmProducts";

// ── Parties (kind 42103) ──────────────────────────────────────────────────

/** `buzz_crm::PartyType`. */
export type CrmPartyType = "customer" | "supplier" | "both";

/** Role filter of `GET /crm/parties` (`both` matches either). */
export type CrmPartyRole = "customers" | "suppliers";

/** Editable party fields (kind 42103 payload without the schema version). */
export type CrmPartyFields = {
  partyType: CrmPartyType;
  name: string;
  phone: string | null;
  email: string | null;
  inn: string | null;
  address: string | null;
  note: string | null;
  archived: boolean;
};

export type CrmPartyPurchases = {
  salesCount: number;
  /** Sum of non-voided sale totals, kopecks. */
  revenueMinor: number;
  lastSaleAt: string | null;
};

export type CrmParty = CrmPartyFields & {
  id: string;
  purchases: CrmPartyPurchases;
  revision: string;
  createdAt: string;
  updatedAt: string;
};

export type CrmPartyPage = { parties: CrmParty[]; next: string | null };

export type CrmPartyListFilter = {
  query?: string;
  role?: CrmPartyRole | null;
  archived?: boolean;
  limit?: number;
  after?: string | null;
};

export type CrmPartyWriteInput = CrmPartyFields & {
  channelId: string;
  /** Picked once per dialog (`crypto.randomUUID()`) for a create. */
  partyId: string;
  /** Current revision; `null` creates the party. */
  expectedRevision: string | null;
};

type RawCrmParty = {
  id: string;
  type: CrmPartyType;
  name: string;
  phone: string | null;
  email: string | null;
  inn: string | null;
  address: string | null;
  note: string | null;
  archived: boolean;
  purchases: {
    sales_count: number;
    revenue_minor: number;
    last_sale_at: string | null;
  };
  revision: string;
  created_at: string;
  updated_at: string;
};

export function fromRawCrmParty(raw: RawCrmParty): CrmParty {
  return {
    id: raw.id,
    partyType: raw.type,
    name: raw.name,
    phone: raw.phone ?? null,
    email: raw.email ?? null,
    inn: raw.inn ?? null,
    address: raw.address ?? null,
    note: raw.note ?? null,
    archived: raw.archived,
    purchases: {
      salesCount: raw.purchases?.sales_count ?? 0,
      revenueMinor: raw.purchases?.revenue_minor ?? 0,
      lastSaleAt: raw.purchases?.last_sale_at ?? null,
    },
    revision: raw.revision,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

export function toRawCrmPartyWrite(input: CrmPartyWriteInput) {
  return {
    channel_id: input.channelId,
    party_id: input.partyId,
    expected_revision: input.expectedRevision,
    party_type: input.partyType,
    name: input.name,
    phone: input.phone,
    email: input.email,
    inn: input.inn,
    address: input.address,
    note: input.note,
    archived: input.archived,
  };
}

// ── Sales (kinds 42104, 42105) ────────────────────────────────────────────

/** `buzz_crm::PaymentMethod`. */
export type CrmPaymentMethod = "cash" | "card" | "transfer";

export type CrmSaleLineInput = {
  productId: string;
  /** Thousandths of the product's unit (AD-004). */
  qtyMilli: number;
  /** Kopecks per unit. */
  priceMinor: number;
  /** Kopecks off the whole line. */
  discountMinor: number;
};

/** One part of a split payment, kopecks. */
export type CrmSalePayment = { method: CrmPaymentMethod; amountMinor: number };

export type CrmSaleInput = {
  channelId: string;
  /** Picked once per sale dialog so a retried submit cannot sell twice. */
  saleId: string;
  warehouseId: string;
  customerId: string | null;
  /** The primary method; with `payments`, one of their methods. */
  payment: CrmPaymentMethod;
  /** Split payment summing to the total; omitted pays it all with `payment`. */
  payments?: CrmSalePayment[];
  /** Cash handed over, at least the cash part (only with cash). */
  cashTenderedMinor?: number | null;
  lines: CrmSaleLineInput[];
  note: string | null;
  /** Booked visit this sale pays for. */
  appointmentId?: string | null;
};

export type CrmSaleLine = {
  productId: string;
  productName: string;
  unit: CrmUnit;
  vat: CrmVatRate;
  qtyMilli: number;
  priceMinor: number;
  discountMinor: number;
  amountMinor: number;
};

export type CrmSaleVoid = {
  voidedAt: string;
  reason: string;
  voidedBy: string;
};

/** What the ККМ printed for one receipt. */
export type CrmFiscalRecord = {
  documentNumber: number;
  documentSign: string;
  receiptNumber: number;
  shiftNumber: number;
  fnNumber: string;
  documentDatetime: string;
  totalMinor: number;
};

export type CrmSale = {
  id: string;
  number: number;
  warehouseId: string;
  warehouseName: string;
  customerId: string | null;
  customerName: string | null;
  payment: CrmPaymentMethod;
  totalMinor: number;
  discountMinor: number;
  lineCount: number;
  note: string | null;
  soldAt: string;
  createdBy: string;
  void: CrmSaleVoid | null;
  /** Payment parts, largest first. */
  payments: CrmSalePayment[];
  cashTenderedMinor: number | null;
  /** Receipts printed for the sale and, after a void, its return. */
  fiscal: { sell: CrmFiscalRecord | null; sellReturn: CrmFiscalRecord | null };
  /** Present on `getCrmSale` only. */
  lines: CrmSaleLine[] | null;
};

export type CrmSalePage = { sales: CrmSale[]; next: number | null };

export type CrmSaleListFilter = {
  customerId?: string | null;
  /** Include voided sales (default true). */
  voided?: boolean;
  limit?: number;
  before?: number | null;
};

type RawFiscalRecord = {
  document_number: number;
  document_sign: string;
  receipt_number: number;
  shift_number: number;
  fn_number: string;
  document_datetime: string;
  total_minor: number;
};

function fromRawFiscal(raw: RawFiscalRecord | null | undefined) {
  return raw
    ? {
        documentNumber: raw.document_number,
        documentSign: raw.document_sign,
        receiptNumber: raw.receipt_number,
        shiftNumber: raw.shift_number,
        fnNumber: raw.fn_number,
        documentDatetime: raw.document_datetime,
        totalMinor: raw.total_minor,
      }
    : null;
}

type RawCrmSale = {
  id: string;
  number: number;
  warehouse_id: string;
  warehouse_name: string;
  customer_id: string | null;
  customer_name: string | null;
  payment: CrmPaymentMethod;
  total_minor: number;
  discount_minor: number;
  line_count: number;
  note: string | null;
  sold_at: string;
  created_by: string;
  void: { voided_at: string; reason: string; voided_by: string } | null;
  payments?: { method: CrmPaymentMethod; amount_minor: number }[];
  cash_tendered_minor?: number | null;
  fiscal?: {
    sell?: RawFiscalRecord | null;
    sell_return?: RawFiscalRecord | null;
  };
  lines?: {
    product_id: string;
    product_name: string;
    unit: CrmUnit;
    vat: CrmVatRate;
    qty_milli: number;
    price_minor: number;
    discount_minor: number;
    amount_minor: number;
  }[];
};

export function fromRawCrmSale(raw: RawCrmSale): CrmSale {
  return {
    id: raw.id,
    number: raw.number,
    warehouseId: raw.warehouse_id,
    warehouseName: raw.warehouse_name,
    customerId: raw.customer_id ?? null,
    customerName: raw.customer_name ?? null,
    payment: raw.payment,
    totalMinor: raw.total_minor,
    discountMinor: raw.discount_minor,
    lineCount: raw.line_count,
    note: raw.note ?? null,
    soldAt: raw.sold_at,
    createdBy: raw.created_by,
    void: raw.void
      ? {
          voidedAt: raw.void.voided_at,
          reason: raw.void.reason,
          voidedBy: raw.void.voided_by,
        }
      : null,
    payments: (raw.payments ?? []).map((part) => ({
      method: part.method,
      amountMinor: part.amount_minor,
    })),
    cashTenderedMinor: raw.cash_tendered_minor ?? null,
    fiscal: {
      sell: fromRawFiscal(raw.fiscal?.sell),
      sellReturn: fromRawFiscal(raw.fiscal?.sell_return),
    },
    lines: raw.lines
      ? raw.lines.map((line) => ({
          productId: line.product_id,
          productName: line.product_name,
          unit: line.unit,
          vat: line.vat,
          qtyMilli: line.qty_milli,
          priceMinor: line.price_minor,
          discountMinor: line.discount_minor,
          amountMinor: line.amount_minor,
        }))
      : null,
  };
}

export function toRawCrmSale(input: CrmSaleInput) {
  return {
    channel_id: input.channelId,
    sale_id: input.saleId,
    warehouse_id: input.warehouseId,
    customer_id: input.customerId,
    payment: input.payment,
    payments: (input.payments ?? []).map((part) => ({
      method: part.method,
      amount_minor: part.amountMinor,
    })),
    cash_tendered_minor: input.cashTenderedMinor ?? null,
    lines: input.lines.map((line) => ({
      product_id: line.productId,
      qty_milli: line.qtyMilli,
      price_minor: line.priceMinor,
      discount_minor: line.discountMinor,
    })),
    note: input.note,
    appointment_id: input.appointmentId ?? null,
  };
}

// ── Analytics ─────────────────────────────────────────────────────────────

export type CrmRevenueBucket<K> = {
  key: K;
  revenueMinor: number;
  salesCount: number;
};

export type CrmAnalytics = {
  /** First day, `YYYY-MM-DD` in the space timezone. */
  from: string;
  /** Last day (inclusive). */
  to: string;
  timezone: string;
  totals: {
    revenueMinor: number;
    salesCount: number;
    avgCheckMinor: number;
    discountMinor: number;
  };
  byDay: CrmRevenueBucket<string>[];
  byHour: CrmRevenueBucket<number>[];
  byPayment: CrmRevenueBucket<CrmPaymentMethod>[];
  topProducts: {
    productId: string;
    name: string;
    unit: CrmUnit;
    qtyMilli: number;
    revenueMinor: number;
  }[];
  topCustomers: {
    customerId: string;
    name: string;
    revenueMinor: number;
    salesCount: number;
  }[];
  customers: {
    buyersCount: number;
    repeatBuyersCount: number;
    newBuyersCount: number;
    identifiedRevenueMinor: number;
    anonymousRevenueMinor: number;
  };
};

type RawBucket<K> = { key: K; revenue_minor: number; sales_count: number };

type RawCrmAnalytics = {
  from: string;
  to: string;
  timezone: string;
  totals: {
    revenue_minor: number;
    sales_count: number;
    avg_check_minor: number;
    discount_minor: number;
  };
  by_day: RawBucket<string>[];
  by_hour: RawBucket<number>[];
  by_payment: RawBucket<CrmPaymentMethod>[];
  top_products: {
    product_id: string;
    name: string;
    unit: CrmUnit;
    qty_milli: number;
    revenue_minor: number;
  }[];
  top_customers: {
    customer_id: string;
    name: string;
    revenue_minor: number;
    sales_count: number;
  }[];
  customers: {
    buyers_count: number;
    repeat_buyers_count: number;
    new_buyers_count: number;
    identified_revenue_minor: number;
    anonymous_revenue_minor: number;
  };
};

function fromRawBucket<K>(raw: RawBucket<K>): CrmRevenueBucket<K> {
  return {
    key: raw.key,
    revenueMinor: raw.revenue_minor,
    salesCount: raw.sales_count,
  };
}

export function fromRawCrmAnalytics(raw: RawCrmAnalytics): CrmAnalytics {
  return {
    from: raw.from,
    to: raw.to,
    timezone: raw.timezone,
    totals: {
      revenueMinor: raw.totals.revenue_minor,
      salesCount: raw.totals.sales_count,
      avgCheckMinor: raw.totals.avg_check_minor,
      discountMinor: raw.totals.discount_minor,
    },
    byDay: raw.by_day.map(fromRawBucket),
    byHour: raw.by_hour.map(fromRawBucket),
    byPayment: raw.by_payment.map(fromRawBucket),
    topProducts: raw.top_products.map((row) => ({
      productId: row.product_id,
      name: row.name,
      unit: row.unit,
      qtyMilli: row.qty_milli,
      revenueMinor: row.revenue_minor,
    })),
    topCustomers: raw.top_customers.map((row) => ({
      customerId: row.customer_id,
      name: row.name,
      revenueMinor: row.revenue_minor,
      salesCount: row.sales_count,
    })),
    customers: {
      buyersCount: raw.customers.buyers_count,
      repeatBuyersCount: raw.customers.repeat_buyers_count,
      newBuyersCount: raw.customers.new_buyers_count,
      identifiedRevenueMinor: raw.customers.identified_revenue_minor,
      anonymousRevenueMinor: raw.customers.anonymous_revenue_minor,
    },
  };
}

// ── Commands ──────────────────────────────────────────────────────────────

type RawCrmWriteResult = { event_id: string; entity_id?: string | null };

function fromRawWrite(raw: RawCrmWriteResult): CrmWriteResult {
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}

export async function listCrmParties(
  filter: CrmPartyListFilter = {},
): Promise<CrmPartyPage> {
  const raw = await invokeTauri<{
    parties: RawCrmParty[];
    next: string | null;
  }>("crm_list_parties", {
    query: filter.query ?? null,
    role: filter.role ?? null,
    archived: filter.archived ?? false,
    limit: filter.limit ?? null,
    after: filter.after ?? null,
  });
  return { parties: raw.parties.map(fromRawCrmParty), next: raw.next ?? null };
}

export async function getCrmParty(partyId: string): Promise<CrmParty> {
  return fromRawCrmParty(
    await invokeTauri<RawCrmParty>("crm_get_party", { partyId }),
  );
}

export async function writeCrmParty(
  input: CrmPartyWriteInput,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_write_party", {
      input: toRawCrmPartyWrite(input),
    }),
  );
}

export async function listCrmSales(
  filter: CrmSaleListFilter = {},
): Promise<CrmSalePage> {
  const raw = await invokeTauri<{ sales: RawCrmSale[]; next: number | null }>(
    "crm_list_sales",
    {
      customer: filter.customerId ?? null,
      voided: filter.voided ?? true,
      limit: filter.limit ?? null,
      before: filter.before ?? null,
    },
  );
  return { sales: raw.sales.map(fromRawCrmSale), next: raw.next ?? null };
}

export async function getCrmSale(saleId: string): Promise<CrmSale> {
  return fromRawCrmSale(
    await invokeTauri<RawCrmSale>("crm_get_sale", { saleId }),
  );
}

export async function postCrmSale(
  input: CrmSaleInput,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_post_sale", {
      input: toRawCrmSale(input),
    }),
  );
}

export async function voidCrmSale(input: {
  channelId: string;
  saleId: string;
  reason: string;
}): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_void_sale", {
      channelId: input.channelId,
      saleId: input.saleId,
      reason: input.reason,
    }),
  );
}

export async function getCrmAnalytics(period: {
  from: string | null;
  to: string | null;
}): Promise<CrmAnalytics> {
  return fromRawCrmAnalytics(
    await invokeTauri<RawCrmAnalytics>("crm_get_analytics", {
      from: period.from,
      to: period.to,
    }),
  );
}
