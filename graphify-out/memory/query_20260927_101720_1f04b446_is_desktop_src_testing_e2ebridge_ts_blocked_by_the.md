---
type: "query"
date: "2026-09-27T10:17:20.963239+00:00"
question: "Is desktop/src/testing/e2eBridge.ts blocked by the file-size ratchet?"
contributor: "graphify"
outcome: "corrected"
correction: "Earlier plan assumed e2eBridge.ts could not grow; it can."
source_nodes: ["handleMockCommand", "allowedLineCount"]
---

# Q: Is desktop/src/testing/e2eBridge.ts blocked by the file-size ratchet?

## Answer

No. desktop/scripts/file-size-policy.mjs only gates src-tauri/src, src-tauri/crates, src/app, src/features, src/shared/{api,context,lib,ui,styles}. src/testing is not a rule root, so e2eBridge.ts (15k lines) may grow; still prefer feature mock modules (e2eBridgeCrm.ts) with a delegation in handleMockCommand's default branch.

## Outcome

- Signal: corrected
- Correction: Earlier plan assumed e2eBridge.ts could not grow; it can.

## Source Nodes

- handleMockCommand
- allowedLineCount