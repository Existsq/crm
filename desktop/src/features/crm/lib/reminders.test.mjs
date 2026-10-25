import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  defaultReminderSettings,
  deliveryLogSince,
  formatOffset,
  nextReminderRule,
  parseChatId,
  REMINDER_PLACEHOLDERS,
  renderReminder,
  ruleKey,
  ruleKeyLabel,
  sameReminderSettings,
  sampleReminderContext,
  unknownPlaceholders,
  validateReminderDraft,
} from "./reminders.ts";

const here = dirname(fileURLToPath(import.meta.url));
const reminderRs = readFileSync(
  resolve(here, "../../../../../crates/buzz-crm/src/reminder.rs"),
  "utf8",
);

test("placeholders match the relay's list", () => {
  const block = reminderRs.match(/PLACEHOLDERS: \[&str; \d+\] = \[([^\]]+)\]/);
  assert.ok(block, "PLACEHOLDERS missing from reminder.rs");
  const relay = [...block[1].matchAll(/"(\w+)"/g)].map((m) => m[1]);
  assert.deepEqual([...REMINDER_PLACEHOLDERS], relay);
});

test("templates render placeholders and keep other braces", () => {
  const context = sampleReminderContext({
    salonName: "Барбершоп",
    address: null,
  });
  assert.equal(
    renderReminder("{client}, {salon} ждёт вас в {time} {x} {", context),
    "Анна, Барбершоп ждёт вас в 10:30 {x} {",
  );
  assert.deepEqual(unknownPlaceholders("{name} {client} {name} { } {}"), [
    "name",
  ]);
});

test("rule keys and labels follow the relay", () => {
  assert.equal(ruleKey({ trigger: "booked", offsetMin: null }), "booked");
  assert.equal(ruleKey({ trigger: "before", offsetMin: 1440 }), "before:1440");
  assert.equal(ruleKeyLabel("booked"), "Подтверждение записи");
  assert.equal(ruleKeyLabel("before:1440"), "За 24 ч");
  assert.equal(ruleKeyLabel("before:150"), "За 2 ч 30 мин");
  assert.equal(formatOffset(30), "30 мин");
  assert.equal(formatOffset(2880), "2 дн");
  assert.equal(formatOffset(10080), "7 дн");
});

test("defaults are valid and adding rules stops at four", () => {
  const defaults = defaultReminderSettings();
  assert.equal(validateReminderDraft(defaults).ok, true);
  const added = nextReminderRule(defaults.rules);
  assert.ok(added);
  assert.equal(added.trigger, "before");
  assert.ok(!defaults.rules.some((rule) => ruleKey(rule) === ruleKey(added)));
  assert.equal(nextReminderRule([...defaults.rules, added]), null);
  const withoutBooked = defaults.rules.slice(1);
  assert.equal(nextReminderRule(withoutBooked)?.trigger, "booked");
});

test("invalid drafts point at the offending rule", () => {
  const draft = defaultReminderSettings();
  const bad = {
    ...draft,
    rules: [
      { ...draft.rules[0], template: "  " },
      { ...draft.rules[1], offsetMin: 20 },
      { ...draft.rules[2], template: "Привет, {name}" },
      { trigger: "before", offsetMin: 120, template: "дубль" },
    ],
  };
  const result = validateReminderDraft(bad);
  assert.equal(result.ok, false);
  assert.deepEqual(Object.keys(result.errors.rules), ["0", "1", "2", "3"]);
  assert.match(result.errors.rules[2], /\{name\}/);

  const trimmed = validateReminderDraft({
    ...draft,
    salonName: "  ",
    rules: [{ ...draft.rules[0], template: "  {client} " }],
  });
  assert.equal(trimmed.ok, true);
  assert.equal(trimmed.settings.salonName, null);
  assert.equal(trimmed.settings.rules[0].template, "{client}");
});

test("settings compare by content", () => {
  const a = defaultReminderSettings();
  const b = defaultReminderSettings();
  assert.equal(sameReminderSettings(a, b), true);
  assert.equal(sameReminderSettings(a, { ...b, enabled: true }), false);
  assert.equal(
    sameReminderSettings(a, { ...b, rules: b.rules.slice(1) }),
    false,
  );
});

test("chat ids are whole non-zero numbers", () => {
  assert.equal(parseChatId(" 123456 "), 123456);
  assert.equal(parseChatId("-100200300"), -100200300);
  for (const raw of ["", "0", "12a", "1.5", "99999999999999999"]) {
    assert.equal(parseChatId(raw), null, raw);
  }
});

test("the log window starts a week back", () => {
  assert.equal(
    deliveryLogSince(new Date("2026-11-09T12:00:00Z")),
    "2026-11-02T12:00:00.000Z",
  );
});
