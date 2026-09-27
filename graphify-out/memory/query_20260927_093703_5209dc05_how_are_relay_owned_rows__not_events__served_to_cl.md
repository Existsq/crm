---
type: "query"
date: "2026-09-27T09:37:03.770661+00:00"
question: "How are relay-owned rows (not events) served to clients?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["authorize_workflow_read", "get_relay_json", "filter_fully_pushable"]
---

# Q: How are relay-owned rows (not events) served to clients?

## Answer

Dedicated authorized GET endpoints like crates/buzz-relay/src/api/workflows.rs (routes /workflows/{id}/runs in router.rs ~305): bind_community(host) -> NIP-98/NIP-FI admission (admit_nip_fi_http_on_state, enforce_http_admission, check_nip98_replay) -> enforce_relay_membership -> channel membership. Desktop reads them via src-tauri relay/get.rs::get_relay_json. REQ/POST /query only push kinds/authors/ids/since/until/#h/#e/#p/#d into SQL; other tags are filtered after LIMIT (short pages).

## Outcome

- Signal: useful

## Source Nodes

- authorize_workflow_read
- get_relay_json
- filter_fully_pushable