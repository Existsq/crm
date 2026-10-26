import { MAX_PERCENT_BP } from "@/features/crm/lib/payroll";
import { parseRubles } from "@/features/crm/lib/money";

/** The space's bonus programme (`GET /crm/loyalty`). */
export type LoyaltyProgramme = {
  enabled: boolean;
  /** Bonuses earned, basis points of the amount paid. */
  cashbackBp: number;
  /** Most of a sale payable with bonuses, basis points. */
  maxRedeemBp: number;
};

function share(amountMinor: number, bp: number): number {
  return Math.floor((Math.max(0, amountMinor) * bp) / MAX_PERCENT_BP);
}

/** Bonuses a sale paying `paidMinor` earns (the relay rounds down too). */
export function cashbackMinor(
  programme: LoyaltyProgramme,
  paidMinor: number,
): number {
  return programme.enabled ? share(paidMinor, programme.cashbackBp) : 0;
}

/**
 * Most bonuses spendable on a sale worth `baseMinor` before bonuses: the
 * programme's share, never above the customer's balance.
 */
export function maxRedeemMinor(
  programme: LoyaltyProgramme,
  baseMinor: number,
  balanceMinor: number,
): number {
  if (!programme.enabled) return 0;
  return Math.max(
    0,
    Math.min(share(baseMinor, programme.maxRedeemBp), balanceMinor),
  );
}

type DiscountableLine = { discountMinor: number };

/**
 * Spread `bonusMinor` over the lines as extra discount, in proportion to
 * each line's amount after its own discount, so the sale total drops by
 * exactly the bonus. `amounts[i]` is line `i`'s amount after discount;
 * returns `null` when the lines cannot absorb the bonus.
 */
export function spreadBonus<T extends DiscountableLine>(
  lines: readonly T[],
  amounts: readonly number[],
  bonusMinor: number,
): T[] | null {
  const total = amounts.reduce((sum, amount) => sum + amount, 0);
  if (bonusMinor <= 0) return [...lines];
  if (bonusMinor > total || lines.length !== amounts.length) return null;
  const shares = amounts.map((amount) =>
    Math.floor((amount * bonusMinor) / total),
  );
  let left = bonusMinor - shares.reduce((sum, value) => sum + value, 0);
  // Hand the rounding remainder out one kopeck at a time to lines with room.
  for (let i = 0; left > 0 && i < lines.length; i += 1) {
    const room = amounts[i] - shares[i];
    const extra = Math.min(room, left);
    shares[i] += extra;
    left -= extra;
  }
  return lines.map((line, i) => ({
    ...line,
    discountMinor: line.discountMinor + shares[i],
  }));
}

/** Rubles with an optional leading minus as signed kopecks; `null` if invalid or zero. */
export function parseSignedRubles(text: string): number | null {
  const trimmed = text.trim();
  const negative = /^[-−–]/.test(trimmed);
  const parsed = parseRubles(negative ? trimmed.slice(1) : trimmed);
  if (!parsed.ok || parsed.minor === 0) return null;
  return negative ? -parsed.minor : parsed.minor;
}
