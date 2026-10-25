import { ru } from "@/features/crm/i18n/ru";
import { parseRubles } from "@/features/crm/lib/money";
import type {
  CrmPaymentMethod,
  CrmSalePayment,
} from "@/shared/api/tauriCrmSales";

/** How the customer pays at the cash desk. */
export type CashPaymentMode = "cash" | "card" | "mixed";

export const CASH_PAYMENT_MODES: readonly CashPaymentMode[] = [
  "cash",
  "card",
  "mixed",
];

/** The payment as typed at the cash desk. */
export type CashPaymentDraft = {
  mode: CashPaymentMode;
  /** Cash handed over; blank means exactly the cash part. */
  tendered: string;
  /** Cash part of a mixed payment; the rest goes on the card. */
  cashPart: string;
};

export type CashPaymentErrors = Partial<
  Record<"tendered" | "cashPart", string>
>;

/** The sale fields a payment draft resolves to, plus the change to give. */
export type EvaluatedCashPayment =
  | {
      ok: true;
      payment: CrmPaymentMethod;
      payments: CrmSalePayment[];
      cashTenderedMinor: number | null;
      cashMinor: number;
      cardMinor: number;
      changeMinor: number;
    }
  | { ok: false; errors: CashPaymentErrors };

export function newCashPayment(): CashPaymentDraft {
  return { mode: "cash", tendered: "", cashPart: "" };
}

function tenderedFor(
  text: string,
  cashMinor: number,
): { ok: true; minor: number | null } | { ok: false; error: string } {
  if (text.trim() === "") return { ok: true, minor: null };
  const parsed = parseRubles(text);
  if (!parsed.ok) return { ok: false, error: ru.cash.validation.tendered };
  if (parsed.minor < cashMinor) {
    return { ok: false, error: ru.cash.validation.tenderedShort };
  }
  return { ok: true, minor: parsed.minor };
}

/**
 * Resolve the payment for a sale of `totalMinor` kopecks. A mixed payment
 * needs a cash part strictly between zero and the total, so both parts are
 * positive as the relay requires (`SalePayload::check_payments`).
 */
export function evaluateCashPayment(
  draft: CashPaymentDraft,
  totalMinor: number,
): EvaluatedCashPayment {
  if (draft.mode === "card") {
    return {
      ok: true,
      payment: "card",
      payments: [],
      cashTenderedMinor: null,
      cashMinor: 0,
      cardMinor: totalMinor,
      changeMinor: 0,
    };
  }

  let cashMinor = totalMinor;
  if (draft.mode === "mixed") {
    const part = parseRubles(draft.cashPart);
    if (!part.ok || part.minor <= 0 || part.minor >= totalMinor) {
      return { ok: false, errors: { cashPart: ru.cash.validation.cashPart } };
    }
    cashMinor = part.minor;
  }
  const tendered = tenderedFor(draft.tendered, cashMinor);
  if (!tendered.ok) return { ok: false, errors: { tendered: tendered.error } };

  const cardMinor = totalMinor - cashMinor;
  return {
    ok: true,
    payment: "cash",
    payments:
      draft.mode === "mixed"
        ? [
            { method: "cash", amountMinor: cashMinor },
            { method: "card", amountMinor: cardMinor },
          ]
        : [],
    cashTenderedMinor: tendered.minor,
    cashMinor,
    cardMinor,
    changeMinor: tendered.minor === null ? 0 : tendered.minor - cashMinor,
  };
}
