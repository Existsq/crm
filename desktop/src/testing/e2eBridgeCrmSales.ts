/**
 * E2E mock for the CRM customer, sale and analytics commands
 * (`crm_*_part*`, `crm_*_sale*`, `crm_get_analytics`).
 *
 * Mirrors the relay contract for kinds 42103–42105: CAS party revisions,
 * unique INN, sales posted once with per-space numbers and relay-computed
 * amounts, one void per sale, and analytics that skip voided sales. Days
 * are taken in UTC; the relay uses the space timezone.
 */

import type { CrmCapability } from "@/shared/api/tauriCrm";

import { mockCrmProductById } from "./e2eBridgeCrmProducts.ts";

export type MockCrmPartySeed = {
  id?: string;
  type?: "customer" | "supplier" | "both";
  name: string;
  phone?: string | null;
  email?: string | null;
  inn?: string | null;
  archived?: boolean;
};

export type MockCrmSaleSeed = {
  customerId?: string | null;
  payment?: "cash" | "card" | "transfer";
  soldAt?: string;
  lines: Array<{ productId: string; qtyMilli: number; priceMinor: number }>;
};

type MockAccess = {
  channelId: string | null;
  warehouses: Array<{ id: string; name: string; archived: boolean }>;
  requireCapability: (capability: CrmCapability) => void;
};

type MockParty = {
  id: string;
  type: "customer" | "supplier" | "both";
  name: string;
  phone: string | null;
  email: string | null;
  inn: string | null;
  address: string | null;
  note: string | null;
  archived: boolean;
  revision: string;
  created_at: string;
  updated_at: string;
};

type MockLine = {
  product_id: string;
  product_name: string;
  unit: string;
  vat: string;
  qty_milli: number;
  price_minor: number;
  discount_minor: number;
  amount_minor: number;
};

type MockSale = {
  id: string;
  number: number;
  warehouse_id: string;
  customer_id: string | null;
  payment: "cash" | "card" | "transfer";
  total_minor: number;
  discount_minor: number;
  note: string | null;
  sold_at: string;
  void: { voided_at: string; reason: string; voided_by: string } | null;
  payments: Array<{ method: MockSale["payment"]; amount_minor: number }>;
  cash_tendered_minor: number | null;
  fiscal: { sell?: MockFiscalRecord; sell_return?: MockFiscalRecord };
  lines: MockLine[];
};

/** What the mock ККМ printed for one receipt (`CrmFiscalRecord`). */
export type MockFiscalRecord = {
  document_number: number;
  document_sign: string;
  receipt_number: number;
  shift_number: number;
  fn_number: string;
  registration_number: string | null;
  document_datetime: string;
  total_minor: number;
};

let parties: MockParty[] = [];
let sales: MockSale[] = [];

function eventId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/** `buzz_crm::line_amount`: half-up rounding of price × qty / 1000. */
function lineAmount(price: number, qty: number): number {
  return Math.round((price * qty) / 1000);
}

export function seedMockCrmSales(
  partySeeds: MockCrmPartySeed[] = [],
  saleSeeds: MockCrmSaleSeed[] = [],
  warehouseId: string | null = null,
): void {
  const now = new Date().toISOString();
  parties = partySeeds.map((seed) => ({
    id: seed.id ?? crypto.randomUUID(),
    type: seed.type ?? "customer",
    name: seed.name,
    phone: seed.phone ?? null,
    email: seed.email ?? null,
    inn: seed.inn ?? null,
    address: null,
    note: null,
    archived: seed.archived ?? false,
    revision: eventId(),
    created_at: now,
    updated_at: now,
  }));
  sales = [];
  for (const seed of saleSeeds) {
    const lines = seed.lines.map((line) => {
      const product = mockCrmProductById(line.productId);
      const amount = lineAmount(line.priceMinor, line.qtyMilli);
      return {
        product_id: line.productId,
        product_name: product?.name ?? "?",
        unit: product?.unit ?? "piece",
        vat: product?.vat ?? "vat22",
        qty_milli: line.qtyMilli,
        price_minor: line.priceMinor,
        discount_minor: 0,
        amount_minor: amount,
      };
    });
    sales.push({
      id: crypto.randomUUID(),
      number: sales.length + 1,
      warehouse_id: warehouseId ?? "",
      customer_id: seed.customerId ?? null,
      payment: seed.payment ?? "cash",
      total_minor: lines.reduce((sum, line) => sum + line.amount_minor, 0),
      discount_minor: 0,
      note: null,
      sold_at: seed.soldAt ?? now,
      void: null,
      payments: [
        {
          method: seed.payment ?? "cash",
          amount_minor: lines.reduce((sum, line) => sum + line.amount_minor, 0),
        },
      ],
      cash_tendered_minor: null,
      fiscal: {},
      lines,
    });
  }
}

function requireMember(access: MockAccess): string {
  if (!access.channelId) {
    throw "relay error 403: not a member of the CRM space";
  }
  return access.channelId;
}

function purchases(partyId: string) {
  const own = sales.filter((s) => s.customer_id === partyId && !s.void);
  return {
    sales_count: own.length,
    revenue_minor: own.reduce((sum, s) => sum + s.total_minor, 0),
    last_sale_at:
      own
        .map((s) => s.sold_at)
        .sort()
        .at(-1) ?? null,
  };
}

function partyView(party: MockParty) {
  return { ...party, purchases: purchases(party.id) };
}

function listParties(
  payload: {
    query?: string | null;
    role?: string | null;
    archived?: boolean | null;
  },
  access: MockAccess,
) {
  requireMember(access);
  const q = payload.query?.trim().toLowerCase() ?? "";
  const digits = q.replace(/\D/g, "");
  const rows = parties
    .filter((p) => payload.archived || !p.archived)
    .filter((p) =>
      payload.role === "customers"
        ? p.type !== "supplier"
        : payload.role === "suppliers"
          ? p.type !== "customer"
          : true,
    )
    .filter(
      (p) =>
        q === "" ||
        p.name.toLowerCase().includes(q) ||
        (p.email ?? "").toLowerCase().includes(q) ||
        (p.inn ?? "").startsWith(q) ||
        (digits.length >= 3 &&
          (p.phone ?? "").replace(/\D/g, "").includes(digits)),
    )
    .sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()));
  return { parties: rows.map(partyView), next: null };
}

function writeParty(
  payload: {
    input: Omit<
      MockParty,
      "id" | "revision" | "created_at" | "updated_at" | "type"
    > & {
      party_type: MockParty["type"];
      channel_id: string;
      party_id: string;
      expected_revision: string | null;
    };
  },
  access: MockAccess,
) {
  requireMember(access);
  access.requireCapability("edit_parties");
  const input = payload.input;
  const current = parties.find((p) => p.id === input.party_id);
  if (!input.expected_revision && current) {
    throw "relay error 409: conflict: party already exists; send expected-revision to update it";
  }
  if (
    input.expected_revision &&
    current?.revision !== input.expected_revision
  ) {
    throw "relay error 409: conflict: party changed since it was loaded";
  }
  const inn = input.inn?.trim() || null;
  if (inn && parties.some((p) => p.inn === inn && p.id !== input.party_id)) {
    throw `relay error 409: conflict: INN ${inn} is already used by another party`;
  }
  const now = new Date().toISOString();
  const next: MockParty = {
    id: input.party_id,
    type: input.party_type,
    name: input.name.trim(),
    phone: input.phone?.trim() || null,
    email: input.email?.trim() || null,
    inn,
    address: input.address?.trim() || null,
    note: input.note?.trim() || null,
    archived: input.archived,
    revision: eventId(),
    created_at: current?.created_at ?? now,
    updated_at: now,
  };
  parties = current
    ? parties.map((p) => (p.id === next.id ? next : p))
    : [...parties, next];
  return { event_id: next.revision, entity_id: next.id };
}

function saleView(sale: MockSale, access: MockAccess, withLines: boolean) {
  const { lines, ...rest } = sale;
  return {
    ...rest,
    warehouse_name:
      access.warehouses.find((w) => w.id === sale.warehouse_id)?.name ?? "",
    customer_name: parties.find((p) => p.id === sale.customer_id)?.name ?? null,
    line_count: lines.length,
    created_by: "0".repeat(64),
    event_id: "0".repeat(64),
    ...(withLines ? { lines } : {}),
  };
}

function postSale(
  payload: {
    input: {
      sale_id: string;
      warehouse_id: string;
      customer_id: string | null;
      payment: MockSale["payment"];
      payments?: Array<{ method: MockSale["payment"]; amount_minor: number }>;
      cash_tendered_minor?: number | null;
      lines: Array<{
        product_id: string;
        qty_milli: number;
        price_minor: number;
        discount_minor: number;
      }>;
      note: string | null;
    };
  },
  access: MockAccess,
) {
  requireMember(access);
  access.requireCapability("post_documents");
  const input = payload.input;
  if (sales.some((s) => s.id === input.sale_id)) {
    throw "relay error 409: conflict: sale already exists";
  }
  if (
    !access.warehouses.some((w) => w.id === input.warehouse_id && !w.archived)
  ) {
    throw "relay error 400: invalid: warehouse_id: not a warehouse of this CRM space";
  }
  if (input.customer_id) {
    const party = parties.find((p) => p.id === input.customer_id);
    if (!party || party.archived || party.type === "supplier") {
      throw "relay error 400: invalid: customer_id: not a customer of this CRM space";
    }
  }
  const lines = input.lines.map((line) => {
    const product = mockCrmProductById(line.product_id);
    if (!product || product.archived) {
      throw `relay error 400: invalid: lines: product ${line.product_id} is not in the catalog`;
    }
    return {
      product_id: line.product_id,
      product_name: product.name,
      unit: product.unit,
      vat: product.vat,
      qty_milli: line.qty_milli,
      price_minor: line.price_minor,
      discount_minor: line.discount_minor,
      amount_minor:
        lineAmount(line.price_minor, line.qty_milli) - line.discount_minor,
    };
  });
  const total = lines.reduce((sum, line) => sum + line.amount_minor, 0);
  const payments =
    input.payments && input.payments.length > 0
      ? input.payments
      : [{ method: input.payment, amount_minor: total }];
  if (payments.reduce((sum, part) => sum + part.amount_minor, 0) !== total) {
    throw "relay error 400: invalid: payments: parts do not sum to the sale total";
  }
  const cash = payments.find((part) => part.method === "cash")?.amount_minor;
  const tendered = input.cash_tendered_minor ?? null;
  if (tendered !== null && (cash === undefined || tendered < cash)) {
    throw "relay error 400: invalid: cash_tendered_minor: is less than the cash part of the payment";
  }
  sales.push({
    id: input.sale_id,
    number: sales.length + 1,
    warehouse_id: input.warehouse_id,
    customer_id: input.customer_id,
    payment: input.payment,
    total_minor: total,
    discount_minor: lines.reduce((sum, line) => sum + line.discount_minor, 0),
    note: input.note,
    sold_at: new Date().toISOString(),
    void: null,
    payments: [...payments].sort((a, b) => b.amount_minor - a.amount_minor),
    cash_tendered_minor: tendered,
    fiscal: {},
    lines,
  });
  return { event_id: eventId(), entity_id: input.sale_id };
}

function voidSale(
  payload: { saleId: string; reason: string },
  access: MockAccess,
) {
  requireMember(access);
  access.requireCapability("void_documents");
  const sale = sales.find((s) => s.id === payload.saleId);
  if (!sale) throw "relay error 409: conflict: sale does not exist";
  if (sale.void) throw "relay error 409: conflict: sale is already voided";
  if (payload.reason.trim() === "") {
    throw "relay error 400: invalid: reason: must not be empty";
  }
  sale.void = {
    voided_at: new Date().toISOString(),
    reason: payload.reason.trim(),
    voided_by: "0".repeat(64),
  };
  return { event_id: eventId(), entity_id: sale.id };
}

function analytics(
  payload: { from?: string | null; to?: string | null },
  access: MockAccess,
) {
  requireMember(access);
  const today = new Date().toISOString().slice(0, 10);
  const to = payload.to ?? today;
  const from = payload.from ?? to;
  const inPeriod = sales.filter((s) => {
    const day = s.sold_at.slice(0, 10);
    return !s.void && day >= from && day <= to;
  });
  const revenue = inPeriod.reduce((sum, s) => sum + s.total_minor, 0);
  const group = <K extends string | number>(key: (s: MockSale) => K) => {
    const map = new Map<
      K,
      { key: K; revenue_minor: number; sales_count: number }
    >();
    for (const sale of inPeriod) {
      const k = key(sale);
      const bucket = map.get(k) ?? { key: k, revenue_minor: 0, sales_count: 0 };
      bucket.revenue_minor += sale.total_minor;
      bucket.sales_count += 1;
      map.set(k, bucket);
    }
    return [...map.values()];
  };
  const products = new Map<
    string,
    {
      product_id: string;
      name: string;
      unit: string;
      qty_milli: number;
      revenue_minor: number;
    }
  >();
  for (const line of inPeriod.flatMap((s) => s.lines)) {
    const row = products.get(line.product_id) ?? {
      product_id: line.product_id,
      name: line.product_name,
      unit: line.unit,
      qty_milli: 0,
      revenue_minor: 0,
    };
    row.qty_milli += line.qty_milli;
    row.revenue_minor += line.amount_minor;
    products.set(line.product_id, row);
  }
  const customers = group((s) => s.customer_id ?? "")
    .filter((bucket) => bucket.key !== "")
    .map((bucket) => ({
      customer_id: bucket.key,
      name: parties.find((p) => p.id === bucket.key)?.name ?? "?",
      revenue_minor: bucket.revenue_minor,
      sales_count: bucket.sales_count,
    }))
    .sort((a, b) => b.revenue_minor - a.revenue_minor);
  const identified = customers.reduce((sum, c) => sum + c.revenue_minor, 0);
  return {
    from,
    to,
    timezone: "UTC",
    totals: {
      revenue_minor: revenue,
      sales_count: inPeriod.length,
      avg_check_minor: inPeriod.length
        ? Math.round(revenue / inPeriod.length)
        : 0,
      discount_minor: inPeriod.reduce((sum, s) => sum + s.discount_minor, 0),
    },
    by_day: group((s) => s.sold_at.slice(0, 10)).sort((a, b) =>
      a.key.localeCompare(b.key),
    ),
    by_hour: group((s) => new Date(s.sold_at).getUTCHours()).sort(
      (a, b) => a.key - b.key,
    ),
    by_payment: group((s) => s.payment).sort(
      (a, b) => b.revenue_minor - a.revenue_minor,
    ),
    top_products: [...products.values()]
      .sort((a, b) => b.revenue_minor - a.revenue_minor)
      .slice(0, 10),
    top_customers: customers.slice(0, 10),
    customers: {
      buyers_count: customers.length,
      repeat_buyers_count: customers.filter((c) => c.sales_count >= 2).length,
      new_buyers_count: customers.length,
      identified_revenue_minor: identified,
      anonymous_revenue_minor: revenue - identified,
    },
  };
}

/** A mock sale as the cash-desk mock reads it. */
export function mockCrmSaleById(saleId: string): MockSale | undefined {
  return sales.find((sale) => sale.id === saleId);
}

/** Record a printed receipt on a sale, as kind 42120 does at the relay. */
export function recordMockCrmReceipt(
  saleId: string,
  operation: "sell" | "sell_return",
  record: MockFiscalRecord,
): void {
  const sale = sales.find((row) => row.id === saleId);
  if (!sale) throw "relay error 409: conflict: sale does not exist";
  if (sale.fiscal[operation]) {
    throw "relay error 409: conflict: receipt already recorded for the sale";
  }
  sale.fiscal[operation] = record;
}

/** Handle a customer/sale/analytics command, or `undefined` otherwise. */
export function handleCrmSalesMockCommand(
  command: string,
  payload: unknown,
  access: MockAccess,
): { value: unknown } | undefined {
  // biome-ignore lint/suspicious/noExplicitAny: mock payloads are untyped IPC.
  const p = payload as any;
  switch (command) {
    case "crm_list_parties":
      return { value: listParties(p, access) };
    case "crm_get_party": {
      requireMember(access);
      const party = parties.find((row) => row.id === p.partyId);
      if (!party) throw "relay error 404: party not found";
      return { value: partyView(party) };
    }
    case "crm_write_party":
      return { value: writeParty(p, access) };
    case "crm_list_sales": {
      requireMember(access);
      const rows = sales
        .filter((s) => p.voided !== false || !s.void)
        .filter((s) => !p.customer || s.customer_id === p.customer)
        .sort((a, b) => b.number - a.number)
        .slice(0, p.limit ?? 50);
      return {
        value: {
          sales: rows.map((s) => saleView(s, access, false)),
          next: null,
        },
      };
    }
    case "crm_get_sale": {
      requireMember(access);
      const sale = sales.find((row) => row.id === p.saleId);
      if (!sale) throw "relay error 404: sale not found";
      return { value: saleView(sale, access, true) };
    }
    case "crm_post_sale":
      return { value: postSale(p, access) };
    case "crm_void_sale":
      return { value: voidSale(p, access) };
    case "crm_get_analytics":
      return { value: analytics(p, access) };
    default:
      return undefined;
  }
}
