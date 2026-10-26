import * as React from "react";

import type { CrmSectionId } from "@/features/crm/lib/crmSections";
import { ViewLoadingFallback } from "@/shared/ui/ViewLoadingFallback";

const CrmView = React.lazy(async () => {
  const module = await import("@/features/crm/ui/CrmView");
  return { default: module.CrmView };
});

export function CrmScreen(props: {
  section: CrmSectionId;
  /** Booked visit the cash desk takes payment for. */
  visit?: string;
  onOpenSection: (section: CrmSectionId, visit?: string) => void;
}) {
  return (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
      <React.Suspense fallback={<ViewLoadingFallback kind="crm" />}>
        <CrmView {...props} />
      </React.Suspense>
    </div>
  );
}
