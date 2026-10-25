import assert from "node:assert/strict";
import test from "node:test";

import {
  addProductToStockLines,
  averageCostMinor,
  evaluateStockLine,
  evaluateTechCardItem,
  formatStockQty,
  inventoryDifference,
  isLowStock,
  isStockedProduct,
  newStockLine,
} from "./stock.ts";

const shampoo = { id: "s", name: "Шампунь", unit: "piece" };
const dye = { id: "d", name: "Краска", unit: "gram" };

test("new lines start at one unit, inventories at the book quantity", () => {
  assert.deepEqual(newStockLine("receipt", shampoo, 3000, 25050), {
    product: shampoo,
    qty: "1",
    cost: "250,50",
    onHandMilli: 3000,
  });
  assert.equal(newStockLine("inventory", shampoo, 4000, null).qty, "4");
  assert.equal(newStockLine("inventory", shampoo, -2000, null).qty, "0");
  assert.equal(newStockLine("write_off", shampoo, null, 100).cost, "");
});

test("adding a listed product bumps its quantity", () => {
  const lines = [newStockLine("write_off", shampoo, null, null)];
  const bumped = addProductToStockLines(
    lines,
    newStockLine("write_off", shampoo, null, null),
  );
  assert.equal(bumped.length, 1);
  assert.equal(bumped[0].qty, "2");
});

test("receipts need a cost, inventories accept zero, pieces stay whole", () => {
  const line = (qty, cost = "") => ({
    product: shampoo,
    qty,
    cost,
    onHandMilli: 2000,
  });
  const receipt = evaluateStockLine("receipt", line("10", "249,90"));
  assert.equal(receipt.ok, true);
  assert.equal(receipt.amountMinor, 249_900);
  assert.deepEqual(receipt.line, {
    productId: "s",
    qtyMilli: 10_000,
    costMinor: 24_990,
  });
  assert.ok(evaluateStockLine("receipt", line("1")).errors.cost);
  assert.ok(evaluateStockLine("write_off", line("0")).errors.qty);
  assert.match(evaluateStockLine("write_off", line("0,5")).errors.qty, /целое/);
  const count = evaluateStockLine("inventory", line("0"));
  assert.equal(count.ok, true);
  assert.equal(count.line.costMinor, null);
  assert.equal(inventoryDifference(line("0")), -2000);
  assert.equal(inventoryDifference({ ...line("1"), onHandMilli: null }), null);
});

test("average cost, low stock and quantity labels", () => {
  assert.equal(averageCostMinor({ qtyMilli: 3000, valueMinor: 1000 }), 333);
  assert.equal(averageCostMinor({ qtyMilli: 0, valueMinor: 0 }), null);
  assert.equal(isLowStock({ qtyMilli: 2000, minStockMilli: 2000 }), true);
  assert.equal(isLowStock({ qtyMilli: -1000, minStockMilli: 0 }), false);
  assert.equal(formatStockQty(1500, "kilogram"), "1,5 кг");
  assert.equal(formatStockQty(60_000, "gram", { signed: true }), "+60 г");
  assert.equal(formatStockQty(-2000, "piece", { signed: true }), "−2 шт");
  assert.equal(isStockedProduct({ paymentObject: "commodity" }), true);
  assert.equal(isStockedProduct({ paymentObject: "service" }), false);
});

test("tech card quantities are positive and respect the unit", () => {
  assert.deepEqual(evaluateTechCardItem({ product: dye, qty: "0,06" }), {
    ok: true,
    item: { productId: "d", qtyMilli: 60 },
  });
  assert.equal(evaluateTechCardItem({ product: dye, qty: "0" }).ok, false);
  assert.equal(
    evaluateTechCardItem({ product: shampoo, qty: "0,5" }).ok,
    false,
  );
});
