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
        message: ru.errors[prefix],
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
