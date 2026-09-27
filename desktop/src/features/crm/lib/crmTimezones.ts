import { ru } from "@/features/crm/i18n/ru";

type KnownTimezone = keyof typeof ru.timezones;

/** Russian IANA timezones offered in CRM settings, west to east. */
export const CRM_TIMEZONES = Object.keys(ru.timezones) as KnownTimezone[];

export type CrmTimezoneOption = { value: string; label: string };

/**
 * Options for the timezone select. A current value outside the list (set via
 * the CLI, say) is kept as the first option so saving never silently changes it.
 */
export function crmTimezoneOptions(current: string): CrmTimezoneOption[] {
  const options: CrmTimezoneOption[] = CRM_TIMEZONES.map((value) => ({
    value,
    label: ru.timezones[value],
  }));
  if (current && !CRM_TIMEZONES.includes(current as KnownTimezone)) {
    options.unshift({ value: current, label: current });
  }
  return options;
}
