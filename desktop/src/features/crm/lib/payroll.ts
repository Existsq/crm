import { addDays } from "@/features/crm/lib/analyticsPeriod";

/** 100 % in basis points. */
export const MAX_PERCENT_BP = 10_000;

/** Period presets of the payroll screen. */
export type PayrollPreset = "month" | "prevMonth";

export const PAYROLL_PRESETS: readonly PayrollPreset[] = ["month", "prevMonth"];

/** Inclusive days of a payroll preset relative to `today`. */
export function payrollPeriod(
  preset: PayrollPreset,
  today: string,
): { from: string; to: string } {
  const monthStart = `${today.slice(0, 8)}01`;
  if (preset === "month") return { from: monthStart, to: today };
  const prevEnd = addDays(monthStart, -1);
  return { from: `${prevEnd.slice(0, 8)}01`, to: prevEnd };
}

export type PercentParseResult =
  | { ok: true; bp: number }
  | { ok: false; reason: "format" | "range" };

/** `40`, `12,5` or `0.05` percent as basis points; empty means 0. */
export function parsePercentBp(text: string): PercentParseResult {
  const compact = text.replace(/\s|%/g, "");
  if (compact.length === 0) return { ok: true, bp: 0 };
  const match = /^(\d+)(?:[.,](\d{0,2}))?$/.exec(compact);
  if (!match) return { ok: false, reason: "format" };
  const bp =
    Number(match[1]) * 100 + Number((match[2] ?? "").padEnd(2, "0") || "0");
  return bp > MAX_PERCENT_BP
    ? { ok: false, reason: "range" }
    : { ok: true, bp };
}

/** Basis points as a percent for display and inputs: `4000` → `40`, `1250` → `12,5`. */
export function formatPercentBp(bp: number): string {
  const whole = Math.trunc(bp / 100);
  const fraction = String(bp % 100)
    .padStart(2, "0")
    .replace(/0+$/, "");
  return fraction ? `${whole},${fraction}` : String(whole);
}
