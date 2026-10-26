/**
 * Mock relay for CRM loyalty (kinds 42170, 42171, `GET /crm/loyalty`) and
 * the bonuses a sale earns and spends.
 *
 * Mirrors the relay contract: members read, `manage_space` changes the
 * programme and adjusts balances, a sale may spend bonuses only while the
 * programme is on and within the balance and the share cap, and a void
 * appends the negation of the sale's moves.
 */
import type { CrmCapability } from "@/shared/api/tauriCrm";
import type {
  CrmBonusSource,
  RawCrmLoyalty,
} from "@/shared/api/tauriCrmLoyalty";
import { mockCrmPartyById, mockCrmSaleById } from "./e2eBridgeCrmSales.ts";

export type MockCrmLoyaltySeed = {
  enabled: boolean;
  cashback_bp: number;
  max_redeem_bp: number;
};

export type MockCrmBonusSeed = { customer_id: string; balance_minor: number };

export type MockCrmLoyaltyAccess = {
  channelId: string | null;
  requireCapability: (capability: CrmCapability) => void;
};

type Move = {
  id: number;
  customer_id: string;
  source: CrmBonusSource;
  delta_minor: number;
  sale_id: string | null;
  reason: string | null;
  created_at: string;
};

const OFF: MockCrmLoyaltySeed = {
  enabled: false,
  cashback_bp: 0,
  max_redeem_bp: 0,
};

let programme: MockCrmLoyaltySeed = OFF;
let moves: Move[] = [];
let adjustments = new Set<string>();

function eventId(): string {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

function append(move: Omit<Move, "id" | "created_at">): void {
  moves.push({
    ...move,
    id: moves.length + 1,
    created_at: new Date().toISOString(),
  });
}

function balance(customerId: string): number {
  return moves
    .filter((move) => move.customer_id === customerId)
    .reduce((sum, move) => sum + move.delta_minor, 0);
}

function share(amount: number, bp: number): number {
  return Math.floor((Math.max(0, amount) * bp) / 10_000);
}

export function seedMockCrmLoyalty(
  seed: MockCrmLoyaltySeed | undefined,
  bonuses: MockCrmBonusSeed[] = [],
): void {
  programme = seed ?? OFF;
  moves = [];
  adjustments = new Set();
  for (const bonus of bonuses) {
    append({
      customer_id: bonus.customer_id,
      source: "manual",
      delta_minor: bonus.balance_minor,
      sale_id: null,
      reason: "seed",
    });
  }
}

type SaleInput = {
  customer_id: string | null;
  bonus_minor?: number | null;
  lines: Array<{
    qty_milli: number;
    price_minor: number;
    discount_minor: number;
  }>;
};

/** Refuse a sale spending bonuses the relay would refuse. */
export function checkMockSaleBonus(input: SaleInput): void {
  const bonus = input.bonus_minor ?? 0;
  if (bonus <= 0) return;
  if (!input.customer_id) {
    throw "relay error 400: invalid: bonus_minor: needs customer_id";
  }
  if (!programme.enabled) {
    throw "relay rejected event: invalid: bonus_minor: the loyalty programme is off";
  }
  const have = balance(input.customer_id);
  if (bonus > have) {
    throw `relay rejected event: invalid: bonus_minor: the customer has ${have} kopecks of bonuses`;
  }
  const total = input.lines.reduce(
    (sum, line) =>
      sum +
      Math.round((line.price_minor * line.qty_milli) / 1000) -
      line.discount_minor,
    0,
  );
  const limit = share(total + bonus, programme.max_redeem_bp);
  if (bonus > limit) {
    throw `relay rejected event: invalid: bonus_minor: at most ${limit} kopecks of this sale may be paid with bonuses`;
  }
}

/** Record the redeem and cashback moves of a posted sale. */
export function recordMockSaleBonuses(
  saleId: string,
  bonusMinor: number | null | undefined,
): void {
  const sale = mockCrmSaleById(saleId);
  if (!sale?.customer_id) return;
  const base = { customer_id: sale.customer_id, sale_id: saleId, reason: null };
  if (bonusMinor && bonusMinor > 0) {
    append({ ...base, source: "redeem", delta_minor: -bonusMinor });
  }
  const earned = programme.enabled
    ? share(sale.total_minor, programme.cashback_bp)
    : 0;
  if (earned > 0) append({ ...base, source: "cashback", delta_minor: earned });
}

/** Append the negation of a voided sale's moves. */
export function revertMockSaleBonuses(saleId: string): void {
  for (const move of moves.filter(
    (m) =>
      m.sale_id === saleId &&
      (m.source === "cashback" || m.source === "redeem"),
  )) {
    append({
      customer_id: move.customer_id,
      source: "void",
      delta_minor: -move.delta_minor,
      sale_id: saleId,
      reason: null,
    });
  }
}

function requireMember(access: MockCrmLoyaltyAccess): void {
  if (!access.channelId) {
    throw "relay error 403: restricted: not a member of the CRM space";
  }
}

function loyalty(
  customerId: string | null,
  access: MockCrmLoyaltyAccess,
): RawCrmLoyalty {
  requireMember(access);
  return {
    settings: { ...programme },
    customer: customerId
      ? {
          balance_minor: balance(customerId),
          moves: moves
            .filter((move) => move.customer_id === customerId)
            .reverse()
            .map(({ customer_id: _, ...move }) => move),
        }
      : undefined,
  };
}

type ProgrammeInput = MockCrmLoyaltySeed & { channel_id: string };

type AdjustInput = {
  channel_id: string;
  adjustment_id: string;
  customer_id: string;
  delta_minor: number;
  reason: string;
};

function write(): { event_id: string; entity_id: null } {
  return { event_id: eventId(), entity_id: null };
}

export function handleCrmLoyaltyMockCommand(
  command: string,
  payload: unknown,
  access: MockCrmLoyaltyAccess,
): { value: unknown } | undefined {
  switch (command) {
    case "crm_get_loyalty":
      return {
        value: loyalty(
          (payload as { customerId?: string | null }).customerId ?? null,
          access,
        ),
      };
    case "crm_set_loyalty": {
      requireMember(access);
      access.requireCapability("manage_space");
      const input = (payload as { input: ProgrammeInput }).input;
      programme = {
        enabled: input.enabled,
        cashback_bp: input.cashback_bp,
        max_redeem_bp: input.max_redeem_bp,
      };
      return { value: write() };
    }
    case "crm_adjust_bonuses": {
      requireMember(access);
      access.requireCapability("manage_space");
      const input = (payload as { input: AdjustInput }).input;
      const party = mockCrmPartyById(input.customer_id);
      if (!party) {
        throw "relay rejected event: invalid: customer_id: not a party of this CRM space";
      }
      if (adjustments.has(input.adjustment_id)) {
        return { value: write() };
      }
      const have = balance(input.customer_id);
      if (input.delta_minor < 0 && have + input.delta_minor < 0) {
        throw `relay rejected event: invalid: delta_minor: the customer has ${have} kopecks of bonuses`;
      }
      adjustments.add(input.adjustment_id);
      append({
        customer_id: input.customer_id,
        source: "manual",
        delta_minor: input.delta_minor,
        sale_id: null,
        reason: input.reason,
      });
      return { value: write() };
    }
    default:
      return undefined;
  }
}
