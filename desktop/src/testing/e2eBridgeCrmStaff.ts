/**
 * E2E mock for the CRM staff commands (`crm_*_staff`, `crm_*_schedule`).
 *
 * Mirrors the relay contract for kinds 42131 and 42132: compare-and-swap
 * staff revisions, services limited to bookable products, archived staff
 * taking no schedule changes, and the interval rules of `SchedulePayload`.
 */

import type { CrmCapability } from "@/shared/api/tauriCrm";
import type {
  CrmWorkInterval,
  RawCrmScheduleDay,
  RawCrmStaff,
} from "@/shared/api/tauriCrmStaff";

import { mockCrmServiceIds } from "./e2eBridgeCrmProducts.ts";

export type MockCrmStaffSeed = {
  id?: string;
  name: string;
  position?: string | null;
  phone?: string | null;
  service_ids?: string[];
  archived?: boolean;
  /** Working days, intervals in minutes after midnight. */
  days?: Array<{ date: string; intervals: CrmWorkInterval[] }>;
};

export type MockCrmStaffAccess = {
  channelId: string | null;
  requireCapability: (capability: CrmCapability) => void;
};

type WriteStaffPayload = {
  input: {
    channel_id: string;
    staff_id: string;
    expected_revision: string | null;
    name: string;
    position: string | null;
    phone: string | null;
    service_ids: string[];
    member: string | null;
    archived: boolean;
  };
};

type SetSchedulePayload = {
  input: {
    channel_id: string;
    staff_id: string;
    days: Array<{
      date: string;
      intervals: Array<{ start: string; end: string }>;
    }>;
  };
};

let staff: RawCrmStaff[] = [];
let days: RawCrmScheduleDay[] = [];

function eventId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function seedMockCrmStaff(seeds: MockCrmStaffSeed[] = []): void {
  const now = new Date().toISOString();
  staff = [];
  days = [];
  for (const seed of seeds) {
    const id = seed.id ?? crypto.randomUUID();
    staff.push({
      id,
      name: seed.name,
      position: seed.position ?? null,
      phone: seed.phone ?? null,
      service_ids: seed.service_ids ?? [],
      member: null,
      archived: seed.archived ?? false,
      revision: eventId(),
      updated_at: now,
    });
    for (const day of seed.days ?? []) {
      days.push({ staff_id: id, date: day.date, intervals: day.intervals });
    }
  }
}

function requireMember(access: MockCrmStaffAccess): void {
  if (!access.channelId) {
    throw "relay error 403: restricted: not a member of the CRM space";
  }
}

function requireSpace(access: MockCrmStaffAccess, channelId: string): void {
  access.requireCapability("edit_catalog");
  if (channelId !== access.channelId) {
    throw "relay error 400: invalid: this channel is not the CRM space";
  }
}

function listStaff(
  { archived }: { archived?: boolean | null },
  access: MockCrmStaffAccess,
) {
  requireMember(access);
  return {
    staff: staff
      .filter((s) => archived || !s.archived)
      .sort(
        (a, b) =>
          a.name.toLowerCase().localeCompare(b.name.toLowerCase()) ||
          a.id.localeCompare(b.id),
      )
      .map((s) => ({ ...s, service_ids: [...s.service_ids] })),
  };
}

function getSchedule(
  { from, to }: { from: string; to: string },
  access: MockCrmStaffAccess,
) {
  requireMember(access);
  return {
    from,
    to,
    days: days
      .filter((d) => d.date >= from && d.date <= to)
      .sort(
        (a, b) =>
          a.date.localeCompare(b.date) || a.staff_id.localeCompare(b.staff_id),
      ),
  };
}

function writeStaff({ input }: WriteStaffPayload, access: MockCrmStaffAccess) {
  requireSpace(access, input.channel_id);
  const name = input.name.trim();
  if (!name) throw "relay rejected event: invalid: name must not be empty";
  const services = mockCrmServiceIds();
  const unknown = input.service_ids.find((id) => !services.has(id));
  if (unknown) {
    throw `relay error 400: invalid: service ${unknown} is not a service of this space`;
  }
  const current = staff.find((s) => s.id === input.staff_id);
  if (input.expected_revision === null) {
    if (current) {
      throw "relay error 409: conflict: staff member already exists; send expected-revision to update it";
    }
  } else {
    if (!current)
      throw "relay error 409: conflict: staff member does not exist";
    if (current.revision !== input.expected_revision) {
      throw "relay error 409: conflict: staff member changed since it was loaded";
    }
  }
  const revision = eventId();
  const next: RawCrmStaff = {
    id: input.staff_id,
    name,
    position: input.position?.trim() || null,
    phone: input.phone?.trim() || null,
    service_ids: [...new Set(input.service_ids)],
    member: input.member,
    archived: input.archived,
    revision,
    updated_at: new Date().toISOString(),
  };
  staff = current
    ? staff.map((s) => (s.id === next.id ? next : s))
    : [...staff, next];
  return { event_id: revision, entity_id: next.id };
}

function minutes(clock: string): number {
  const match = /^(\d{2}):(\d{2})$/.exec(clock);
  if (!match)
    throw `relay rejected event: invalid: ${clock} is not a HH:MM time`;
  return Number(match[1]) * 60 + Number(match[2]);
}

function setSchedule(
  { input }: SetSchedulePayload,
  access: MockCrmStaffAccess,
) {
  requireSpace(access, input.channel_id);
  const member = staff.find((s) => s.id === input.staff_id);
  if (!member) {
    throw "relay error 400: invalid: staff member does not exist";
  }
  if (member.archived) {
    throw "relay error 400: invalid: staff member is archived";
  }
  if (input.days.length === 0 || input.days.length > 93) {
    throw "relay rejected event: invalid: days: must set 1–93 days";
  }
  const parsed = input.days.map((day) => {
    const intervals = day.intervals.map((i) => ({
      start: minutes(i.start),
      end: minutes(i.end),
    }));
    intervals.forEach((interval, index) => {
      const previous = intervals[index - 1];
      if (
        interval.start >= interval.end ||
        interval.start % 5 !== 0 ||
        interval.end % 5 !== 0 ||
        interval.end > 1440 ||
        (previous && previous.end > interval.start)
      ) {
        throw "relay rejected event: invalid: intervals: must be sorted, non-overlapping and on a 5-minute grid";
      }
    });
    return { staff_id: member.id, date: day.date, intervals };
  });
  const dates = new Set(parsed.map((d) => d.date));
  days = [
    ...days.filter((d) => d.staff_id !== member.id || !dates.has(d.date)),
    ...parsed,
  ];
  const revision = eventId();
  return { event_id: revision, entity_id: null };
}

/** Handle a staff command, or `undefined` for anything else. */
export function handleCrmStaffMockCommand(
  command: string,
  payload: unknown,
  access: MockCrmStaffAccess,
): { value: unknown } | undefined {
  switch (command) {
    case "crm_list_staff":
      return {
        value: listStaff(payload as { archived?: boolean | null }, access),
      };
    case "crm_get_schedule":
      return {
        value: getSchedule(payload as { from: string; to: string }, access),
      };
    case "crm_write_staff":
      return { value: writeStaff(payload as WriteStaffPayload, access) };
    case "crm_set_schedule":
      return { value: setSchedule(payload as SetSchedulePayload, access) };
    default:
      return undefined;
  }
}
