import assert from "node:assert/strict";
import test from "node:test";

import { CRM_TIMEZONES, crmTimezoneOptions } from "./crmTimezones.ts";

test("Moscow is offered and every option has a label", () => {
  const options = crmTimezoneOptions("Europe/Moscow");
  assert.equal(options.length, CRM_TIMEZONES.length);
  assert.ok(options.some((o) => o.value === "Europe/Moscow"));
  for (const option of options) {
    assert.ok(option.label.length > 0, option.value);
  }
});

test("an unknown current zone is preserved as the first option", () => {
  const options = crmTimezoneOptions("America/New_York");
  assert.deepEqual(options[0], {
    value: "America/New_York",
    label: "America/New_York",
  });
  assert.equal(options.length, CRM_TIMEZONES.length + 1);
});
