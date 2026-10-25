import assert from "node:assert/strict";
import test from "node:test";

import {
  addProductToLines,
  evaluateLine,
  newSaleLine,
  saleTotals,
} from "./saleDraft.ts";

const kefir = { id: "k", name: "Кефир", unit: "piece", priceMinor: 8990 };
const cheese = { id: "c", name: "Сыр", unit: "kilogram", priceMinor: 59990 };

test("a new line starts at one unit at the catalog price", () => {
  assert.deepEqual(newSaleLine(kefir), {
    product: kefir,
    qty: "1",
    price: "89,90",
    discount: "",
  });
});

test("adding a listed product bumps its quantity instead of a second line", () => {
  let lines = addProductToLines([], kefir);
  lines = addProductToLines(lines, cheese);
  lines = addProductToLines(lines, kefir);
  assert.deepEqual(
    lines.map((line) => [line.product.id, line.qty]),
    [
      ["k", "2"],
      ["c", "1"],
    ],
  );
});

test("lines compute amounts like the relay, discount included", () => {
  const line = evaluateLine({
    product: cheese,
    qty: "1,25",
    price: "599,90",
    discount: "",
  });
  assert.equal(line.ok, true);
  // 1.25 × 599.90 = 749.875 → 749.88 (half-up, as buzz_crm::line_amount).
  assert.equal(line.amountMinor, 74_988);
  assert.deepEqual(line.line, {
    productId: "c",
    qtyMilli: 1250,
    priceMinor: 59_990,
    discountMinor: 0,
  });
  const discounted = evaluateLine({
    product: kefir,
    qty: "2",
    price: "89,90",
    discount: "9,80",
  });
  assert.equal(discounted.amountMinor, 17_000);
});

test("invalid lines name the failing field", () => {
  const cases = [
    [{ qty: "0" }, "qty"],
    [{ qty: "1,5" }, "qty"],
    [{ price: "abc" }, "price"],
    [{ discount: "-1" }, "discount"],
    [{ discount: "200" }, "discount"],
  ];
  for (const [patch, field] of cases) {
    const result = evaluateLine({ ...newSaleLine(kefir), ...patch });
    assert.equal(result.ok, false, JSON.stringify(patch));
    assert.ok(result.errors[field], `${JSON.stringify(patch)} → ${field}`);
  }
});

test("totals skip lines that do not parse", () => {
  const totals = saleTotals([
    { ...newSaleLine(kefir), qty: "2", discount: "9,80" },
    { ...newSaleLine(cheese), qty: "oops" },
  ]);
  assert.deepEqual(totals, {
    grossMinor: 17_980,
    discountMinor: 980,
    totalMinor: 17_000,
  });
});
