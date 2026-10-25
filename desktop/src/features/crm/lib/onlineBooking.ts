import { ru } from "@/features/crm/i18n/ru";
import { formatDuration } from "@/features/crm/lib/catalog";
import type { CrmOnlineBookingSettings } from "@/shared/api/tauriCrmOnlineBooking";

/** Contract bounds of the kind 42135 payload. */
export const ONLINE_BOOKING_TITLE_MAX = 128;
export const ONLINE_BOOKING_ABOUT_MAX = 1000;
export const ONLINE_BOOKING_ADDRESS_MAX = 256;
export const ONLINE_BOOKING_HORIZON_MIN = 1;
export const ONLINE_BOOKING_HORIZON_MAX = 180;

/** Minimum notice choices in minutes: none, 30 min, 1 h, 2 h, 1 day. */
export const MIN_NOTICE_PRESETS: readonly number[] = [0, 30, 60, 120, 1440];
/** Start-time grid choices in minutes. */
export const STEP_PRESETS: readonly number[] = [5, 10, 15, 30, 60];

/**
 * The form's editable state. Text fields hold what the user typed, the
 * horizon included, so a half-typed number is not coerced while editing.
 */
export type OnlineBookingDraft = {
  enabled: boolean;
  title: string;
  about: string;
  address: string;
  phone: string;
  horizonDays: string;
  minNoticeMin: number;
  stepMin: number;
  staffIds: string[];
  serviceIds: string[];
};

export const DEFAULT_ONLINE_BOOKING_DRAFT: OnlineBookingDraft = {
  enabled: false,
  title: "",
  about: "",
  address: "",
  phone: "",
  horizonDays: "30",
  minNoticeMin: 60,
  stepMin: 15,
  staffIds: [],
  serviceIds: [],
};

/** The draft for saved settings, or the defaults before the first save. */
export function draftFromSettings(
  settings: CrmOnlineBookingSettings | null,
): OnlineBookingDraft {
  if (!settings) return DEFAULT_ONLINE_BOOKING_DRAFT;
  return {
    enabled: settings.enabled,
    title: settings.title,
    about: settings.about ?? "",
    address: settings.address ?? "",
    phone: settings.phone ?? "",
    horizonDays: String(settings.horizonDays),
    minNoticeMin: settings.minNoticeMin,
    stepMin: settings.stepMin,
    staffIds: [...settings.staffIds],
    serviceIds: [...settings.serviceIds],
  };
}

function sameIds(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  const set = new Set(a);
  return b.every((id) => set.has(id));
}

export function sameDraft(a: OnlineBookingDraft, b: OnlineBookingDraft) {
  return (
    a.enabled === b.enabled &&
    a.title === b.title &&
    a.about === b.about &&
    a.address === b.address &&
    a.phone === b.phone &&
    a.horizonDays === b.horizonDays &&
    a.minNoticeMin === b.minNoticeMin &&
    a.stepMin === b.stepMin &&
    sameIds(a.staffIds, b.staffIds) &&
    sameIds(a.serviceIds, b.serviceIds)
  );
}

export type OnlineBookingField = "title" | "horizonDays";

/** The draft as settings, or the first field that fails the contract. */
export function settingsFromDraft(
  draft: OnlineBookingDraft,
):
  | { ok: true; settings: CrmOnlineBookingSettings }
  | { ok: false; field: OnlineBookingField; message: string } {
  const title = draft.title.trim();
  if (title.length === 0) {
    return {
      ok: false,
      field: "title",
      message: ru.onlineBooking.titleRequired,
    };
  }
  const horizon = Number(draft.horizonDays.trim());
  if (
    !/^\d+$/.test(draft.horizonDays.trim()) ||
    horizon < ONLINE_BOOKING_HORIZON_MIN ||
    horizon > ONLINE_BOOKING_HORIZON_MAX
  ) {
    return {
      ok: false,
      field: "horizonDays",
      message: ru.onlineBooking.horizonInvalid,
    };
  }
  const optional = (value: string) => {
    const trimmed = value.trim();
    return trimmed.length > 0 ? trimmed : null;
  };
  return {
    ok: true,
    settings: {
      enabled: draft.enabled,
      title,
      about: optional(draft.about),
      address: optional(draft.address),
      phone: optional(draft.phone),
      horizonDays: horizon,
      minNoticeMin: draft.minNoticeMin,
      stepMin: draft.stepMin,
      staffIds: [...draft.staffIds],
      serviceIds: [...draft.serviceIds],
    },
  };
}

/**
 * Minimum notice options; a value set elsewhere that is not a preset stays
 * selectable so opening the form never silently changes it.
 */
export function minNoticeOptions(
  current: number,
): Array<{ value: number; label: string }> {
  const values = MIN_NOTICE_PRESETS.includes(current)
    ? MIN_NOTICE_PRESETS
    : [...MIN_NOTICE_PRESETS, current].sort((a, b) => a - b);
  return values.map((value) => ({ value, label: minNoticeLabel(value) }));
}

export function minNoticeLabel(minutes: number): string {
  const labels = ru.onlineBooking.minNotice;
  if (minutes === 0) return labels.none;
  if (minutes === 1440) return labels.day;
  return formatDuration(minutes);
}

/** Step options, keeping a non-preset value set elsewhere. */
export function stepOptions(current: number): number[] {
  return STEP_PRESETS.includes(current)
    ? [...STEP_PRESETS]
    : [...STEP_PRESETS, current].sort((a, b) => a - b);
}

/** Add or remove `id`, keeping order otherwise. */
export function toggleId(
  ids: readonly string[],
  id: string,
  checked: boolean,
): string[] {
  const without = ids.filter((other) => other !== id);
  return checked ? [...without, id] : without;
}
