/**
 * E2E mock for the CRM Tauri commands (`crm_*`).
 *
 * Mirrors the relay contract closely enough for UI tests: role × action
 * capabilities from ADR-0002, compare-and-swap revisions, the one-default
 * warehouse rule and `restricted:` / `conflict:` / `invalid:` error prefixes.
 * Specs seed state with `window.__BUZZ_E2E_SEED_CRM__`.
 */

import type {
  CrmCapability,
  CrmRole,
  RawCrmSpace,
} from "@/shared/api/tauriCrm";

type RawSettings = Extract<RawCrmSpace, { member: true }>["settings"];

type MockWarehouse = {
  id: string;
  name: string;
  is_default: boolean;
  archived: boolean;
  revision: string;
};

type MockCrmState = {
  /** Whether the current identity may create a space (community admin). */
  canCreate: boolean;
  /** `null` while no space exists. */
  space: {
    channelId: string;
    settings: RawSettings;
    revision: string;
    /** `null` means the identity is not a member of the space channel. */
    role: CrmRole | null;
    warehouses: MockWarehouse[];
  } | null;
};

export type MockCrmSeed = {
  canCreate?: boolean;
  space?: {
    channelId: string;
    role: CrmRole | null;
    settings?: Partial<RawSettings>;
    warehouses?: Array<{ id: string; name: string; is_default?: boolean }>;
  } | null;
};

declare global {
  interface Window {
    __BUZZ_E2E_SEED_CRM__?: (seed: MockCrmSeed) => void;
  }
}

const CAPABILITIES: Record<CrmRole, CrmCapability[]> = {
  owner: [
    "read",
    "manage_space",
    "manage_warehouses",
    "edit_catalog",
    "edit_parties",
    "post_documents",
    "void_documents",
    "record_fiscal",
  ],
  admin: [
    "read",
    "manage_space",
    "manage_warehouses",
    "edit_catalog",
    "edit_parties",
    "post_documents",
    "void_documents",
    "record_fiscal",
  ],
  member: [
    "read",
    "edit_catalog",
    "edit_parties",
    "post_documents",
    "record_fiscal",
  ],
  guest: ["read"],
  bot: ["read", "edit_catalog", "edit_parties", "post_documents"],
};

const DEFAULT_SETTINGS: RawSettings = {
  currency: "RUB",
  timezone: "Europe/Moscow",
  allow_negative_stock: false,
  fiscalization: "required",
  default_vat: "vat22",
};

let state: MockCrmState = { canCreate: true, space: null };

function revision(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export function resetMockCrm(): void {
  state = { canCreate: true, space: null };
}

export function seedMockCrm(seed: MockCrmSeed): void {
  state = {
    canCreate: seed.canCreate ?? true,
    space: seed.space
      ? {
          channelId: seed.space.channelId,
          settings: { ...DEFAULT_SETTINGS, ...seed.space.settings },
          revision: revision(),
          role: seed.space.role,
          warehouses: (seed.space.warehouses ?? []).map((w, index) => ({
            id: w.id,
            name: w.name,
            is_default: w.is_default ?? index === 0,
            archived: false,
            revision: revision(),
          })),
        }
      : null,
  };
}

function spaceView(): RawCrmSpace {
  const space = state.space;
  if (!space) return { exists: false, can_create: state.canCreate };
  if (!space.role) return { exists: true, member: false };
  return {
    exists: true,
    member: true,
    channel_id: space.channelId,
    settings: space.settings,
    revision: space.revision,
    my_role: space.role,
    capabilities: CAPABILITIES[space.role],
    warehouses: space.warehouses
      .filter((w) => !w.archived)
      .sort(
        (a, b) =>
          Number(b.is_default) - Number(a.is_default) ||
          a.name.localeCompare(b.name),
      ),
  };
}

function requireCapability(capability: CrmCapability) {
  const role = state.space?.role;
  if (!role || !CAPABILITIES[role].includes(capability)) {
    throw `relay error 403: restricted: role ${role ?? "none"} may not ${capability}`;
  }
}

type CreateSpacePayload = {
  channelId: string;
  settings: RawSettings;
  warehouseName: string;
};

type UpdateSpacePayload = {
  channelId: string;
  settings: RawSettings;
  expectedRevision: string;
};

type WriteWarehousePayload = {
  input: {
    channel_id: string;
    warehouse_id: string | null;
    name: string;
    is_default: boolean;
    archived: boolean;
    expected_revision: string | null;
  };
};

function createSpace({
  channelId,
  settings,
  warehouseName,
}: CreateSpacePayload) {
  if (!state.canCreate) {
    throw "relay error 403: restricted: only community owners and admins can create a CRM space";
  }
  if (state.space) {
    throw "relay error 409: conflict: this community already has a CRM space";
  }
  const eventId = revision();
  const warehouseId = crypto.randomUUID();
  state.space = {
    channelId,
    settings,
    revision: eventId,
    role: "owner",
    warehouses: [
      {
        id: warehouseId,
        name: warehouseName,
        is_default: true,
        archived: false,
        revision: eventId,
      },
    ],
  };
  return { event_id: eventId, entity_id: warehouseId };
}

function updateSpace({ settings, expectedRevision }: UpdateSpacePayload) {
  requireCapability("manage_space");
  const space = state.space;
  if (!space || space.revision !== expectedRevision) {
    throw "relay error 409: conflict: CRM space settings changed since they were loaded";
  }
  space.settings = settings;
  space.revision = revision();
  return { event_id: space.revision, entity_id: null };
}

/** Like the relay: the demoted default gets the moving event's revision. */
function demoteDefault(
  warehouses: MockWarehouse[],
  keepId: string,
  eventId: string,
) {
  for (const w of warehouses) {
    if (w.is_default && w.id !== keepId) {
      w.is_default = false;
      w.revision = eventId;
    }
  }
}

function writeWarehouse({ input }: WriteWarehousePayload) {
  requireCapability("manage_warehouses");
  const space = state.space;
  if (!space)
    throw "relay error 400: invalid: this channel is not the CRM space";
  const eventId = revision();
  if (input.warehouse_id === null) {
    const id = crypto.randomUUID();
    if (input.is_default) {
      demoteDefault(space.warehouses, id, eventId);
    }
    space.warehouses.push({
      id,
      name: input.name,
      is_default: input.is_default,
      archived: false,
      revision: eventId,
    });
    return { event_id: eventId, entity_id: id };
  }
  const current = space.warehouses.find((w) => w.id === input.warehouse_id);
  if (!current) throw "relay error 409: conflict: warehouse does not exist";
  if (current.revision !== input.expected_revision) {
    throw "relay error 409: conflict: warehouse changed since it was loaded";
  }
  if (current.is_default && (!input.is_default || input.archived)) {
    throw "relay error 400: invalid: make another warehouse the default first";
  }
  if (input.is_default) {
    demoteDefault(space.warehouses, current.id, eventId);
  }
  Object.assign(current, {
    name: input.name,
    is_default: input.is_default,
    archived: input.archived,
    revision: eventId,
  });
  return { event_id: eventId, entity_id: current.id };
}

/**
 * Handle a `crm_*` command, or return `undefined` for anything else so the
 * main bridge can report the command as unsupported.
 */
export function handleCrmMockCommand(
  command: string,
  payload: unknown,
): { value: unknown } | undefined {
  switch (command) {
    case "crm_get_space":
      return { value: spaceView() };
    case "crm_create_space":
      return { value: createSpace(payload as CreateSpacePayload) };
    case "crm_update_space":
      return { value: updateSpace(payload as UpdateSpacePayload) };
    case "crm_write_warehouse":
      return { value: writeWarehouse(payload as WriteWarehousePayload) };
    default:
      return undefined;
  }
}

export function installCrmMockSeedHook(): void {
  resetMockCrm();
  window.__BUZZ_E2E_SEED_CRM__ = seedMockCrm;
}
