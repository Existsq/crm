import { ru } from "@/features/crm/i18n/ru";
import { isWholeOnlyUnit } from "@/features/crm/lib/catalog";
import {
  formatQty,
  formatRublesInput,
  lineAmount,
  parseQty,
  parseRubles,
} from "@/features/crm/lib/money";
import type { CrmProduct } from "@/shared/api/tauriCrmProducts";
import type {
  CrmPaymentMethod,
  CrmSaleLineInput,
} from "@/shared/api/tauriCrmSales";

/** Payment methods in form order. */
export const CRM_PAYMENT_METHODS: readonly CrmPaymentMethod[] = [
  "cash",
  "card",
  "transfer",
];

/** Lines per sale (`buzz_crm::sale::MAX_SALE_LINES`). */
export const MAX_SALE_LINES = 200;

/** One editable line: the product as picked and its fields as typed. */
export type SaleLineDraft = {
  product: Pick<CrmProduct, "id" | "name" | "unit" | "priceMinor">;
  qty: string;
  price: string;
  discount: string;
};

export type SaleLineErrors = Partial<
  Record<"qty" | "price" | "discount", string>
>;

/** A line with its parsed values, or the reasons it cannot be submitted. */
export type EvaluatedLine =
  | {
      ok: true;
      line: CrmSaleLineInput;
      grossMinor: number;
      amountMinor: number;
    }
  | { ok: false; errors: SaleLineErrors };

export function newSaleLine(product: SaleLineDraft["product"]): SaleLineDraft {
  return {
    product,
    qty: formatQty(1000),
    price: formatRublesInput(product.priceMinor),
    discount: "",
  };
}

/** Add `product`, or bump its quantity by one unit when already listed. */
export function addProductToLines(
  lines: SaleLineDraft[],
  product: SaleLineDraft["product"],
): SaleLineDraft[] {
  const index = lines.findIndex((line) => line.product.id === product.id);
  if (index === -1) return [...lines, newSaleLine(product)];
  return lines.map((line, i) => {
    if (i !== index) return line;
    const qty = parseQty(line.qty, { wholeOnly: false });
    return qty.ok ? { ...line, qty: formatQty(qty.milli + 1000) } : line;
  });
}

export function evaluateLine(draft: SaleLineDraft): EvaluatedLine {
  const errors: SaleLineErrors = {};
  const qty = parseQty(draft.qty, {
    wholeOnly: isWholeOnlyUnit(draft.product.unit),
  });
  const price = parseRubles(draft.price);
  const discount =
    draft.discount.trim() === ""
      ? ({ ok: true, minor: 0 } as const)
      : parseRubles(draft.discount);
  if (!qty.ok || qty.milli <= 0) {
    errors.qty =
      !qty.ok && qty.reason === "whole"
        ? ru.sales.validation.qtyWhole
        : ru.sales.validation.qty;
  }
  if (!price.ok) errors.price = ru.sales.validation.price;
  if (!discount.ok) errors.discount = ru.sales.validation.discountFormat;
  if (!qty.ok || qty.milli <= 0 || !price.ok || !discount.ok) {
    return { ok: false, errors };
  }
  let grossMinor: number;
  try {
    grossMinor = lineAmount(price.minor, qty.milli);
  } catch {
    return { ok: false, errors: { price: ru.sales.validation.price } };
  }
  if (discount.minor > grossMinor) {
    return { ok: false, errors: { discount: ru.sales.validation.discount } };
  }
  return {
    ok: true,
    grossMinor,
    amountMinor: grossMinor - discount.minor,
    line: {
      productId: draft.product.id,
      qtyMilli: qty.milli,
      priceMinor: price.minor,
      discountMinor: discount.minor,
    },
  };
}

export type SaleTotals = {
  grossMinor: number;
  discountMinor: number;
  totalMinor: number;
};

/** Totals over the lines that currently parse; invalid lines count as 0. */
export function saleTotals(lines: SaleLineDraft[]): SaleTotals {
  let grossMinor = 0;
  let totalMinor = 0;
  for (const draft of lines) {
    const evaluated = evaluateLine(draft);
    if (!evaluated.ok) continue;
    grossMinor += evaluated.grossMinor;
    totalMinor += evaluated.amountMinor;
  }
  return { grossMinor, discountMinor: grossMinor - totalMinor, totalMinor };
}
