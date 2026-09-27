---
type: "query"
date: "2026-09-27T09:37:03.491066+00:00"
question: "How does an event flow from EVENT/POST /events to storage and fan-out in buzz-relay?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["ingest_event_inner", "required_scope_for_kind", "dispatch_persistent_event"]
---

# Q: How does an event flow from EVENT/POST /events to storage and fan-out in buzz-relay?

## Answer

Both WebSocket (handlers/event.rs::handle_event) and HTTP POST /events (api/bridge.rs::submit_event) call ingest::ingest_event -> ingest_event_inner (crates/buzz-relay/src/handlers/ingest.rs). Order: community write fence, reject AUTH/relay-only kinds, signature, created_at within ±900 s, content <=256 KB, pubkey == authed pubkey, required_scope_for_kind (unknown kinds rejected), command kinds -> command_executor, table-only branches, ban check, h-tag channel + is_global_only_kind + requires_h_channel_scope, check_channel_membership (member OR open channel), per-kind validators, thread metadata, storage (generic/replaceable/addressable/canvas CAS/reaction tx), handle_side_effects, dispatch_persistent_event (audit + Redis publish + local fan-out + workflow engine).

## Outcome

- Signal: useful

## Source Nodes

- ingest_event_inner
- required_scope_for_kind
- dispatch_persistent_event