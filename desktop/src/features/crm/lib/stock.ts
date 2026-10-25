import { ru } from "@/features/crm/i18n/ru";
import { isWholeOnlyUnit } from "@/features/crm/lib/catalog";
import {
  formatQty,
  formatRublesInput,
  lineAmount,
  parseQty,
  parseRubles,
  QTY_SCALE,
} from "@/features/crm/lib/money";
import type { CrmProduct, CrmUnit } from "@/shared/api/tauriCrmProducts";
import type {
  CrmStockDocLineInput,
  CrmStockDocType,
  CrmStockItem,
} from "@/shared/api/tauriCrmStock";

/** Document types in menu order. */
export const CRM_STOCK_DOC_TYPES: readonly CrmStockDocType[] = [
  "receipt",
  "write_off",
  "inventory",
  "transfer",
];

/** Lines per document (`buzz_crm::stock::MAX_STOCK_DOC_LINES`). */
export const MAX_STOCK_DOC_LINES = 500;
/** Consumables per tech card (`buzz_crm::stock::MAX_TECH_CARD_ITEMS`). */
export const MAX_TECH_CARD_ITEMS = 50;

/** Fiscal types kept in stock (`buzz_crm::stock::is_stocked`). */
export function isStockedProduct(
  product: Pick<CrmProduct, "paymentObject">,
): boolean {
  return (
    product.paymentObject === "commodity" || product.paymentObject === "excise"
  );
}

/** One editable document line: the product as picked and its fields as typed. */
export type StockLineDraft = {
  product: { id: string; name: string; unit: CrmUnit };
  qty: string;
  cost: string;
  /** Book quantity on the document's warehouse, when known (inventories). */
  onHandMilli: number | null;
};

export type StockLineErrors = Partial<Record<"qty" | "cost", string>>;

export type EvaluatedStockLine =
  | { ok: true; line: CrmStockDocLineInput; amountMinor: number }
  | { ok: false; errors: StockLineErrors };

export function newStockLine(
  type: CrmStockDocType,
  product: StockLineDraft["product"],
  onHandMilli: number | null,
  lastCostMinor: number | null,
): StockLineDraft {
  return {
    product,
    // An inventory starts from the book quantity, so only differences are typed.
    qty: formatQty(
      type === "inventory" ? Math.max(onHandMilli ?? 0, 0) : QTY_SCALE,
    ),
    cost:
      type === "receipt" && lastCostMinor !== null
        ? formatRublesInput(lastCostMinor)
        : "",
    onHandMilli,
  };
}

/** Add `product`, or bump its quantity by one unit when already listed. */
export function addProductToStockLines(
  lines: StockLineDraft[],
  draft: StockLineDraft,
): StockLineDraft[] {
  const index = lines.findIndex((line) => line.product.id === draft.product.id);
  if (index === -1) return [...lines, draft];
  return lines.map((line, i) => {
    if (i !== index) return line;
    const qty = parseQty(line.qty, { wholeOnly: false });
    return qty.ok ? { ...line, qty: formatQty(qty.milli + QTY_SCALE) } : line;
  });
}

export function evaluateStockLine(
  type: CrmStockDocType,
  draft: StockLineDraft,
): EvaluatedStockLine {
  const validation = ru.stock.validation;
  const qty = parseQty(draft.qty, {
    wholeOnly: isWholeOnlyUnit(draft.product.unit),
  });
  const zeroAllowed = type === "inventory";
  const errors: StockLineErrors = {};
  if (!qty.ok || (!zeroAllowed && qty.milli <= 0)) {
    errors.qty =
      !qty.ok && qty.reason === "whole"
        ? validation.qtyWhole
        : zeroAllowed
          ? validation.counted
          : validation.qty;
  }
  let costMinor: number | null = null;
  if (type === "receipt") {
    const cost = parseRubles(draft.cost);
    if (cost.ok) costMinor = cost.minor;
    else errors.cost = validation.cost;
  }
  if (!qty.ok || errors.qty || errors.cost) return { ok: false, errors };
  let amountMinor = 0;
  if (costMinor !== null) {
    try {
      amountMinor = lineAmount(costMinor, qty.milli);
    } catch {
      return { ok: false, errors: { cost: validation.cost } };
    }
  }
  return {
    ok: true,
    amountMinor,
    line: { productId: draft.product.id, qtyMilli: qty.milli, costMinor },
  };
}

/** Counted minus book quantity of an inventory line, when both are known. */
export function inventoryDifference(draft: StockLineDraft): number | null {
  if (draft.onHandMilli === null) return null;
  const qty = parseQty(draft.qty, { wholeOnly: false });
  return qty.ok ? qty.milli - draft.onHandMilli : null;
}

/** Average cost of one unit in kopecks, rounded half-up; null without stock. */
export function averageCostMinor(
  item: Pick<CrmStockItem, "qtyMilli" | "valueMinor">,
): number | null {
  if (item.qtyMilli <= 0) return null;
  const raw = BigInt(item.valueMinor) * BigInt(QTY_SCALE);
  const qty = BigInt(item.qtyMilli);
  return Number((raw + qty / 2n) / qty);
}

/** Running low: at or below a configured minimum. */
export function isLowStock(
  item: Pick<CrmStockItem, "qtyMilli" | "minStockMilli">,
): boolean {
  return item.minStockMilli > 0 && item.qtyMilli <= item.minStockMilli;
}

/** Thousandths with the short unit name (`1,5` + kilogram); signed deltas get `+`. */
export function formatStockQty(
  milli: number,
  unit: CrmUnit,
  options: { signed?: boolean } = {},
): string {
  const sign = options.signed && milli > 0 ? "+" : "";
  return `${sign}${formatQty(milli).replace("-", "−")} ${ru.units.short[unit]}`;
}

/** One editable tech-card row. */
export type TechCardDraft = {
  product: { id: string; name: string; unit: CrmUnit };
  qty: string;
};

export type EvaluatedTechCardItem =
  | { ok: true; item: { productId: string; qtyMilli: number } }
  | { ok: false; error: string };

export function evaluateTechCardItem(
  draft: TechCardDraft,
): EvaluatedTechCardItem {
  const qty = parseQty(draft.qty, {
    wholeOnly: isWholeOnlyUnit(draft.product.unit),
  });
  if (!qty.ok || qty.milli <= 0) {
    return {
      ok: false,
      error:
        !qty.ok && qty.reason === "whole"
          ? ru.stock.validation.qtyWhole
          : ru.stock.validation.qty,
    };
  }
  return {
    ok: true,
    item: { productId: draft.product.id, qtyMilli: qty.milli },
  };
}
