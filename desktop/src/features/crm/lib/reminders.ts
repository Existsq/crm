import { ru } from "@/features/crm/i18n/ru";
import type {
  CrmReminderRule,
  CrmReminderSettings,
} from "@/shared/api/tauriCrmReminders";

/** Relay bounds of `ReminderSettingsPayload` (`buzz_crm::reminder`). */
export const MAX_REMINDER_RULES = 4;
export const MAX_TEMPLATE_CHARS = 1000;
export const MIN_OFFSET_MIN = 30;
export const MAX_OFFSET_MIN = 7 * 24 * 60;
export const OFFSET_STEP_MIN = 15;
export const MAX_ADDRESS_CHARS = 200;

/** Substitutions a template may use, in the order the UI offers them. */
export const REMINDER_PLACEHOLDERS = [
  "client",
  "date",
  "time",
  "services",
  "staff",
  "salon",
  "address",
  "total",
] as const;

export type ReminderPlaceholder = (typeof REMINDER_PLACEHOLDERS)[number];
export type ReminderContext = Record<ReminderPlaceholder, string>;

/** Offsets offered in the picker, minutes before the visit. */
export const REMINDER_OFFSET_CHOICES: readonly number[] = [
  30, 60, 120, 180, 360, 720, 1440, 2880, 4320, 10080,
];

/** Rule identity, the same as the relay's `ReminderRule::key`. */
export function ruleKey(rule: Pick<CrmReminderRule, "trigger" | "offsetMin">) {
  return rule.trigger === "before" && rule.offsetMin !== null
    ? `before:${rule.offsetMin}`
    : "booked";
}

/** `{word}` tokens of `template` that are not placeholders. */
export function unknownPlaceholders(template: string): string[] {
  const unknown = new Set<string>();
  for (const match of template.matchAll(/\{([A-Za-z0-9_]+)\}/g)) {
    const name = match[1] ?? "";
    if (!(REMINDER_PLACEHOLDERS as readonly string[]).includes(name)) {
      unknown.add(name);
    }
  }
  return [...unknown];
}

/** Substitute every placeholder; other text in braces stays as is. */
export function renderReminder(
  template: string,
  context: ReminderContext,
): string {
  return template.replace(/\{([A-Za-z0-9_]+)\}/g, (whole, name: string) =>
    (REMINDER_PLACEHOLDERS as readonly string[]).includes(name)
      ? context[name as ReminderPlaceholder]
      : whole,
  );
}

/** Sample values for the live preview, with the salon's own name/address. */
export function sampleReminderContext(
  settings: Pick<CrmReminderSettings, "salonName" | "address">,
): ReminderContext {
  const sample = ru.reminders.sample;
  return {
    client: sample.client,
    date: sample.date,
    time: sample.time,
    services: sample.services,
    staff: sample.staff,
    salon: settings.salonName?.trim() || sample.salon,
    address: settings.address?.trim() || sample.address,
    total: sample.total,
  };
}

/** Rules offered before the space has saved any. */
export function defaultReminderSettings(): CrmReminderSettings {
  const templates = ru.reminders.defaults;
  return {
    enabled: false,
    rules: [
      { trigger: "booked", offsetMin: null, template: templates.booked },
      { trigger: "before", offsetMin: 1440, template: templates.dayBefore },
      { trigger: "before", offsetMin: 120, template: templates.hoursBefore },
    ],
    salonName: null,
    address: null,
  };
}

/** An offset as a short Russian phrase ("24 h", "2 h 30 min", "7 days"). */
export function formatOffset(minutes: number): string {
  const units = ru.reminders.units;
  // One day reads as 24 hours; longer whole days as days.
  if (minutes > 1440 && minutes % 1440 === 0) {
    return `${minutes / 1440} ${units.days}`;
  }
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) return `${rest} ${units.minutes}`;
  return rest === 0
    ? `${hours} ${units.hours}`
    : `${hours} ${units.hours} ${rest} ${units.minutes}`;
}

/** Log label of a rule key ("booking confirmation", "24 h before"). */
export function ruleKeyLabel(key: string): string {
  if (key === "booked") return ru.reminders.rule.booked;
  const offset = Number(key.startsWith("before:") ? key.slice(7) : Number.NaN);
  return Number.isFinite(offset)
    ? ru.reminders.rule.before.replace("{value}", formatOffset(offset))
    : key;
}

/** A rule the user may still add, or `null` when every slot is taken. */
export function nextReminderRule(
  rules: readonly CrmReminderRule[],
): CrmReminderRule | null {
  if (rules.length >= MAX_REMINDER_RULES) return null;
  const used = new Set(rules.map(ruleKey));
  if (!used.has("booked")) {
    return {
      trigger: "booked",
      offsetMin: null,
      template: ru.reminders.defaults.booked,
    };
  }
  const offset = REMINDER_OFFSET_CHOICES.find(
    (choice) => !used.has(`before:${choice}`),
  );
  return offset === undefined
    ? null
    : {
        trigger: "before",
        offsetMin: offset,
        template: ru.reminders.defaults.genericBefore,
      };
}

export type ReminderDraftErrors = {
  /** Per rule index. */
  rules: Record<number, string>;
  salonName?: string;
  address?: string;
};

export type ReminderValidation =
  | { ok: true; settings: CrmReminderSettings }
  | { ok: false; errors: ReminderDraftErrors };

/** Mirror of the relay's validation, so mistakes show next to the field. */
export function validateReminderDraft(
  draft: CrmReminderSettings,
): ReminderValidation {
  const messages = ru.reminders.errors;
  const errors: ReminderDraftErrors = { rules: {} };
  const seen = new Set<string>();
  const rules = draft.rules.map((rule) => ({
    ...rule,
    template: rule.template.trim(),
  }));
  rules.forEach((rule, index) => {
    const key = ruleKey(rule);
    if (rule.trigger === "before") {
      const offset = rule.offsetMin ?? 0;
      if (
        offset < MIN_OFFSET_MIN ||
        offset > MAX_OFFSET_MIN ||
        offset % OFFSET_STEP_MIN !== 0
      ) {
        errors.rules[index] = messages.offset;
        return;
      }
    }
    if (seen.has(key)) {
      errors.rules[index] = messages.duplicate;
      return;
    }
    seen.add(key);
    if (rule.template.length === 0) {
      errors.rules[index] = messages.templateEmpty;
      return;
    }
    if ([...rule.template].length > MAX_TEMPLATE_CHARS) {
      errors.rules[index] = messages.templateLong;
      return;
    }
    const unknown = unknownPlaceholders(rule.template);
    if (unknown.length > 0) {
      errors.rules[index] = messages.placeholder.replace(
        "{value}",
        unknown.map((name) => `{${name}}`).join(", "),
      );
    }
  });
  const salonName = draft.salonName?.trim() || null;
  const address = draft.address?.trim() || null;
  if (salonName && [...salonName].length > 128) {
    errors.salonName = messages.salonLong;
  }
  if (address && [...address].length > MAX_ADDRESS_CHARS) {
    errors.address = messages.addressLong;
  }
  if (rules.length > MAX_REMINDER_RULES) {
    errors.rules[MAX_REMINDER_RULES] = messages.tooMany;
  }
  const failed =
    Object.keys(errors.rules).length > 0 ||
    errors.salonName !== undefined ||
    errors.address !== undefined;
  return failed
    ? { ok: false, errors }
    : {
        ok: true,
        settings: { enabled: draft.enabled, rules, salonName, address },
      };
}

export function sameReminderSettings(
  a: CrmReminderSettings,
  b: CrmReminderSettings,
): boolean {
  return (
    a.enabled === b.enabled &&
    (a.salonName ?? "") === (b.salonName ?? "") &&
    (a.address ?? "") === (b.address ?? "") &&
    a.rules.length === b.rules.length &&
    a.rules.every((rule, index) => {
      const other = b.rules[index];
      return (
        other !== undefined &&
        rule.trigger === other.trigger &&
        rule.offsetMin === other.offsetMin &&
        rule.template === other.template
      );
    })
  );
}

/** Parse a Telegram chat id typed by hand (sandbox testing only). */
export function parseChatId(raw: string): number | null {
  const trimmed = raw.trim();
  if (!/^-?\d{1,16}$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return Number.isSafeInteger(value) && value !== 0 ? value : null;
}

/** Start of the default log window: seven days back. */
export function deliveryLogSince(now: Date, days = 7): string {
  return new Date(now.getTime() - days * 24 * 60 * 60 * 1000).toISOString();
}
