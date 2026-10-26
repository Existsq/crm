import assert from "node:assert/strict";
import test from "node:test";

import { formatPercentBp, parsePercentBp, payrollPeriod } from "./payroll.ts";

test("payroll periods cover this and the previous calendar month", () => {
  assert.deepEqual(payrollPeriod("month", "2026-10-25"), {
    from: "2026-10-01",
    to: "2026-10-25",
  });
  assert.deepEqual(payrollPeriod("prevMonth", "2026-10-25"), {
    from: "2026-09-01",
    to: "2026-09-30",
  });
  assert.deepEqual(payrollPeriod("prevMonth", "2026-01-03"), {
    from: "2025-12-01",
    to: "2025-12-31",
  });
});

test("percents parse to basis points and format back", () => {
  assert.deepEqual(parsePercentBp("40"), { ok: true, bp: 4000 });
  assert.deepEqual(parsePercentBp("12,5"), { ok: true, bp: 1250 });
  assert.deepEqual(parsePercentBp("0.05 %"), { ok: true, bp: 5 });
  assert.deepEqual(parsePercentBp(""), { ok: true, bp: 0 });
  assert.deepEqual(parsePercentBp("100"), { ok: true, bp: 10_000 });
  assert.deepEqual(parsePercentBp("100,01"), { ok: false, reason: "range" });
  assert.deepEqual(parsePercentBp("1,234"), { ok: false, reason: "format" });
  assert.deepEqual(parsePercentBp("-5"), { ok: false, reason: "format" });
  assert.equal(formatPercentBp(4000), "40");
  assert.equal(formatPercentBp(1250), "12,5");
  assert.equal(formatPercentBp(5), "0,05");
  assert.equal(formatPercentBp(0), "0");
});
