import * as React from "react";
import { createFileRoute, redirect } from "@tanstack/react-router";

import {
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
});

function CrmRouteComponent() {
  usePreviewFeatureWarning("crm");
  const { section } = Route.useParams();
  const current = parseCrmSection(section) ?? DEFAULT_CRM_SECTION;

  return (
    <React.Suspense fallback={<ViewLoadingFallback kind="crm" />}>
      <CrmScreen section={current} />
    </React.Suspense>
  );
}
