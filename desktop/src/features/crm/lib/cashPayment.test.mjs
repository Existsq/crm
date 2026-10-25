import assert from "node:assert/strict";
import test from "node:test";

import { evaluateCashPayment, newCashPayment } from "./cashPayment.ts";

const TOTAL = 123_450; // 1 234,50 ₽

test("cash with nothing typed is exactly the total and no change", () => {
  const result = evaluateCashPayment(newCashPayment(), TOTAL);
  assert.equal(result.ok, true);
  assert.equal(result.payment, "cash");
  assert.deepEqual(result.payments, []);
  assert.equal(result.cashTenderedMinor, null);
  assert.equal(result.changeMinor, 0);
});

test("cash handed over gives the change", () => {
  const result = evaluateCashPayment(
    { ...newCashPayment(), tendered: "2 000" },
    TOTAL,
  );
  assert.equal(result.ok, true);
  assert.equal(result.cashTenderedMinor, 200_000);
  assert.equal(result.changeMinor, 76_550);
});

test("cash handed over below the total is rejected", () => {
  const result = evaluateCashPayment(
    { ...newCashPayment(), tendered: "1 000" },
    TOTAL,
  );
  assert.equal(result.ok, false);
  assert.ok(result.errors.tendered);
});

test("card pays the whole total and never sends tendered cash", () => {
  const result = evaluateCashPayment(
    { mode: "card", tendered: "5 000", cashPart: "" },
    TOTAL,
  );
  assert.equal(result.ok, true);
  assert.equal(result.payment, "card");
  assert.equal(result.cashTenderedMinor, null);
  assert.equal(result.cardMinor, TOTAL);
});

test("mixed splits into two positive parts that sum to the total", () => {
  const result = evaluateCashPayment(
    { mode: "mixed", tendered: "500", cashPart: "234,50" },
    TOTAL,
  );
  assert.equal(result.ok, true);
  assert.deepEqual(result.payments, [
    { method: "cash", amountMinor: 23_450 },
    { method: "card", amountMinor: 100_000 },
  ]);
  assert.equal(
    result.payments.reduce((sum, part) => sum + part.amountMinor, 0),
    TOTAL,
  );
  assert.equal(result.changeMinor, 50_000 - 23_450);
});

test("a mixed cash part of zero or the whole total is rejected", () => {
  for (const cashPart of ["0", "1 234,50", "2000", "", "abc"]) {
    const result = evaluateCashPayment(
      { mode: "mixed", tendered: "", cashPart },
      TOTAL,
    );
    assert.equal(result.ok, false, cashPart);
    assert.ok(result.errors.cashPart, cashPart);
  }
});
