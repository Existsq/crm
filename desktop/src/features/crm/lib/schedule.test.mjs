import assert from "node:assert/strict";
import test from "node:test";

import { ru } from "../i18n/ru.ts";
import {
  addDays,
  buildTemplateDays,
  formatIntervals,
  MAX_SCHEDULE_DAYS,
  MAX_TEMPLATE_WEEKS,
  parseClock,
  validateIntervals,
  weekDays,
  weekStart,
  weekdayIndex,
} from "./schedule.ts";

test("weeks start on Monday and cross month boundaries", () => {
  assert.equal(weekdayIndex("2026-11-02"), 0);
  assert.equal(weekdayIndex("2026-11-08"), 6);
  assert.equal(weekStart("2026-11-08"), "2026-11-02");
  assert.equal(weekStart("2026-11-01"), "2026-10-26");
  assert.deepEqual(weekDays("2026-12-28").slice(3), [
    "2026-12-31",
    "2027-01-01",
    "2027-01-02",
    "2027-01-03",
  ]);
  // Daylight-saving changes do not shift dates.
  assert.equal(addDays("2026-03-28", 2), "2026-03-30");
});

test("clock times parse like the relay", () => {
  assert.equal(parseClock("09:00"), 540);
  assert.equal(parseClock("9:05"), 545);
  assert.equal(parseClock("24:00"), 1440);
  assert.equal(parseClock("24:05"), null);
  assert.equal(parseClock("12:60"), null);
  assert.equal(parseClock("noon"), null);
});

test("intervals are validated, sorted and formatted", () => {
  const ok = validateIntervals([
    { start: "14:00", end: "20:00" },
    { start: "09:00", end: "13:00" },
  ]);
  assert.ok(ok.ok);
  assert.deepEqual(ok.intervals, [
    { start: 540, end: 780 },
    { start: 840, end: 1200 },
  ]);
  assert.equal(formatIntervals(ok.intervals), "09:00–13:00, 14:00–20:00");
  const v = ru.schedule.validation;
  assert.deepEqual(validateIntervals([{ start: "09:02", end: "10:00" }]), {
    ok: false,
    error: v.time,
    index: 0,
  });
  assert.deepEqual(validateIntervals([{ start: "10:00", end: "10:00" }]), {
    ok: false,
    error: v.order,
    index: 0,
  });
  assert.deepEqual(
    validateIntervals([
      { start: "09:00", end: "13:00" },
      { start: "12:00", end: "15:00" },
    ]),
    { ok: false, error: v.overlap, index: 1 },
  );
});

const base = {
  from: "2026-11-02",
  weeks: 2,
  start: "10:00",
  end: "20:00",
  breakStart: "",
  breakEnd: "",
};

test("a weekday template sets working days and days off", () => {
  const result = buildTemplateDays({
    ...base,
    pattern: { type: "weekdays", weekdays: [0, 1, 2, 3, 4] },
  });
  assert.ok(result.ok);
  assert.equal(result.days.length, 14);
  assert.equal(result.workDays, 10);
  assert.deepEqual(result.days[0], {
    date: "2026-11-02",
    intervals: [{ start: 600, end: 1200 }],
  });
  assert.deepEqual(result.days[5], { date: "2026-11-07", intervals: [] });
});

test("a shift template repeats work and rest from the start date", () => {
  const result = buildTemplateDays({
    ...base,
    from: "2026-11-04",
    pattern: { type: "shift", work: 2, off: 2 },
    breakStart: "14:00",
    breakEnd: "15:00",
  });
  assert.ok(result.ok);
  assert.deepEqual(
    result.days.slice(0, 5).map((d) => d.intervals.length > 0),
    [true, true, false, false, true],
  );
  assert.deepEqual(result.days[0].intervals, [
    { start: 600, end: 840 },
    { start: 900, end: 1200 },
  ]);
});

test("template errors name their fields", () => {
  const v = ru.schedule.validation;
  const result = buildTemplateDays({
    ...base,
    weeks: MAX_TEMPLATE_WEEKS + 1,
    pattern: { type: "weekdays", weekdays: [] },
    breakStart: "09:00",
    breakEnd: "11:00",
  });
  assert.equal(result.ok, false);
  assert.deepEqual(result.errors, {
    weeks: v.weeks,
    weekdays: v.weekdays,
    break: v.breakInside,
  });
  const shift = buildTemplateDays({
    ...base,
    pattern: { type: "shift", work: 0, off: 15 },
    end: "09:00",
  });
  assert.deepEqual(shift.errors, { work: v.shift, off: v.shift, end: v.order });
  assert.ok(MAX_TEMPLATE_WEEKS * 7 <= MAX_SCHEDULE_DAYS);
});
