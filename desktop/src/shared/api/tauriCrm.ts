import { invokeTauri } from "@/shared/api/tauri";

// ── Domain types ──────────────────────────────────────────────────────────

export type CrmFiscalization = "required" | "optional" | "off";

export type CrmVatRate =
  | "none"
  | "vat0"
  | "vat5"
  | "vat7"
  | "vat10"
  | "vat20"
  | "vat22"
  | "vat105"
  | "vat107"
  | "vat110"
  | "vat120"
  | "vat122";

export type CrmRole = "owner" | "admin" | "member" | "guest" | "bot";

export type CrmCapability =
  | "read"
  | "manage_space"
  | "manage_warehouses"
  | "edit_catalog"
  | "edit_parties"
  | "post_documents"
  | "void_documents"
  | "record_fiscal";

export type CrmSettings = {
  currency: "RUB";
  timezone: string;
  allowNegativeStock: boolean;
  fiscalization: CrmFiscalization;
  defaultVat: CrmVatRate;
};

export type CrmWarehouse = {
  id: string;
  name: string;
  isDefault: boolean;
  archived: boolean;
  revision: string;
};

/** `GET /crm/space` as the caller may see it. */
export type CrmSpaceState =
  | { status: "none"; canCreate: boolean }
  | { status: "no-access" }
  | {
      status: "member";
      channelId: string;
      settings: CrmSettings;
      revision: string;
      role: CrmRole;
      capabilities: ReadonlySet<CrmCapability>;
      warehouses: CrmWarehouse[];
    };

export type CrmMemberSpace = Extract<CrmSpaceState, { status: "member" }>;

export type CrmWriteResult = {
  eventId: string;
  entityId: string | null;
};

export type CrmWarehouseWriteInput = {
  channelId: string;
  /**
   * Warehouse id. For a create, pick it once when the dialog opens
   * (`crypto.randomUUID()`) so a retry cannot create a duplicate.
   */
  warehouseId: string;
  name: string;
  isDefault: boolean;
  archived: boolean;
  /** Current revision; required for updates, `null` for creation. */
  expectedRevision: string | null;
};

// ── Raw types (snake_case from backend) ───────────────────────────────────

type RawCrmSettings = {
  currency: "RUB";
  timezone: string;
  allow_negative_stock: boolean;
  fiscalization: CrmFiscalization;
  default_vat: CrmVatRate;
};

type RawCrmWarehouse = {
  id: string;
  name: string;
  is_default: boolean;
  archived: boolean;
  revision: string;
};

export type RawCrmSpace =
  | { exists: false; can_create: boolean }
  | { exists: true; member: false }
  | {
      exists: true;
      member: true;
      channel_id: string;
      settings: RawCrmSettings;
      revision: string;
      my_role: CrmRole;
      capabilities: CrmCapability[];
      warehouses: RawCrmWarehouse[];
    };

type RawCrmWriteResult = {
  event_id: string;
  entity_id?: string | null;
};

// ── Mappers ───────────────────────────────────────────────────────────────

export function fromRawCrmSpace(raw: RawCrmSpace): CrmSpaceState {
  if (!raw.exists) {
    return { status: "none", canCreate: raw.can_create };
  }
  if (!raw.member) {
    return { status: "no-access" };
  }
  return {
    status: "member",
    channelId: raw.channel_id,
    settings: fromRawCrmSettings(raw.settings),
    revision: raw.revision,
    role: raw.my_role,
    capabilities: new Set(raw.capabilities),
    warehouses: raw.warehouses.map((warehouse) => ({
      id: warehouse.id,
      name: warehouse.name,
      isDefault: warehouse.is_default,
      archived: warehouse.archived,
      revision: warehouse.revision,
    })),
  };
}

function fromRawCrmSettings(raw: RawCrmSettings): CrmSettings {
  return {
    currency: raw.currency,
    timezone: raw.timezone,
    allowNegativeStock: raw.allow_negative_stock,
    fiscalization: raw.fiscalization,
    defaultVat: raw.default_vat,
  };
}

export function toRawCrmSettings(settings: CrmSettings): RawCrmSettings {
  return {
    currency: settings.currency,
    timezone: settings.timezone,
    allow_negative_stock: settings.allowNegativeStock,
    fiscalization: settings.fiscalization,
    default_vat: settings.defaultVat,
  };
}

function fromRawWrite(raw: RawCrmWriteResult): CrmWriteResult {
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}

// ── Commands ──────────────────────────────────────────────────────────────

export async function getCrmSpace(): Promise<CrmSpaceState> {
  return fromRawCrmSpace(await invokeTauri<RawCrmSpace>("crm_get_space"));
}

export async function createCrmSpace(input: {
  channelId: string;
  settings: CrmSettings;
  warehouseName: string;
}): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_create_space", {
      channelId: input.channelId,
      settings: toRawCrmSettings(input.settings),
      warehouseName: input.warehouseName,
    }),
  );
}

export async function updateCrmSpace(input: {
  channelId: string;
  settings: CrmSettings;
  expectedRevision: string;
}): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_update_space", {
      channelId: input.channelId,
      settings: toRawCrmSettings(input.settings),
      expectedRevision: input.expectedRevision,
    }),
  );
}

export async function writeCrmWarehouse(
  input: CrmWarehouseWriteInput,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_write_warehouse", {
      input: {
        channel_id: input.channelId,
        warehouse_id: input.warehouseId,
        name: input.name,
        is_default: input.isDefault,
        archived: input.archived,
        expected_revision: input.expectedRevision,
      },
    }),
  );
}
