/**
 * E2E mock for the CRM journal commands (`crm_list_appointments`,
 * `crm_get_slots`, `crm_write_appointment`).
 *
 * Mirrors the relay contract for kind 42133: compare-and-swap revisions,
 * services the staff member performs, working hours unless booked outside
 * them, no overlap with another visit that holds its time, and free start
 * times on the requested step.
 */

import type { CrmCapability } from "@/shared/api/tauriCrm";
import type {
  CrmAppointmentStatus,
  RawCrmAppointment,
} from "@/shared/api/tauriCrmAppointments";

import { mockCrmProductById } from "./e2eBridgeCrmProducts.ts";
import { mockCrmPartyById } from "./e2eBridgeCrmSales.ts";
import {
  mockCrmActiveStaff,
  mockCrmAllStaff,
  mockCrmStaffDays,
  mockCrmStaffById,
  mockCrmStaffIntervals,
} from "./e2eBridgeCrmStaff.ts";

export type MockCrmAppointmentSeed = {
  id?: string;
  staff_id: string;
  date: string;
  /** `HH:MM`. */
  start: string;
  service_ids: string[];
  customer_id?: string | null;
  client_name?: string | null;
  client_phone?: string | null;
  status?: CrmAppointmentStatus;
  note?: string | null;
};

export type MockCrmAppointmentAccess = {
  channelId: string | null;
  requireCapability: (capability: CrmCapability) => void;
};

type WriteAppointmentPayload = {
  input: {
    channel_id: string;
    appointment_id: string;
    expected_revision: string | null;
    staff_id: string;
    date: string;
    start: string;
    services: Array<{
      product_id: string;
      duration_min: number;
      price_minor: number;
    }>;
    customer_id: string | null;
    client_name: string | null;
    client_phone: string | null;
    status: CrmAppointmentStatus;
    note: string | null;
    outside_hours: boolean;
  };
};

let appointments: RawCrmAppointment[] = [];

function eventId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function minutes(clock: string): number {
  const match = /^(\d{2}):(\d{2})$/.exec(clock);
  if (!match || Number(match[2]) >= 60) {
    throw `relay rejected event: invalid: start: ${clock} is not a HH:MM time`;
  }
  return Number(match[1]) * 60 + Number(match[2]);
}

function clock(total: number): string {
  return `${String(Math.floor(total / 60)).padStart(2, "0")}:${String(total % 60).padStart(2, "0")}`;
}

function holdsTime(status: CrmAppointmentStatus): boolean {
  return status !== "no_show" && status !== "cancelled";
}

function withClient(row: RawCrmAppointment): RawCrmAppointment {
  const party = row.customer_id ? mockCrmPartyById(row.customer_id) : undefined;
  return {
    ...row,
    customer_name: party?.name ?? null,
    customer_phone: party?.phone ?? null,
  };
}

export function seedMockCrmAppointments(
  seeds: MockCrmAppointmentSeed[] = [],
): void {
  appointments = seeds.map((seed) => {
    const services = seed.service_ids.map((id) => {
      const product = mockCrmProductById(id);
      return {
        product_id: id,
        name: product?.name ?? "?",
        duration_min: product?.duration_min ?? 60,
        price_minor: product?.price_minor ?? 0,
      };
    });
    const start = minutes(seed.start);
    const duration = services.reduce((sum, s) => sum + s.duration_min, 0);
    return {
      id: seed.id ?? crypto.randomUUID(),
      staff_id: seed.staff_id,
      date: seed.date,
      start,
      end: start + duration,
      services,
      total_minor: services.reduce((sum, s) => sum + s.price_minor, 0),
      customer_id: seed.customer_id ?? null,
      customer_name: null,
      customer_phone: null,
      client_name: seed.customer_id ? null : (seed.client_name ?? "Client"),
      client_phone: seed.client_phone ?? null,
      status: seed.status ?? "booked",
      note: seed.note ?? null,
      source: "staff",
      outside_hours: false,
      sale_id: null,
      revision: eventId(),
    };
  });
}

function requireMember(access: MockCrmAppointmentAccess): void {
  if (!access.channelId) {
    throw "relay error 403: restricted: not a member of the CRM space";
  }
}

function listAppointments(
  p: { from?: string | null; to?: string | null; customerId?: string | null },
  access: MockCrmAppointmentAccess,
) {
  requireMember(access);
  const rows = appointments
    .filter((a) =>
      p.customerId
        ? a.customer_id === p.customerId
        : a.date >= (p.from ?? "") && a.date <= (p.to ?? ""),
    )
    .sort(
      (a, b) =>
        a.date.localeCompare(b.date) ||
        a.start - b.start ||
        a.staff_id.localeCompare(b.staff_id),
    );
  return { appointments: rows.map(withClient) };
}

function busy(staffId: string, date: string, except: string | null) {
  return appointments.filter(
    (a) =>
      a.staff_id === staffId &&
      a.date === date &&
      a.id !== except &&
      holdsTime(a.status),
  );
}

function getSlots(
  p: {
    date: string;
    serviceIds: string[];
    staffId?: string | null;
    step?: number | null;
  },
  access: MockCrmAppointmentAccess,
) {
  requireMember(access);
  const step = p.step ?? 15;
  const services = p.serviceIds.map((id) => {
    const product = mockCrmProductById(id);
    if (!product || product.duration_min == null) {
      throw `relay error 400: service_ids: ${id} is not an active service`;
    }
    return product;
  });
  const duration = services.reduce((sum, s) => sum + (s.duration_min ?? 0), 0);
  const staff = mockCrmActiveStaff().filter(
    (s) =>
      (!p.staffId || s.id === p.staffId) &&
      p.serviceIds.every((id) => s.service_ids.includes(id)),
  );
  return {
    date: p.date,
    duration_min: duration,
    total_minor: services.reduce((sum, s) => sum + s.price_minor, 0),
    step,
    staff: staff.map((s) => {
      const taken = busy(s.id, p.date, null);
      const starts: string[] = [];
      for (const interval of mockCrmStaffIntervals(s.id, p.date)) {
        const first = Math.ceil(interval.start / step) * step;
        for (let t = first; t + duration <= interval.end; t += step) {
          if (!taken.some((a) => a.start < t + duration && t < a.end)) {
            starts.push(clock(t));
          }
        }
      }
      return { staff_id: s.id, name: s.name, starts };
    }),
  };
}

function writeAppointment(
  { input }: WriteAppointmentPayload,
  access: MockCrmAppointmentAccess,
) {
  access.requireCapability("post_documents");
  if (input.channel_id !== access.channelId) {
    throw "relay error 400: invalid: this channel is not the CRM space";
  }
  const name = input.client_name?.trim() || null;
  if (!input.customer_id && !name) {
    throw "relay rejected event: invalid: client_name: name the client or pick a customer";
  }
  if (input.services.length === 0) {
    throw "relay rejected event: invalid: services: book at least one service";
  }
  const current = appointments.find((a) => a.id === input.appointment_id);
  if (input.expected_revision === null) {
    if (current) {
      throw "relay error 409: conflict: appointment already exists; send expected-revision to update it";
    }
  } else {
    if (!current) throw "relay error 409: conflict: appointment does not exist";
    if (current.revision !== input.expected_revision) {
      throw "relay error 409: conflict: appointment changed since it was loaded";
    }
  }
  const member = mockCrmStaffById(input.staff_id);
  if (!member) {
    throw "relay error 400: invalid: staff_id is not a staff member of this space";
  }
  const start = minutes(input.start);
  const duration = input.services.reduce((sum, s) => sum + s.duration_min, 0);
  const end = start + duration;
  if (end > 1440) {
    throw "relay rejected event: invalid: start: the visit must end by midnight";
  }
  const status = input.status;
  const services = input.services.map((s) => {
    const product = mockCrmProductById(s.product_id);
    if (!product) {
      throw `relay error 400: invalid: services: ${s.product_id} is not an active service of this space`;
    }
    if (holdsTime(status) && !member.service_ids.includes(s.product_id)) {
      throw `relay error 400: invalid: services: the staff member does not perform ${s.product_id}`;
    }
    return { ...s, name: product.name };
  });
  if (holdsTime(status)) {
    const inside = mockCrmStaffIntervals(member.id, input.date).some(
      (i) => i.start <= start && end <= i.end,
    );
    if (!input.outside_hours && !inside) {
      throw "relay error 400: invalid: the visit is outside the staff member's working hours";
    }
    const clash = busy(member.id, input.date, input.appointment_id).find(
      (a) => a.start < end && start < a.end,
    );
    if (clash) {
      throw `relay error 409: conflict: the staff member is booked ${clock(clash.start)}–${clock(clash.end)}`;
    }
  }
  const revision = eventId();
  const next: RawCrmAppointment = {
    id: input.appointment_id,
    staff_id: member.id,
    date: input.date,
    start,
    end,
    services,
    total_minor: services.reduce((sum, s) => sum + s.price_minor, 0),
    customer_id: input.customer_id,
    customer_name: null,
    customer_phone: null,
    client_name: input.customer_id ? null : name,
    client_phone: input.customer_id ? null : input.client_phone?.trim() || null,
    status,
    note: input.note?.trim() || null,
    source: "staff",
    outside_hours: input.outside_hours,
    sale_id: current?.sale_id ?? null,
    revision,
  };
  appointments = current
    ? appointments.map((a) => (a.id === next.id ? next : a))
    : [...appointments, next];
  return { event_id: revision, entity_id: next.id };
}

/** Throw the relay's rejection when a sale may not pay for `appointmentId`. */
export function checkMockAppointmentPayable(appointmentId: string): void {
  const visit = appointments.find((a) => a.id === appointmentId);
  if (!visit) {
    throw "relay rejected event: invalid: appointment_id: not an appointment of this CRM space";
  }
  if (visit.sale_id) {
    throw "relay error 409: conflict: the visit is already paid";
  }
  if (!holdsTime(visit.status)) {
    throw "relay rejected event: invalid: appointment_id: a cancelled or missed visit cannot be paid";
  }
}

/** Link a posted sale to its visit, as the relay does for kind 42104. */
export function payMockAppointment(
  appointmentId: string,
  saleId: string,
): void {
  appointments = appointments.map((a) =>
    a.id === appointmentId
      ? { ...a, sale_id: saleId, status: "done", revision: eventId() }
      : a,
  );
}

/** Unlink a voided sale from its visit, as the relay does for kind 42105. */
export function unpayMockAppointment(saleId: string): void {
  appointments = appointments.map((a) =>
    a.sale_id === saleId
      ? { ...a, sale_id: null, status: "arrived", revision: eventId() }
      : a,
  );
}

function visitAnalytics(
  p: { from?: string | null; to?: string | null },
  access: MockCrmAppointmentAccess,
) {
  requireMember(access);
  const to = p.to ?? new Date().toISOString().slice(0, 10);
  const from = p.from ?? to;
  const inPeriod = appointments.filter((a) => a.date >= from && a.date <= to);
  const held = inPeriod.filter((a) => holdsTime(a.status));
  const staff = mockCrmAllStaff()
    .map((s) => {
      const mine = inPeriod.filter((a) => a.staff_id === s.id);
      const kept = mine.filter((a) => holdsTime(a.status));
      const done = mine.filter((a) => a.status === "done");
      return {
        staff_id: s.id,
        name: s.name,
        archived: s.archived,
        visits: kept.length,
        done: done.length,
        no_shows: mine.filter((a) => a.status === "no_show").length,
        cancelled: mine.filter((a) => a.status === "cancelled").length,
        revenue_minor: done.reduce((sum, a) => sum + a.total_minor, 0),
        booked_min: kept.reduce((sum, a) => sum + a.end - a.start, 0),
        worked_min: mockCrmStaffDays()
          .filter((d) => d.staff_id === s.id && d.date >= from && d.date <= to)
          .flatMap((d) => d.intervals)
          .reduce((sum, i) => sum + i.end - i.start, 0),
        any: mine.length > 0,
      };
    })
    .filter((s) => !s.archived || s.any)
    .sort((a, b) => b.revenue_minor - a.revenue_minor || b.visits - a.visits)
    .map(({ archived: _archived, any: _any, ...row }) => row);
  const services = new Map<
    string,
    {
      product_id: string;
      name: string;
      visits: number;
      revenue_minor: number;
      minutes: number;
    }
  >();
  for (const visit of held) {
    for (const s of visit.services) {
      const row = services.get(s.product_id) ?? {
        product_id: s.product_id,
        name: s.name,
        visits: 0,
        revenue_minor: 0,
        minutes: 0,
      };
      row.visits += 1;
      row.revenue_minor += s.price_minor;
      row.minutes += s.duration_min;
      services.set(s.product_id, row);
    }
  }
  const clients = new Set(
    held.flatMap((a) => (a.customer_id ? [a.customer_id] : [])),
  );
  const returning = new Set(
    appointments
      .filter((a) => a.date < from && holdsTime(a.status) && a.customer_id)
      .map((a) => a.customer_id),
  );
  return {
    from,
    to,
    staff,
    services: [...services.values()].sort((a, b) => b.visits - a.visits),
    clients: {
      clients: clients.size,
      new_clients: [...clients].filter((id) => !returning.has(id)).length,
      walk_ins: held.filter((a) => !a.customer_id).length,
    },
  };
}

/** Handle a journal command, or `undefined` for anything else. */
export function handleCrmAppointmentMockCommand(
  command: string,
  payload: unknown,
  access: MockCrmAppointmentAccess,
): { value: unknown } | undefined {
  switch (command) {
    case "crm_list_appointments":
      return {
        value: listAppointments(
          payload as Parameters<typeof listAppointments>[0],
          access,
        ),
      };
    case "crm_get_appointment": {
      requireMember(access);
      const id = (payload as { appointmentId: string }).appointmentId;
      const visit = appointments.find((a) => a.id === id);
      if (!visit) throw "relay error 404: appointment not found";
      return { value: withClient(visit) };
    }
    case "crm_get_visit_analytics":
      return {
        value: visitAnalytics(
          payload as Parameters<typeof visitAnalytics>[0],
          access,
        ),
      };
    case "crm_get_slots":
      return {
        value: getSlots(payload as Parameters<typeof getSlots>[0], access),
      };
    case "crm_write_appointment":
      return {
        value: writeAppointment(payload as WriteAppointmentPayload, access),
      };
    default:
      return undefined;
  }
}

/** Every booked visit, for other CRM mocks (payroll). */
export function mockCrmAppointments(): readonly RawCrmAppointment[] {
  return appointments;
}
