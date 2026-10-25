import assert from "node:assert/strict";
import test from "node:test";

import {
  emptyPartyDraft,
  isValidEmail,
  isValidInn,
  isValidPhone,
  partyDraftFrom,
  samePartyDraft,
  validatePartyDraft,
} from "./parties.ts";

test("INN checksums match the relay vectors", () => {
  for (const valid of ["7707083893", "7736207543", "500100732259"]) {
    assert.equal(isValidInn(valid), true, valid);
  }
  for (const invalid of [
    "7707083894",
    "500100732258",
    "772170412837",
    "12345",
    "77070838a3",
  ]) {
    assert.equal(isValidInn(invalid), false, invalid);
  }
});

test("phones and emails follow the relay rules", () => {
  assert.equal(isValidPhone("+7 (912) 000-11-22"), true);
  assert.equal(isValidPhone("12"), false);
  assert.equal(isValidPhone("call me"), false);
  assert.equal(isValidEmail("maria@example.ru"), true);
  for (const bad of [
    "maria@",
    "@example.ru",
    "ma ria@x.ru",
    "a@b@c.ru",
    "a@b.",
  ]) {
    assert.equal(isValidEmail(bad), false, bad);
  }
});

test("a valid draft trims fields and turns blanks into null", () => {
  const result = validatePartyDraft({
    ...emptyPartyDraft(),
    name: "  Мария  ",
    phone: " +7 912 000-11-22 ",
    email: "  ",
    note: " звонить после 18 ",
  });
  assert.equal(result.ok, true);
  assert.deepEqual(result.fields, {
    partyType: "customer",
    name: "Мария",
    phone: "+7 912 000-11-22",
    email: null,
    inn: null,
    address: null,
    note: "звонить после 18",
    archived: false,
  });
});

test("invalid fields are reported per field", () => {
  const result = validatePartyDraft({
    ...emptyPartyDraft(),
    name: " ",
    phone: "1",
    email: "nope",
    inn: "500100732258",
  });
  assert.equal(result.ok, false);
  assert.deepEqual(Object.keys(result.errors).sort(), [
    "email",
    "inn",
    "name",
    "phone",
  ]);
});

test("a draft from a party round-trips and compares equal", () => {
  const party = {
    id: "p1",
    partyType: "supplier",
    name: "Молокозавод",
    phone: null,
    email: "milk@example.ru",
    inn: "7707083893",
    address: null,
    note: null,
    archived: true,
    purchases: { salesCount: 0, revenueMinor: 0, lastSaleAt: null },
    revision: "r",
    createdAt: "",
    updatedAt: "",
  };
  const draft = partyDraftFrom(party);
  assert.equal(draft.phone, "");
  assert.equal(samePartyDraft(draft, partyDraftFrom(party)), true);
  assert.equal(samePartyDraft(draft, { ...draft, name: "x" }), false);
});
