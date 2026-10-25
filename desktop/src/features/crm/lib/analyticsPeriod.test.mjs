import assert from "node:assert/strict";
import test from "node:test";

import {
  addDays,
  fillDays,
  percent,
  presetPeriod,
  todayIn,
} from "./analyticsPeriod.ts";

test("presets end today and cover whole days", () => {
  const today = "2026-10-25";
  assert.deepEqual(presetPeriod("today", today), { from: today, to: today });
  assert.deepEqual(presetPeriod("d7", today), {
    from: "2026-10-19",
    to: today,
  });
  assert.deepEqual(presetPeriod("d30", today), {
    from: "2026-09-26",
    to: today,
  });
  assert.deepEqual(presetPeriod("month", today), {
    from: "2026-10-01",
    to: today,
  });
  assert.equal(addDays("2026-03-01", -1), "2026-02-28");
});

test("today follows the space timezone, not the computer's", () => {
  // 2026-10-25 20:30 UTC is already the 26th in Vladivostok (UTC+10).
  const now = new Date("2026-10-25T20:30:00Z");
  assert.equal(todayIn("Asia/Vladivostok", now), "2026-10-26");
  assert.equal(todayIn("Europe/Moscow", now), "2026-10-25");
  assert.equal(todayIn("Not/AZone", now), "2026-10-25");
});

test("missing days are filled with zeros", () => {
  const days = fillDays("2026-10-01", "2026-10-03", [
    { key: "2026-10-02", revenueMinor: 500, salesCount: 1 },
  ]);
  assert.deepEqual(
    days.map((day) => [day.key, day.revenueMinor]),
    [
      ["2026-10-01", 0],
      ["2026-10-02", 500],
      ["2026-10-03", 0],
    ],
  );
  assert.equal(percent(1, 3), 33);
  assert.equal(percent(5, 0), 0);
});
