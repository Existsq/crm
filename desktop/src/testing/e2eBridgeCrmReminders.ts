/**
 * E2E mock for the CRM reminder commands (`crm_get_reminders`,
 * `crm_list_reminder_deliveries`, `crm_get_client_telegram`,
 * `crm_write_reminder_settings`, `crm_write_client_telegram`).
 *
 * Mirrors the relay contract for kinds 42150/42151: compare-and-swap
 * revisions on the settings, known placeholders only, distinct rule keys and
 * links to customers of the space. The delivery log is seeded by specs.
 */

import type { CrmCapability } from "@/shared/api/tauriCrm";
import type {
  RawCrmClientTelegram,
  RawCrmReminderDelivery,
  RawCrmReminderRule,
  RawCrmReminderSettings,
} from "@/shared/api/tauriCrmReminders";

import { mockCrmPartyById } from "./e2eBridgeCrmSales.ts";

const PLACEHOLDERS = [
  "client",
  "date",
  "time",
  "services",
  "staff",
  "salon",
  "address",
  "total",
];

export type MockCrmRemindersSeed = {
  transport?: "sandbox" | "telegram";
  botUsername?: string | null;
  settings?: Omit<RawCrmReminderSettings, "v"> | null;
  deliveries?: RawCrmReminderDelivery[];
  telegram?: Array<{
    customer_id: string;
    chat_id: number | null;
    username?: string | null;
    opted_out?: boolean;
    linked_via?: "staff" | "bot";
  }>;
};

export type MockCrmReminderAccess = {
  channelId: string | null;
  requireCapability: (capability: CrmCapability) => void;
};

type WriteSettingsPayload = {
  input: {
    channel_id: string;
    expected_revision: string | null;
    enabled: boolean;
    rules: RawCrmReminderRule[];
    salon_name: string | null;
    address: string | null;
  };
};

type WriteTelegramPayload = {
  input: {
    channel_id: string;
    customer_id: string;
    chat_id: number | null;
    opted_out: boolean;
  };
};

type MockLink = Omit<RawCrmClientTelegram, "invite_url" | "start_code">;

let transport: "sandbox" | "telegram" = "sandbox";
let botUsername: string | null = null;
let settings: RawCrmReminderSettings | null = null;
let revision: string | null = null;
let updatedAt: string | null = null;
let deliveries: RawCrmReminderDelivery[] = [];
let links = new Map<string, MockLink>();

function eventId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function seedMockCrmReminders(seed: MockCrmRemindersSeed = {}): void {
  transport = seed.transport ?? "sandbox";
  botUsername =
    seed.botUsername ?? (transport === "telegram" ? "salon_bot" : null);
  settings = seed.settings ? { v: 1, ...seed.settings } : null;
  revision = settings ? eventId() : null;
  updatedAt = settings ? new Date().toISOString() : null;
  deliveries = [...(seed.deliveries ?? [])];
  links = new Map(
    (seed.telegram ?? []).map((link) => [
      link.customer_id,
      {
        customer_id: link.customer_id,
        linked: link.chat_id !== null,
        chat_id: link.chat_id,
        username: link.username ?? null,
        opted_out: link.opted_out ?? false,
        linked_via: link.linked_via ?? "bot",
        linked_at: link.chat_id !== null ? new Date().toISOString() : null,
      },
    ]),
  );
}

function invalid(message: string): never {
  throw `relay rejected event: invalid: ${message}`;
}

function writeSettings({ input }: WriteSettingsPayload) {
  if (input.expected_revision === null && revision !== null) {
    throw "relay error 409: conflict: reminder settings already exist; send expected-revision to update them";
  }
  if (
    input.expected_revision !== null &&
    input.expected_revision !== revision
  ) {
    throw "relay error 409: conflict: reminder settings changed since they were loaded";
  }
  if (input.rules.length > 4) invalid("rules: must list at most 4 rules");
  const keys = new Set<string>();
  const rules = input.rules.map((rule) => {
    const key =
      rule.trigger === "booked" ? "booked" : `before:${rule.offset_min}`;
    if (keys.has(key)) invalid(`rules: ${key} is listed twice`);
    keys.add(key);
    for (const match of rule.template.matchAll(/\{([A-Za-z0-9_]+)\}/g)) {
      if (!PLACEHOLDERS.includes(match[1] ?? "")) {
        invalid(`template: {${match[1]}} is not a placeholder`);
      }
    }
    return { ...rule, template: rule.template.trim() };
  });
  settings = {
    v: 1,
    enabled: input.enabled,
    rules,
    salon_name: input.salon_name?.trim() || null,
    address: input.address?.trim() || null,
  };
  revision = eventId();
  updatedAt = new Date().toISOString();
  return { event_id: revision, entity_id: null };
}

function linkView(customerId: string): RawCrmClientTelegram {
  const link = links.get(customerId);
  const startCode = `c${customerId.replaceAll("-", "").slice(0, 20)}`;
  return {
    customer_id: customerId,
    linked: link?.linked ?? false,
    chat_id: link?.chat_id ?? null,
    username: link?.username ?? null,
    opted_out: link?.opted_out ?? false,
    linked_via: link?.linked_via ?? null,
    linked_at: link?.linked_at ?? null,
    invite_url:
      transport === "telegram" && botUsername
        ? `https://t.me/${botUsername}?start=${startCode}`
        : null,
    start_code: startCode,
  };
}

function writeTelegram({ input }: WriteTelegramPayload) {
  if (!mockCrmPartyById(input.customer_id)) {
    invalid("customer_id is not a customer of this space");
  }
  if (input.chat_id === 0) invalid("chat_id: is not a Telegram chat id");
  const current = links.get(input.customer_id);
  const sameChat = current?.chat_id === input.chat_id;
  links.set(input.customer_id, {
    customer_id: input.customer_id,
    linked: input.chat_id !== null,
    chat_id: input.chat_id,
    username: sameChat ? (current?.username ?? null) : null,
    opted_out: input.opted_out,
    linked_via: sameChat ? (current?.linked_via ?? "staff") : "staff",
    linked_at:
      input.chat_id === null
        ? null
        : sameChat
          ? (current?.linked_at ?? null)
          : new Date().toISOString(),
  });
  return { event_id: eventId(), entity_id: input.customer_id };
}

export function handleCrmReminderMockCommand(
  command: string,
  payload: unknown,
  access: MockCrmReminderAccess,
): { value: unknown } | undefined {
  const requireMember = () => {
    if (!access.channelId) {
      throw "relay error 403: not a member of the CRM space";
    }
  };
  switch (command) {
    case "crm_get_reminders":
      requireMember();
      return {
        value: {
          settings,
          enabled: settings?.enabled ?? false,
          revision,
          updated_at: updatedAt,
          transport: { mode: transport, bot_username: botUsername },
          placeholders: PLACEHOLDERS,
        },
      };
    case "crm_list_reminder_deliveries": {
      requireMember();
      const filter = payload as {
        since: string | null;
        customerId: string | null;
        appointmentId: string | null;
      };
      const since = filter.since ? Date.parse(filter.since) : null;
      return {
        value: {
          deliveries: deliveries
            .filter(
              (d) =>
                (since === null || Date.parse(d.created_at) >= since) &&
                (!filter.customerId || d.customer_id === filter.customerId) &&
                (!filter.appointmentId ||
                  d.appointment_id === filter.appointmentId),
            )
            .sort((a, b) => b.created_at.localeCompare(a.created_at)),
        },
      };
    }
    case "crm_get_client_telegram":
      requireMember();
      return {
        value: linkView((payload as { partyId: string }).partyId),
      };
    case "crm_write_reminder_settings":
      access.requireCapability("manage_space");
      return { value: writeSettings(payload as WriteSettingsPayload) };
    case "crm_write_client_telegram":
      access.requireCapability("edit_parties");
      return { value: writeTelegram(payload as WriteTelegramPayload) };
    default:
      return undefined;
  }
}
