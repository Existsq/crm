import assert from "node:assert/strict";
import test from "node:test";

import {
  addDays,
  changePercent,
  fillDays,
  niceTicks,
  percent,
  presetPeriod,
  previousPeriod,
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

test("the previous period has the same length and ends the day before", () => {
  assert.deepEqual(previousPeriod({ from: "2026-10-25", to: "2026-10-25" }), {
    from: "2026-10-24",
    to: "2026-10-24",
  });
  assert.deepEqual(previousPeriod({ from: "2026-09-26", to: "2026-10-25" }), {
    from: "2026-08-27",
    to: "2026-09-25",
  });
  assert.deepEqual(previousPeriod({ from: "2026-03-01", to: "2026-03-10" }), {
    from: "2026-02-19",
    to: "2026-02-28",
  });
});

test("change is relative to the previous value and absent without one", () => {
  assert.equal(changePercent(120, 100), 20);
  assert.equal(changePercent(50, 100), -50);
  assert.equal(changePercent(0, 100), -100);
  assert.equal(changePercent(100, 0), null);
});

test("axis ticks are round and cover the maximum", () => {
  assert.deepEqual(niceTicks(0), [0]);
  assert.deepEqual(niceTicks(13_400), [0, 5000, 10_000, 15_000]);
  assert.deepEqual(niceTicks(900_000), [0, 500_000, 1_000_000, 1_500_000]);
  assert.deepEqual(niceTicks(600), [0, 200, 400, 600]);
  for (const max of [1, 7, 99, 1234, 98_765, 3_000_001]) {
    const ticks = niceTicks(max);
    assert.ok(ticks.at(-1) >= max, `covers ${max}`);
    assert.ok(ticks.at(-1) < max * 4, `not far above ${max}`);
  }
});
