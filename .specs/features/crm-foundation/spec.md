# CRM Foundation Specification

## Problem Statement

The CRM fork needs a shared backbone before any retail screen can exist: a place
where CRM data lives (the CRM space), who may read and write it, the event kinds
that carry it, and the relay machinery that applies them atomically. Without it,
catalog, stock, sales and fiscal features would each invent their own storage and
access rules. See VISION_CRM.md and ADR-0001/0002.

## Goals

- [ ] A community owner/admin can bind one private channel as the CRM space and
      configure it (currency RUB, timezone, negative-stock policy, fiscalization
      mode, default VAT) through a signed event, and warehouses can be created
      and edited in it.
- [ ] Every CRM write goes through one relay ingest branch that validates,
      authorizes (role × action, uncached, inside the write transaction) and
      stores the event together with its projection rows in one transaction.
- [ ] Members read the space through `GET /crm/space`; everyone else is denied.
- [ ] Shared pure domain code (`buzz-crm`: money, quantity, VAT, authz,
      validation) is reusable by relay, SDK, CLI and desktop.

## Out of Scope

| Feature | Reason |
| ------- | ------ |
| Products, parties, documents, stock, fiscal receipts | Later branches (`crm/catalog`, `crm/customers`, `crm/ledger`, `crm/pos`) |
| Moving-average costing and discount distribution | Belong with the ledger that uses them (`crm/ledger`) |
| Desktop UI | `crm/desktop-shell` |
| Archiving / deleting the CRM space | Not needed for v1; a space is created once per community |
| Excluding CRM kinds from full-text search | Would rewrite the partitioned `events` generated column; search is kind-filtered and membership-gated, so CRM events only surface for CRM members who ask for CRM kinds |

---

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --------------------- | -------------- | --------- | ---------- |
| Kind numbers | 42100 space, 42101 warehouse (block 42100–42199 reserved for CRM) | Free range next to 42000 product feedback, outside reserved 47000 user-groups | y (plan) |
| Event shape | Regular stored events with JSON content `{ "v": 1, ... }`, `h` tag = space channel, `crm-id` tag = entity UUID, optional `expected-revision` tag | Addressable events replace per author and would fork shared master data (ADR-0001) | y (plan) |
| Auth scope | `MessagesWrite` | Least invasive; the CRM role table is the real authority | y (plan) |
| Who creates a space | Community owner/admin (relay_members role) who is also owner/admin of the channel | Binding a channel exposes the community's retail data surface | y (plan) |
| One active space per community | Enforced by a partial unique index | Simplest mental model for v1 | y (plan) |
| Space settings update | Same kind 42100 with `expected-revision` = current revision event id | Mirrors canvas/workflow CAS | y (plan) |
| Warehouse create/update | Kind 42101 with `crm-id` = warehouse UUID; create has no `expected-revision`, update requires it | Same CAS model as all master data | y (plan) |
| Duplicate submission | Re-sending the identical event returns `duplicate:` and changes nothing | Idempotent retries | y |
| Re-signed retry of a create | A different event for an existing entity without `expected-revision` is rejected with `conflict: … already exists` | Creates are single-shot; retries of the same signed event are the duplicate path | y |
| Timestamp tolerance | Relay-wide ±900 s already applies; no extra CRM rule | Consistent with other kinds | y |
| Name bounds | Space/warehouse names 1–128 chars after trim | Matches fiscal item-name limit used later | y |
| Timezone validation | Shape check in `buzz-crm`, then existence in PostgreSQL `pg_timezone_names` inside the write transaction | Analytics day boundaries depend on it; avoids a tz-database dependency that could disagree with Postgres | y |

**Open questions:** none - all resolved or logged above.

---

## User Stories

### P1: Create a CRM space ⭐ MVP

**User Story**: As a community owner, I want to turn a private channel into the
CRM space so that only its members can see and work with retail data.

**Why P1**: Every other CRM feature is scoped to the space.

**Acceptance Criteria**:

1. WHEN a community owner/admin who is owner/admin of a private, non-archived, non-TTL stream channel submits a valid kind 42100 event without `expected-revision` THEN the relay SHALL accept it and create a `crm_spaces` row plus the default warehouse named in the payload in the same transaction.
2. IF the submitter is not a community owner/admin THEN the relay SHALL reject the space creation with a message starting `restricted:`.
3. IF the target channel is open, a DM, archived or has a TTL THEN the relay SHALL reject the space creation with a message starting `invalid:`.
4. IF the community already has an active CRM space THEN the relay SHALL reject another space creation with a message starting `conflict:`.
5. WHEN the identical 42100 event is submitted twice THEN the relay SHALL answer the second submission with a `duplicate:` message and SHALL NOT create a second row.

**Independent Test**: relay e2e — owner creates private channel, publishes 42100, `GET /crm/space` returns the space and default warehouse.

---

### P1: Update space settings and manage warehouses ⭐ MVP

**User Story**: As a CRM admin, I want to change settings and add warehouses so
that the CRM matches how the shop works.

**Why P1**: Warehouses are required by every stock movement.

**Acceptance Criteria**:

1. WHEN a channel owner/admin submits a kind 42100 update whose `expected-revision` equals the current revision THEN the relay SHALL replace the settings and set the revision to the new event id.
2. IF the `expected-revision` of a space or warehouse update differs from the current revision THEN the relay SHALL reject it with a message starting `conflict:` and SHALL leave the row unchanged.
3. WHEN a channel owner/admin submits a kind 42101 event for a new warehouse UUID THEN the relay SHALL create the warehouse in the space.
4. IF a member, guest or bot submits kind 42100 or 42101 THEN the relay SHALL reject it with a message starting `restricted:`.
5. IF a warehouse update would leave the space with zero default warehouses or archive the default warehouse THEN the relay SHALL reject it with a message starting `invalid:`.

**Independent Test**: Postgres tests on the store functions plus relay e2e for the role matrix.

---

### P1: Read the space ⭐ MVP

**User Story**: As a CRM member, I want to load the space, my role and the
warehouses so that the app knows what I can do.

**Why P1**: The desktop gate screen depends on it.

**Acceptance Criteria**:

1. WHEN a member of the space channel calls `GET /crm/space` with valid NIP-98 auth THEN the relay SHALL return 200 with `channel_id`, `settings`, `revision`, `my_role`, `capabilities` and non-archived `warehouses` (archived ones too with `?archived=true`, so a warehouse can be restored).
2. WHEN a community member who is not in the space channel calls `GET /crm/space` THEN the relay SHALL return 200 with `{"exists": true, "member": false}` and no settings or warehouses.
3. WHEN the community has no CRM space THEN `GET /crm/space` SHALL return 200 with `{"exists": false, "can_create": <bool>}`, where `can_create` is true only for community owners/admins.
4. IF the request has no NIP-98 / dev auth THEN the relay SHALL reject it with HTTP 401 (the status the workflow read endpoints use); IF relay membership is required and the caller is not a relay member THEN it SHALL reject it the same way the workflow read endpoints do.

**Independent Test**: relay e2e with two users (member, non-member).

---

### P1: CRM data stays CRM data ⭐ MVP

**User Story**: As an owner, I want CRM history to be tamper-evident so that
nobody silently erases sales or settings.

**Why P1**: Accounting records must not disappear through chat moderation paths.

**Acceptance Criteria**:

1. IF a kind 5 or kind 9005 deletion targets a CRM-kind event THEN the relay SHALL reject it with a message starting `invalid:`.
2. IF a kind 9002 edit sets `visibility=open`, a non-empty `ttl` or `archived=true` on the active CRM space channel THEN the relay SHALL reject it with a message starting `invalid:`, and the database SHALL refuse any channel update that would do so (trigger `crm_guard_space_channel`).
3. IF a kind 9008 deletes the active CRM space channel THEN the relay SHALL reject it with a message starting `invalid:`.
4. The relay SHALL keep CRM kinds out of the push kind list (guarded by a test); the home-feed and unread paths use explicit chat-kind allowlists that do not include CRM kinds.

**Independent Test**: unit tests for the guards plus relay e2e deletion attempt.

---

### P2: Domain library and CLI

**User Story**: As an agent or developer, I want typed builders and a CLI so that
CRM events are built the same way everywhere.

**Why P2**: The desktop and agents need it, but the relay can be proven with raw events.

**Acceptance Criteria**:

1. The `buzz-crm` crate SHALL expose `Money` (kopecks) with checked addition/subtraction/sum and `Qty` (thousandths) with bounded constructors, and `line_amount` SHALL return an error instead of overflowing.
2. The `buzz-crm` crate SHALL validate Russian INN checksums for 10- and 12-digit INNs.
3. The authorization table SHALL return the decision listed in ADR-0002 for every role × action pair.
4. WHEN `buzz crm space show` runs THEN the CLI SHALL print the `GET /crm/space` JSON; WHEN `buzz crm space init` runs THEN it SHALL publish a 42100 event and print `{event_id, accepted, message}`.

**Independent Test**: `cargo test -p buzz-crm`, CLI against a local relay.

---

## Edge Cases

- IF the 42100/42101 content is not valid JSON, has an unknown `v`, or exceeds bounds THEN the relay SHALL reject it with a message starting `invalid:`.
- IF a CRM event carries more than one `crm-id` or `expected-revision` tag THEN the relay SHALL reject it with a message starting `invalid:`.
- IF a CRM event's `h` channel is not the community's active CRM space (for kinds other than space creation) THEN the relay SHALL reject it with a message starting `invalid:`.
- WHEN a member is removed from the space channel THEN the relay SHALL reject that member's next CRM write, because the role is read inside the write transaction.
- WHEN the default warehouse moves THEN the demoted warehouse SHALL get the moving event's revision, so a stale client cannot re-promote it (`conflict:`).
- WHEN two creates of the same warehouse id race THEN exactly one SHALL win and the other SHALL get `conflict:`; warehouse writes are serialized per space.

---

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| -------------- | ----- | ----- | ------ |
| CRMF-01 | P1: Create a CRM space (AC1) | Execute | Pending |
| CRMF-02 | P1: Create a CRM space (AC2–AC4) | Execute | Pending |
| CRMF-03 | P1: Create a CRM space (AC5) | Execute | Pending |
| CRMF-04 | P1: Update settings / warehouses (AC1–AC2) | Execute | Pending |
| CRMF-05 | P1: Update settings / warehouses (AC3–AC5) | Execute | Pending |
| CRMF-06 | P1: Read the space (AC1–AC4) | Execute | Pending |
| CRMF-07 | P1: CRM data stays CRM data (AC1–AC4) | Execute | Pending |
| CRMF-08 | P2: Domain library and CLI (AC1–AC3) | Execute | Pending |
| CRMF-09 | P2: Domain library and CLI (AC4) | Execute | Pending |
