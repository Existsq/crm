import { ru } from "@/features/crm/i18n/ru";

/**
 * CRM sections in navigation order; `/crm` opens the first one, the catalog.
 * Each feature branch adds its section here.
 */
export const CRM_SECTIONS = [
  { id: "products", label: ru.sections.products },
  { id: "journal", label: ru.sections.journal },
  { id: "services", label: ru.sections.services },
  { id: "staff", label: ru.sections.staff },
  { id: "schedule", label: ru.sections.schedule },
  { id: "parties", label: ru.sections.parties },
  { id: "cash", label: ru.sections.cash },
  { id: "sales", label: ru.sections.sales },
  { id: "analytics", label: ru.sections.analytics },
  { id: "staff-analytics", label: ru.sections.staffAnalytics },
  { id: "settings", label: ru.sections.settings },
] as const;

export type CrmSectionId = (typeof CRM_SECTIONS)[number]["id"];

export const DEFAULT_CRM_SECTION: CrmSectionId = CRM_SECTIONS[0].id;

export function parseCrmSection(raw: unknown): CrmSectionId | null {
  return CRM_SECTIONS.find((section) => section.id === raw)?.id ?? null;
}
