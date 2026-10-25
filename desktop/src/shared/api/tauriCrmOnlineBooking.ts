import { invokeTauri } from "@/shared/api/tauri";
import type { CrmWriteResult } from "@/shared/api/tauriCrm";

// ── Domain types ──────────────────────────────────────────────────────────

/** Editable online booking settings (kind 42135 payload). */
export type CrmOnlineBookingSettings = {
  /** Whether the public page accepts bookings. */
  enabled: boolean;
  /** Page heading, 1–128 characters. */
  title: string;
  about: string | null;
  address: string | null;
  phone: string | null;
  /** How many days ahead clients may book, 1–180. */
  horizonDays: number;
  /** Earliest booking from now in minutes, 0–10080. */
  minNoticeMin: number;
  /** Start-time grid in minutes: 5..60, a multiple of 5. */
  stepMin: number;
  /** Offered staff; empty offers every active staff member. */
  staffIds: string[];
  /** Offered services; empty offers every active service with a duration. */
  serviceIds: string[];
};

export type CrmOnlineBooking = {
  /** `null` until the settings are saved for the first time. */
  settings: CrmOnlineBookingSettings | null;
  /** CAS key for the next write; `null` before the first save. */
  revision: string | null;
  /** Absolute public page URL, `null` when the relay sent no path. */
  publicUrl: string | null;
};

export type CrmOnlineBookingWriteInput = CrmOnlineBookingSettings & {
  channelId: string;
  /** Current revision; `null` creates the settings row. */
  expectedRevision: string | null;
};

// ── Raw types (snake_case from backend) ───────────────────────────────────

export type RawCrmOnlineBookingSettings = {
  v?: number;
  enabled: boolean;
  title: string;
  about?: string | null;
  address?: string | null;
  phone?: string | null;
  horizon_days: number;
  min_notice_min: number;
  step_min: number;
  staff_ids?: string[];
  service_ids?: string[];
};

export type RawCrmOnlineBooking = {
  settings: RawCrmOnlineBookingSettings | null;
  revision: string | null;
  public_path?: string | null;
  /** Added by the Tauri command from the relay HTTP base. */
  public_url?: string | null;
};

type RawCrmWriteResult = {
  event_id: string;
  entity_id?: string | null;
};

// ── Mappers ───────────────────────────────────────────────────────────────

export function fromRawCrmOnlineBooking(
  raw: RawCrmOnlineBooking,
): CrmOnlineBooking {
  const s = raw.settings;
  return {
    settings: s
      ? {
          enabled: s.enabled,
          title: s.title,
          about: s.about ?? null,
          address: s.address ?? null,
          phone: s.phone ?? null,
          horizonDays: s.horizon_days,
          minNoticeMin: s.min_notice_min,
          stepMin: s.step_min,
          staffIds: s.staff_ids ?? [],
          serviceIds: s.service_ids ?? [],
        }
      : null,
    revision: raw.revision ?? null,
    publicUrl: raw.public_url ?? null,
  };
}

export function toRawCrmOnlineBookingInput(input: CrmOnlineBookingWriteInput) {
  return {
    channel_id: input.channelId,
    expected_revision: input.expectedRevision,
    enabled: input.enabled,
    title: input.title,
    about: input.about,
    address: input.address,
    phone: input.phone,
    horizon_days: input.horizonDays,
    min_notice_min: input.minNoticeMin,
    step_min: input.stepMin,
    staff_ids: input.staffIds,
    service_ids: input.serviceIds,
  };
}

// ── Commands ──────────────────────────────────────────────────────────────

export async function getCrmOnlineBooking(): Promise<CrmOnlineBooking> {
  const raw = await invokeTauri<RawCrmOnlineBooking>("crm_get_online_booking");
  return fromRawCrmOnlineBooking(raw);
}

/** Create (no revision) or replace (CAS on `expectedRevision`) the settings. */
export async function writeCrmOnlineBooking(
  input: CrmOnlineBookingWriteInput,
): Promise<CrmWriteResult> {
  const raw = await invokeTauri<RawCrmWriteResult>("crm_write_online_booking", {
    input: toRawCrmOnlineBookingInput(input),
  });
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}
