import { useRouterState } from "@tanstack/react-router";
import {
  BarChart3,
  CalendarDays,
  Contact,
  type LucideIcon,
  Package,
  ReceiptText,
  Scissors,
  Settings,
  ShoppingCart,
  Users,
  Warehouse,
} from "lucide-react";

import { ru } from "@/features/crm/i18n/ru";
import {
  CRM_SECTIONS,
  type CrmSectionId,
  parseCrmSection,
} from "@/features/crm/lib/crmSections";
import {
  SidebarMenuSub,
  SidebarMenuSubButton,
  SidebarMenuSubItem,
} from "@/shared/ui/sidebar";

const SECTION_ICONS: Record<CrmSectionId, LucideIcon> = {
  products: Package,
  stock: Warehouse,
  services: Scissors,
  staff: Contact,
  schedule: CalendarDays,
  parties: Users,
  cash: ShoppingCart,
  sales: ReceiptText,
  analytics: BarChart3,
  settings: Settings,
};

/** The section of a `/crm/<section>` path, or null elsewhere. */
export function crmSectionOfPath(pathname: string): CrmSectionId | null {
  const match = /^\/crm\/([^/]+)/.exec(pathname);
  return match ? parseCrmSection(match[1]) : null;
}

/** CRM sections as sidebar sub-items under the CRM entry. */
export function CrmSidebarSections({
  onSelect,
}: {
  onSelect: (section: CrmSectionId) => void;
}) {
  const current = useRouterState({
    select: (state) => crmSectionOfPath(state.location.pathname),
  });
  return (
    <nav aria-label={ru.shell.sectionsLabel}>
      <SidebarMenuSub data-testid="crm-sidebar-sections">
        {CRM_SECTIONS.map((item) => {
          const Icon = SECTION_ICONS[item.id];
          const active = item.id === current;
          return (
            <SidebarMenuSubItem key={item.id}>
              <SidebarMenuSubButton asChild isActive={active}>
                <button
                  aria-current={active ? "page" : undefined}
                  className="w-full"
                  data-testid={`crm-section-${item.id}`}
                  onClick={() => onSelect(item.id)}
                  type="button"
                >
                  <Icon aria-hidden="true" />
                  <span>{item.label}</span>
                </button>
              </SidebarMenuSubButton>
            </SidebarMenuSubItem>
          );
        })}
      </SidebarMenuSub>
    </nav>
  );
}
