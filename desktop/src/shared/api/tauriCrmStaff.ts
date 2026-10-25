import { invokeTauri } from "@/shared/api/tauri";
import type { CrmWriteResult } from "@/shared/api/tauriCrm";

// ── Domain types ──────────────────────────────────────────────────────────

/** Editable staff fields (kind 42131 payload without the schema version). */
export type CrmStaffFields = {
  name: string;
  position: string | null;
  phone: string | null;
  /** Bookable products this person performs. */
  serviceIds: string[];
  /** Hex pubkey of the channel member who works as this staff member. */
  member: string | null;
  archived: boolean;
};

export type CrmStaff = CrmStaffFields & {
  id: string;
  revision: string;
  updatedAt: string;
};

export type CrmStaffWriteInput = CrmStaffFields & {
  channelId: string;
  /** Staff id; for a create pick it once per dialog so a retry is a duplicate. */
  staffId: string;
  /** Current revision; required for updates, `null` for creation. */
  expectedRevision: string | null;
};

/** A working interval in minutes after midnight, `start < end <= 1440`. */
export type CrmWorkInterval = { start: number; end: number };

/** One scheduled day; no intervals is a day off. */
export type CrmScheduleDay = {
  staffId: string;
  /** `YYYY-MM-DD` in the space timezone. */
  date: string;
  intervals: CrmWorkInterval[];
};

export type CrmScheduleInput = {
  channelId: string;
  staffId: string;
  /** Days to set; days not listed keep their hours. */
  days: { date: string; intervals: CrmWorkInterval[] }[];
};

// ── Raw types (snake_case from backend) ───────────────────────────────────

export type RawCrmStaff = {
  id: string;
  name: string;
  position: string | null;
  phone: string | null;
  service_ids: string[];
  member: string | null;
  archived: boolean;
  revision: string;
  updated_at: string;
};

export type RawCrmScheduleDay = {
  staff_id: string;
  date: string;
  intervals: CrmWorkInterval[];
};

type RawCrmWriteResult = {
  event_id: string;
  entity_id?: string | null;
};

// ── Mappers ───────────────────────────────────────────────────────────────

export function fromRawCrmStaff(raw: RawCrmStaff): CrmStaff {
  return {
    id: raw.id,
    name: raw.name,
    position: raw.position ?? null,
    phone: raw.phone ?? null,
    serviceIds: raw.service_ids ?? [],
    member: raw.member ?? null,
    archived: raw.archived,
    revision: raw.revision,
    updatedAt: raw.updated_at,
  };
}

/** Minutes after midnight as `HH:MM` (the event wire format). */
export function formatClock(minutes: number): string {
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

export function toRawCrmScheduleInput(input: CrmScheduleInput) {
  return {
    channel_id: input.channelId,
    staff_id: input.staffId,
    days: input.days.map((day) => ({
      date: day.date,
      intervals: day.intervals.map((interval) => ({
        start: formatClock(interval.start),
        end: formatClock(interval.end),
      })),
    })),
  };
}

// ── Commands ──────────────────────────────────────────────────────────────

export async function listCrmStaff(archived = false): Promise<CrmStaff[]> {
  const raw = await invokeTauri<{ staff: RawCrmStaff[] }>("crm_list_staff", {
    archived,
  });
  return raw.staff.map(fromRawCrmStaff);
}

export async function getCrmSchedule(
  from: string,
  to: string,
): Promise<CrmScheduleDay[]> {
  const raw = await invokeTauri<{ days: RawCrmScheduleDay[] }>(
    "crm_get_schedule",
    { from, to },
  );
  return raw.days.map((day) => ({
    staffId: day.staff_id,
    date: day.date,
    intervals: day.intervals,
  }));
}

export async function writeCrmStaff(
  input: CrmStaffWriteInput,
): Promise<CrmWriteResult> {
  const raw = await invokeTauri<RawCrmWriteResult>("crm_write_staff", {
    input: {
      channel_id: input.channelId,
      staff_id: input.staffId,
      expected_revision: input.expectedRevision,
      name: input.name,
      position: input.position,
      phone: input.phone,
      service_ids: input.serviceIds,
      member: input.member,
      archived: input.archived,
    },
  });
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}

export async function setCrmSchedule(
  input: CrmScheduleInput,
): Promise<CrmWriteResult> {
  const raw = await invokeTauri<RawCrmWriteResult>("crm_set_schedule", {
    input: toRawCrmScheduleInput(input),
  });
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}
