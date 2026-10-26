/**
 * Mock relay for CRM payroll (kinds 42160–42162, `GET /crm/payroll`).
 *
 * Mirrors the relay contract: only `manage_space` reads and writes, accruals
 * use the current rates over done visits and scheduled days up to today, and
 * a payout id is recorded once.
 */
import type { CrmCapability } from "@/shared/api/tauriCrm";
import type { RawCrmPayroll } from "@/shared/api/tauriCrmPayroll";
import { mockCrmAppointments } from "./e2eBridgeCrmAppointments.ts";
import {
  mockCrmAllStaff,
  mockCrmStaffById,
  mockCrmStaffDays,
} from "./e2eBridgeCrmStaff.ts";

export type MockCrmPayRatesSeed = {
  staff_id: string;
  percent_bp?: number;
  per_visit_minor?: number;
  shift_minor?: number;
};

export type MockCrmPayrollAccess = {
  channelId: string | null;
  requireCapability: (capability: CrmCapability) => void;
};

type Rates = {
  percent_bp: number;
  per_visit_minor: number;
  shift_minor: number;
  revision: string;
};
type Payout = RawCrmPayroll["payouts"][number];

let rates = new Map<string, Rates>();
let payouts: Payout[] = [];

function eventId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function seedMockCrmPayroll(seeds: MockCrmPayRatesSeed[] = []): void {
  rates = new Map(
    seeds.map((seed) => [
      seed.staff_id,
      {
        percent_bp: seed.percent_bp ?? 0,
        per_visit_minor: seed.per_visit_minor ?? 0,
        shift_minor: seed.shift_minor ?? 0,
        revision: eventId(),
      },
    ]),
  );
  payouts = [];
}

function requireManager(access: MockCrmPayrollAccess): void {
  if (!access.channelId) {
    throw "relay error 403: restricted: not a member of the CRM space";
  }
  access.requireCapability("manage_space");
}

function requireStaff(staffId: string): void {
  if (!mockCrmStaffById(staffId)) {
    throw "relay rejected event: invalid: staff_id is not a staff member of this space";
  }
}

function payroll(
  p: { from: string; to: string },
  access: MockCrmPayrollAccess,
): RawCrmPayroll {
  requireManager(access);
  const today = new Date().toISOString().slice(0, 10);
  const shiftEnd = p.to < today ? p.to : today;
  const live = payouts.filter(
    (payout) => payout.date >= p.from && payout.date <= p.to,
  );
  const staff = mockCrmAllStaff()
    .map((s) => {
      const r = rates.get(s.id);
      const done = mockCrmAppointments().filter(
        (a) =>
          a.staff_id === s.id &&
          a.status === "done" &&
          a.date >= p.from &&
          a.date <= p.to,
      );
      const revenue = done.reduce((sum, a) => sum + a.total_minor, 0);
      const shifts = mockCrmStaffDays().filter(
        (d) =>
          d.staff_id === s.id &&
          d.intervals.length > 0 &&
          d.date >= p.from &&
          d.date <= shiftEnd,
      ).length;
      const paid = live
        .filter((payout) => payout.staff_id === s.id && !payout.voided)
        .reduce((sum, payout) => sum + payout.amount_minor, 0);
      const percentBp = r?.percent_bp ?? 0;
      const accrued =
        Math.floor((revenue * percentBp) / 10_000) +
        done.length * (r?.per_visit_minor ?? 0) +
        shifts * (r?.shift_minor ?? 0);
      return {
        staff_id: s.id,
        name: s.name,
        archived: s.archived,
        percent_bp: percentBp,
        per_visit_minor: r?.per_visit_minor ?? 0,
        shift_minor: r?.shift_minor ?? 0,
        rates_revision: r?.revision ?? null,
        visits: done.length,
        revenue_minor: revenue,
        shifts,
        accrued_minor: accrued,
        paid_minor: paid,
        balance_minor: accrued - paid,
      };
    })
    .filter((row) => !row.archived || row.visits > 0 || row.paid_minor > 0)
    .sort((a, b) => a.name.localeCompare(b.name));
  return {
    from: p.from,
    to: p.to,
    staff,
    payouts: [...live].sort(
      (a, b) =>
        b.date.localeCompare(a.date) ||
        b.created_at.localeCompare(a.created_at),
    ),
  };
}

type RatesPayload = {
  input: {
    channel_id: string;
    staff_id: string;
    percent_bp: number;
    per_visit_minor: number;
    shift_minor: number;
  };
};

type PayoutPayload = {
  input: {
    channel_id: string;
    payout_id: string;
    staff_id: string;
    amount_minor: number;
    date: string;
    note: string | null;
  };
};

export function handleCrmPayrollMockCommand(
  command: string,
  payload: unknown,
  access: MockCrmPayrollAccess,
): { value: unknown } | undefined {
  switch (command) {
    case "crm_get_payroll":
      return {
        value: payroll(payload as { from: string; to: string }, access),
      };
    case "crm_set_pay_rates": {
      requireManager(access);
      const { input } = payload as RatesPayload;
      requireStaff(input.staff_id);
      if (input.percent_bp < 0 || input.percent_bp > 10_000) {
        throw "relay rejected event: invalid: percent_bp: must be 0–10000 (0–100 %)";
      }
      const id = eventId();
      rates.set(input.staff_id, {
        percent_bp: input.percent_bp,
        per_visit_minor: input.per_visit_minor,
        shift_minor: input.shift_minor,
        revision: id,
      });
      return { value: { event_id: id, entity_id: null } };
    }
    case "crm_record_payout": {
      requireManager(access);
      const { input } = payload as PayoutPayload;
      requireStaff(input.staff_id);
      if (payouts.some((payout) => payout.id === input.payout_id)) {
        throw "relay error 409: conflict: the payout is already recorded";
      }
      payouts.push({
        id: input.payout_id,
        staff_id: input.staff_id,
        date: input.date,
        amount_minor: input.amount_minor,
        note: input.note,
        voided: false,
        created_at: new Date().toISOString(),
      });
      return { value: { event_id: eventId(), entity_id: input.payout_id } };
    }
    case "crm_void_payout": {
      requireManager(access);
      const { payoutId } = payload as { payoutId: string };
      const payout = payouts.find((row) => row.id === payoutId);
      if (!payout) {
        throw "relay rejected event: invalid: payout_id is not a payout of this space";
      }
      if (payout.voided) {
        throw "relay error 409: conflict: the payout is already cancelled";
      }
      payout.voided = true;
      return { value: { event_id: eventId(), entity_id: null } };
    }
    default:
      return undefined;
  }
}
