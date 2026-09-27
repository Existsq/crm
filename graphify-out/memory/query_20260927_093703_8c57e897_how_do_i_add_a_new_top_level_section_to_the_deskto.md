---
type: "query"
date: "2026-09-27T09:37:03.869325+00:00"
question: "How do I add a new top-level section to the desktop app?"
contributor: "graphify"
outcome: "useful"
source_nodes: ["deriveShellRoute", "FeatureGate", "resetCommunityState"]
---

# Q: How do I add a new top-level section to the desktop app?

## Answer

TanStack Router virtual routes: desktop/src/app/routes.ts + file in desktop/src/app/routes/ (lazy XScreen, usePreviewFeatureWarning). Touch app/navigation/useAppNavigation.ts (goX), app/AppShell.helpers.ts (AppView union + deriveShellRoute, test AppShell.helpers.test.mjs), features/sidebar/ui/AppSidebar.types.ts, AppSidebar.tsx, AppSidebarPinnedHeader.tsx (FeatureGate + SidebarMenuButton + lucide icon), app/AppShell.tsx, shared/ui/ViewLoadingFallback.tsx. Gate with preview-features.json + <FeatureGate>. New React Query roots go in RELAY_QUERY_ROOTS (shared/api/relayQueryInvalidation.ts); module-level caches reset in resetCommunityState() (features/communities/useCommunityInit.ts).

## Outcome

- Signal: useful

## Source Nodes

- deriveShellRoute
- FeatureGate
- resetCommunityState