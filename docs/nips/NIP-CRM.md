NIP-CRM
=======

Retail CRM Records
------------------

`draft` `optional` `relay`

**Depends on**: NIP-01 (event format), NIP-29 (channels, membership, roles), NIP-98 (HTTP auth for read models). Decisions: [ADR-0001](../crm/adr/0001-crm-data-as-events-with-relay-projections.md), [ADR-0002](../crm/adr/0002-crm-space-private-channel.md).

## Abstract

This NIP reserves kinds `42100–42199` for retail CRM records: the CRM space and
its settings, warehouses, products, customers and suppliers, stock documents and
fiscal receipts. Every record is a **regular stored event**, `h`-scoped to one
private channel per community — the *CRM space*. The relay validates each event,
authorizes it against the author's channel role, and applies it to relay-owned
projection tables **in the same database transaction** that stores the event.
Clients read the projections through NIP-98-authenticated `GET /crm/...`
endpoints.

This document specifies the foundation: the space (`42100`) and warehouses
(`42101`). Later CRM kinds extend it section by section.

## Why regular events

Shared master data (a product, a warehouse) is edited by many people. NIP-33
addressable events are replaced per `(author, kind, d)`, so two editors would
produce two diverging "current" rows. CRM events are therefore regular,
append-only revisions: the relay keeps one current projection row per entity and
serializes edits with a compare-and-swap on the `expected-revision` tag. The
full, signed edit history is retained as events.

Stock accounting needs atomic checks ("do not sell more than is in stock") and
consistent cost bookkeeping. Only the relay's database can provide them, which
is why CRM events are projected inside the ingest transaction rather than by
clients.

## Conventions

All CRM events:

- carry exactly one `["h", <space channel uuid>]` tag;
- carry JSON content with a top-level `"v": 1` schema version; unknown versions
  are rejected with `invalid: unsupported CRM payload version <n>`;
- identify their entity with `["crm-id", <uuid>]` where the kind has one;
- carry `["expected-revision", <64-hex event id>]` on updates — the id of the
  event that produced the entity's current revision.

Money is an integer number of kopecks; quantities are integer thousandths of a
unit. Line amounts are `round_half_up(price × qty / 1000)`
(see `crates/buzz-crm/fixtures/money_vectors.json`).

## Authorization

Reads require membership of the space channel (any role). Writes are decided by
the author's current channel role, read inside the write transaction:

| Action | owner | admin | member | guest | bot |
|---|---|---|---|---|---|
| read | ✓ | ✓ | ✓ | ✓ | ✓ |
| manage space, warehouses; void documents | ✓ | ✓ | – | – | – |
| edit catalog, parties; post documents | ✓ | ✓ | ✓ | – | ✓ |
| record fiscal receipts and cash shifts | ✓ | ✓ | ✓ | – | – |

Creating the space additionally requires the author to be a community
(`relay_members`) owner or admin.

## Kinds

### `42100` — CRM space

Creation binds a private, non-archived, permanent stream channel as the
community's only CRM space. It has no `expected-revision` tag and creates the
default warehouse in the same transaction:

```json
{
  "kind": 42100,
  "tags": [["h", "a923f6a5-d467-4b37-8178-240a600737c7"]],
  "content": "{\"v\":1,\"settings\":{\"currency\":\"RUB\",\"timezone\":\"Europe/Moscow\",\"allow_negative_stock\":false,\"fiscalization\":\"required\",\"default_vat\":\"vat22\"},\"default_warehouse\":{\"id\":\"15235c80-47f1-4e01-b2a9-26136267a7f5\",\"name\":\"Основной склад\"}}"
}
```

An update carries `expected-revision` and replaces `settings` as a whole; it must
not carry `default_warehouse`.

| Setting | Values |
|---|---|
| `currency` | `"RUB"` |
| `timezone` | IANA name known to PostgreSQL (`pg_timezone_names`) |
| `allow_negative_stock` | boolean |
| `fiscalization` | `"required"`, `"optional"`, `"off"` |
| `default_vat` | `none`, `vat0`, `vat5`, `vat7`, `vat10`, `vat22`, `vat105`, `vat107`, `vat110`, `vat122` (`vat20`/`vat120` are historical and rejected as defaults) |

### `42101` — Warehouse

```json
{
  "kind": 42101,
  "tags": [
    ["h", "<space channel>"],
    ["crm-id", "<warehouse uuid>"],
    ["expected-revision", "<current revision>"]
  ],
  "content": "{\"v\":1,\"name\":\"Магазин на Ленина\",\"is_default\":true,\"archived\":false}"
}
```

- Without `expected-revision` the event creates the warehouse; an existing id is
  a `conflict:`.
- Setting `is_default` moves the default flag to this warehouse atomically. The
  demoted warehouse also takes the new event id as its revision, so a client
  holding its old revision cannot silently re-promote it. Warehouse writes are
  serialized per space.
- Clearing `is_default` on the current default, or archiving it, is rejected with
  `invalid: make another warehouse the default first`.

### `42131` — Staff member

The people clients book. Authorized like catalog edits.

```json
{
  "kind": 42131,
  "tags": [["h", "<space channel>"], ["crm-id", "<staff uuid>"]],
  "content": "{\"v\":1,\"name\":\"Ольга\",\"position\":\"Барбер\",\"service_ids\":[\"<product uuid>\"],\"archived\":false}"
}
```

- Create and CAS update follow the warehouse rules (`crm-id`,
  `expected-revision`).
- `service_ids` (at most 200, duplicates dropped) must name products of the
  space whose `payment_object` is `service` or `job`; otherwise `invalid:`.
- Optional `phone`, and `member`: the hex pubkey of the channel member who works
  as this staff member.
- Bookable products may carry `duration_min` (5–720, a multiple of 5) on
  kind `42102`; goods may not.

### `42132` — Staff working days

Sets the working hours of the listed days of one staff member; days not listed
keep their hours. It has no `crm-id`.

```json
{
  "kind": 42132,
  "tags": [["h", "<space channel>"]],
  "content": "{\"v\":1,\"staff_id\":\"<staff uuid>\",\"days\":[{\"date\":\"2026-11-02\",\"intervals\":[{\"start\":\"09:00\",\"end\":\"13:00\"},{\"start\":\"14:00\",\"end\":\"20:00\"}]}]}"
}
```

- 1–93 distinct days per event; an empty `intervals` list is a day off.
- Intervals are `HH:MM` on a 5-minute grid (`24:00` allowed as an end), at most
  4 per day, sorted and non-overlapping.
- Archived or unknown staff are `invalid:`.

### `42133` — Appointment

A client booked with one staff member. Authorized like posting documents.

```json
{
  "kind": 42133,
  "tags": [["h", "<space channel>"], ["crm-id", "<appointment uuid>"]],
  "content": "{\"v\":1,\"staff_id\":\"<staff uuid>\",\"date\":\"2026-11-02\",\"start\":\"10:00\",\"services\":[{\"product_id\":\"<product uuid>\",\"duration_min\":60,\"price_minor\":150000}],\"client_name\":\"Анна\",\"client_phone\":\"+7 900 000-00-01\",\"status\":\"booked\"}"
}
```

- Create and CAS update follow the warehouse rules (`crm-id`,
  `expected-revision`).
- `start` is `HH:MM` on a 5-minute grid; the visit (the sum of the service
  durations) must end by midnight. 1–20 distinct services, each one the staff
  member performs; the relay stores the service names as a snapshot.
- The client is either `customer_id` (a live customer card) or `client_name`
  with an optional `client_phone`.
- `status` is `booked`, `confirmed`, `arrived`, `done`, `no_show` or
  `cancelled`. Every status but `no_show` and `cancelled` holds the time.
- A time-holding visit must fit the staff member's working intervals for the
  day unless `outside_hours` is `true`, and never overlaps another
  time-holding visit of the same staff member (`conflict: …`). `source` is
  `staff` (default) or `online`.
- A paid visit cannot be moved to another staff member, day or time.
- A sale (`42104`) whose content carries `"appointment_id": "<uuid>"` pays
  for that visit: the relay links the sale, sets the status to `done` and
  makes the sale event the visit's revision. A missing, cancelled or missed
  visit is `invalid:`; a visit another sale already paid is `conflict:`.
  Voiding the sale (`42105`) unlinks it and puts the visit back to
  `arrived`.
### `42106` — Stock document

Receipt (`receipt`), write-off (`write_off`), inventory (`inventory`) or
transfer (`transfer`), posted once under `crm-id`; requires `post_documents`.

```json
{
  "kind": 42106,
  "tags": [["h", "<space channel>"], ["crm-id", "<document uuid>"]],
  "content": "{\"v\":1,\"type\":\"receipt\",\"warehouse_id\":\"<uuid>\",\"supplier_id\":\"<party uuid>\",\"reference\":\"ТН-15\",\"lines\":[{\"product_id\":\"<uuid>\",\"qty_milli\":10000,\"cost_minor\":25000}]}"
}
```

- 1–500 lines, one per product; products must be goods (`payment_object`
  `commodity` or `excise`) and quantities must respect the unit.
- `receipt`: every line has `cost_minor` (purchase price per unit); optional
  `supplier_id` (a live `supplier`/`both` party) and `reference`.
- `write_off` and `transfer` take no costs; a transfer names
  `to_warehouse_id` (another live warehouse).
- `inventory`: `qty_milli` is the counted quantity (zero allowed); the relay
  books the difference. A surplus is valued at `cost_minor` or the average cost.
- Stock is valued at the moving average cost. With `allow_negative_stock`
  off, a document or sale that would leave less than zero is `invalid: … not
  enough stock …`.
- The relay numbers documents per space and snapshots names and units.

### `42107` — Stock document void

`{"v":1,"reason":"…"}` under the document's `crm-id`; requires
`void_documents`. It appends the reverse of every movement of the document.
Voiding a receipt whose goods were already used is `invalid:` while negative
stock is not allowed. A second void is `conflict:`.

### `42108` — Service tech card

The consumables of one service (`crm-id` = the service product, `service` or
`job`), per service unit: `{"v":1,"items":[{"product_id":"<goods uuid>",
"qty_milli":60000}]}` (0–50 goods, one per product). Create without
`expected-revision`, replace with it; requires `edit_catalog`.

### Stock effects of sales

A sale (`42104`) takes its goods lines off its warehouse, and for each service
line the tech-card consumables × quantity. Goods follow `allow_negative_stock`;
consumables never block a sale and may go negative. A sale void (`42105`)
returns everything the sale took.

## Relay responses

| Situation | `OK` message | HTTP bridge |
|---|---|---|
| accepted | empty | 200 |
| identical event re-sent | `duplicate:` (accepted) | 200 |
| malformed payload or tags, bad channel | `invalid: …` | 400 |
| missing role or community admin rights | `restricted: …` | 403 |
| stale `expected-revision`, second space, id reuse | `conflict: …` | 409 |

## Integrity guards

- Kind `5` and kind `9005` deletions targeting a CRM event are rejected:
  corrections are new events, never deletions.
- While a channel is the CRM space, kind `9002` may not make it open, give it a
  TTL or archive it, and kind `9008` may not delete it.
- CRM kinds are not push-delivered and are not part of timeline or unread kind
  lists.

## Read model

`GET /crm/space` (NIP-98). Responses:

- `{"exists": false, "can_create": <bool>}` — no space; `can_create` is true for
  community owners/admins;
- `{"exists": true, "member": false}` — the caller is not in the space channel;
- `{"exists": true, "member": true, "channel_id", "settings", "revision",
  "my_role", "capabilities": [...], "warehouses": [{"id", "name", "is_default",
  "archived", "revision"}]}`. Archived warehouses are listed only with
  `?archived=true` (used to restore one).

`GET /crm/staff` lists `{"staff": [{"id", "name", "position", "phone",
"service_ids", "member", "archived", "revision", …}]}` (`?archived=true` adds
archived staff); `GET /crm/staff/{id}` returns one. `GET
/crm/schedule?from=YYYY-MM-DD&to=YYYY-MM-DD` (at most 93 days) returns
`{"from", "to", "days": [{"staff_id", "date", "intervals": [{"start", "end"}]}]}`
with interval bounds in minutes after midnight. `GET /crm/products` takes
`kind=services` or `kind=goods`.

`GET /crm/appointments?from&to[&staff_id]` (at most 93 days), or
`?customer_id=<uuid>` alone for a client's history, returns
`{"appointments": [{"id", "staff_id", "date", "start", "end", "services",
"total_minor", "customer_id", "customer_name", "customer_phone",
"client_name", "client_phone", "status", "note", "source", "outside_hours",
"sale_id", "revision", …}]}` with `start`/`end` in minutes after midnight;
`GET /crm/appointments/{id}` returns one. `GET
/crm/slots?date&service_ids=<uuid>,<uuid>[&staff_id][&step=15]` returns
`{"date", "duration_min", "total_minor", "step", "staff": [{"staff_id", "name",
"starts": ["HH:MM"]}]}`: the free start times of every active staff member who
performs all the services. Past days have no slots; today's start after the
current time in the space timezone.

`GET /crm/analytics/services?from&to` (same period rules as
`/crm/analytics`) returns `{"from", "to", "staff": [{"staff_id", "name",
"visits", "done", "no_shows", "cancelled", "revenue_minor", "booked_min",
"worked_min"}], "services": [{"product_id", "name", "visits",
"revenue_minor", "minutes"}], "clients": {"clients", "new_clients",
"walk_ins"}}`. Visits count when they hold time; revenue counts done visits
at their booked prices; `worked_min` sums the scheduled intervals.
`GET /crm/stock?warehouse=<uuid>&q=&low=true` returns `{"warehouse_id",
"truncated", "items": [{"product_id", "name", "sku", "unit", "category",
"price_minor", "min_stock_milli", "qty_milli", "value_minor", "archived"}],
"totals": {"value_minor", "low_count"}}` (all warehouses summed without
`warehouse`). At most 2000 items are returned by name; `truncated` says more
matched, and `totals` always cover every match. `low` keeps products with a
minimum set (`min_stock_milli > 0`) and stock at or below it.
`GET /crm/stock/moves?warehouse=&product=&before=&limit=` returns the movement
journal newest first (`source` is `receipt`, `write_off`, `inventory`,
`transfer`, `sale` or `consumable`; `reversal` marks void moves), with `next`.
`GET /crm/stock/docs?type=&before=&limit=` and `GET /crm/stock/docs/{id}` (with
`lines`) read documents; `GET /crm/tech-cards` returns `{"cards": [{"service_id",
"service_name", "items": [{"product_id", "product_name", "unit",
"qty_milli"}], "revision"}]}`.

Unauthenticated requests get 401; when relay membership is required, callers
who are not relay members get 403 `relay_membership_required`.
