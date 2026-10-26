import type { LoyaltyProgramme } from "@/features/crm/lib/loyalty";
import { invokeTauri } from "@/shared/api/tauri";
import type { CrmWriteResult } from "@/shared/api/tauriCrm";

export type CrmBonusSource = "cashback" | "redeem" | "manual" | "void";

export type CrmBonusMove = {
  id: number;
  source: CrmBonusSource;
  deltaMinor: number;
  saleId: string | null;
  reason: string | null;
  createdAt: string;
};

/** `GET /crm/loyalty`: the programme and, when asked, one customer's bonuses. */
export type CrmLoyalty = {
  programme: LoyaltyProgramme;
  customer: { balanceMinor: number; moves: CrmBonusMove[] } | null;
};

export type CrmLoyaltyInput = LoyaltyProgramme & { channelId: string };

export type CrmBonusAdjustInput = {
  channelId: string;
  /** Pick once per dialog so a retried submit cannot apply twice. */
  adjustmentId: string;
  customerId: string;
  deltaMinor: number;
  reason: string;
};

export type RawCrmLoyalty = {
  settings: { enabled: boolean; cashback_bp: number; max_redeem_bp: number };
  customer?: {
    balance_minor: number;
    moves: Array<{
      id: number;
      source: CrmBonusSource;
      delta_minor: number;
      sale_id: string | null;
      reason: string | null;
      created_at: string;
    }>;
  };
};

type RawCrmWriteResult = { event_id: string; entity_id?: string | null };

export function fromRawCrmLoyalty(raw: RawCrmLoyalty): CrmLoyalty {
  return {
    programme: {
      enabled: raw.settings.enabled,
      cashbackBp: raw.settings.cashback_bp,
      maxRedeemBp: raw.settings.max_redeem_bp,
    },
    customer: raw.customer
      ? {
          balanceMinor: raw.customer.balance_minor,
          moves: raw.customer.moves.map((move) => ({
            id: move.id,
            source: move.source,
            deltaMinor: move.delta_minor,
            saleId: move.sale_id ?? null,
            reason: move.reason ?? null,
            createdAt: move.created_at,
          })),
        }
      : null,
  };
}

export async function getCrmLoyalty(
  customerId: string | null,
): Promise<CrmLoyalty> {
  return fromRawCrmLoyalty(
    await invokeTauri<RawCrmLoyalty>("crm_get_loyalty", { customerId }),
  );
}

function fromRawWrite(raw: RawCrmWriteResult): CrmWriteResult {
  return { eventId: raw.event_id, entityId: raw.entity_id ?? null };
}

export async function setCrmLoyalty(
  input: CrmLoyaltyInput,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_set_loyalty", {
      input: {
        channel_id: input.channelId,
        enabled: input.enabled,
        cashback_bp: input.cashbackBp,
        max_redeem_bp: input.maxRedeemBp,
      },
    }),
  );
}

export async function adjustCrmBonuses(
  input: CrmBonusAdjustInput,
): Promise<CrmWriteResult> {
  return fromRawWrite(
    await invokeTauri<RawCrmWriteResult>("crm_adjust_bonuses", {
      input: {
        channel_id: input.channelId,
        adjustment_id: input.adjustmentId,
        customer_id: input.customerId,
        delta_minor: input.deltaMinor,
        reason: input.reason,
      },
    }),
  );
}
