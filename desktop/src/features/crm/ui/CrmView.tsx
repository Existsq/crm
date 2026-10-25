import { RefreshCw } from "lucide-react";

import { useCrmLiveUpdates, useCrmSpaceQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import {
  CRM_SECTIONS,
  type CrmSectionId,
} from "@/features/crm/lib/crmSections";
import { CrmAnalyticsSection } from "@/features/crm/ui/analytics/CrmAnalyticsSection";
import { CrmCashSection } from "@/features/crm/ui/cash/CrmCashSection";
import { CrmJournalSection } from "@/features/crm/ui/journal/CrmJournalSection";
import { CrmSettingsSection } from "@/features/crm/ui/CrmSettingsSection";
import { CrmPartiesSection } from "@/features/crm/ui/parties/CrmPartiesSection";
import { CrmProductsSection } from "@/features/crm/ui/products/CrmProductsSection";
import { CrmSalesSection } from "@/features/crm/ui/sales/CrmSalesSection";
import { CrmScheduleSection } from "@/features/crm/ui/staff/CrmScheduleSection";
import { CrmStaffSection } from "@/features/crm/ui/staff/CrmStaffSection";
import { CrmSpaceGate } from "@/features/crm/ui/CrmSpaceGate";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";

type CrmViewProps = {
  section: CrmSectionId;
  /** Booked visit the cash desk takes payment for. */
  visit?: string;
  onOpenSection: (section: CrmSectionId, visit?: string) => void;
};

function CrmSectionContent({
  onOpenSection,
  section,
  space,
  visit,
}: CrmViewProps & { space: CrmMemberSpace }) {
  switch (section) {
    case "products":
      return <CrmProductsSection space={space} />;
    case "journal":
      return (
        <CrmJournalSection
          onPayVisit={(id) => onOpenSection("cash", id)}
          space={space}
        />
      );
    case "services":
      return <CrmProductsSection kind="services" space={space} />;
    case "staff":
      return <CrmStaffSection space={space} />;
    case "schedule":
      return <CrmScheduleSection space={space} />;
    case "parties":
      return <CrmPartiesSection space={space} />;
    case "cash":
      return (
        <CrmCashSection
          onVisitDone={() => onOpenSection("cash")}
          space={space}
          visit={visit ?? null}
        />
      );
    case "sales":
      return <CrmSalesSection space={space} />;
    case "analytics":
      return <CrmAnalyticsSection space={space} />;
    case "settings":
      return <CrmSettingsSection space={space} />;
  }
}

export function CrmView({ onOpenSection, section, visit }: CrmViewProps) {
  const spaceQuery = useCrmSpaceQuery();
  const space = spaceQuery.data;
  useCrmLiveUpdates(space?.status === "member" ? space.channelId : null);

  return (
    <div
      className="relative flex min-h-0 flex-1 overflow-hidden"
      data-testid="crm-view"
    >
      <div
        className="flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden overscroll-contain px-4 py-5 sm:px-6 sm:py-6"
        data-scroll-restoration-id="crm"
      >
        <div className="mx-auto w-full max-w-6xl space-y-6 [container-type:inline-size]">
          <div className="flex items-center justify-between gap-3">
            <h1 className="text-sm font-medium text-muted-foreground">
              {ru.shell.title}
              <span aria-hidden="true"> · </span>
              <span className="sr-only"> </span>
              {CRM_SECTIONS.find((item) => item.id === section)?.label}
            </h1>
            <Button
              aria-label={ru.shell.refreshLabel}
              className="h-8 w-8"
              data-testid="crm-refresh"
              disabled={spaceQuery.isFetching}
              onClick={() => void spaceQuery.refetch()}
              size="icon"
              variant="ghost"
            >
              <RefreshCw
                className={cn(
                  "h-4 w-4",
                  spaceQuery.isFetching && "animate-spin",
                )}
              />
            </Button>
          </div>

          <CrmSpaceGate query={spaceQuery}>
            {(memberSpace) => (
              <CrmSectionContent
                onOpenSection={onOpenSection}
                section={section}
                space={memberSpace}
                visit={visit}
              />
            )}
          </CrmSpaceGate>
        </div>
      </div>
    </div>
  );
}
