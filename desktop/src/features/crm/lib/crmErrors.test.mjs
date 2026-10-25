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
