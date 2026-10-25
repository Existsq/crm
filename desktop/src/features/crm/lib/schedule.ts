import { ru } from "@/features/crm/i18n/ru";
import { type CrmWorkInterval, formatClock } from "@/shared/api/tauriCrmStaff";

/** Rules of `buzz_crm::staff::SchedulePayload`. */
export const SCHEDULE_STEP_MIN = 5;
export const DAY_MINUTES = 24 * 60;
export const MAX_DAY_INTERVALS = 4;
export const MAX_SCHEDULE_DAYS = 93;
export const MAX_TEMPLATE_WEEKS = 13;
export const MAX_SHIFT_DAYS = 14;

export { formatClock };

/** A calendar date as `YYYY-MM-DD`, read in local time. */
export function toIsoDate(date: Date): string {
  const y = date.getFullYear();
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/** Local midnight of a `YYYY-MM-DD` date. */
export function fromIsoDate(iso: string): Date {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y, m - 1, d);
}

export function addDays(iso: string, days: number): string {
  const date = fromIsoDate(iso);
  date.setDate(date.getDate() + days);
  return toIsoDate(date);
}

/** Monday 0 … Sunday 6. */
export function weekdayIndex(iso: string): number {
  return (fromIsoDate(iso).getDay() + 6) % 7;
}

/** The Monday of the week that holds `iso`. */
export function weekStart(iso: string): string {
  return addDays(iso, -weekdayIndex(iso));
}

export function weekDays(monday: string): string[] {
  return Array.from({ length: 7 }, (_, i) => addDays(monday, i));
}

/** `HH:MM` (00:00–24:00) as minutes after midnight, `null` if malformed. */
export function parseClock(raw: string): number | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec(raw.trim());
  if (!match) return null;
  const hours = Number(match[1]);
  const minutes = Number(match[2]);
  const total = hours * 60 + minutes;
  if (minutes >= 60 || total > DAY_MINUTES) return null;
  return total;
}

export function formatIntervals(intervals: readonly CrmWorkInterval[]): string {
  return intervals
    .map((i) => `${formatClock(i.start)}–${formatClock(i.end)}`)
    .join(", ");
}

/** Working minutes of a day. */
export function workedMinutes(intervals: readonly CrmWorkInterval[]): number {
  return intervals.reduce((sum, i) => sum + (i.end - i.start), 0);
}

export type IntervalDraft = { start: string; end: string };

export type IntervalsResult =
  | { ok: true; intervals: CrmWorkInterval[] }
  | { ok: false; error: string; index: number };

/** Validate typed intervals with the relay's rules; they come back sorted. */
export function validateIntervals(
  drafts: readonly IntervalDraft[],
): IntervalsResult {
  const v = ru.schedule.validation;
  const parsed: Array<CrmWorkInterval & { index: number }> = [];
  for (const [index, draft] of drafts.entries()) {
    const start = parseClock(draft.start);
    const end = parseClock(draft.end);
    if (
      start === null ||
      end === null ||
      start % SCHEDULE_STEP_MIN !== 0 ||
      end % SCHEDULE_STEP_MIN !== 0
    ) {
      return { ok: false, error: v.time, index };
    }
    if (end <= start) return { ok: false, error: v.order, index };
    parsed.push({ start, end, index });
  }
  parsed.sort((a, b) => a.start - b.start);
  for (let i = 1; i < parsed.length; i += 1) {
    if (parsed[i].start < parsed[i - 1].end) {
      return { ok: false, error: v.overlap, index: parsed[i].index };
    }
  }
  return {
    ok: true,
    intervals: parsed.map(({ start, end }) => ({ start, end })),
  };
}

export type SchedulePattern =
  | { type: "weekdays"; weekdays: readonly number[] }
  | { type: "shift"; work: number; off: number };

export type ScheduleTemplate = {
  from: string;
  weeks: number;
  pattern: SchedulePattern;
  start: string;
  end: string;
  breakStart: string;
  breakEnd: string;
};

export type TemplateField =
  | "weekdays"
  | "work"
  | "off"
  | "from"
  | "weeks"
  | "start"
  | "end"
  | "break";

export type TemplateResult =
  | {
      ok: true;
      days: Array<{ date: string; intervals: CrmWorkInterval[] }>;
      workDays: number;
    }
  | { ok: false; errors: Partial<Record<TemplateField, string>> };

/**
 * Expand a template into days: working days get the day's hours (split by
 * the break), the others become days off. At most 13 weeks, one event.
 */
export function buildTemplateDays(template: ScheduleTemplate): TemplateResult {
  const v = ru.schedule.validation;
  const errors: Partial<Record<TemplateField, string>> = {};
  if (!/^\d{4}-\d{2}-\d{2}$/.test(template.from)) errors.from = v.from;
  if (
    !Number.isInteger(template.weeks) ||
    template.weeks < 1 ||
    template.weeks > MAX_TEMPLATE_WEEKS
  ) {
    errors.weeks = v.weeks;
  }
  const { pattern } = template;
  if (pattern.type === "weekdays" && pattern.weekdays.length === 0) {
    errors.weekdays = v.weekdays;
  }
  if (pattern.type === "shift") {
    const bad = (n: number) =>
      !Number.isInteger(n) || n < 1 || n > MAX_SHIFT_DAYS;
    if (bad(pattern.work)) errors.work = v.shift;
    if (bad(pattern.off)) errors.off = v.shift;
  }
  const day = validateIntervals([{ start: template.start, end: template.end }]);
  if (!day.ok) {
    if (parseClock(template.start) === null) errors.start = day.error;
    else errors.end = day.error;
  }
  let intervals: CrmWorkInterval[] = day.ok ? day.intervals : [];
  const hasBreak = template.breakStart.trim() || template.breakEnd.trim();
  if (hasBreak && day.ok) {
    const pause = validateIntervals([
      { start: template.breakStart, end: template.breakEnd },
    ]);
    const [work] = day.intervals;
    if (!template.breakStart.trim() || !template.breakEnd.trim()) {
      errors.break = v.breakBoth;
    } else if (!pause.ok) {
      errors.break = pause.error;
    } else {
      const [gap] = pause.intervals;
      if (gap.start <= work.start || gap.end >= work.end) {
        errors.break = v.breakInside;
      } else {
        intervals = [
          { start: work.start, end: gap.start },
          { start: gap.end, end: work.end },
        ];
      }
    }
  }
  if (Object.keys(errors).length > 0) return { ok: false, errors };

  const count = template.weeks * 7;
  const days = Array.from({ length: count }, (_, offset) => {
    const date = addDays(template.from, offset);
    const working =
      pattern.type === "weekdays"
        ? pattern.weekdays.includes(weekdayIndex(date))
        : offset % (pattern.work + pattern.off) < pattern.work;
    return { date, intervals: working ? intervals : [] };
  });
  return {
    ok: true,
    days,
    workDays: days.filter((d) => d.intervals.length > 0).length,
  };
}
