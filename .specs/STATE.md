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

- **Goal**: replicate YCLIENTS (salon/barbershop booking) on top of the retail CRM; 8-phase plan: 1 services + staff + schedules, 2 appointment journal, 3 visit payment via the cash desk, 4 public online booking, 5 reminders, 6 payroll + service analytics, 7 stock ledger + tech cards, 8 loyalty
- **Completed (phase 4, 2026-10-25)**: visit analytics `GET /crm/analytics/services` (per staff: visits, done, no-shows, revenue at booked prices, booked vs scheduled minutes; top services; new/returning/walk-in clients), desktop section «Аналитика мастеров»; stock branch `claude/project-thread-fm5rig` merged back (warehouse section, `buzz crm stock`, schema.sql repair)
- **Next**: payroll (staff percent/fixed rates over done visits), loyalty (cards, bonuses); online booking and reminders run on their own branches
- **Completed (phase 1)**: `duration_min` on bookable products (service/job, 5–720 step 5) and `kind=services|goods` on `GET /crm/products`; kinds 42131 (staff, CAS) and 42132 (working days of one staff member, days not listed keep their hours); migration 0055 + `schema.sql`; `GET /crm/staff`, `/crm/schedule`; SDK builders; NIP-CRM section; desktop sidebar sections Услуги / Сотрудники / График работы (week grid, day editor, weekday and shift templates); Postgres tests, relay e2e `e2e_crm_staff.rs`, unit tests, Playwright `crm-staff.spec.ts`
- **Completed (phase 2)**: kind 42133 appointments (CAS; staff row locked FOR UPDATE; working hours unless `outside_hours`; no overlap with visits holding time; `no_show`/`cancelled` free it), migration 0056, `GET /crm/appointments` (range or `customer_id`) and `GET /crm/slots`; desktop section «Журнал записи» (day grid per staff member, book from a free cell, slots in the time picker, status change, cancel); Postgres tests, relay e2e `e2e_crm_appointments.rs`, Playwright `crm-journal.spec.ts`
- **Completed (phase 5, branch `claude/project-thread-d1phu8`)**: visit reminders. Kinds 42150 (reminder rules, CAS; `booked` confirmation and `before` N minutes, templates with `{client}`…`{total}`) and 42151 (client Telegram link / opt-out), migration 0064 (`crm_reminder_settings`, `crm_client_telegram`, relay-owned `crm_reminder_deliveries`), relay worker `crm_reminders.rs` (plan with ON CONFLICT, leased claim, re-check live visit, backoff retries, fenced finish by `attempts`), Telegram transport or sandbox when `BUZZ_CRM_TELEGRAM_BOT_TOKEN` is unset, webhook `POST /crm/telegram/webhook` (secret header; `/start <code>`, `/stop`, `/resume`), `GET /crm/reminders`, `/crm/reminders/deliveries`, `/crm/parties/{id}/telegram`; desktop section «Напоминания» and the Telegram block on the customer card; Postgres tests, relay e2e `e2e_crm_reminders.rs` against a mock Bot API, Playwright `crm-reminders.spec.ts`. Not done: confirm/cancel buttons in Telegram, SMS, one bot per community host only
- **Completed (phase 3)**: sale payload `appointment_id` pays for a visit (relay links `sale_id`, status `done`, sale event becomes the visit revision; void unpays back to `arrived`); journal «Оплатить в кассе» opens `/crm/cash?visit=<id>` with the services in the cart; client card shows visit history; e2e + Playwright
- **Parallel thread**: stock ledger on `claude/project-thread-fm5rig` (migration 0060, kinds 42106–42108; sales deduct goods); merge before phase 3 touches the sale handler
- **Earlier on this branch**: customers, sales, analytics, cash desk (ATOL DTO10)
- **Not done**: slots count the edited visit as busy (dialog keeps its own time as an option); staff are not linked to channel members in the UI (`member` is stored only); no draft PR opened (the user decides)
- **Next step**: phase 4, staff and service analytics (revenue, visits, utilisation per staff member; top services); online booking and reminders requested as parallel threads
- **Blockers**: none
- **Branch**: claude/project-thread-yqqm67
