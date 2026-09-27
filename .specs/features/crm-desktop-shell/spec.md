# CRM Desktop Shell Specification

## Problem Statement

The relay now hosts a CRM space (branch `crm/foundation`), but the desktop app
has no way to reach it. Staff need one CRM entry point in the sidebar that
explains what state their community is in (no space yet, no access, ready) and
lets an admin create and configure the space and its warehouses. Every later CRM
screen (catalog, stock, POS, analytics) plugs into this shell.

## Goals

- [ ] A "CRM" sidebar entry behind the `crm` preview feature opens `/crm/...`
      routes rendered with the existing page patterns and Russian copy.
- [ ] The gate screen shows the correct state for: loading, load error (with
      retry), no space + may create, no space + may not create, space exists but
      caller is not a member, ready.
- [ ] A community admin can create the space (existing private channel or a new
      one) and then edit settings and warehouses; members see read-only settings.

## Out of Scope

| Feature | Reason |
| ------- | ------ |
| Products, stock, customers, documents, POS, analytics screens | Later branches |
| ATOL device settings ("Касса на этом компьютере") | `crm/pos` |
| Full i18n library | AD-005: dictionary module only |
| Requesting access from inside the app (DM to admin) | Members are added through the existing channel member UI |

---

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale | Confirmed? |
| --------------------- | -------------- | --------- | ---------- |
| Preview flag default | `crm` enabled by default | The fork's purpose is the CRM; users can switch it off in Settings → Experiments | y (fork goal) |
| Routing | `/crm` redirects to the first section; `/crm/$section` renders a known section, unknown sections redirect | Matches flat route style (`/projects`, `/projects/$projectId`) | y |
| Sections in this branch | `settings` only; later branches append to one section registry | Keeps each branch reviewable | y |
| Creating the space channel | Offer existing private stream channels I am a member of, or create a new private stream channel named `crm` | The relay requires owner/admin of a private stream channel; creation via the existing `create_channel` command makes the creator owner | y |
| Relay errors in UI | Map `restricted:` / `invalid:` / `conflict:` prefixes to Russian messages; show the raw relay text as detail | Relay messages are English protocol text | y |
| Live refresh | Subscribe to CRM kinds with `#h` = space channel and invalidate `["crm"]` queries | Keeps several open clients consistent | y |
| Timezone choices | Fixed list of Russian IANA zones plus the current value | Avoids free-text errors; relay still validates | y |

**Open questions:** none - all resolved or logged above.

---

## User Stories

### P1: Enter the CRM ⭐ MVP

**User Story**: As a staff member, I want a CRM entry in the sidebar so that I can reach retail tools.

**Acceptance Criteria**:

1. WHERE the `crm` preview feature is enabled the sidebar SHALL show a "CRM" item (`data-testid="open-crm-view"`) that navigates to `/crm`.
2. WHERE the `crm` preview feature is disabled the sidebar SHALL NOT render the CRM item.
3. WHEN `/crm` opens THEN the app SHALL navigate to the first CRM section (`/crm/settings` in this branch).
4. IF the route names an unknown section THEN the app SHALL redirect to the first section.
5. WHILE a `/crm/...` route is open the shell SHALL mark the CRM sidebar item active.

### P1: Gate states ⭐ MVP

**User Story**: As any community member, I want the CRM to tell me what state it is in so that I know what to do next.

**Acceptance Criteria**:

1. WHILE the space is loading the view SHALL show a skeleton with `aria-busy="true"`.
2. IF loading the space fails THEN the view SHALL show the Russian load-error text and a "Повторить" button that refetches.
3. WHEN no space exists and `can_create` is true THEN the view SHALL show the create-space form.
4. WHEN no space exists and `can_create` is false THEN the view SHALL show text asking a community administrator to create the CRM, and no form.
5. WHEN a space exists and the caller is not a member THEN the view SHALL show text asking to be added to the CRM channel, and no settings.
6. WHEN the caller is a member THEN the view SHALL render the requested section.

### P1: Create and configure the space ⭐ MVP

**User Story**: As a community admin, I want to create the CRM space and manage warehouses so that the shop is set up.

**Acceptance Criteria**:

1. WHEN the admin submits the create form with an existing private channel THEN the app SHALL call `crm_create_space` with that channel id, the chosen settings and warehouse name, and SHALL show the ready state after success.
2. WHEN the admin chooses "new channel" THEN the app SHALL create a private stream channel named `crm` first and then call `crm_create_space` with its id.
3. IF `crm_create_space` fails THEN the form SHALL stay filled and show the mapped Russian error in an element with `role="alert"`.
4. WHEN a user with `manage_space` saves changed settings THEN the app SHALL call `crm_update_space` with the current `revision` as expected revision.
5. WHERE the caller lacks `manage_space` / `manage_warehouses` the settings and warehouse controls SHALL be read-only (disabled or hidden).
6. WHEN a user with `manage_warehouses` adds, renames, sets default or archives a warehouse THEN the app SHALL call `crm_write_warehouse` with the warehouse's current revision (none for create) and SHALL refresh the list.
7. IF a warehouse write returns a `conflict:` error THEN the app SHALL show the "данные изменились, обновите" message and refetch the space.

### P2: Russian copy stays in one place

**Acceptance Criteria**:

1. The CRM feature SHALL keep user-visible Russian strings only in `features/crm/i18n/ru.ts`; a test SHALL fail if a `.tsx` file under `features/crm` contains Cyrillic.
2. The desktop CRM kind constants SHALL equal the relay's `kind.rs` values; a test SHALL fail on drift.

---

## Edge Cases

- IF the channel list has no eligible private stream channel THEN the create form SHALL default to "new channel".
- WHEN a live CRM event arrives for the space channel THEN the app SHALL invalidate CRM queries at most once per 300 ms burst.
- IF the community switches THEN CRM queries SHALL not leak (per-community QueryClient) and the live subscription SHALL be disposed.

---

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| -------------- | ----- | ----- | ------ |
| CRMD-01 | P1: Enter the CRM (AC1–AC5) | Execute | Pending |
| CRMD-02 | P1: Gate states (AC1–AC6) | Execute | Pending |
| CRMD-03 | P1: Create and configure (AC1–AC3) | Execute | Pending |
| CRMD-04 | P1: Create and configure (AC4–AC7) | Execute | Pending |
| CRMD-05 | P2: Russian copy / kinds drift | Execute | Pending |
