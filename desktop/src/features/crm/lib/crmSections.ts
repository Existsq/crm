import { ru } from "@/features/crm/i18n/ru";

/**
 * CRM sections in navigation order; `/crm` opens the first one, the catalog.
 * Each feature branch adds its section here.
 */
export const CRM_SECTIONS = [
  { id: "products", label: ru.sections.products },
  { id: "parties", label: ru.sections.parties },
  { id: "sales", label: ru.sections.sales },
  { id: "analytics", label: ru.sections.analytics },
  { id: "settings", label: ru.sections.settings },
] as const;

export type CrmSectionId = (typeof CRM_SECTIONS)[number]["id"];

export const DEFAULT_CRM_SECTION: CrmSectionId = CRM_SECTIONS[0].id;

export function parseCrmSection(raw: unknown): CrmSectionId | null {
  return CRM_SECTIONS.find((section) => section.id === raw)?.id ?? null;
}
