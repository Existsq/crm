import { ru } from "@/features/crm/i18n/ru";

/**
 * CRM sections in navigation order. Each feature branch appends its section;
 * `/crm` opens the first one.
 */
export const CRM_SECTIONS = [
  { id: "settings", label: ru.sections.settings },
] as const;

export type CrmSectionId = (typeof CRM_SECTIONS)[number]["id"];

export const DEFAULT_CRM_SECTION: CrmSectionId = CRM_SECTIONS[0].id;

export function parseCrmSection(raw: unknown): CrmSectionId | null {
  return CRM_SECTIONS.find((section) => section.id === raw)?.id ?? null;
}
