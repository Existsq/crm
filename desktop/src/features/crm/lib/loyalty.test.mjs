import assert from "node:assert/strict";
import test from "node:test";

import {
  cashbackMinor,
  maxRedeemMinor,
  parseSignedRubles,
  spreadBonus,
} from "./loyalty.ts";

const on = { enabled: true, cashbackBp: 500, maxRedeemBp: 3000 };

test("cashback and the redeem cap follow the programme", () => {
  assert.equal(cashbackMinor(on, 199_999), 9_999);
  assert.equal(cashbackMinor({ ...on, enabled: false }, 100_000), 0);
  assert.equal(maxRedeemMinor(on, 200_000, 1_000_000), 60_000);
  assert.equal(maxRedeemMinor(on, 200_000, 25_000), 25_000);
  assert.equal(maxRedeemMinor(on, 200_000, -500), 0);
  assert.equal(maxRedeemMinor({ ...on, enabled: false }, 200_000, 9_999), 0);
});

test("bonuses spread over lines and lower the total exactly", () => {
  const lines = [
    { id: "a", discountMinor: 0 },
    { id: "b", discountMinor: 100 },
    { id: "c", discountMinor: 0 },
  ];
  const amounts = [100_000, 49_900, 1];
  const spread = spreadBonus(lines, amounts, 10_001);
  assert.ok(spread);
  const added = spread.map(
    (line, i) => line.discountMinor - lines[i].discountMinor,
  );
  assert.equal(
    added.reduce((sum, value) => sum + value, 0),
    10_001,
  );
  for (const [i, value] of added.entries()) assert.ok(value <= amounts[i]);
  assert.deepEqual(spreadBonus(lines, amounts, 0), lines);
  assert.equal(spreadBonus(lines, amounts, 150_000), null);
  // Everything paid with bonuses: every line is discounted to zero.
  const all = spreadBonus(lines, amounts, 149_901);
  assert.deepEqual(
    all?.map((line, i) => line.discountMinor - lines[i].discountMinor),
    amounts,
  );
});

test("manual adjustments accept a leading minus", () => {
  assert.equal(parseSignedRubles("200"), 20_000);
  assert.equal(parseSignedRubles("−150,5"), -15_050);
  assert.equal(parseSignedRubles("-1"), -100);
  assert.equal(parseSignedRubles("0"), null);
  assert.equal(parseSignedRubles("abc"), null);
  assert.equal(parseSignedRubles("--5"), null);
});
