# crm-foundation Verifier Report (round 3)

## Validation: crm-foundation - FAIL

**Date**: 2026-09-27
**Spec**: `.specs/features/crm-foundation/spec.md` (CRMF-01..09, committed in `53b3501`)
**Protocol / decisions**: `docs/nips/NIP-CRM.md`, `docs/crm/adr/0001`, `docs/crm/adr/0002`
**Diff range**: round 3 is `b2fe852..53b3501` (`8e8ee89` fix, `53b3501` docs). The code part (`8e8ee89`) touches 6 files, +413/-25. The feature as a whole is `c8c7b95..53b3501`.
**Verifier**: independent sub-agent (the author is not the verifier). Worked read-only on the real tree. Every mutation ran in a scratch `git worktree` at `53b3501`.
**Iteration**: fix → re-verify loop 3 of 3. The loop bound is reached, so this goes to the user.

Every round-2 gap is resolved, both round-2 survivors (M2, M12) are now killed, and every test lane is green (60 passed, 0 failed). The verdict is still FAIL for two reasons:

- **N1 (blocking, new)**: the branch breaks CI before any test runs.
  - The e2e tests are marked `#[ignore = "requires a running relay and its Postgres"]` (`crates/buzz-test-client/tests/e2e_crm_space.rs:264`, `:424`, `:487`, `:685`).
  - The repository's PostgreSQL-discovery validator rejects that reason outside a `postgres_tests` module.
  - As a result, GitHub CI on this exact commit fails at "Validate PostgreSQL test discovery", and every required check reports failure.
  - The fix is one string: change the ignore reason.
  - The defect has been present since `c4d29d4` (round 1). Rounds 1 and 2 missed it.
- **M16 (minor)**: one fresh mutant survived. The CLI restore test does not assert which `expected-revision` it sends. The fix is one assertion.

---

## Round 2 → Round 3

| # | Round-2 gap | Status | Evidence |
|---|---|---|---|
| 1 | M2: the per-space serialization of warehouse writes is untested | **RESOLVED** | The new e2e `crm_space_concurrent_default_moves_serialize` (`e2e_crm_space.rs:686-741`) fires 8 concurrent default moves per round, for 3 rounds. Each move must be accepted or get 409 `conflict:` (`e2e_crm_space.rs:725-728`), and exactly one default must remain (`e2e_crm_space.rs:737`). Re-run M2 (`FOR SHARE`): **KILLED 10/10** (1 full-suite run + 9 isolated reps), every time in round 0 at `e2e_crm_space.rs:725` with `500 internal server error`. The clean build passed 11/11 (9 isolated reps + 2 full runs). The lock stays at the relay seam, and the race runs at the relay seam (lesson L-005) |
| 2 | M12 / R-AC4 membership clause untested | **RESOLVED** | The new relay Postgres test `api::crm::postgres_tests::space_read_requires_real_auth_and_relay_membership` (`crates/buzz-relay/src/api/crm.rs:330-382`) turns on `require_auth_token` and `require_relay_membership` (`crm.rs:336-337`) and signs real NIP-98 proofs. It asserts: no auth → 401 (`crm.rs:349`); non-member → 403 with `error == "relay_membership_required"` (`crm.rs:358-359`); relay member → 200 `{"exists":false,"can_create":false}` (`crm.rs:373-374`). Re-run M12: **KILLED** at `crm.rs:358` (left 200, right 403). The test is under `postgres_tests`, so the CI Postgres lane discovers it (`.config/nextest.toml:18-21`) |
| 3 | P2-AC4 CLI has no test | **RESOLVED** | `run()` now returns the output that `dispatch` prints (`crates/buzz-cli/src/commands/crm.rs:9-19`). Fake-relay (axum) tests drive the real `BuzzClient`: `space show` (`crm.rs:286-296`, output == GET body), `space init` (`crm.rs:299-342`: one 42100 event, `h` = channel, parsed settings and seed; printed `event_id`, `accepted`, `warehouse_id`). Fresh mutant M17 (init drops `warehouse_id`) KILLED at `crm.rs:338` |
| 4 | CLI restore path unreachable | **RESOLVED** | The relay accepts `GET /crm/space?archived=true` (`crates/buzz-relay/src/api/crm.rs:130-135`, `:155-158`, `:186-192`). The CLI update reads it (`crates/buzz-cli/src/commands/crm.rs:82-85`). Tests: e2e `e2e_crm_space.rs:597-606` (the archived row is listed with `archived == true` and the archive event's revision; M13 killed); CLI `crm.rs:345-370` (restore publishes `archived == false` and keeps the name; M15 killed). The recorded CLI run below restores an archived warehouse with real NIP-98 (exit 0) |
| 5 | E1 `contains` checks | **RESOLVED** | `e2e_crm_space.rs:457`, `:470` and `:478` now assert `message.starts_with("invalid:")` together with the reason |
| opt | Round-1 defect 4: deletion guard runs before the target-count check | OPEN (optional) | `crates/buzz-relay/src/handlers/ingest.rs` is unchanged since `c4d29d4`; the guard is still at `ingest.rs:2788` |
| opt | Stale `space_for_write` doc / always-true `exclusive` flag | **RESOLVED** (documented) | `crates/buzz-db/src/store/crm.rs:202-203` now names both modes. Both callers still pass `true` (`crates/buzz-relay/src/handlers/crm/space.rs:121`, `:156`); the flag is kept and documented |
| opt | Demoted-revision rule missing from NIP-CRM | **RESOLVED** | `docs/nips/NIP-CRM.md:112-115` |
| opt | 200 ms sleep in the create race test | **RESOLVED** | `wait_until_blocked_on` polls `pg_stat_activity` for a lock wait (`crates/buzz-db/src/store/crm/postgres_tests.rs:66-83`), called at `postgres_tests.rs:513`. The test fails loudly if the race never interleaves |

---

## New defect found in round 3

### N1 (blocking): the CRM e2e ignore reason fails CI's PostgreSQL-discovery validator

- **Where**: the four e2e tests carry `#[ignore = "requires a running relay and its Postgres"]` at `crates/buzz-test-client/tests/e2e_crm_space.rs:264`, `:424`, `:487` and `:685`.
- **Rule**: `scripts/check-postgres-test-discovery.py:369-374` requires any ignored test whose reason mentions Postgres to sit under a `postgres_tests` module or in a `postgres_*` binary.
- **Local reproduction**: `scripts/test-postgres-test-discovery.sh` exits 1 and lists exactly these four tests. `scripts/postgres-test-packages.sh` prints no packages.
- **Real failure**: CI run 36333155174 on `53b3501` (PR #2), job "Detect Changed Paths":
  - Step 6 "Validate PostgreSQL test discovery" (`.github/workflows/ci.yml:96-100`) fails with the same four messages.
  - "Rust", "Relay and PostgreSQL" and "PostgreSQL Domain" are skipped.
  - Every required gate fails: Unit Tests, Rust Lint, PostgreSQL Tests, Backend Integration (relay e2e), Relay E2E and the others.
  - Even if that step passed, `.github/workflows/_ci-relay.yml:97-104` would stop with "no PostgreSQL test packages were discovered".
- **Effect**: none of the CRM tests, and none of the fork's other Rust tests, have run in GitHub CI on this branch. Round 2's "e2e runs in CI" (L-003) holds on paper but not in practice.
- **Fix**: use a reason that does not name Postgres, such as `#[ignore = "requires a running relay"]`. `crates/buzz-test-client/tests/e2e_event_reminder.rs:162` uses bare `#[ignore]` for the same kind of test.
- **Verified in scratch**: with that change, `scripts/test-postgres-test-discovery.sh` prints "PostgreSQL test discovery convention checks passed", and the package list is `buzz-admin buzz-audit buzz-db buzz-deletion buzz-push-gateway buzz-relay buzz-search buzz-workflow`. The scratch change was reverted.
- **Unrelated**: the same PR's "Docker image" failure comes from GHCR permissions (`denied: permission_denied: The requested installation does not exist` while exporting cache). The image compiled.

---

## Environment note

- **Disk**: 5.0 GB free at start, 4.3 GB at the lowest, 11 GB at the end (another session freed space). Builds used the shared `CARGO_TARGET_DIR=/home/user/crm/target` with `CARGO_INCREMENTAL=0`. No other target dir was created.
- **Relays**:
  - The author's relay on `:3000` (DB `buzz_crm_e2e`) was not touched and still answered 200 at the end.
  - My relays ran on `:3100` (health `:8180`, metrics `:9202`, Redis db 1) against `buzz_crm_verify`. That DB was migrated through `BUZZ_AUTO_MIGRATE=true` (to version 50) and seeded with hosts `localhost:3100`, `127.0.0.1:3100`, `localhost` and `127.0.0.1`.
- **Cleanup**:
  - Relay stopped by PID, `buzz_crm_verify` dropped, Redis db 1 flushed (it was empty before). No `buzz_nt_*` template databases remain.
  - Worktree removed and pruned.
  - After the last mutant I rebuilt every mutated unit from clean `53b3501` sources: relay binary and lib, relay lib-test binary, and CLI lib and test binary. So `target/debug/deps` holds clean-HEAD artifacts.
  - I restored the author's uplifted `target/debug/{buzz-relay,buzz,libbuzz_relay.rlib,libbuzz_cli.rlib}` and their `.d` files from hard links taken before the first build. Note: the restored `buzz-relay` is the author's 11:39 build, which predates `8e8ee89`. A `cargo build -p buzz-relay` in the main tree refreshes it.
  - The real-tree `git status --porcelain` matches the pre-sensor baseline. The only entry is this untracked report. HEAD is still `53b3501`.

---

## Task Completion

No `tasks.md` exists (the Tasks phase was skipped). Round 3 landed as `8e8ee89` (fix) and `53b3501` (docs, spec, lessons, handoff). Nothing is partial in git. The spec and NIP-CRM are now committed.

---

## Spec-Anchored Acceptance Criteria

Legend: COVERED = the assertion targets the spec outcome; PARTIAL = covered only at a lower seam, or the assertion is weaker than the spec; MISSING = no `file:line` test evidence. Line numbers are at `53b3501`.

### P1: Create a CRM space (CRMF-01..03)

| AC | Spec-defined outcome | `file:line` + assertion | Verdict |
|---|---|---|---|
| C-AC1 | Accepted; `crm_spaces` row + named default warehouse in one tx | `e2e_crm_space.rs:289-293` accepted with empty message; `e2e_crm_space.rs:300-307` channel, settings, `revision == create.id`, default warehouse; `postgres_tests.rs:105-130` | COVERED |
| C-AC2 | Not community owner/admin → `restricted:` | `e2e_crm_space.rs:278` `starts_with("restricted:")`; `crates/buzz-relay/src/handlers/crm/decide.rs:138-165` full matrix | COVERED |
| C-AC3 | Open/DM/archived/TTL → `invalid:` | `e2e_crm_space.rs:436` exact `"invalid: a CRM space channel must be private"`; `decide.rs:99-135` exact messages for the other cases | COVERED (DM/archived/TTL at the unit seam) |
| C-AC4 | Second space → `conflict:` | `e2e_crm_space.rs:320` `starts_with("conflict:")` with 409; `postgres_tests.rs:132-167` | COVERED |
| C-AC5 | Identical event twice → `duplicate:`, no second row | `e2e_crm_space.rs:297` `message == "duplicate:"` | COVERED ("no second row" rests on the unique index) |

### P1: Update settings and manage warehouses (CRMF-04..05)

| AC | Spec-defined outcome | `file:line` + assertion | Verdict |
|---|---|---|---|
| U-AC1 | Settings replaced **and revision = new event id** | `e2e_crm_space.rs:526-529` `revision == first.id`; `postgres_tests.rs:172-189` | COVERED (M3 killed in round 2) |
| U-AC2 | Stale space **or** warehouse revision → `conflict:`, row unchanged | Space: `e2e_crm_space.rs:533` 409 + `starts_with("conflict:")`. Warehouse: `e2e_crm_space.rs:356` and `e2e_crm_space.rs:582`; `postgres_tests.rs:189` `RevisionMismatch` | COVERED ("row unchanged" for the space holds through tx rollback, `crates/buzz-relay/src/handlers/crm/mod.rs:76-79`) |
| U-AC3 | New warehouse UUID → created | `e2e_crm_space.rs:350-377`; `postgres_tests.rs:239-260` | COVERED |
| U-AC4 | Member, guest or bot × 42100/42101 → `restricted:` | `e2e_crm_space.rs:339` (member × 42101), `e2e_crm_space.rs:621` (guest and bot × 42101), `e2e_crm_space.rs:633` (member × 42100); `crates/buzz-crm/src/authz.rs:168-181` | COVERED (guest/bot × 42100 at the unit seam) |
| U-AC5 | Clearing or archiving the default → `invalid:` | `e2e_crm_space.rs:547` exact `"invalid: make another warehouse the default first"`; archiving the default at `postgres_tests.rs:285-296` | COVERED |

### P1: Read the space (CRMF-06)

| AC | Spec-defined outcome | `file:line` + assertion | Verdict |
|---|---|---|---|
| R-AC1 | Member → 200 with channel_id, settings, revision, my_role, capabilities, **non-archived** warehouses; archived ones too with `?archived=true` | `e2e_crm_space.rs:300-307`; `e2e_crm_space.rs:596` archived row absent by default; `e2e_crm_space.rs:597-606` listed with `?archived=true` (`archived == true`, revision == archive event id); body shape `crates/buzz-relay/src/api/crm.rs:201-226` + `crm.rs:235-273` | COVERED (M4 killed in round 2, M13 killed now). The member body comes over dev `X-Pubkey` in the e2e. The real NIP-98 member path is shown by the recorded CLI run, and real NIP-98 admission by `crm.rs:373` |
| R-AC2 | Non-member → `{"exists":true,"member":false}` | `e2e_crm_space.rs:310-313` exact JSON | COVERED |
| R-AC3 | No space → `{"exists":false,"can_create":<bool>}` | `e2e_crm_space.rs:283` exact `{"exists":false,"can_create":true}`, `e2e_crm_space.rs:285` outsider `false`; `crm.rs:374` relay member (not admin) `false` | COVERED |
| R-AC4 | No auth → 401; membership required and caller not a relay member → as workflow reads (403 `relay_membership_required`) | `crm.rs:349` 401; `crm.rs:358-359` 403 + `relay_membership_required`; `crm.rs:381` malformed query without auth still 401; `e2e_crm_space.rs:501` 401 on the dev relay | **COVERED** (M12 and M14 killed) |

### P1: CRM data stays CRM data (CRMF-07)

| AC | Spec-defined outcome | `file:line` + assertion | Verdict |
|---|---|---|---|
| D-AC1 | Kind 5 or 9005 on a CRM event → `invalid:` | `e2e_crm_space.rs:416` (kind 5) and `e2e_crm_space.rs:662` (9005), both `starts_with("invalid: CRM events cannot be deleted")` | COVERED |
| D-AC2 | 9002 open/ttl/archived on the space → `invalid:`; DB refuses such updates | `e2e_crm_space.rs:408-412` exact private message; `crates/buzz-relay/src/handlers/crm/guards.rs:165-199`; trigger `postgres_tests.rs:532-557` (`expect_err` at `postgres_tests.rs:553`) | COVERED |
| D-AC3 | 9008 on the space → `invalid:` | `guards.rs:194-198` exact message; DB refuses `deleted_at` (`postgres_tests.rs:532-557`) | COVERED (unit + DB seams) |
| D-AC4 | CRM kinds out of push (tested); feed/unread use chat allowlists | `guards.rs:202-209`; `crates/buzz-db/src/store/feed.rs:106-110`, `feed.rs:274-276` | COVERED |

### P2: Domain library and CLI (CRMF-08..09)

| AC | Spec-defined outcome | `file:line` + assertion | Verdict |
|---|---|---|---|
| P2-AC1 | `Money` checked add/sub/sum, `Qty` bounded constructors, `line_amount` errors | `crates/buzz-crm/src/money.rs:188-229` | COVERED |
| P2-AC2 | INN 10/12 checksums | `crates/buzz-crm/src/validate.rs:101-115` | COVERED |
| P2-AC3 | ADR-0002 table for every role × action | `crates/buzz-crm/src/authz.rs:168-181` | COVERED |
| P2-AC4 | `space show` prints the GET JSON; `space init` publishes 42100 and prints `{event_id, accepted, message}` | `crates/buzz-cli/src/commands/crm.rs:292-295` output == GET body; `crm.rs:317-333` one 42100 event with `h` = channel and the given settings; `crm.rs:336-341` printed `event_id`, `accepted`, `warehouse_id` | **COVERED** (M17 killed). Nit: printed `message` is not asserted. It passes through unchanged in `create_response_with_id_if_accepted` (`crates/buzz-cli/src/client.rs:1509-1516`) |

**Totals**: 21 ACs, 21 COVERED, 0 PARTIAL, 0 MISSING. No spec-precision gaps.

---

## Edge Cases

- [x] E1: bad JSON, unknown `v`, out of bounds → `invalid:` (`e2e_crm_space.rs:457`, `:470`, `:478`, all `starts_with("invalid:")`).
- [x] E2: duplicate `crm-id` / `expected-revision` → `invalid:` (`crates/buzz-crm/src/tags.rs:102-110`, unit seam).
- [x] E3: `h` is not the active space → `invalid:`. Asserted exactly at `e2e_crm_space.rs:646`.
- [x] E4: a removed member's next write is rejected (`postgres_tests.rs:381-431`, store seam).
- [x] E5: the demoted default takes the moving event's revision (`postgres_tests.rs:333-344`, `e2e_crm_space.rs:572`, and the stale re-promote → 409 at `e2e_crm_space.rs:582`).
- [x] E6: racing creates → exactly one wins; warehouse writes serialize per space.
  - Creates: `postgres_tests.rs:470-527`, now with a real lock-wait barrier at `postgres_tests.rs:513`.
  - Serialization: `e2e_crm_space.rs:686-741` (M2 killed 10/10).

---

## Discrimination Sensor

Sensor depth: P0 tier (auth and data integrity). This round re-ran the two round-2 survivors and added 6 fresh behavior-level mutants in code that `8e8ee89` touched: 8 executed. Each was applied in the scratch worktree, built, tested and reverted before the next. The worktree porcelain was 0 after every revert.

- **Relay e2e mutants**: rebuilt the relay (about 2-3 min), restarted it on `:3100`, then ran `cargo test -p buzz-test-client --test e2e_crm_space -- --ignored --test-threads=1`.
- **Relay Postgres mutants**: `scripts/postgres-test-run.sh -p buzz-relay --lib -E 'test(/crm::/)'`.
- **CLI mutants**: `cargo test -p buzz-cli --lib crm`.

| # | Site | Mutation | Lane | Outcome |
|---|---|---|---|---|
| M2 (re-run) | `crates/buzz-relay/src/handlers/crm/space.rs:156` | Warehouse writes lock the space `FOR SHARE` (`false`) | e2e ×10 | **KILLED 10/10** by `crm_space_concurrent_default_moves_serialize` at `e2e_crm_space.rs:725` (`round 0: 500 internal server error`) |
| M12 (re-run) | `crates/buzz-relay/src/api/crm.rs:94-102` | Drop `enforce_relay_membership` | pg (relay) | **KILLED** by `api::crm::postgres_tests::space_read_requires_real_auth_and_relay_membership` at `crm.rs:358` (left 200, right 403) |
| M13 | `crates/buzz-relay/src/api/crm.rs:191` | Relay ignores `?archived=true` (always lists live rows only) | e2e | **KILLED** by `crm_space_cas_roles_and_guards` at `e2e_crm_space.rs:604` (`archived warehouse listed`) |
| M14 | `crates/buzz-relay/src/api/crm.rs:148-158` | Parse the query **before** admission (malformed query pre-empts 401) | pg (relay) | **KILLED** at `crm.rs:381` (left 400, right 401) |
| M15 | `crates/buzz-cli/src/commands/crm.rs:84` | CLI update reads `/crm/space` without `?archived=true` (restore impossible again) | cli | **KILLED** by `warehouse_update_can_restore_an_archived_warehouse` at `crm.rs:365` (usage error) |
| M16 | `crates/buzz-cli/src/commands/crm.rs:144` | For an archived row, the CLI sends a well-formed but wrong `expected-revision` (`"00"×32`) | cli | **SURVIVED** (7/7 pass). `crm.rs:345-370` asserts `archived` and `name` of the restore event but not its `expected-revision` tag |
| M16b | `crates/buzz-cli/src/commands/crm.rs:144` | Natural variant: never fall back to the row's revision (`unwrap_or_default()`) | cli | **KILLED** by `renaming_the_default_keeps_it_default` (`crm.rs:195`) and by the restore test (`crm.rs:365`) |
| M17 | `crates/buzz-cli/src/commands/crm.rs:48-53` | `space init` prints the relay response without `warehouse_id` | cli | **KILLED** by `space_init_publishes_a_space_event_and_prints_the_warehouse_id` at `crm.rs:338` (left `Null`) |

**Sensor outcome**: 8 executed, 7 killed, 1 survived (M16).

M16 is an adversarial variant: the natural fault (M16b) is killed. It still exposes a real blind spot:

- A regression that restores with the wrong CAS revision would pass the CLI tests.
- At runtime it fails closed: the relay answers 409 `conflict:`, as the recorded run's step 10 shows. So no data is at risk.

---

## Interactive UAT: recorded CLI run (P2-AC4 and the restore fix)

The binary was `buzz` built from clean `53b3501`, run against my `:3100` relay with a fresh key. It used real NIP-98, with no `X-Pubkey`. Output is verbatim (trimmed):

| # | Command | Result |
|---|---|---|
| 1 | `buzz crm space show` (no space, not admin) | `{"can_create":false,"exists":false}`, exit 0 |
| 2 | `buzz crm space show` (after the community admin grant) | `{"can_create":true,"exists":false}`, exit 0 |
| 3 | `buzz crm space init --channel <private stream> --fiscalization optional` | `{"accepted":true,"event_id":"566df8da…","message":"","warehouse_id":"f01cbbb8-…"}`, exit 0 |
| 4 | `buzz crm space show` | member body: `my_role":"owner"`, 8 capabilities, `revision` = init event id; warehouses `[Основной склад (default)]`, exit 0 |
| 5 | `… warehouses update --warehouse <default> --name "Главный склад"` | accepted, exit 0 |
| 6 | `… warehouses update --warehouse <shop> --archived true` | accepted, exit 0 |
| 7 | `buzz crm space show` | warehouses `[Главный склад (default)]`: the archived shop is hidden, exit 0 |
| 8 | `… warehouses update --warehouse <shop> --archived false` | `{"accepted":true,"event_id":"39bf6914…","message":""}`, **exit 0**. Round-2 step 7 failed here with exit 1 |
| 9 | `buzz crm space show` | warehouses `[Главный склад (default), Магазин (live)]`, exit 0 |
| 10 | `… update --warehouse <shop> --archived true --revision <wrong>` | `{"error":"relay_error","message":"relay error 409: conflict: warehouse changed since it was loaded"}`, exit 2 |

This is runtime evidence; P2-AC4's test evidence is the CLI test rows above. Relay-backed behavior still needs the human test required by the "Before opening a PR" checklist (see Next steps).

---

## Code Quality (round-3 diff, `8e8ee89`)

| Principle | Status |
|---|---|
| Minimum code / surgical changes | OK. Six files, each tied to a round-2 gap. `dispatch` → `run` is the smallest change that makes CLI output testable |
| Matches patterns | OK. Query parsing after admission mirrors the workflow reads (`crates/buzz-relay/src/api/mod.rs:48-56`). The router test reuses `bridge_handler_test_state` (visibility widened to `pub(crate)` in `crates/buzz-relay/src/api/bridge.rs:2890`, `:4269`). The lock-wait barrier replaces a sleep |
| No `unwrap`/`expect` in production paths | OK (only in tests) |
| New public API documented | OK (`SpaceParams` at `crates/buzz-relay/src/api/crm.rs:130-135`) |
| rustfmt / clippy | OK. `cargo fmt --all -- --check` is clean. `cargo clippy -p buzz-relay -p buzz-cli -p buzz-db -p buzz-test-client -p buzz-crm -p buzz-sdk --all-targets -- -D warnings` is clean |
| Repo CI conventions | **FAIL**: N1 (ignore reason vs. `scripts/check-postgres-test-discovery.py`) |
| Falsifiable at the production seam (Review-Proven Rule 3) | OK for M2, M12, M13, M14, M15 and M17. Gap: M16 (the restore test does not bind the CAS revision) |
| Dead data | Nit. `e2e_crm_space.rs:703` stores `event.id.to_hex()` in `warehouses`, but the loop reads only the id (`map(|(id, _)| …)`) |
| Test duplication | Nit. The new `get_json` helper (`e2e_crm_space.rs:669-678`) duplicates `get_space` except for the path |
| Flakiness risk | Low. The concurrent e2e relies on 8 parallel HTTP writes interleaving. Locally the mutant failed in round 0 on 10/10 runs and the clean build passed 11/11. CI hardware may interleave less, and the test would then pass vacuously for the mutant, but it can never false-fail on correct code |
| Guidelines followed | `AGENTS.md` (Review-Proven Rules 1-5, Quality Gates), `TESTING.md` |

---

## Gate Check (clean scratch worktree at `53b3501`)

| Lane | Command | Outcome |
|---|---|---|
| Domain | `cargo test -p buzz-crm` | 23 passed |
| Relay unit | `cargo nextest run -p buzz-relay --lib -E 'test(/crm::/)'` | 7 passed |
| Postgres (relay + db) CRM + migrations | `scripts/postgres-test-run.sh -p buzz-relay -p buzz-db --lib -E 'test(/crm::/) \| test(/runtime::migration::/) \| test(/deletion_surface/)'` | 16 passed (9 CRM, including the new relay router test, + 7 migration). Run twice: before the sensor and after the clean rebuild |
| E2e | `cargo test -p buzz-test-client --test e2e_crm_space -- --ignored --test-threads=1` against `ws://localhost:3100` | 4 passed, before the sensor and again on the final clean build. The concurrent test alone passed 9/9 more times |
| SDK | `cargo test -p buzz-sdk --lib crm` | 3 passed |
| CLI | `cargo test -p buzz-cli --lib crm` | 7 passed (before the sensor and after the clean rebuild) |
| Format / lint | `cargo fmt --all -- --check`; `cargo clippy … --all-targets -- -D warnings` | clean |
| **CI discovery contract** | `scripts/test-postgres-test-discovery.sh` (the CI step at `.github/workflows/ci.yml:96-100`) | **FAILED** (exit 1, N1). Confirmed in GitHub CI run 36333155174 |

- Tests: 60 passed, 0 failed, 0 skipped. One CI contract lane fails.
- Feature tests: 53, up from 48 in round 2 (+1 relay router test, +1 e2e, +3 CLI). None were deleted.
- One assertion was intentionally changed: the create-race test's `sleep(200ms)` became a lock-wait barrier. This strengthens it.
- No assertion was weakened. The E1 checks were strengthened from `contains` to `starts_with("invalid:") && contains(...)`.

---

## Remaining gaps (ranked fix tasks)

1. **N1: CI discovery failure** (Blocker: every required CI check on PR #2 is red and no Rust test runs in CI).
   - Where: `crates/buzz-test-client/tests/e2e_crm_space.rs:264`, `:424`, `:487`, `:685`.
   - Fix: replace `#[ignore = "requires a running relay and its Postgres"]` with a reason that does not mention Postgres or Redis, such as `#[ignore = "requires a running relay"]`. Keep the `RELAY_URL` / `DATABASE_URL` note in the module docs.
   - Done when: `scripts/test-postgres-test-discovery.sh` exits 0 (verified in scratch), and the PR's "Detect Changed Paths" job passes, so "Backend Integration (relay e2e)" actually runs `binary(e2e_crm_space)`.
2. **M16: the restore test does not bind the CAS revision** (Minor; fails closed at runtime).
   - Where: `crates/buzz-cli/src/commands/crm.rs:345-370`.
   - Fix: also assert that the published event's `expected-revision` tag equals `"ab".repeat(32)`, the archived row's revision in the fake relay body. For example, use `buzz_crm::tags` or filter `events[0].tags` the way `crm.rs:320-325` filters `h`.
   - Done when: M16 fails the test.
3. **Optional**:
   - Round-1 defect 4: move the CRM deletion guard after the single-target check (`ingest.rs:2788`).
   - Drop the unused revision in `e2e_crm_space.rs:703`.
   - Assert the printed `message` in the init test (`crm.rs:336-341`).

---

## Requirement Traceability Update (proposed; the Verifier does not edit spec.md)

| Requirement | Round 2 | Round 3 |
|---|---|---|
| CRMF-01 | Verified | Verified |
| CRMF-02 | Verified | Verified |
| CRMF-03 | Verified | Verified |
| CRMF-04 | Verified | Verified |
| CRMF-05 | Needs Fix (M2) | **Verified** (E6 at the relay seam; M2 killed) |
| CRMF-06 | Needs Fix (M12) | **Verified** (R-AC4 with real NIP-98 and membership; archived listing) |
| CRMF-07 | Verified | Verified |
| CRMF-08 | Verified | Verified |
| CRMF-09 | Needs Fix (no test) | **Verified** at the test seam; M16 is a minor gap in the restore path, which is not an AC |

All 9 requirements meet their ACs locally. The feature is not done until N1 is fixed, because the project's CI gate is red. The loop bound (3 iterations) is reached, so I escalate to the user.

---

## Lessons to distill (not recorded)

This run was instructed not to run `lessons.py`. L-001..L-008 are already recorded. New candidates for the orchestrator:

1. (`gate_fail`, scope `ci`) An `#[ignore = "…"]` reason that mentions Postgres or Redis puts the test into PostgreSQL discovery. Outside a `postgres_tests` module it fails `scripts/test-postgres-test-discovery.sh` and blocks all of CI. Run that script whenever you add an ignored test. Evidence: `e2e_crm_space.rs:264`, CI run 36333155174.
2. (`gate_fail`, scope `verification`) Wiring a test into a CI lane (L-003) is not the same as CI running it. Verify with the PR's actual CI run, or run the repo's CI contract scripts locally, before calling a lane green. Rounds 1-2 accepted the `_ci-relay.yml` wiring without either.
3. (`surviving_mutant`, scope `cli`) A fake-relay CLI test for a CAS write must assert the `expected-revision` tag it sends, not only the payload. Evidence: M16, `crates/buzz-cli/src/commands/crm.rs:345-370`.

---

## Summary

**Overall**: FAIL. Every functional gap from rounds 1-2 is closed, but a CI-blocking defect remains that all earlier rounds missed.

- **Spec-anchored check**: 21 of 21 ACs covered; 0 spec-precision gaps.
- **Sensor**: 8 executed, 7 killed, 1 survived (M16, minor). Both round-2 survivors (M2, M12) are now killed.
- **Gate**: 60 tests passed, 0 failed; fmt and clippy clean. The CI discovery contract fails (N1).

**What works now**:
- Per-space serialization of warehouse writes, proven at the relay seam.
- `GET /crm/space` auth and membership under real NIP-98 with the deployment flags on.
- The archived listing and a working CLI restore.
- CLI `space show` / `space init` under executable tests.
- Strict `invalid:` prefixes and a non-sleeping create-race barrier.

**Next steps**:
1. Fix N1: one string, four lines. Confirm "Detect Changed Paths" and "Backend Integration (relay e2e)" go green on PR #2.
2. Add the M16 assertion.
3. Hand the human the manual-test steps that `AGENTS.md` "Before opening a PR" requires. Agents never add `buzz-review-completed`.
