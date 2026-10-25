import { invokeTauri } from "@/shared/api/tauri";

// ── Cash desk: the ATOL DTO10 cash register (ADR-0003) ────────────────────

/** Where the DTO10 web server is and who is at the till. */
export type CrmCashSettings = { baseUrl: string; operatorName: string };

/** Shift state as the device reports it. */
export type CrmCashDeviceStatus = {
  reachable: boolean;
  shiftState: "opened" | "closed" | "expired" | null;
  shiftNumber: number | null;
  error: string | null;
};

export type CrmCashShiftResult = {
  shiftNumber: number;
  documentNumber: number;
  /** Set when the device printed the report but the relay did not record it. */
  recordError: string | null;
};

/** `buzz_atol::JobState`. */
export type CrmCashJobState =
  | "pending"
  | "printed"
  | "failed"
  | "needs_attention";

/** A receipt in the durable queue (`crm-receipt-queue.db`). */
export type CrmCashJob = {
  operation: "sell" | "sell_return";
  saleId: string;
  saleNumber: number;
  totalMinor: number;
  state: CrmCashJobState;
  error: string | null;
  updatedAt: number;
};

/** A person's decision about a stuck receipt. */
export type CrmCashJobAction = "retry" | "resume" | "discard";

/** One shift from `GET /crm/shifts`. */
export type CrmCashShift = {
  fnNumber: string;
  shiftNumber: number;
  openedDatetime: string | null;
  closedDatetime: string | null;
  closed: boolean;
  sellCount: number;
  sellTotalMinor: number;
  returnCount: number;
  returnTotalMinor: number;
  cashMinor: number;
  updatedAt: string;
};

type RawCashJob = {
  operation: "sell" | "sell_return";
  sale_id: string;
  sale_number: number;
  total_minor: number;
  state: CrmCashJobState;
  error: string | null;
  updated_at: number;
};

type RawCashShift = {
  fn_number: string;
  shift_number: number;
  opened_datetime: string | null;
  closed_datetime: string | null;
  closed: boolean;
  sell_count: number;
  sell_total_minor: number;
  return_count: number;
  return_total_minor: number;
  cash_minor: number;
  updated_at: string;
};

export function fromRawCashJob(raw: RawCashJob): CrmCashJob {
  return {
    operation: raw.operation,
    saleId: raw.sale_id,
    saleNumber: raw.sale_number,
    totalMinor: raw.total_minor,
    state: raw.state,
    error: raw.error ?? null,
    updatedAt: raw.updated_at,
  };
}

export function fromRawCashShift(raw: RawCashShift): CrmCashShift {
  return {
    fnNumber: raw.fn_number,
    shiftNumber: raw.shift_number,
    openedDatetime: raw.opened_datetime ?? null,
    closedDatetime: raw.closed_datetime ?? null,
    closed: raw.closed,
    sellCount: raw.sell_count,
    sellTotalMinor: raw.sell_total_minor,
    returnCount: raw.return_count,
    returnTotalMinor: raw.return_total_minor,
    cashMinor: raw.cash_minor,
    updatedAt: raw.updated_at,
  };
}

export async function getCrmCashSettings(): Promise<CrmCashSettings> {
  const raw = await invokeTauri<{ base_url: string; operator_name: string }>(
    "cash_get_settings",
  );
  return { baseUrl: raw.base_url, operatorName: raw.operator_name };
}

export async function setCrmCashSettings(
  settings: CrmCashSettings,
): Promise<CrmCashSettings> {
  const raw = await invokeTauri<{ base_url: string; operator_name: string }>(
    "cash_set_settings",
    {
      settings: {
        base_url: settings.baseUrl,
        operator_name: settings.operatorName,
      },
    },
  );
  return { baseUrl: raw.base_url, operatorName: raw.operator_name };
}

export async function getCrmCashDeviceStatus(): Promise<CrmCashDeviceStatus> {
  const raw = await invokeTauri<{
    reachable: boolean;
    shift_state: CrmCashDeviceStatus["shiftState"];
    shift_number: number | null;
    error: string | null;
  }>("cash_device_status");
  return {
    reachable: raw.reachable,
    shiftState: raw.shift_state ?? null,
    shiftNumber: raw.shift_number ?? null,
    error: raw.error ?? null,
  };
}

type RawShiftResult = {
  shift_number: number;
  document_number: number;
  record_error: string | null;
};

function fromRawShiftResult(raw: RawShiftResult): CrmCashShiftResult {
  return {
    shiftNumber: raw.shift_number,
    documentNumber: raw.document_number,
    recordError: raw.record_error ?? null,
  };
}

export async function openCrmCashShift(
  channelId: string,
): Promise<CrmCashShiftResult> {
  return fromRawShiftResult(
    await invokeTauri<RawShiftResult>("cash_open_shift", { channelId }),
  );
}

export async function closeCrmCashShift(
  channelId: string,
): Promise<CrmCashShiftResult> {
  return fromRawShiftResult(
    await invokeTauri<RawShiftResult>("cash_close_shift", { channelId }),
  );
}

export async function printCrmCashReportX(): Promise<void> {
  await invokeTauri<null>("cash_report_x");
}

/**
 * Queue and print the receipt of a sale (`sell`) or of its return after a
 * void (`sell_return`). Resolves to the queued job while it still needs
 * something, or `null` once printed and recorded.
 */
export async function fiscalizeCrmSale(input: {
  channelId: string;
  saleId: string;
  operation: "sell" | "sell_return";
}): Promise<CrmCashJob | null> {
  const raw = await invokeTauri<RawCashJob | null>("cash_fiscalize_sale", {
    channelId: input.channelId,
    saleId: input.saleId,
    operation: input.operation,
  });
  return raw ? fromRawCashJob(raw) : null;
}

export async function listCrmCashQueue(): Promise<CrmCashJob[]> {
  return (await invokeTauri<RawCashJob[]>("cash_queue")).map(fromRawCashJob);
}

export async function processCrmCashQueue(): Promise<CrmCashJob[]> {
  return (await invokeTauri<RawCashJob[]>("cash_process_queue")).map(
    fromRawCashJob,
  );
}

export async function resolveCrmCashJob(input: {
  operation: CrmCashJob["operation"];
  saleId: string;
  action: CrmCashJobAction;
}): Promise<CrmCashJob[]> {
  return (
    await invokeTauri<RawCashJob[]>("cash_resolve_job", {
      operation: input.operation,
      saleId: input.saleId,
      action: input.action,
    })
  ).map(fromRawCashJob);
}

export async function listCrmCashShifts(): Promise<CrmCashShift[]> {
  const raw = await invokeTauri<{ shifts: RawCashShift[] }>("crm_list_shifts");
  return raw.shifts.map(fromRawCashShift);
}
