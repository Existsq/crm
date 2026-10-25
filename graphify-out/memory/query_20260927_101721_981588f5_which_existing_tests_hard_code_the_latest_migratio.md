---
type: "query"
date: "2026-09-27T10:17:21.183959+00:00"
question: "Which existing tests hard-code the latest migration and must be updated when adding a migration?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["embedded_migrator_contains_consolidated_initial_schema"]
---

# Q: Which existing tests hard-code the latest migration and must be updated when adding a migration?

## Answer

crates/buzz-db/src/runtime/migration.rs embedded_migrator_contains_consolidated_initial_schema (count + last version) — bump it. deletion_surface_parity_between_migration_0029_and_schema_sql now unions fence attachments of all migrations after 0029 (no edit needed if the new migration attaches its fences). migration_0044_drops_populated_nip_fi_ledger_cleanly runs the full migrator before validate_catalog; thread_window prebuild test asserts version >= 49.

## Outcome

- Signal: useful

## Source Nodes

- embedded_migrator_contains_consolidated_initial_schema