---
type: "query"
date: "2026-09-27T09:37:03.679972+00:00"
question: "What must a new per-community Postgres table satisfy in this repo?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["attach_community_write_fence", "EXPECTED_SCOPED_TABLES", "PURGE_SCOPED_TABLES"]
---

# Q: What must a new per-community Postgres table satisfy in this repo?

## Answer

(1) community_id UUID NOT NULL REFERENCES communities(id) and every PK/UNIQUE/FK/unique index leads with community_id (lint tests in crates/buzz-db/src/runtime/migration.rs); (2) SELECT attach_community_write_fence('t') in the migration AND schema/schema.sql; (3) add to EXPECTED_SCOPED_TABLES and PURGE_SCOPED_TABLES (children first) in crates/buzz-db/src/store/deletion.rs; (4) hand-edit schema/schema.sql alongside migrations/NNNN_*.sql (parity tests); (5) bump migration count in embedded_migrator_contains_consolidated_initial_schema. Queries use runtime sqlx::query().bind(), no macros; Postgres tests are #[ignore = "requires Postgres"] in mod postgres_tests.

## Outcome

- Signal: useful

## Source Nodes

- attach_community_write_fence
- EXPECTED_SCOPED_TABLES
- PURGE_SCOPED_TABLES