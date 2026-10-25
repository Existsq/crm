import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  CRM_KINDS,
  KIND_CRM_ONLINE_BOOKING,
  KIND_CRM_ONLINE_BOOKING_SETTINGS,
  KIND_CRM_PARTY,
  KIND_CRM_PRODUCT,
  KIND_CRM_SALE,
  KIND_CRM_SALE_VOID,
  KIND_CRM_SPACE,
  KIND_CRM_WAREHOUSE,
} from "./crmKinds.ts";

const here = dirname(fileURLToPath(import.meta.url));
const kindRs = readFileSync(
  resolve(here, "../../../../../crates/buzz-core/src/kind.rs"),
  "utf8",
);

function relayKind(name) {
  const match = kindRs.match(new RegExp(`pub const ${name}: u32 = (\\d+);`));
  assert.ok(match, `${name} missing from kind.rs`);
  return Number(match[1]);
}

test("desktop CRM kinds match the relay registry", () => {
  assert.equal(KIND_CRM_SPACE, relayKind("KIND_CRM_SPACE"));
  assert.equal(KIND_CRM_WAREHOUSE, relayKind("KIND_CRM_WAREHOUSE"));
  assert.equal(KIND_CRM_PRODUCT, relayKind("KIND_CRM_PRODUCT"));
  assert.equal(KIND_CRM_PARTY, relayKind("KIND_CRM_PARTY"));
  assert.equal(KIND_CRM_SALE, relayKind("KIND_CRM_SALE"));
  assert.equal(KIND_CRM_SALE_VOID, relayKind("KIND_CRM_SALE_VOID"));
});

test("online booking kinds follow the wire contract", () => {
  assert.equal(KIND_CRM_ONLINE_BOOKING, 42134);
  assert.equal(KIND_CRM_ONLINE_BOOKING_SETTINGS, 42135);
});

// Kinds the desktop handles before the relay registry defines them (the
// online booking backend lands separately). Once kind.rs has them this set
// matches nothing and the check below is exact equality again.
const PENDING_RELAY_KINDS = new Set([
  KIND_CRM_ONLINE_BOOKING,
  KIND_CRM_ONLINE_BOOKING_SETTINGS,
]);

test("the desktop subscribes to every CRM kind the relay defines", () => {
  const relayKinds = [
    ...kindRs.matchAll(/pub const KIND_CRM_\w+: u32 = (\d+);/g),
  ].map((match) => Number(match[1]));
  for (const kind of relayKinds) {
    assert.ok(CRM_KINDS.includes(kind), `relay kind ${kind} not subscribed`);
  }
  const extra = CRM_KINDS.filter((kind) => !relayKinds.includes(kind));
  for (const kind of extra) {
    assert.ok(PENDING_RELAY_KINDS.has(kind), `${kind} unknown to the relay`);
  }
});

test("every CRM kind sits in the reserved CRM block", () => {
  const min = relayKind("CRM_KIND_MIN");
  const max = relayKind("CRM_KIND_MAX");
  for (const kind of CRM_KINDS) {
    assert.ok(kind >= min && kind <= max, `${kind} outside ${min}–${max}`);
  }
  assert.equal(new Set(CRM_KINDS).size, CRM_KINDS.length);
});
