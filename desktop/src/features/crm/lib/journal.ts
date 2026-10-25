import { ru } from "@/features/crm/i18n/ru";
import type {
  CrmAppointment,
  CrmAppointmentService,
  CrmAppointmentStatus,
  CrmAppointmentWriteInput,
} from "@/shared/api/tauriCrmAppointments";
import type { CrmProduct } from "@/shared/api/tauriCrmProducts";
import type { CrmWorkInterval } from "@/shared/api/tauriCrmStaff";

/** Rows of the day grid, in minutes. */
export const JOURNAL_ROW_MIN = 30;
/** Hours shown when nobody works that day. */
export const DEFAULT_JOURNAL_WINDOW = { start: 9 * 60, end: 21 * 60 };
/** Relay bound of `AppointmentPayload` services. */
export const MAX_APPOINTMENT_SERVICES = 20;

/** Statuses whose visit still holds the staff member's time. */
export function holdsTime(status: CrmAppointmentStatus): boolean {
  return status !== "no_show" && status !== "cancelled";
}

/** The name shown for the client of a visit. */
export function clientLabel(appointment: CrmAppointment): string {
  return appointment.customerName ?? appointment.clientName ?? "";
}

export function clientPhone(appointment: CrmAppointment): string | null {
  return appointment.customerPhone ?? appointment.clientPhone;
}

/**
 * Hours the grid shows: every working interval and every visit, widened to
 * whole hours; nine to nine when the day is empty.
 */
export function journalWindow(
  intervals: readonly CrmWorkInterval[],
  appointments: readonly Pick<CrmAppointment, "start" | "end">[],
): { start: number; end: number } {
  const spans = [...intervals, ...appointments];
  if (spans.length === 0) return DEFAULT_JOURNAL_WINDOW;
  const start = Math.min(...spans.map((span) => span.start));
  const end = Math.max(...spans.map((span) => span.end));
  return {
    start: Math.floor(start / 60) * 60,
    end: Math.min(24 * 60, Math.ceil(end / 60) * 60),
  };
}

/** Row start times of the grid window. */
export function journalRows(window: { start: number; end: number }): number[] {
  const rows: number[] = [];
  for (let t = window.start; t < window.end; t += JOURNAL_ROW_MIN) rows.push(t);
  return rows;
}

export function isWorking(
  intervals: readonly CrmWorkInterval[],
  start: number,
  end: number,
): boolean {
  return intervals.some((i) => i.start <= start && end <= i.end);
}

/**
 * Side-by-side lanes for visits that overlap in one column (a missed visit
 * and the one booked over it). Each item gets its lane and the lane count of
 * its overlap group.
 */
export function assignLanes<T extends { start: number; end: number }>(
  items: readonly T[],
): Array<{ item: T; lane: number; lanes: number }> {
  const sorted = [...items].sort((a, b) => a.start - b.start || a.end - b.end);
  const placed: Array<{ item: T; lane: number; lanes: number }> = [];
  let group: Array<{ item: T; lane: number; lanes: number }> = [];
  let groupEnd = -1;
  let laneEnds: number[] = [];
  const closeGroup = () => {
    for (const entry of group) entry.lanes = laneEnds.length;
    placed.push(...group);
    group = [];
    laneEnds = [];
  };
  for (const item of sorted) {
    if (item.start >= groupEnd) {
      closeGroup();
      groupEnd = item.end;
    }
    let lane = laneEnds.findIndex((end) => end <= item.start);
    if (lane < 0) {
      lane = laneEnds.length;
      laneEnds.push(item.end);
    } else {
      laneEnds[lane] = item.end;
    }
    groupEnd = Math.max(groupEnd, item.end);
    group.push({ item, lane, lanes: 0 });
  }
  closeGroup();
  return placed;
}

/** Bookable services the staff member performs, in catalog order. */
export function staffServices(
  services: readonly CrmProduct[],
  serviceIds: readonly string[],
): CrmProduct[] {
  const ids = new Set(serviceIds);
  return services.filter(
    (service) =>
      ids.has(service.id) && !service.archived && service.durationMin !== null,
  );
}

export function servicesTotal(services: readonly CrmAppointmentService[]): {
  durationMin: number;
  totalMinor: number;
} {
  return services.reduce(
    (sum, service) => ({
      durationMin: sum.durationMin + service.durationMin,
      totalMinor: sum.totalMinor + service.priceMinor,
    }),
    { durationMin: 0, totalMinor: 0 },
  );
}

/** `HH:MM` as minutes, `null` when malformed. */
export function clockMinutes(raw: string): number | null {
  const match = /^(\d{2}):(\d{2})$/.exec(raw);
  if (!match) return null;
  const minutes = Number(match[1]) * 60 + Number(match[2]);
  return Number(match[2]) < 60 && minutes < 24 * 60 ? minutes : null;
}

/**
 * Start times the time picker offers: the free slots plus the visit's own
 * time (the relay counts a visit as busy in its own slots) and the time the
 * user clicked in the grid.
 */
export function startChoices(
  slots: readonly string[],
  keep: number | null,
): number[] {
  const choices = new Set<number>();
  for (const slot of slots) {
    const minutes = clockMinutes(slot);
    if (minutes !== null) choices.add(minutes);
  }
  if (keep !== null) choices.add(keep);
  return [...choices].sort((a, b) => a - b);
}

export type AppointmentDraft = {
  staffId: string;
  date: string;
  start: number | null;
  services: CrmAppointmentService[];
  customer: { id: string; name: string; phone: string | null } | null;
  clientName: string;
  clientPhone: string;
  status: CrmAppointmentStatus;
  note: string;
  outsideHours: boolean;
};

export type AppointmentDraftResult =
  | {
      ok: true;
      fields: Omit<
        CrmAppointmentWriteInput,
        "channelId" | "appointmentId" | "expectedRevision"
      >;
    }
  | { ok: false; field: "client" | "services" | "time"; error: string };

export function validateAppointmentDraft(
  draft: AppointmentDraft,
): AppointmentDraftResult {
  const v = ru.journal.dialog.validation;
  const clientName = draft.clientName.trim();
  if (!draft.customer && clientName.length === 0) {
    return { ok: false, field: "client", error: v.client };
  }
  if (draft.services.length === 0) {
    return { ok: false, field: "services", error: v.services };
  }
  if (draft.start === null) {
    return { ok: false, field: "time", error: v.time };
  }
  const phone = draft.clientPhone.trim();
  return {
    ok: true,
    fields: {
      staffId: draft.staffId,
      date: draft.date,
      start: draft.start,
      services: draft.services,
      customerId: draft.customer?.id ?? null,
      clientName: draft.customer ? null : clientName,
      clientPhone: draft.customer || phone.length === 0 ? null : phone,
      status: draft.status,
      note: draft.note.trim() || null,
      outsideHours: draft.outsideHours,
    },
  };
}

export function draftFromAppointment(
  appointment: CrmAppointment,
): AppointmentDraft {
  return {
    staffId: appointment.staffId,
    date: appointment.date,
    start: appointment.start,
    services: appointment.services.map((service) => ({ ...service })),
    customer: appointment.customerId
      ? {
          id: appointment.customerId,
          name: appointment.customerName ?? "",
          phone: appointment.customerPhone,
        }
      : null,
    clientName: appointment.clientName ?? "",
    clientPhone: appointment.clientPhone ?? "",
    status: appointment.status,
    note: appointment.note ?? "",
    outsideHours: appointment.outsideHours,
  };
}

/** The write that moves a loaded visit to `status`, nothing else changed. */
export function statusWrite(
  appointment: CrmAppointment,
  channelId: string,
  status: CrmAppointmentStatus,
): CrmAppointmentWriteInput {
  return {
    channelId,
    appointmentId: appointment.id,
    expectedRevision: appointment.revision,
    staffId: appointment.staffId,
    date: appointment.date,
    start: appointment.start,
    services: appointment.services,
    customerId: appointment.customerId,
    clientName: appointment.customerId ? null : appointment.clientName,
    clientPhone: appointment.customerId ? null : appointment.clientPhone,
    status,
    note: appointment.note,
    outsideHours: appointment.outsideHours,
  };
}
