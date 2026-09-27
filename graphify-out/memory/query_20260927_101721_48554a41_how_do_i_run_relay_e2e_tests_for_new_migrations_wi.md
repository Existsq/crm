---
type: "query"
date: "2026-09-27T10:17:21.081352+00:00"
question: "How do I run relay e2e tests for new migrations without polluting the dev DB?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["run_migrations"]
---

# Q: How do I run relay e2e tests for new migrations without polluting the dev DB?

## Answer

Create an isolated DB (createdb buzz_crm_e2e), run DATABASE_URL=.../buzz_crm_e2e cargo run -p buzz-admin -- migrate, PGDATABASE=buzz_crm_e2e ./scripts/seed-local-community.sh, start the relay with that DATABASE_URL (source .env first; MinIO must be up), then RELAY_URL=ws://localhost:3000 DATABASE_URL=... cargo test -p buzz-test-client --test <e2e> -- --ignored --test-threads=1. sqlx pins migration checksums, so never apply a WIP migration to the shared dev DB. Env script: scratchpad env/dev-env.sh (Docker via mirror.gcr.io, MinIO from ghcr.io/block/buzz-minio).

## Outcome

- Signal: useful

## Source Nodes

- run_migrations