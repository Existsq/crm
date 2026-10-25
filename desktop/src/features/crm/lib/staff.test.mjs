import assert from "node:assert/strict";
import test from "node:test";

import { ru } from "../i18n/ru.ts";
import {
  emptyStaffDraft,
  sameStaffDraft,
  staffDraftFrom,
  validateStaffDraft,
} from "./staff.ts";

test("a valid draft is trimmed and its services deduplicated", () => {
  const result = validateStaffDraft({
    ...emptyStaffDraft(),
    name: "  Olga ",
    position: " Barber ",
    phone: " +7 912 000-11-22 ",
    serviceIds: ["a", "b", "a"],
  });
  assert.deepEqual(result, {
    ok: true,
    fields: {
      name: "Olga",
      position: "Barber",
      phone: "+7 912 000-11-22",
      serviceIds: ["a", "b"],
      member: null,
      archived: false,
    },
  });
});

test("invalid fields are reported", () => {
  const result = validateStaffDraft({
    ...emptyStaffDraft(),
    name: " ",
    position: "x".repeat(65),
    phone: "call me",
  });
  assert.deepEqual(result, {
    ok: false,
    errors: {
      name: ru.errors.nameRequired,
      position: ru.staff.validation.position,
      phone: ru.parties.validation.phone,
    },
  });
});

test("a stored staff member round-trips and service order does not matter", () => {
  const staff = {
    id: "s1",
    name: "Olga",
    position: null,
    phone: null,
    serviceIds: ["a", "b"],
    member: null,
    archived: false,
    revision: "ab".repeat(32),
    updatedAt: "2026-10-25T10:00:00Z",
  };
  const draft = staffDraftFrom(staff);
  assert.ok(sameStaffDraft(draft, { ...draft, serviceIds: ["b", "a"] }));
  assert.ok(!sameStaffDraft(draft, { ...draft, serviceIds: ["a"] }));
  const result = validateStaffDraft(draft);
  assert.ok(result.ok);
  const { id, revision, updatedAt, ...fields } = staff;
  assert.deepEqual(result.fields, fields);
});
