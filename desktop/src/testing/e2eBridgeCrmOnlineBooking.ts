/**
 * E2E mock for the CRM online booking commands (`crm_get_online_booking`,
 * `crm_write_online_booking`).
 *
 * Mirrors the relay contract for kind 42135: one settings row per space,
 * owner/admin writes (`manage_space`), create without a revision and
 * compare-and-swap updates, `invalid:` field bounds.
 */

import type { CrmCapability } from "@/shared/api/tauriCrm";
import type { RawCrmOnlineBookingSettings } from "@/shared/api/tauriCrmOnlineBooking";

/** Where the mock relay serves the public page. */
const MOCK_PUBLIC_PATH = "/book";
const MOCK_RELAY_HTTP_BASE = "http://localhost:3000";

export type MockCrmOnlineBookingSeed = {
  settings: RawCrmOnlineBookingSettings | null;
};

export type MockCrmOnlineBookingAccess = {
  channelId: string | null;
  requireCapability: (capability: CrmCapability) => void;
};

type WriteOnlineBookingPayload = {
  input: Omit<RawCrmOnlineBookingSettings, "v"> & {
    channel_id: string;
    expected_revision: string | null;
  };
};

let settings: RawCrmOnlineBookingSettings | null = null;
let revision: string | null = null;

function eventId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function seedMockCrmOnlineBooking(seed?: MockCrmOnlineBookingSeed) {
  settings = seed?.settings ? { v: 1, ...seed.settings } : null;
  revision = settings ? eventId() : null;
}

function validate(input: WriteOnlineBookingPayload["input"]) {
  const title = input.title.trim();
  if (title.length === 0 || title.length > 128) {
    throw "relay rejected event: invalid: title must be 1–128 characters";
  }
  if (input.horizon_days < 1 || input.horizon_days > 180) {
    throw "relay rejected event: invalid: horizon_days must be 1–180";
  }
  if (input.min_notice_min < 0 || input.min_notice_min > 10_080) {
    throw "relay rejected event: invalid: min_notice_min must be 0–10080";
  }
  if (input.step_min < 5 || input.step_min > 60 || input.step_min % 5 !== 0) {
    throw "relay rejected event: invalid: step_min must be 5–60 and a multiple of 5";
  }
}

function write(
  { input }: WriteOnlineBookingPayload,
  access: MockCrmOnlineBookingAccess,
) {
  access.requireCapability("manage_space");
  if (input.channel_id !== access.channelId) {
    throw "relay error 400: invalid: this channel is not the CRM space";
  }
  if (input.expected_revision === null && settings !== null) {
    throw "relay error 409: conflict: online booking settings already exist; send expected-revision to update them";
  }
  if (
    input.expected_revision !== null &&
    input.expected_revision !== revision
  ) {
    throw "relay error 409: conflict: online booking settings changed since they were loaded";
  }
  validate(input);
  const blank = (value: string | null | undefined) =>
    value && value.trim().length > 0 ? value.trim() : null;
  settings = {
    v: 1,
    enabled: input.enabled,
    title: input.title.trim(),
    about: blank(input.about),
    address: blank(input.address),
    phone: blank(input.phone),
    horizon_days: input.horizon_days,
    min_notice_min: input.min_notice_min,
    step_min: input.step_min,
    staff_ids: [...new Set(input.staff_ids ?? [])],
    service_ids: [...new Set(input.service_ids ?? [])],
  };
  revision = eventId();
  return { event_id: revision, entity_id: null };
}

/** Handle an online booking command, or `undefined` for anything else. */
export function handleCrmOnlineBookingMockCommand(
  command: string,
  payload: unknown,
  access: MockCrmOnlineBookingAccess,
): { value: unknown } | undefined {
  switch (command) {
    case "crm_get_online_booking":
      if (!access.channelId) {
        throw "relay error 403: restricted: not a member of the CRM space";
      }
      return {
        value: {
          settings,
          revision,
          public_path: MOCK_PUBLIC_PATH,
          public_url: `${MOCK_RELAY_HTTP_BASE}${MOCK_PUBLIC_PATH}`,
        },
      };
    case "crm_write_online_booking":
      return {
        value: write(payload as WriteOnlineBookingPayload, access),
      };
    default:
      return undefined;
  }
}
