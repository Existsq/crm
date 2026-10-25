import { ru } from "@/features/crm/i18n/ru";
import {
  formatQty,
  formatRublesInput,
  parseQty,
  parseRubles,
} from "@/features/crm/lib/money";
import type { CrmVatRate } from "@/shared/api/tauriCrm";
import type {
  CrmPaymentObject,
  CrmProduct,
  CrmProductFields,
  CrmUnit,
} from "@/shared/api/tauriCrmProducts";

/** Units in display order (`buzz_crm::Unit::ALL`). */
export const CRM_UNITS: readonly CrmUnit[] = [
  "piece",
  "kilogram",
  "gram",
  "liter",
  "milliliter",
  "meter",
  "square_meter",
  "hour",
  "service_unit",
];

/** Fiscal item types in display order (`buzz_crm::PaymentObject::ALL`). */
export const CRM_PAYMENT_OBJECTS: readonly CrmPaymentObject[] = [
  "commodity",
  "excise",
  "job",
  "service",
  "another",
];

/** VAT rates the relay accepts on new product revisions. */
export const CRM_PRODUCT_VAT_CHOICES: readonly CrmVatRate[] = [
  "none",
  "vat0",
  "vat5",
  "vat7",
  "vat10",
  "vat22",
  "vat105",
  "vat107",
  "vat110",
  "vat122",
];

export const MAX_BARCODES = 20;
const SKU_PATTERN = /^[A-Za-z0-9._/-]{1,64}$/;

/** Units sold in whole numbers only (`Unit::whole_only`). */
export function isWholeOnlyUnit(unit: CrmUnit): boolean {
  return unit === "piece" || unit === "service_unit";
}

/** The 20 % rates survive only on historical data. */
export function isHistoricalVat(rate: CrmVatRate): boolean {
  return rate === "vat20" || rate === "vat120";
}

/**
 * VAT options for the product form. A historical rate already on the product
 * is shown first so the form never silently changes it.
 */
export function productVatOptions(current: CrmVatRate): CrmVatRate[] {
  return CRM_PRODUCT_VAT_CHOICES.includes(current)
    ? [...CRM_PRODUCT_VAT_CHOICES]
    : [current, ...CRM_PRODUCT_VAT_CHOICES];
}

/** Split a barcode field on commas, semicolons and whitespace; drop repeats. */
export function parseBarcodes(text: string): string[] {
  const codes = text.split(/[\s,;]+/).filter((code) => code.length > 0);
  return [...new Set(codes)];
}

/** The product form as typed: numbers stay text until validation. */
export type ProductDraft = {
  name: string;
  sku: string;
  barcodes: string;
  unit: CrmUnit;
  price: string;
  vat: CrmVatRate;
  paymentObject: CrmPaymentObject;
  category: string;
  minStock: string;
  requiresMarking: boolean;
  archived: boolean;
};

export type ProductDraftField =
  | "name"
  | "sku"
  | "barcodes"
  | "price"
  | "vat"
  | "minStock";

/** Invalid fields in form order, for focusing the first one. */
export const PRODUCT_DRAFT_FIELDS: readonly ProductDraftField[] = [
  "name",
  "sku",
  "price",
  "vat",
  "minStock",
  "barcodes",
];

export type ProductDraftErrors = Partial<Record<ProductDraftField, string>>;

export type ProductDraftResult =
  | { ok: true; fields: CrmProductFields }
  | { ok: false; errors: ProductDraftErrors };

export function emptyProductDraft(defaultVat: CrmVatRate): ProductDraft {
  return {
    name: "",
    sku: "",
    barcodes: "",
    unit: "piece",
    price: "",
    vat: isHistoricalVat(defaultVat) ? "vat22" : defaultVat,
    paymentObject: "commodity",
    category: "",
    minStock: "0",
    requiresMarking: false,
    archived: false,
  };
}

export function productDraftFrom(product: CrmProduct): ProductDraft {
  return {
    name: product.name,
    sku: product.sku ?? "",
    barcodes: product.barcodes.join("\n"),
    unit: product.unit,
    price: formatRublesInput(product.priceMinor),
    vat: product.vat,
    paymentObject: product.paymentObject,
    category: product.category ?? "",
    minStock: formatQty(product.minStockMilli),
    requiresMarking: product.requiresMarking,
    archived: product.archived,
  };
}

export function sameProductDraft(a: ProductDraft, b: ProductDraft): boolean {
  return (Object.keys(a) as Array<keyof ProductDraft>).every(
    (key) => a[key] === b[key],
  );
}

/** Validate the draft with the relay's rules; money never touches a float. */
export function validateProductDraft(draft: ProductDraft): ProductDraftResult {
  const errors: ProductDraftErrors = {};
  const name = draft.name.trim();
  if (!name) errors.name = ru.errors.nameRequired;

  const sku = draft.sku.trim();
  if (sku && !SKU_PATTERN.test(sku)) errors.sku = ru.products.validation.sku;

  const barcodes = parseBarcodes(draft.barcodes);
  if (barcodes.length > MAX_BARCODES) {
    errors.barcodes = ru.products.validation.barcodes;
  }

  const price = parseRubles(draft.price);
  if (!price.ok) errors.price = ru.products.validation.price[price.reason];

  if (isHistoricalVat(draft.vat)) {
    errors.vat = ru.products.validation.historicalVat;
  }

  const minStock = parseQty(draft.minStock.trim() || "0", {
    wholeOnly: isWholeOnlyUnit(draft.unit),
  });
  if (!minStock.ok) {
    errors.minStock = ru.products.validation.minStock[minStock.reason];
  }

  if (!price.ok || !minStock.ok || Object.keys(errors).length > 0) {
    return { ok: false, errors };
  }
  const category = draft.category.trim();
  return {
    ok: true,
    fields: {
      name,
      sku: sku || null,
      barcodes,
      unit: draft.unit,
      priceMinor: price.minor,
      vat: draft.vat,
      paymentObject: draft.paymentObject,
      category: category || null,
      minStockMilli: minStock.milli,
      requiresMarking: draft.requiresMarking,
      archived: draft.archived,
    },
  };
}
