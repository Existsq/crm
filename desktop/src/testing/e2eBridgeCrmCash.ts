/**
 * E2E mock for the cash-desk commands (`cash_*`, `crm_list_shifts`).
 *
 * Stands in for `buzz-atol` and the DTO10 web server: one shift at a time,
 * sequential fiscal document numbers, and a receipt queue in which a
 * receipt printed while the shift is closed fails without printing, as the
 * device refuses it (error 69 on real ATOL registers).
 */

import type { CrmCapability } from "@/shared/api/tauriCrm";

import { mockCrmSaleById, recordMockCrmReceipt } from "./e2eBridgeCrmSales.ts";

export type MockCrmCashSeed = {
  /** Whether the device is reachable (default true). */
  reachable?: boolean;
  /** Whether a shift is open (default false). */
  shiftOpen?: boolean;
};

type MockAccess = {
  channelId: string | null;
  requireCapability: (capability: CrmCapability) => void;
};

type Operation = "sell" | "sell_return";

type MockJob = {
  operation: Operation;
  sale_id: string;
  sale_number: number;
  total_minor: number;
  state: "pending" | "printed" | "failed" | "needs_attention";
  error: string | null;
  updated_at: number;
};

type MockShift = {
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

const FN_NUMBER = "9999078900000001";
const SHIFT_CLOSED_ERROR = "ККМ отказала (69): Смена не открыта";

let reachable = true;
let shiftOpen = false;
let shiftNumber = 0;
let documentNumber = 0;
let receiptNumber = 0;
let settings = { base_url: "http://127.0.0.1:16732", operator_name: "Кассир" };
let queue: MockJob[] = [];
let shifts: MockShift[] = [];

export function seedMockCrmCash(seed: MockCrmCashSeed = {}): void {
  reachable = seed.reachable ?? true;
  shiftOpen = false;
  shiftNumber = 0;
  documentNumber = 0;
  receiptNumber = 0;
  settings = { base_url: "http://127.0.0.1:16732", operator_name: "Кассир" };
  queue = [];
  shifts = [];
  if (seed.shiftOpen) openShift();
}

function requireDevice() {
  if (!reachable) throw "error sending request: connection refused";
}

function now(): string {
  return new Date().toISOString().slice(0, 19);
}

function openShift() {
  shiftOpen = true;
  shiftNumber += 1;
  receiptNumber = 0;
  documentNumber += 1;
  shifts.unshift({
    fn_number: FN_NUMBER,
    shift_number: shiftNumber,
    opened_datetime: now(),
    closed_datetime: null,
    closed: false,
    sell_count: 0,
    sell_total_minor: 0,
    return_count: 0,
    return_total_minor: 0,
    cash_minor: 0,
    updated_at: new Date().toISOString(),
  });
}

function shiftOperation(action: "open" | "close", access: MockAccess) {
  access.requireCapability("record_fiscal");
  requireDevice();
  if (action === "open") {
    if (shiftOpen) throw "ККМ отказала (68): Смена уже открыта";
    openShift();
  } else {
    if (!shiftOpen) throw SHIFT_CLOSED_ERROR;
    shiftOpen = false;
    documentNumber += 1;
    const current = shifts[0];
    if (current) {
      current.closed = true;
      current.closed_datetime = now();
    }
  }
  return {
    shift_number: shiftNumber,
    document_number: documentNumber,
    record_error: null,
  };
}

/** Print one job if the shift allows it; the job leaves the queue when done. */
function drive(job: MockJob) {
  if (!reachable) {
    job.state = "pending";
    job.error = "error sending request: connection refused";
    return;
  }
  if (!shiftOpen) {
    job.state = "failed";
    job.error = SHIFT_CLOSED_ERROR;
    return;
  }
  documentNumber += 1;
  receiptNumber += 1;
  recordMockCrmReceipt(job.sale_id, job.operation, {
    document_number: documentNumber,
    document_sign: String(1_000_000_000 + documentNumber),
    receipt_number: receiptNumber,
    shift_number: shiftNumber,
    fn_number: FN_NUMBER,
    registration_number: "0000000001012345",
    document_datetime: now(),
    total_minor: job.total_minor,
  });
  const shift = shifts[0];
  const cash =
    mockCrmSaleById(job.sale_id)?.payments.find((p) => p.method === "cash")
      ?.amount_minor ?? 0;
  if (shift) {
    if (job.operation === "sell") {
      shift.sell_count += 1;
      shift.sell_total_minor += job.total_minor;
      shift.cash_minor += cash;
    } else {
      shift.return_count += 1;
      shift.return_total_minor += job.total_minor;
      shift.cash_minor -= cash;
    }
  }
  queue = queue.filter((row) => row !== job);
}

function fiscalize(
  payload: { saleId: string; operation: Operation },
  access: MockAccess,
) {
  access.requireCapability("record_fiscal");
  const sale = mockCrmSaleById(payload.saleId);
  if (!sale) throw "relay error 404: sale not found";
  if (payload.operation === "sell_return" && !sale.void) {
    throw "relay error 400: invalid: a return receipt needs a voided sale";
  }
  let job = queue.find(
    (row) =>
      row.sale_id === payload.saleId && row.operation === payload.operation,
  );
  if (!job) {
    job = {
      operation: payload.operation,
      sale_id: sale.id,
      sale_number: sale.number,
      total_minor: sale.total_minor,
      state: "pending",
      error: null,
      updated_at: Date.now(),
    };
    queue.push(job);
  }
  drive(job);
  return queue.includes(job) ? job : null;
}

/** Handle a cash-desk command, or `undefined` otherwise. */
export function handleCrmCashMockCommand(
  command: string,
  payload: unknown,
  access: MockAccess,
): { value: unknown } | undefined {
  // biome-ignore lint/suspicious/noExplicitAny: mock payloads are untyped IPC.
  const p = payload as any;
  switch (command) {
    case "cash_get_settings":
      return { value: settings };
    case "cash_set_settings":
      settings = { ...p.settings };
      return { value: settings };
    case "cash_device_status":
      return {
        value: reachable
          ? {
              reachable: true,
              shift_state: shiftOpen ? "opened" : "closed",
              shift_number: shiftNumber,
              error: null,
            }
          : {
              reachable: false,
              shift_state: null,
              shift_number: null,
              error: "error sending request: connection refused",
            },
      };
    case "cash_open_shift":
      return { value: shiftOperation("open", access) };
    case "cash_close_shift":
      return { value: shiftOperation("close", access) };
    case "cash_report_x":
      requireDevice();
      if (!shiftOpen) throw SHIFT_CLOSED_ERROR;
      return { value: null };
    case "cash_fiscalize_sale":
      return { value: fiscalize(p, access) };
    case "cash_queue":
      return { value: queue };
    case "cash_process_queue":
      for (const job of [...queue]) {
        if (job.state === "pending" || job.state === "printed") drive(job);
      }
      return { value: queue };
    case "cash_resolve_job": {
      const job = queue.find(
        (row) => row.sale_id === p.saleId && row.operation === p.operation,
      );
      if (job && p.action === "discard") {
        queue = queue.filter((row) => row !== job);
      } else if (job) {
        drive(job);
      }
      return { value: queue };
    }
    case "crm_list_shifts":
      if (!access.channelId) throw "relay error 403: not a member";
      return { value: { shifts } };
    default:
      return undefined;
  }
}
