import { invokeTauri } from "@/shared/api/tauri";
import type { CrmWriteResult } from "@/shared/api/tauriCrm";

// ── Domain types ──────────────────────────────────────────────────────────

/** When a rule fires: right after booking, or `offsetMin` before the visit. */
export type CrmReminderTrigger = "booked" | "before";

export type CrmReminderRule = {
  trigger: CrmReminderTrigger;
  /** Minutes before the visit; `null` for the booking confirmation. */
  offsetMin: number | null;
  template: string;
};

/** Editable reminder settings (kind 42150 payload). */
export type CrmReminderSettings = {
  enabled: boolean;
  rules: CrmReminderRule[];
  salonName: string | null;
  address: string | null;
};

export type CrmReminderTransport = {
  mode: "sandbox" | "telegram";
  botUsername: string | null;
};

export type CrmReminders = {
  /** `null` until the rules are saved for the first time. */
  settings: CrmReminderSettings | null;
  revision: string | null;
  updatedAt: string | null;
  transport: CrmReminderTransport;
  placeholders: string[];
};

export type CrmDeliveryStatus =
  | "pending"
  | "sent"
  | "failed"
  | "skipped"
  | "cancelled";

export type CrmDeliveryReason =
  | "no_telegram"
  | "opted_out"
  | "visit_changed"
  | "visit_passed"
  | "disabled"
  | "blocked"
  | "error";

export type CrmReminderDelivery = {
  id: string;
  appointmentId: string;
  /** `booked` or `before:<minutes>`. */
  ruleKey: string;
  customerId: string | null;
  clientName: string;
  chatId: number | null;
  text: string;
  status: CrmDeliveryStatus;
  reason: CrmDeliveryReason | null;
  transport: "telegram" | "sandbox" | null;
  attempts: number;
  lastError: string | null;
  dueAt: string;
  visitAt: string;
  createdAt: string;
  sentAt: string | null;
};

/** Where a customer gets reminders. */
export type CrmClientTelegram = {
  customerId: string;
  linked: boolean;
  chatId: number | null;
  username: string | null;
  optedOut: boolean;
  linkedVia: "staff" | "bot" | null;
  linkedAt: string | null;
  /** Bot deep link for the client; `null` in the sandbox. */
  inviteUrl: string | null;
  startCode: string;
};

export type CrmReminderSettingsWriteInput = CrmReminderSettings & {
  channelId: string;
  /** Current revision; `null` for the first save. */
  expectedRevision: string | null;
};

export type CrmClientTelegramWriteInput = {
  channelId: string;
  customerId: string;
  /** `null` unlinks the customer. */
  chatId: number | null;
  optedOut: boolean;
};

// ── Raw types (snake_case from backend) ───────────────────────────────────

export type RawCrmReminderRule = {
  trigger: CrmReminderTrigger;
  offset_min?: number | null;
  template: string;
};

export type RawCrmReminderSettings = {
  v: number;
  enabled: boolean;
  rules: RawCrmReminderRule[];
  salon_name?: string | null;
  address?: string | null;
};

export type RawCrmReminders = {
  settings: RawCrmReminderSettings | null;
  enabled: boolean;
  revision: string | null;
  updated_at: string | null;
  transport: { mode: "sandbox" | "telegram"; bot_username: string | null };
  placeholders: string[];
};

export type RawCrmReminderDelivery = {
  id: string;
  appointment_id: string;
  rule_key: string;
  customer_id: string | null;
  client_name: string;
  chat_id: number | null;
  text: string;
  status: CrmDeliveryStatus;
  reason: CrmDeliveryReason | null;
  transport: "telegram" | "sandbox" | null;
  attempts: number;
  last_error: string | null;
  due_at: string;
  visit_at: string;
  created_at: string;
  sent_at: string | null;
};

export type RawCrmClientTelegram = {
  customer_id: string;
  linked: boolean;
  chat_id: number | null;
  username: string | null;
  opted_out: boolean;
  linked_via: "staff" | "bot" | null;
  linked_at: string | null;
  invite_url: string | null;
  start_code: string;
};

type RawCrmWriteResult = {
  event_id: string;
  entity_id?: string | null;
};

// ── Mappers ───────────────────────────────────────────────────────────────

export function fromRawCrmReminders(raw: RawCrmReminders): CrmReminders {
  return {
    settings: raw.settings
      ? {
          enabled: raw.settings.enabled,
          rules: raw.settings.rules.map((rule) => ({
            trigger: rule.trigger,
            offsetMin: rule.offset_min ?? null,
            template: rule.template,
          })),
          salonName: raw.settings.salon_name ?? null,
          address: raw.settings.address ?? null,
        }
      : null,
    revision: raw.revision ?? null,
    updatedAt: raw.updated_at ?? null,
    transport: {
      mode: raw.transport.mode,
      botUsername: raw.transport.bot_username ?? null,
    },
    placeholders: raw.placeholders,
  };
}

export function fromRawCrmReminderDelivery(
  raw: RawCrmReminderDelivery,
): CrmReminderDelivery {
  return {
    id: raw.id,
    appointmentId: raw.appointment_id,
    ruleKey: raw.rule_key,
    customerId: raw.customer_id ?? null,
    clientName: raw.client_name,
    chatId: raw.chat_id ?? null,
    text: raw.text,
    status: raw.status,
    reason: raw.reason ?? null,
    transport: raw.transport ?? null,
    attempts: raw.attempts,
    lastError: raw.last_error ?? null,
    dueAt: raw.due_at,
    visitAt: raw.visit_at,
    createdAt: raw.created_at,
    sentAt: raw.sent_at ?? null,
  };
}

export function fromRawCrmClientTelegram(
  raw: RawCrmClientTelegram,
): CrmClientTelegram {
  return {
    customerId: raw.customer_id,
    linked: raw.linked,
    chatId: raw.chat_id ?? null,
    username: raw.username ?? null,
    optedOut: raw.opted_out,
    linkedVia: raw.linked_via ?? null,
    linkedAt: raw.linked_at ?? null,
    inviteUrl: raw.invite_url ?? null,
    startCode: raw.start_code,
  };
}

export function toRawCrmReminderSettingsInput(
  input: CrmReminderSettingsWriteInput,
) {
  return {
    channel_id: input.channelId,
    expected_revision: input.expectedRevision,
    enabled: input.enabled,
    rules: input.rules.map((rule) =>
      rule.trigger === "booked"
        ? { trigger: rule.trigger, template: rule.template }
        : {
            trigger: rule.trigger,
            offset_min: rule.offsetMin,
            template: rule.template,
          },
    ),
    salon_name: input.salonName,
    address: input.address,
  };
}

// ── Commands ──────────────────────────────────────────────────────────────

export async function getCrmReminders(): Promise<CrmReminders> {
  return fromRawCrmReminders(
    await invokeTauri<RawCrmReminders>("crm_get_reminders"),
  );
}

/** The delivery log planned since `since` (RFC 3339), newest first. */
export async function listCrmReminderDeliveries(filter: {
  since?: string | null;
  appointmentId?: string | null;
  customerId?: string | null;
  limit?: number | null;
}): Promise<CrmReminderDelivery[]> {
  const raw = await invokeTauri<{ deliveries: RawCrmReminderDelivery[] }>(
    "crm_list_reminder_deliveries",
    {
      since: filter.since ?? null,
      appointmentId: filter.appointmentId ?? null,
      customerId: filter.customerId ?? null,
      limit: filter.limit ?? null,
    },
  );
  return raw.deliveries.map(fromRawCrmReminderDelivery);
}

export async function getCrmClientTelegram(
  partyId: string,
): Promise<CrmClientTelegram> {
  return fromRawCrmClientTelegram(
    await invokeTauri<RawCrmClientTelegram>("crm_get_client_telegram", {
      partyId,
    }),
  );
}

export async function writeCrmReminderSettings(
  input: CrmReminderSettingsWriteInput,
): Promise<CrmWriteResult> {
  const raw = await invokeTauri<RawCrmWriteResult>(
    "crm_write_reminder_settings",
    { input: toRawCrmReminderSettingsInput(input) },
  );
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}

export async function writeCrmClientTelegram(
  input: CrmClientTelegramWriteInput,
): Promise<CrmWriteResult> {
  const raw = await invokeTauri<RawCrmWriteResult>(
    "crm_write_client_telegram",
    {
      input: {
        channel_id: input.channelId,
        customer_id: input.customerId,
        chat_id: input.chatId,
        opted_out: input.optedOut,
      },
    },
  );
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}
