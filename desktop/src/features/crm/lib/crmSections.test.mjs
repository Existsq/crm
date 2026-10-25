import assert from "node:assert/strict";
import test from "node:test";

import {
  CRM_SECTIONS,
  DEFAULT_CRM_SECTION,
  parseCrmSection,
} from "./crmSections.ts";

test("known sections parse, anything else is rejected", () => {
  for (const section of CRM_SECTIONS) {
    assert.equal(parseCrmSection(section.id), section.id);
  }
  for (const raw of ["", "SETTINGS", "../settings", undefined, null, 1]) {
    assert.equal(parseCrmSection(raw), null);
  }
});

test("the default section is the first one: the catalog", () => {
  assert.equal(DEFAULT_CRM_SECTION, CRM_SECTIONS[0].id);
  assert.equal(DEFAULT_CRM_SECTION, "products");
  assert.deepEqual(
    CRM_SECTIONS.map((section) => section.id),
    [
      "products",
      "journal",
      "reminders",
      "services",
      "staff",
      "schedule",
      "parties",
      "cash",
      "sales",
      "analytics",
      "settings",
    ],
  );
});
