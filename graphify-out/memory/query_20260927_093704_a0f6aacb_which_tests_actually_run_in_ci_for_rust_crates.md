---
type: "query"
date: "2026-09-27T09:37:04.057893+00:00"
question: "Which tests actually run in CI for Rust crates?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["test-unit", "run-tests.sh"]
---

# Q: Which tests actually run in CI for Rust crates?

## Answer

Nothing runs cargo test --workspace. The Justfile test-unit recipe lists crates explicitly and scripts/run-tests.sh unit is the fallback list; a new crate or relay test module must be added to BOTH or its tests run nowhere. Postgres tests are discovered by postgres_tests:: module names (nextest profile postgres-ci, scripts/check-postgres-test-discovery.py).

## Outcome

- Signal: useful

## Source Nodes

- test-unit
- run-tests.sh