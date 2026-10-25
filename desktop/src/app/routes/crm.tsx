import { createFileRoute, redirect } from "@tanstack/react-router";

import { DEFAULT_CRM_SECTION } from "@/features/crm/lib/crmSections";

// `/crm` has no page of its own: it opens the first CRM section.
export const Route = createFileRoute("/crm")({
  beforeLoad: () => {
    throw redirect({
      to: "/crm/$section",
      params: { section: DEFAULT_CRM_SECTION },
      replace: true,
    });
  },
});
