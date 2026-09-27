---
type: "query"
date: "2026-09-27T09:37:03.961021+00:00"
question: "What are the desktop test and e2e mock constraints?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["handleMockCommand", "installMockBridge"]
---

# Q: What are the desktop test and e2e mock constraints?

## Answer

Unit tests: node --test colocated *.test.mjs / *.jsdom-test.mjs (not vitest). E2E: Playwright specs in desktop/tests/e2e registered in playwright.config.ts smoke testMatch; always pnpm build:e2e (mock bridge only in --mode e2e). desktop/src/testing/e2eBridge.ts is 15,342 lines and the differential file-size gate forbids growing oversized files: new mocks must live in separate modules (e.g. e2eBridgeCrm.ts) with a one-line delegation. File limits: 1200 lines TS/TSX, 1500 Tauri Rust. Text sizes must be rem tokens (pnpm check:px-text).

## Outcome

- Signal: useful

## Source Nodes

- handleMockCommand
- installMockBridge