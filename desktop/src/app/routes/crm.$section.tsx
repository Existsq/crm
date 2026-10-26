import * as React from "react";
import { createFileRoute, redirect } from "@tanstack/react-router";

import {
  type CrmSectionId,
  DEFAULT_CRM_SECTION,
  parseCrmSection,
} from "@/features/crm/lib/crmSections";
import { usePreviewFeatureWarning } from "@/shared/features";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const CrmScreen = React.lazy(async () => {
  const module = await import("@/features/crm/ui/CrmScreen");
  return { default: module.CrmScreen };
});

export const Route = createFileRoute("/crm/$section")({
  beforeLoad: ({ params }) => {
    if (parseCrmSection(params.section) === null) {
      throw redirect({
        to: "/crm/$section",
        params: { section: DEFAULT_CRM_SECTION },
        replace: true,
      });
    }
  },
  component: CrmRouteComponent,
  // `visit`: the booked visit the cash desk opens to take payment for.
  validateSearch: (search: Record<string, unknown>): { visit?: string } =>
    typeof search.visit === "string" ? { visit: search.visit } : {},
});

function CrmRouteComponent() {
  usePreviewFeatureWarning("crm");
  const { section } = Route.useParams();
  const { visit } = Route.useSearch();
  const navigate = Route.useNavigate();
  const current = parseCrmSection(section) ?? DEFAULT_CRM_SECTION;
  const openSection = React.useCallback(
    (next: CrmSectionId, nextVisit?: string) =>
      void navigate({
        to: "/crm/$section",
        params: { section: next },
        search: nextVisit ? { visit: nextVisit } : {},
      }),
    [navigate],
  );

  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="crm" />}>
      <CrmScreen onOpenSection={openSection} section={current} visit={visit} />
    </React.Suspense>
  );
}
