import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  formatQty,
  formatRubles,
  formatRublesInput,
  lineAmount,
  MAX_MONEY_MINOR,
  parseQty,
  parseRubles,
} from "./money.ts";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(
    resolve(here, "../../../../../crates/buzz-crm/fixtures/money_vectors.json"),
    "utf8",
  ),
);

/** ru-RU grouping and the currency gap use (narrow) no-break spaces. */
const plainSpaces = (text) => text.replace(/[  ]/g, " ");

test("lineAmount reproduces every relay golden vector", () => {
  assert.ok(vectors.length >= 10, "fixture has vectors");
  for (const { price_minor, qty_milli, amount_minor } of vectors) {
    assert.equal(
      lineAmount(price_minor, qty_milli),
      amount_minor,
      `price ${price_minor} × qty ${qty_milli}`,
    );
  }
});

test("lineAmount rounds half away from zero and guards the bound", () => {
  assert.equal(lineAmount(1, 500), 1);
  assert.equal(lineAmount(1, 499), 0);
  assert.equal(lineAmount(1, -500), -1);
  assert.equal(lineAmount(MAX_MONEY_MINOR, 1000), MAX_MONEY_MINOR);
  assert.throws(() => lineAmount(MAX_MONEY_MINOR, 1001), RangeError);
});

test("parseRubles accepts the spec examples", () => {
  assert.deepEqual(parseRubles("1 234,50"), { ok: true, minor: 123450 });
  assert.deepEqual(parseRubles("1234.5"), { ok: true, minor: 123450 });
  assert.deepEqual(parseRubles("1234"), { ok: true, minor: 123400 });
});

test("parseRubles accepts separators people actually type", () => {
  const cases = [
    ["1 234,50", 123450],
    ["1 234,50 ₽", 123450],
    ["  89,90 ", 8990],
    ["0", 0],
    [",5", 50],
    ["12,", 1200],
    ["0.01", 1],
    ["1 000 000 000", MAX_MONEY_MINOR],
    ["1000000000,00", MAX_MONEY_MINOR],
  ];
  for (const [text, minor] of cases) {
    assert.deepEqual(parseRubles(text), { ok: true, minor }, text);
  }
});

test("parseRubles rejects invalid input with a reason", () => {
  const cases = [
    ["", "empty"],
    ["   ", "empty"],
    ["-1", "negative"],
    ["−5,00", "negative"],
    ["12,345", "precision"],
    ["0.001", "precision"],
    ["abc", "format"],
    ["12a", "format"],
    ["1.234,50", "format"],
    [".", "format"],
    ["+5", "format"],
    ["1e3", "format"],
    ["1000000000,01", "overflow"],
    ["99999999999999999999999", "overflow"],
  ];
  for (const [text, reason] of cases) {
    assert.deepEqual(parseRubles(text), { ok: false, reason }, text);
  }
});

test("formatRubles renders ru-RU rubles exactly", () => {
  const cases = [
    [123450, "1 234,50 ₽"],
    [8990, "89,90 ₽"],
    [0, "0,00 ₽"],
    [5, "0,05 ₽"],
    [MAX_MONEY_MINOR, "1 000 000 000,00 ₽"],
    [-50, "-0,50 ₽"],
    [-123456, "-1 234,56 ₽"],
  ];
  for (const [minor, expected] of cases) {
    assert.equal(plainSpaces(formatRubles(minor)), expected, String(minor));
  }
});

test("formatted and input values round-trip through parseRubles", () => {
  for (const minor of [0, 1, 99, 100, 8990, 123450, MAX_MONEY_MINOR]) {
    assert.deepEqual(parseRubles(formatRubles(minor)), { ok: true, minor });
    assert.deepEqual(parseRubles(formatRublesInput(minor)), {
      ok: true,
      minor,
    });
  }
  assert.equal(formatRublesInput(123450), "1234,50");
});

test("parseQty keeps thousandths and whole-only units", () => {
  assert.deepEqual(parseQty("1,5", { wholeOnly: false }), {
    ok: true,
    milli: 1500,
  });
  assert.deepEqual(parseQty("0.125", { wholeOnly: false }), {
    ok: true,
    milli: 125,
  });
  assert.deepEqual(parseQty("5", { wholeOnly: true }), {
    ok: true,
    milli: 5000,
  });
  assert.deepEqual(parseQty("1,5", { wholeOnly: true }), {
    ok: false,
    reason: "whole",
  });
  assert.deepEqual(parseQty("0,0001", { wholeOnly: false }), {
    ok: false,
    reason: "precision",
  });
  assert.deepEqual(parseQty("-1", { wholeOnly: false }), {
    ok: false,
    reason: "negative",
  });
  assert.deepEqual(parseQty("1 000 001", { wholeOnly: false }), {
    ok: false,
    reason: "overflow",
  });
});

test("formatQty trims trailing zeros and round-trips", () => {
  const cases = [
    [0, "0"],
    [5000, "5"],
    [1500, "1,5"],
    [125, "0,125"],
    [1_000_000_000, "1000000"],
  ];
  for (const [milli, text] of cases) {
    assert.equal(formatQty(milli), text);
    assert.deepEqual(parseQty(text, { wholeOnly: false }), {
      ok: true,
      milli,
    });
  }
});
