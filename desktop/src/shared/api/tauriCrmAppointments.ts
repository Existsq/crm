import { invokeTauri } from "@/shared/api/tauri";
import type { CrmWriteResult } from "@/shared/api/tauriCrm";
import { formatClock } from "@/shared/api/tauriCrmStaff";

// ── Domain types ──────────────────────────────────────────────────────────

/** Where a visit is in its life; `no_show` and `cancelled` free the time. */
export type CrmAppointmentStatus =
  | "booked"
  | "confirmed"
  | "arrived"
  | "done"
  | "no_show"
  | "cancelled";

export const CRM_APPOINTMENT_STATUSES: readonly CrmAppointmentStatus[] = [
  "booked",
  "confirmed",
  "arrived",
  "done",
  "no_show",
  "cancelled",
];

/** One booked service with the duration and price agreed for it. */
export type CrmAppointmentService = {
  productId: string;
  durationMin: number;
  priceMinor: number;
};

/** Editable appointment fields (kind 42133 payload). */
export type CrmAppointmentFields = {
  staffId: string;
  /** `YYYY-MM-DD` in the space timezone. */
  date: string;
  /** Minutes after midnight. */
  start: number;
  services: CrmAppointmentService[];
  customerId: string | null;
  clientName: string | null;
  clientPhone: string | null;
  status: CrmAppointmentStatus;
  note: string | null;
  outsideHours: boolean;
};

export type CrmAppointment = CrmAppointmentFields & {
  id: string;
  /** Minutes after midnight. */
  end: number;
  /** Service names as booked. */
  serviceNames: string[];
  totalMinor: number;
  customerName: string | null;
  customerPhone: string | null;
  source: "staff" | "online";
  saleId: string | null;
  revision: string;
};

export type CrmAppointmentWriteInput = CrmAppointmentFields & {
  channelId: string;
  /** Appointment id; for a create pick it once per dialog. */
  appointmentId: string;
  /** Current revision; required for updates, `null` for creation. */
  expectedRevision: string | null;
};

/** Free start times of one staff member for the requested services. */
export type CrmStaffSlots = {
  staffId: string;
  name: string;
  /** `HH:MM`. */
  starts: string[];
};

export type CrmSlots = {
  date: string;
  durationMin: number;
  totalMinor: number;
  step: number;
  staff: CrmStaffSlots[];
};

// ── Raw types (snake_case from backend) ───────────────────────────────────

export type RawCrmAppointmentService = {
  product_id: string;
  name: string;
  duration_min: number;
  price_minor: number;
};

export type RawCrmAppointment = {
  id: string;
  staff_id: string;
  date: string;
  start: number;
  end: number;
  services: RawCrmAppointmentService[];
  total_minor: number;
  customer_id: string | null;
  customer_name: string | null;
  customer_phone: string | null;
  client_name: string | null;
  client_phone: string | null;
  status: CrmAppointmentStatus;
  note: string | null;
  source: "staff" | "online";
  outside_hours: boolean;
  sale_id: string | null;
  revision: string;
};

export type RawCrmSlots = {
  date: string;
  duration_min: number;
  total_minor: number;
  step: number;
  staff: Array<{ staff_id: string; name: string; starts: string[] }>;
};

type RawCrmWriteResult = {
  event_id: string;
  entity_id?: string | null;
};

// ── Mappers ───────────────────────────────────────────────────────────────

export function fromRawCrmAppointment(raw: RawCrmAppointment): CrmAppointment {
  return {
    id: raw.id,
    staffId: raw.staff_id,
    date: raw.date,
    start: raw.start,
    end: raw.end,
    services: raw.services.map((service) => ({
      productId: service.product_id,
      durationMin: service.duration_min,
      priceMinor: service.price_minor,
    })),
    serviceNames: raw.services.map((service) => service.name),
    totalMinor: raw.total_minor,
    customerId: raw.customer_id ?? null,
    customerName: raw.customer_name ?? null,
    customerPhone: raw.customer_phone ?? null,
    clientName: raw.client_name ?? null,
    clientPhone: raw.client_phone ?? null,
    status: raw.status,
    note: raw.note ?? null,
    source: raw.source,
    outsideHours: raw.outside_hours,
    saleId: raw.sale_id ?? null,
    revision: raw.revision,
  };
}

export function toRawCrmAppointmentInput(input: CrmAppointmentWriteInput) {
  return {
    channel_id: input.channelId,
    appointment_id: input.appointmentId,
    expected_revision: input.expectedRevision,
    staff_id: input.staffId,
    date: input.date,
    start: formatClock(input.start),
    services: input.services.map((service) => ({
      product_id: service.productId,
      duration_min: service.durationMin,
      price_minor: service.priceMinor,
    })),
    customer_id: input.customerId,
    client_name: input.clientName,
    client_phone: input.clientPhone,
    status: input.status,
    note: input.note,
    outside_hours: input.outsideHours,
  };
}

// ── Commands ──────────────────────────────────────────────────────────────

/** The journal `from..=to` (at most 93 days). */
export async function listCrmAppointments(filter: {
  from: string;
  to: string;
  staffId?: string | null;
}): Promise<CrmAppointment[]> {
  const raw = await invokeTauri<{ appointments: RawCrmAppointment[] }>(
    "crm_list_appointments",
    { from: filter.from, to: filter.to, staffId: filter.staffId ?? null },
  );
  return raw.appointments.map(fromRawCrmAppointment);
}

/** Every visit of one client, newest first. */
export async function listCrmCustomerAppointments(
  customerId: string,
): Promise<CrmAppointment[]> {
  const raw = await invokeTauri<{ appointments: RawCrmAppointment[] }>(
    "crm_list_appointments",
    { customerId },
  );
  return raw.appointments.map(fromRawCrmAppointment);
}

export async function getCrmSlots(request: {
  date: string;
  serviceIds: string[];
  staffId?: string | null;
  step?: number;
}): Promise<CrmSlots> {
  const raw = await invokeTauri<RawCrmSlots>("crm_get_slots", {
    date: request.date,
    serviceIds: request.serviceIds,
    staffId: request.staffId ?? null,
    step: request.step ?? null,
  });
  return {
    date: raw.date,
    durationMin: raw.duration_min,
    totalMinor: raw.total_minor,
    step: raw.step,
    staff: raw.staff.map((s) => ({
      staffId: s.staff_id,
      name: s.name,
      starts: s.starts,
    })),
  };
}

export async function writeCrmAppointment(
  input: CrmAppointmentWriteInput,
): Promise<CrmWriteResult> {
  const raw = await invokeTauri<RawCrmWriteResult>("crm_write_appointment", {
    input: toRawCrmAppointmentInput(input),
  });
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}
