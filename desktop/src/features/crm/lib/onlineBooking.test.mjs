import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_ONLINE_BOOKING_DRAFT,
  draftFromSettings,
  minNoticeOptions,
  sameDraft,
  settingsFromDraft,
  stepOptions,
  toggleId,
} from "./onlineBooking.ts";

const SAVED = {
  enabled: true,
  title: "Борода",
  about: null,
  address: "ул. Ленина, 1",
  phone: null,
  horizonDays: 30,
  minNoticeMin: 60,
  stepMin: 15,
  staffIds: ["a", "b"],
  serviceIds: [],
};

test("saved settings round-trip through the draft", () => {
  const draft = draftFromSettings(SAVED);
  assert.equal(draft.about, "");
  assert.equal(draft.horizonDays, "30");
  const parsed = settingsFromDraft(draft);
  assert.equal(parsed.ok, true);
  assert.deepEqual(parsed.settings, SAVED);
});

test("no settings yet means the defaults, disabled", () => {
  const draft = draftFromSettings(null);
  assert.deepEqual(draft, DEFAULT_ONLINE_BOOKING_DRAFT);
  assert.equal(draft.enabled, false);
});

test("drafts compare id lists as sets", () => {
  const a = draftFromSettings(SAVED);
  const b = { ...a, staffIds: ["b", "a"] };
  assert.equal(sameDraft(a, b), true);
  assert.equal(sameDraft(a, { ...a, staffIds: ["a"] }), false);
  assert.equal(sameDraft(a, { ...a, title: "Другое" }), false);
});

test("blank title and out-of-range horizons are rejected by field", () => {
  const draft = draftFromSettings(SAVED);
  assert.equal(settingsFromDraft({ ...draft, title: "   " }).field, "title");
  for (const horizonDays of ["0", "181", "", "1.5", "abc", "-3"]) {
    const result = settingsFromDraft({ ...draft, horizonDays });
    assert.equal(result.ok, false, horizonDays);
    assert.equal(result.field, "horizonDays", horizonDays);
  }
  for (const horizonDays of ["1", "180", " 45 "]) {
    assert.equal(settingsFromDraft({ ...draft, horizonDays }).ok, true);
  }
});

test("text fields are trimmed and blanks become null", () => {
  const draft = { ...draftFromSettings(SAVED), title: "  Салон ", phone: " " };
  const parsed = settingsFromDraft(draft);
  assert.equal(parsed.settings.title, "Салон");
  assert.equal(parsed.settings.phone, null);
});

test("preset choices keep a value set elsewhere", () => {
  assert.deepEqual(
    minNoticeOptions(60).map((o) => o.value),
    [0, 30, 60, 120, 1440],
  );
  assert.equal(minNoticeOptions(0)[0].label, "Без ограничения");
  assert.equal(minNoticeOptions(60)[2].label, "1 ч");
  assert.equal(minNoticeOptions(60)[4].label, "1 день");
  assert.deepEqual(
    minNoticeOptions(90).map((o) => o.value),
    [0, 30, 60, 90, 120, 1440],
  );
  assert.deepEqual(stepOptions(15), [5, 10, 15, 30, 60]);
  assert.deepEqual(stepOptions(20), [5, 10, 15, 20, 30, 60]);
});

test("toggling ids adds once and removes", () => {
  assert.deepEqual(toggleId(["a"], "b", true), ["a", "b"]);
  assert.deepEqual(toggleId(["a", "b"], "b", true), ["a", "b"]);
  assert.deepEqual(toggleId(["a", "b"], "a", false), ["b"]);
});
