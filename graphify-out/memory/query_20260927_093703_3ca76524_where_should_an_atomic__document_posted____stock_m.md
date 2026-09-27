---
type: "query"
date: "2026-09-27T09:37:03.583832+00:00"
question: "Where should an atomic 'document posted -> stock movement' write go in the relay?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["insert_event_in_transaction", "begin_event_write_transaction", "handle_side_effects"]
---

# Q: Where should an atomic 'document posted -> stock movement' write go in the relay?

## Answer

Follow the reaction branch in ingest.rs (~3096-3200) and crates/buzz-db/src/store/reaction.rs:171: begin_event_write_transaction, buzz_deletion guard_transaction, domain rows + insert_event_in_transaction in ONE tx, commit, emit conformance trace actions (else EmitGuard coverage breach), then dispatch_persistent_event. Do NOT use handle_side_effects (failures only logged) or command_executor (domain writes outside the event tx and command events are not broadcast). CRM uses this pattern: handlers/crm (AD-001).

## Outcome

- Signal: useful

## Source Nodes

- insert_event_in_transaction
- begin_event_write_transaction
- handle_side_effects