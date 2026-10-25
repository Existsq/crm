import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { CRM_KINDS, KIND_CRM_SPACE, KIND_CRM_WAREHOUSE } from "./crmKinds.ts";

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
});

test("every CRM kind sits in the reserved CRM block", () => {
  const min = relayKind("CRM_KIND_MIN");
  const max = relayKind("CRM_KIND_MAX");
  for (const kind of CRM_KINDS) {
    assert.ok(kind >= min && kind <= max, `${kind} outside ${min}–${max}`);
  }
  assert.equal(new Set(CRM_KINDS).size, CRM_KINDS.length);
});
