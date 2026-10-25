import { RefreshCw } from "lucide-react";

import { useCrmLiveUpdates, useCrmSpaceQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import {
  CRM_SECTIONS,
  type CrmSectionId,
} from "@/features/crm/lib/crmSections";
import { CrmSettingsSection } from "@/features/crm/ui/CrmSettingsSection";
import { CrmProductsSection } from "@/features/crm/ui/products/CrmProductsSection";
import { CrmSpaceGate } from "@/features/crm/ui/CrmSpaceGate";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { PageHeader } from "@/shared/ui/PageHeader";

type CrmViewProps = {
  section: CrmSectionId;
  onSelectSection: (section: CrmSectionId) => void;
};

function CrmSectionContent({
  section,
  space,
}: {
  section: CrmSectionId;
  space: CrmMemberSpace;
}) {
  switch (section) {
    case "products":
      return <CrmProductsSection space={space} />;
    case "settings":
      return <CrmSettingsSection space={space} />;
  }
}

export function CrmView({ section, onSelectSection }: CrmViewProps) {
  const spaceQuery = useCrmSpaceQuery();
  const space = spaceQuery.data;
  useCrmLiveUpdates(space?.status === "member" ? space.channelId : null);

  return (
    <div
      className="relative flex min-h-0 flex-1 overflow-hidden"
      data-testid="crm-view"
    >
      <div
        className="flex min-h-0 flex-1 flex-col overflow-y-auto overflow-x-hidden overscroll-contain px-4 py-7 sm:px-6 sm:py-8"
        data-scroll-restoration-id="crm"
      >
        <div className="mx-auto w-full max-w-6xl space-y-6 [container-type:inline-size]">
          <PageHeader
            action={
              <Button
                aria-label={ru.shell.refreshLabel}
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
            }
            description={ru.shell.description}
            title={ru.shell.title}
          />

          {space?.status === "member" ? (
            <nav
              aria-label={ru.shell.sectionsLabel}
              className="flex gap-1 border-b border-border/60"
            >
              {CRM_SECTIONS.map((item) => {
                const active = item.id === section;
                return (
                  <button
                    aria-current={active ? "page" : undefined}
                    className={cn(
                      "-mb-px border-b-2 px-3 py-2 text-sm transition-colors focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
                      active
                        ? "border-primary font-medium text-foreground"
                        : "border-transparent text-muted-foreground hover:text-foreground",
                    )}
                    data-testid={`crm-section-${item.id}`}
                    key={item.id}
                    onClick={() => onSelectSection(item.id)}
                    type="button"
                  >
                    {item.label}
                  </button>
                );
              })}
            </nav>
          ) : null}

          <CrmSpaceGate query={spaceQuery}>
            {(memberSpace) => (
              <CrmSectionContent section={section} space={memberSpace} />
            )}
          </CrmSpaceGate>
        </div>
      </div>
    </div>
  );
}
