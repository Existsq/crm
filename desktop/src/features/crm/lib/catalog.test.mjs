import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { ru } from "../i18n/ru.ts";
import {
  CRM_PAYMENT_OBJECTS,
  CRM_PRODUCT_VAT_CHOICES,
  CRM_UNITS,
  emptyProductDraft,
  emptyServiceDraft,
  formatDuration,
  parseBarcodes,
  productDraftFrom,
  productVatOptions,
  sameProductDraft,
  validateProductDraft,
} from "./catalog.ts";

const here = dirname(fileURLToPath(import.meta.url));
const catalogRs = readFileSync(
  resolve(here, "../../../../../crates/buzz-crm/src/catalog.rs"),
  "utf8",
);

/** Wire names from a `pub const fn as_str` match block in catalog.rs. */
function wireNames(typeName) {
  const pattern = new RegExp(`${typeName}::\\w+ => "(\\w+)"`, "g");
  return [...catalogRs.matchAll(pattern)].map((match) => match[1]);
}

test("units and payment objects match the relay enums in order", () => {
  assert.deepEqual([...CRM_UNITS], wireNames("Unit"));
  assert.deepEqual([...CRM_PAYMENT_OBJECTS], wireNames("PaymentObject"));
  for (const unit of CRM_UNITS) {
    assert.ok(ru.units.short[unit] && ru.units.full[unit], unit);
  }
  for (const object of CRM_PAYMENT_OBJECTS) {
    assert.ok(ru.paymentObjects[object], object);
  }
});

test("historical VAT rates are shown only when already set", () => {
  assert.ok(!CRM_PRODUCT_VAT_CHOICES.includes("vat20"));
  assert.ok(!CRM_PRODUCT_VAT_CHOICES.includes("vat120"));
  assert.deepEqual(productVatOptions("vat10"), [...CRM_PRODUCT_VAT_CHOICES]);
  assert.equal(productVatOptions("vat20")[0], "vat20");
  assert.equal(emptyProductDraft("vat120").vat, "vat22");
  assert.equal(emptyProductDraft("vat10").vat, "vat10");
});

test("barcodes split on commas, semicolons and whitespace without repeats", () => {
  assert.deepEqual(
    parseBarcodes(" 4600051000057,INT-42\n4600051000057;  96385074 \n"),
    ["4600051000057", "INT-42", "96385074"],
  );
  assert.deepEqual(parseBarcodes("   "), []);
});

function draft(patch = {}) {
  return {
    ...emptyProductDraft("vat22"),
    name: "  Молоко 3,2% ",
    price: "89,90",
    ...patch,
  };
}

test("a valid draft becomes trimmed product fields in kopecks", () => {
  const result = validateProductDraft(
    draft({
      sku: " MLK-001 ",
      barcodes: "4600051000057",
      category: "  ",
      minStock: "5",
      price: "1 234,50",
    }),
  );
  assert.deepEqual(result, {
    ok: true,
    fields: {
      name: "Молоко 3,2%",
      sku: "MLK-001",
      barcodes: ["4600051000057"],
      unit: "piece",
      priceMinor: 123450,
      vat: "vat22",
      paymentObject: "commodity",
      category: null,
      minStockMilli: 5000,
      requiresMarking: false,
      archived: false,
      durationMin: null,
    },
  });
});

test("invalid fields are reported and never submitted", () => {
  const result = validateProductDraft(
    draft({
      name: " ",
      sku: "bad sku!",
      barcodes: Array.from({ length: 21 }, (_, i) => `CODE-${i}`).join(","),
      price: "12,345",
      vat: "vat20",
      minStock: "1,5",
    }),
  );
  assert.equal(result.ok, false);
  assert.deepEqual(result.errors, {
    name: ru.errors.nameRequired,
    sku: ru.products.validation.sku,
    barcodes: ru.products.validation.barcodes,
    price: ru.products.validation.price.precision,
    vat: ru.products.validation.historicalVat,
    minStock: ru.products.validation.minStock.whole,
  });
});

test("price reasons map to their own messages", () => {
  for (const [price, reason] of [
    ["", "empty"],
    ["-5", "negative"],
    ["abc", "format"],
    ["2000000000", "overflow"],
  ]) {
    const result = validateProductDraft(draft({ price }));
    assert.equal(result.ok, false);
    assert.equal(result.errors.price, ru.products.validation.price[reason]);
  }
});

test("weighed goods accept fractional minimum stock; blank means zero", () => {
  const weighed = validateProductDraft(
    draft({ unit: "kilogram", minStock: "1,5" }),
  );
  assert.equal(weighed.ok && weighed.fields.minStockMilli, 1500);
  const blank = validateProductDraft(draft({ minStock: " " }));
  assert.equal(blank.ok && blank.fields.minStockMilli, 0);
});

test("a stored product round-trips through the draft unchanged", () => {
  const product = {
    id: "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e",
    name: "Сыр",
    sku: "CHS-1",
    barcodes: ["4600051000057", "INT-42"],
    unit: "kilogram",
    priceMinor: 89900,
    vat: "vat10",
    paymentObject: "commodity",
    category: "Молочные продукты",
    minStockMilli: 2500,
    requiresMarking: true,
    archived: false,
    durationMin: null,
    revision: "ab".repeat(32),
    updatedAt: "2026-09-27T10:00:00Z",
  };
  const fromProduct = productDraftFrom(product);
  assert.ok(sameProductDraft(fromProduct, productDraftFrom(product)));
  assert.ok(!sameProductDraft(fromProduct, { ...fromProduct, name: "x" }));
  const result = validateProductDraft(fromProduct);
  assert.ok(result.ok);
  const { id, revision, updatedAt, ...fields } = product;
  assert.deepEqual(result.fields, fields);
});

test("services carry a 5-minute-grid duration; goods never do", () => {
  const service = (duration) =>
    validateProductDraft({
      ...emptyServiceDraft("none"),
      name: "Haircut",
      price: "1500",
      duration,
    });
  const ok = service("45");
  assert.equal(ok.ok && ok.fields.durationMin, 45);
  assert.equal(ok.ok && ok.fields.paymentObject, "service");
  assert.equal(ok.ok && ok.fields.unit, "service_unit");
  const blank = service(" ");
  assert.equal(blank.ok && blank.fields.durationMin, null);
  for (const bad of ["0", "32", "725", "1.5", "abc"]) {
    const result = service(bad);
    assert.equal(result.ok, false, bad);
    assert.equal(result.errors.duration, ru.services.validation.duration);
  }
  // A duration left over from a service is dropped when it becomes goods.
  const goods = validateProductDraft(draft({ duration: "45" }));
  assert.equal(goods.ok && goods.fields.durationMin, null);
});

test("durations read as hours and minutes", () => {
  assert.equal(
    formatDuration(45),
    ru.services.duration.minutes.replace("{m}", "45"),
  );
  assert.equal(
    formatDuration(120),
    ru.services.duration.hours.replace("{h}", "2"),
  );
  assert.equal(
    formatDuration(90),
    ru.services.duration.hoursMinutes.replace("{h}", "1").replace("{m}", "30"),
  );
});
