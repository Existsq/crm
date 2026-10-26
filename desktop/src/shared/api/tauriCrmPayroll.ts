import { invokeTauri } from "@/shared/api/tauri";
import type { CrmWriteResult } from "@/shared/api/tauriCrm";

/** One staff member's payroll for a period (`GET /crm/payroll`). */
export type CrmStaffPayroll = {
  staffId: string;
  name: string;
  archived: boolean;
  /** Share of performed services, basis points (1500 = 15 %). */
  percentBp: number;
  perVisitMinor: number;
  shiftMinor: number;
  /** `null` until rates are set. */
  ratesRevision: string | null;
  visits: number;
  revenueMinor: number;
  shifts: number;
  accruedMinor: number;
  paidMinor: number;
  balanceMinor: number;
};

export type CrmPayout = {
  id: string;
  staffId: string;
  date: string;
  amountMinor: number;
  note: string | null;
  voided: boolean;
  createdAt: string;
};

export type CrmPayroll = {
  from: string;
  to: string;
  staff: CrmStaffPayroll[];
  payouts: CrmPayout[];
};

export type CrmPayRatesInput = {
  channelId: string;
  staffId: string;
  percentBp: number;
  perVisitMinor: number;
  shiftMinor: number;
};

export type CrmPayoutInput = {
  channelId: string;
  /** Pick once per dialog so a retried submit cannot pay twice. */
  payoutId: string;
  staffId: string;
  amountMinor: number;
  date: string;
  note: string | null;
};

export type RawCrmPayroll = {
  from: string;
  to: string;
  staff: Array<{
    staff_id: string;
    name: string;
    archived: boolean;
    percent_bp: number;
    per_visit_minor: number;
    shift_minor: number;
    rates_revision: string | null;
    visits: number;
    revenue_minor: number;
    shifts: number;
    accrued_minor: number;
    paid_minor: number;
    balance_minor: number;
  }>;
  payouts: Array<{
    id: string;
    staff_id: string;
    date: string;
    amount_minor: number;
    note: string | null;
    voided: boolean;
    created_at: string;
  }>;
};

type RawCrmWriteResult = { event_id: string; entity_id?: string | null };

export function fromRawCrmPayroll(raw: RawCrmPayroll): CrmPayroll {
  return {
    from: raw.from,
    to: raw.to,
    staff: raw.staff.map((s) => ({
      staffId: s.staff_id,
      name: s.name,
      archived: s.archived,
      percentBp: s.percent_bp,
      perVisitMinor: s.per_visit_minor,
      shiftMinor: s.shift_minor,
      ratesRevision: s.rates_revision ?? null,
      visits: s.visits,
      revenueMinor: s.revenue_minor,
      shifts: s.shifts,
      accruedMinor: s.accrued_minor,
      paidMinor: s.paid_minor,
      balanceMinor: s.balance_minor,
    })),
    payouts: raw.payouts.map((p) => ({
      id: p.id,
      staffId: p.staff_id,
      date: p.date,
      amountMinor: p.amount_minor,
      note: p.note ?? null,
      voided: p.voided,
      createdAt: p.created_at,
    })),
  };
}

export async function getCrmPayroll(period: {
  from: string;
  to: string;
}): Promise<CrmPayroll> {
  return fromRawCrmPayroll(
    await invokeTauri<RawCrmPayroll>("crm_get_payroll", {
      from: period.from,
      to: period.to,
    }),
  );
}

function fromRawWrite(raw: RawCrmWriteResult): CrmWriteResult {
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}

export async function setCrmPayRates(
  input: CrmPayRatesInput,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_set_pay_rates", {
      input: {
        channel_id: input.channelId,
        staff_id: input.staffId,
        percent_bp: input.percentBp,
        per_visit_minor: input.perVisitMinor,
        shift_minor: input.shiftMinor,
      },
    }),
  );
}

export async function recordCrmPayout(
  input: CrmPayoutInput,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_record_payout", {
      input: {
        channel_id: input.channelId,
        payout_id: input.payoutId,
        staff_id: input.staffId,
        amount_minor: input.amountMinor,
        date: input.date,
        note: input.note,
      },
    }),
  );
}

export async function voidCrmPayout(
  channelId: string,
  payoutId: string,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_void_payout", {
      channelId,
      payoutId,
    }),
  );
}
