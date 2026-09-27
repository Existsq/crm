# STATE

## Decisions

### AD-001
- **Decision**: CRM data is written as regular signed events (kinds 42100–42199, `h`-tagged to the CRM space channel) and the relay maintains `crm_*` projection tables in the same transaction as the event insert (reaction-style ingest branch).
- **Reason**: Stock checks and moving-average cost must be atomic with the write; custom-tag REQ filters are applied after SQL `LIMIT`; addressable events replace per author, which would fork shared master data.
- **Trade-off**: The relay gains domain logic ("relay is the pipe" in VISION.md is deliberately bent); every new table must follow the multi-tenant rules (community_id-leading keys, write fence, deletion manifests).
- **Scope**: all CRM features — relay (`handlers/crm`, `api/crm`), buzz-db (`store/crm`), buzz-crm, SDK, CLI, desktop. See docs/crm/adr/0001.
- **Date**: 2026-09-27
- **Status**: active

### AD-002
- **Decision**: The CRM space is a `crm_spaces` row bound to an ordinary private stream channel; reads = channel membership, writes = role × action table in `buzz_crm::authz`, checked uncached inside the posting transaction.
- **Reason**: Reuses existing membership, invites and agent onboarding; avoids a new `ChannelType` that would ripple through the DB enum and every client.
- **Trade-off**: Guards are needed so the channel cannot be made open, given a TTL or deleted while it is an active CRM space.
- **Scope**: relay authz for all CRM kinds and `GET /crm/*`; desktop gate screen. See docs/crm/adr/0002.
- **Date**: 2026-09-27
- **Status**: active

### AD-003
- **Decision**: Fiscalization talks to the local ATOL ДТО10 web server from the Tauri Rust side via the Tauri-free crate `buzz-atol`, with a durable SQLite outbox; the sale is posted to the relay before fiscalization and the ATOL task uuid is `UUIDv5("sell:" + doc_id)`.
- **Reason**: Physical cash register at the cashier's machine; no lost or double receipts across crashes (AGENTS.md rule 1); testable without Tauri via `buzz-atol-emulator`.
- **Trade-off**: No offline POS; ambiguous states (`blocked`, purged results) need a human decision (`needs_attention`).
- **Scope**: `crm/atol-core`, `crm/pos`. See docs/crm/adr/0003.
- **Date**: 2026-09-27
- **Status**: active

### AD-004
- **Decision**: Money is integer kopecks (`i64`), quantities are integer thousandths of a unit; the same rounding rules are shared by Rust and TypeScript through golden vectors in `crates/buzz-crm/fixtures/money_vectors.json`.
- **Reason**: No float drift between relay, CLI and desktop; fiscal amounts must match to the kopeck.
- **Trade-off**: Every input/format path converts explicitly; TS uses BigInt for arithmetic.
- **Scope**: buzz-crm, SDK, relay, desktop `features/crm/lib/money.ts`.
- **Date**: 2026-09-27
- **Status**: active

### AD-005
- **Decision**: CRM UI strings are Russian and live only in `desktop/src/features/crm/i18n/ru.ts` (no i18n library); a node test fails on Cyrillic in CRM `.tsx` files outside `i18n/`.
- **Reason**: The users are Russian-speaking retail staff; the rest of Buzz stays English, and a single dictionary keeps a later i18n migration cheap.
- **Trade-off**: Two UI languages coexist in one app.
- **Scope**: all desktop CRM screens.
- **Date**: 2026-09-27
- **Status**: active

## Handoff

- **Feature**: CRM fork bootstrap (branch 0: tooling + memory)
- **Phase / Task**: tooling setup done; next is `crm/foundation`
- **Completed**: graphify project install + SessionStart hook, agent-skills install, VISION_CRM.md, ADR-0001..0003
- **In-progress** (file:line): none
- **Next step**: Specify `.specs/features/crm-foundation/spec.md`, then implement buzz-crm + kinds + migration 0050 + relay handlers on branch `crm/foundation`.
- **Blockers**: none
- **Uncommitted files**: none
- **Branch**: cl/charming-shannon-af17bu
