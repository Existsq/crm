# LESSONS - auto-maintained by scripts/lessons.py

> Machine-owned. Do NOT hand-edit. Changes are overwritten on the next `lessons.py` write.
> Canonical state lives in `.specs/lessons.json`. Edit lessons only via the script.
> promote_threshold=2 distinct features · window_days=45 · quarantine_threshold=2

## Confirmed (load these at Specify/Design)

Corroborated across multiple features. Safe to apply as guidance.

_none_

## Candidates (under observation - do NOT load as guidance yet)

Seen once or not yet corroborated. Tracked, not trusted.

### L-001 - After every relay CRM write, assert through the public read that the revision equals the new event id; store-level CAS tests do not prove the relay passes it.
- signal: `surviving_mutant` · recurrence: 1 feature(s) · scope: `relay/crm` · harmful: 0
- features: crm-foundation
- evidence: crates/buzz-relay/src/handlers/crm/space.rs:update revision (relay/crm)
- last seen: 2026-09-27T10:43:19Z

### L-002 - When a write changes a sibling row (moving a default flag), advance that row's revision in the same transaction or CAS misses the change.
- signal: `surviving_mutant` · recurrence: 1 feature(s) · scope: `db/crm` · harmful: 0
- features: crm-foundation
- evidence: crates/buzz-db/src/store/crm.rs:write_warehouse demote (db/crm)
- last seen: 2026-09-27T10:43:19Z

### L-003 - A relay e2e file that is the only killer of a guard must be wired into a CI lane (_ci-relay.yml).
- signal: `gate_fail` · recurrence: 1 feature(s) · scope: `ci` · harmful: 0
- features: crm-foundation
- evidence: crates/buzz-test-client/tests/e2e_crm_space.rs (ci)
- last seen: 2026-09-27T10:43:19Z

### L-004 - Assert relay rejection prefixes with starts_with or equality, never contains(<reason>).
- signal: `spec_precision_gap` · recurrence: 1 feature(s) · scope: `tests` · harmful: 0
- features: crm-foundation
- evidence: e2e_crm_space.rs assertions (tests)
- last seen: 2026-09-27T10:43:19Z

### L-005 - A lock chosen at the relay seam is invisible to a store-level race test; race at the relay seam (concurrent e2e) or take the lock inside the store function the test calls.
- signal: `surviving_mutant` · recurrence: 1 feature(s) · scope: `relay/crm` · harmful: 0
- features: crm-foundation
- evidence: validation.md round2 M2 (relay/crm)
- last seen: 2026-09-27T16:21:11Z

### L-006 - Branches gated by deployment flags (require_auth_token, require_relay_membership) need a router test with the flag on and real NIP-98 proofs; dev-mode e2e cannot kill them.
- signal: `surviving_mutant` · recurrence: 1 feature(s) · scope: `relay/api` · harmful: 0
- features: crm-foundation
- evidence: validation.md round2 M12 (relay/api)
- last seen: 2026-09-27T16:21:11Z

### L-007 - A CLI acceptance criterion needs an executable test: return command output from a run() function and drive it against an axum fake relay.
- signal: `ac_gap` · recurrence: 1 feature(s) · scope: `cli` · harmful: 0
- features: crm-foundation
- evidence: crm-foundation P2-AC4 (cli)
- last seen: 2026-09-27T16:21:11Z

### L-008 - Worktrees sharing one CARGO_TARGET_DIR overwrite same-named workspace crates; touch the crate sources (or use a separate target dir) after another worktree built.
- signal: `gate_fail` · recurrence: 1 feature(s) · scope: `tooling` · harmful: 0
- features: crm-catalog
- evidence: e2e build: unresolved import buzz_crm::ProductPayload (tooling)
- last seen: 2026-09-27T16:21:11Z

## Quarantined (failed when applied - ignore)

A confirmed lesson that recurred alongside failure. Kept for the maintainer to review.

_none_
