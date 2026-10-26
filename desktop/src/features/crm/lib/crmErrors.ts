import { ru } from "@/features/crm/i18n/ru";

export type CrmErrorKind =
  | "restricted"
  | "conflict"
  | "invalid"
  | "network"
  | "unknown";

export type DescribedCrmError = {
  kind: CrmErrorKind;
  /** Russian message for the user. */
  message: string;
  /** Relay's own (English) reason, shown as secondary detail when present. */
  detail: string | null;
};

const RELAY_PREFIXES = ["restricted", "conflict", "invalid"] as const;

/** Relay reasons with a specific message; `{value}` is the first group. */
const KNOWN_REASONS: ReadonlyArray<{
  prefix: (typeof RELAY_PREFIXES)[number];
  pattern: RegExp;
  template: string;
}> = [
  {
    prefix: "conflict",
    pattern: /^SKU "?(.+?)"? is already used by another product/,
    template: ru.errors.skuTaken,
  },
  {
    prefix: "conflict",
    pattern: /^barcode "?([^"\s]+)"? is already used by another product/,
    template: ru.errors.barcodeTaken,
  },
  {
    prefix: "conflict",
    pattern: /^INN (\d+) is already used by another party/,
    template: ru.errors.innTaken,
  },
  {
    prefix: "conflict",
    pattern: /^the staff member is booked (\S+)/,
    template: ru.errors.staffBusy,
  },
  {
    prefix: "invalid",
    pattern: /^the visit is outside the staff member's working hours()/,
    template: ru.errors.outsideHours,
  },
  {
    prefix: "invalid",
    pattern: /^the visit is paid()/,
    template: ru.errors.visitPaid,
  },
  // Stock rejections name the product; say what to do about them.
  {
    prefix: "invalid",
    pattern: /not enough stock of "(.+?)" to void/,
    template: ru.errors.stockAlreadyUsed,
  },
  {
    prefix: "invalid",
    pattern: /not enough stock of "(.+?)"/,
    template: ru.errors.notEnoughStock,
  },
];

function relayMessage(
  prefix: (typeof RELAY_PREFIXES)[number],
  reason: string,
): string {
  for (const known of KNOWN_REASONS) {
    const match = known.prefix === prefix && known.pattern.exec(reason);
    if (match) return known.template.replace("{value}", match[1]);
  }
  return ru.errors[prefix];
}

/**
 * Classify a CRM command failure by the relay's machine-readable prefix.
 *
 * Tauri surfaces relay rejections as e.g. `relay error 409: conflict: …` or
 * `relay rejected event: invalid: …`; transport failures start with
 * `relay unreachable:`.
 */
export function describeCrmError(error: unknown): DescribedCrmError {
  const raw = errorText(error);
  if (/relay unreachable/i.test(raw)) {
    return { kind: "network", message: ru.errors.network, detail: null };
  }
  for (const prefix of RELAY_PREFIXES) {
    const index = raw.indexOf(`${prefix}:`);
    if (index >= 0) {
      const reason = raw.slice(index + prefix.length + 1).trim();
      return {
        kind: prefix,
        message: relayMessage(prefix, reason),
        detail: reason.length > 0 ? reason : null,
      };
    }
  }
  return {
    kind: "unknown",
    message: ru.errors.unknown,
    detail: raw.length > 0 ? raw : null,
  };
}

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return "";
}
