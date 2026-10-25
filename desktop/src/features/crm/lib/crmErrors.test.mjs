import assert from "node:assert/strict";
import test from "node:test";

import { ru } from "../i18n/ru.ts";
import { describeCrmError } from "./crmErrors.ts";

test("relay prefixes map to Russian messages with the relay reason as detail", () => {
  const cases = [
    [
      "relay error 409: conflict: warehouse changed since it was loaded",
      "conflict",
      "warehouse changed since it was loaded",
    ],
    [
      "relay error 403: restricted: role member may not manage_warehouses",
      "restricted",
      "role member may not manage_warehouses",
    ],
    [
      "relay rejected event: invalid: a CRM space channel must be private",
      "invalid",
      "a CRM space channel must be private",
    ],
  ];
  for (const [raw, kind, detail] of cases) {
    const described = describeCrmError(new Error(raw));
    assert.equal(described.kind, kind);
    assert.equal(described.message, ru.errors[kind]);
    assert.equal(described.detail, detail);
  }
});

test("duplicate SKU and barcode conflicts name the taken value", () => {
  const sku = describeCrmError(
    new Error(
      'relay error 409: conflict: SKU "MLK-001" is already used by another product',
    ),
  );
  assert.equal(sku.kind, "conflict");
  assert.equal(
    sku.message,
    "Артикул «MLK-001» уже используется другим товаром.",
  );
  assert.equal(sku.detail, 'SKU "MLK-001" is already used by another product');

  const barcode = describeCrmError(
    "relay rejected event: conflict: barcode 4600051000057 is already used by another product",
  );
  assert.equal(
    barcode.message,
    "Штрихкод 4600051000057 уже используется другим товаром.",
  );

  const stale = describeCrmError(
    "relay error 409: conflict: product changed since it was loaded",
  );
  assert.equal(stale.message, ru.errors.conflict);
});

test("transport failures are network errors without relay detail", () => {
  const described = describeCrmError("relay unreachable: request timed out");
  assert.deepEqual(described, {
    kind: "network",
    message: ru.errors.network,
    detail: null,
  });
});

test("unrecognised failures keep the raw text as detail", () => {
  assert.deepEqual(describeCrmError(new Error("boom")), {
    kind: "unknown",
    message: ru.errors.unknown,
    detail: "boom",
  });
  assert.deepEqual(describeCrmError(undefined), {
    kind: "unknown",
    message: ru.errors.unknown,
    detail: null,
  });
});

test("journal rejections name the busy time or the broken rule", () => {
  const busy = describeCrmError(
    "relay error 409: conflict: the staff member is booked 12:00–12:45",
  );
  assert.equal(busy.kind, "conflict");
  assert.equal(
    busy.message,
    ru.errors.staffBusy.replace("{value}", "12:00–12:45"),
  );
  assert.equal(
    describeCrmError(
      "relay error 400: invalid: the visit is outside the staff member's working hours",
    ).message,
    ru.errors.outsideHours,
  );
});
