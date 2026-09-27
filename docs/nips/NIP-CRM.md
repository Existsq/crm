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

Unauthenticated requests get 401; when relay membership is required, callers
who are not relay members get 403 `relay_membership_required`.
