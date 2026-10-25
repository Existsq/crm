import type { CrmRevenueBucket } from "@/shared/api/tauriCrmSales";

/** Period presets of the analytics screen. */
export type AnalyticsPreset = "today" | "d7" | "d30" | "d90" | "month";

export const ANALYTICS_PRESETS: readonly AnalyticsPreset[] = [
  "today",
  "d7",
  "d30",
  "d90",
  "month",
];

/** Today's date (`YYYY-MM-DD`) in an IANA timezone. */
export function todayIn(timezone: string, now: Date = new Date()): string {
  try {
    return new Intl.DateTimeFormat("en-CA", { timeZone: timezone }).format(now);
  } catch {
    return now.toISOString().slice(0, 10);
  }
}

/** `YYYY-MM-DD` shifted by `days` (calendar arithmetic in UTC). */
export function addDays(day: string, days: number): string {
  const date = new Date(`${day}T00:00:00Z`);
  date.setUTCDate(date.getUTCDate() + days);
  return date.toISOString().slice(0, 10);
}

/** Inclusive `{from, to}` days of a preset ending `today`. */
export function presetPeriod(
  preset: AnalyticsPreset,
  today: string,
): { from: string; to: string } {
  switch (preset) {
    case "today":
      return { from: today, to: today };
    case "d7":
      return { from: addDays(today, -6), to: today };
    case "d30":
      return { from: addDays(today, -29), to: today };
    case "d90":
      return { from: addDays(today, -89), to: today };
    case "month":
      return { from: `${today.slice(0, 8)}01`, to: today };
  }
}

/** Every day `from..=to` with its bucket, zero for days without sales. */
export function fillDays(
  from: string,
  to: string,
  buckets: readonly CrmRevenueBucket<string>[],
): CrmRevenueBucket<string>[] {
  const byKey = new Map(buckets.map((bucket) => [bucket.key, bucket]));
  const days: CrmRevenueBucket<string>[] = [];
  for (let day = from; day <= to; day = addDays(day, 1)) {
    days.push(byKey.get(day) ?? { key: day, revenueMinor: 0, salesCount: 0 });
    if (days.length > 400) break;
  }
  return days;
}

/** `part / whole` as a whole percentage (0 when `whole` is 0). */
export function percent(part: number, whole: number): number {
  return whole > 0 ? Math.round((part / whole) * 100) : 0;
}

/** The period of the same length right before `period` (for comparison). */
export function previousPeriod(period: { from: string; to: string }): {
  from: string;
  to: string;
} {
  const from = new Date(`${period.from}T00:00:00Z`).getTime();
  const to = new Date(`${period.to}T00:00:00Z`).getTime();
  const days = Math.round((to - from) / 86_400_000) + 1;
  return { from: addDays(period.from, -days), to: addDays(period.from, -1) };
}

/**
 * Relative change from `previous` to `current` in whole percent, or `null`
 * when there is nothing to compare against.
 */
export function changePercent(
  current: number,
  previous: number,
): number | null {
  if (previous <= 0) return null;
  return Math.round(((current - previous) / previous) * 100);
}

/**
 * Axis ticks from zero to a round maximum at or above `max`, e.g. `0, 5000,
 * 10000, 15000` for 13 400. Always three gaps, so grid lines stay sparse.
 */
export function niceTicks(max: number): number[] {
  if (max <= 0) return [0];
  const rough = max / 3;
  const magnitude = 10 ** Math.floor(Math.log10(rough));
  const step =
    [1, 2, 2.5, 5, 10].map((m) => m * magnitude).find((s) => s >= rough) ??
    10 * magnitude;
  return [0, step, step * 2, step * 3];
}
