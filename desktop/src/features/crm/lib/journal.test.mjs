import assert from "node:assert/strict";
import test from "node:test";

import { ru } from "../i18n/ru.ts";
import {
  assignLanes,
  clientLabel,
  DEFAULT_JOURNAL_WINDOW,
  holdsTime,
  journalRows,
  journalWindow,
  isWorking,
  servicesTotal,
  startChoices,
  staffServices,
  statusWrite,
  validateAppointmentDraft,
} from "./journal.ts";

function appointment(overrides = {}) {
  return {
    id: "a1",
    staffId: "s1",
    date: "2026-11-02",
    start: 600,
    end: 690,
    services: [{ productId: "p1", durationMin: 90, priceMinor: 150000 }],
    serviceNames: ["Haircut"],
    totalMinor: 150000,
    customerId: null,
    customerName: null,
    customerPhone: null,
    clientName: "Anna",
    clientPhone: "+7 900",
    status: "booked",
    note: null,
    source: "staff",
    outsideHours: false,
    saleId: null,
    revision: "r1",
    ...overrides,
  };
}

test("missed and cancelled visits free their time", () => {
  assert.deepEqual(
    ["booked", "confirmed", "arrived", "done", "no_show", "cancelled"].map(
      holdsTime,
    ),
    [true, true, true, true, false, false],
  );
});

test("the window covers hours and visits in whole hours", () => {
  assert.deepEqual(journalWindow([], []), DEFAULT_JOURNAL_WINDOW);
  assert.deepEqual(
    journalWindow(
      [
        { start: 570, end: 780 },
        { start: 840, end: 1170 },
      ],
      [{ start: 1200, end: 1245 }],
    ),
    { start: 540, end: 1260 },
  );
  assert.deepEqual(journalRows({ start: 540, end: 660 }), [540, 570, 600, 630]);
});

test("working time must hold the whole span", () => {
  const day = [
    { start: 540, end: 780 },
    { start: 840, end: 1200 },
  ];
  assert.equal(isWorking(day, 540, 570), true);
  assert.equal(isWorking(day, 750, 810), false);
  assert.equal(isWorking([], 600, 630), false);
});

test("overlapping visits share lanes, separate ones get the full width", () => {
  const placed = assignLanes([
    { id: "late", start: 720, end: 780 },
    { id: "a", start: 600, end: 690 },
    { id: "b", start: 630, end: 660 },
    { id: "c", start: 660, end: 700 },
  ]);
  const byId = Object.fromEntries(placed.map((p) => [p.item.id, p]));
  assert.deepEqual(
    [byId.a.lane, byId.b.lane, byId.c.lane, byId.late.lane],
    [0, 1, 1, 0],
  );
  assert.equal(byId.a.lanes, 2);
  assert.equal(byId.c.lanes, 2);
  assert.equal(byId.late.lanes, 1);
});

test("staff services keep active services with a duration", () => {
  const products = [
    { id: "p1", archived: false, durationMin: 60 },
    { id: "p2", archived: true, durationMin: 30 },
    { id: "p3", archived: false, durationMin: null },
    { id: "p4", archived: false, durationMin: 30 },
  ];
  assert.deepEqual(
    staffServices(products, ["p1", "p2", "p3"]).map((p) => p.id),
    ["p1"],
  );
  assert.deepEqual(
    servicesTotal([
      { productId: "p1", durationMin: 60, priceMinor: 100 },
      { productId: "p4", durationMin: 30, priceMinor: 50 },
    ]),
    { durationMin: 90, totalMinor: 150 },
  );
});

test("start choices merge free slots with the kept time", () => {
  assert.deepEqual(
    startChoices(["09:00", "10:30", "bad"], 600),
    [540, 600, 630],
  );
  assert.deepEqual(startChoices(["09:00"], null), [540]);
});

test("drafts need a client, services and a time", () => {
  const v = ru.journal.dialog.validation;
  const draft = {
    staffId: "s1",
    date: "2026-11-02",
    start: 600,
    services: [{ productId: "p1", durationMin: 60, priceMinor: 100 }],
    customer: null,
    clientName: "  Anna ",
    clientPhone: " ",
    status: "booked",
    note: "",
    outsideHours: false,
  };
  const ok = validateAppointmentDraft(draft);
  assert.equal(ok.ok, true);
  assert.equal(ok.fields.clientName, "Anna");
  assert.equal(ok.fields.clientPhone, null);
  assert.equal(ok.fields.note, null);
  assert.deepEqual(validateAppointmentDraft({ ...draft, clientName: " " }), {
    ok: false,
    field: "client",
    error: v.client,
  });
  const card = validateAppointmentDraft({
    ...draft,
    clientName: "",
    customer: { id: "c1", name: "Maria", phone: null },
  });
  assert.equal(card.fields.customerId, "c1");
  assert.equal(card.fields.clientName, null);
  assert.equal(
    validateAppointmentDraft({ ...draft, services: [] }).field,
    "services",
  );
  assert.equal(
    validateAppointmentDraft({ ...draft, start: null }).field,
    "time",
  );
});

test("a status change keeps the visit and carries its revision", () => {
  const visit = appointment({
    customerId: "c1",
    customerName: "Maria",
    clientName: null,
  });
  assert.equal(clientLabel(visit), "Maria");
  const write = statusWrite(visit, "chan", "arrived");
  assert.equal(write.expectedRevision, "r1");
  assert.equal(write.status, "arrived");
  assert.equal(write.start, 600);
  assert.equal(write.customerId, "c1");
  assert.equal(write.clientName, null);
});
