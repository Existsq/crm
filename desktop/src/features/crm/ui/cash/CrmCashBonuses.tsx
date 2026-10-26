import { ru } from "@/features/crm/i18n/ru";
import {
  cashbackMinor,
  type LoyaltyProgramme,
  maxRedeemMinor,
} from "@/features/crm/lib/loyalty";
import {
  formatRubles,
  formatRublesInput,
  parseRubles,
} from "@/features/crm/lib/money";
import { fillTemplate } from "@/features/crm/ui/analytics/CrmAnalyticsParts";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

export type CashBonusState =
  | { ok: true; bonusMinor: number }
  | { ok: false; maxMinor: number };

/**
 * Bonuses typed at the cash desk: empty means none; anything else must be a
 * ruble amount within `maxMinor`.
 */
export function evaluateCashBonus(
  text: string,
  maxMinor: number,
): CashBonusState {
  if (text.trim() === "") return { ok: true, bonusMinor: 0 };
  const parsed = parseRubles(text);
  return parsed.ok && parsed.minor <= maxMinor
    ? { ok: true, bonusMinor: parsed.minor }
    : { ok: false, maxMinor };
}

/** Pay part of the receipt with the customer's bonuses. */
export function CrmCashBonuses({
  balanceMinor,
  baseMinor,
  onChange,
  programme,
  showErrors,
  state,
  value,
}: {
  balanceMinor: number;
  /** Receipt total before bonuses. */
  baseMinor: number;
  onChange: (value: string) => void;
  programme: LoyaltyProgramme;
  showErrors: boolean;
  state: CashBonusState;
  value: string;
}) {
  const t = ru.loyalty.cash;
  const max = maxRedeemMinor(programme, baseMinor, balanceMinor);
  const paid = baseMinor - (state.ok ? state.bonusMinor : 0);
  const earns = cashbackMinor(programme, paid);
  const error =
    showErrors && !state.ok
      ? fillTemplate(t.invalid, { amount: formatRubles(max) })
      : undefined;
  return (
    <div className="space-y-2" data-testid="crm-cash-bonuses">
      {max > 0 || value.trim() !== "" ? (
        <CrmField
          error={error}
          hint={fillTemplate(t.available, {
            amount: formatRubles(max),
            balance: formatRubles(balanceMinor),
          })}
          id="crm-cash-bonus"
          label={t.label}
        >
          <div className="flex gap-2">
            <Input
              {...crmControlProps("crm-cash-bonus", true, error)}
              inputMode="decimal"
              onChange={(event) => onChange(event.target.value)}
              placeholder="0"
              value={value}
            />
            <Button
              data-testid="crm-cash-bonus-max"
              disabled={max === 0}
              onClick={() => onChange(formatRublesInput(max))}
              type="button"
              variant="outline"
            >
              {t.useMax}
            </Button>
          </div>
        </CrmField>
      ) : null}
      {earns > 0 ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="crm-cash-bonus-earns"
        >
          {fillTemplate(t.earns, { amount: formatRubles(earns) })}
        </p>
      ) : null}
    </div>
  );
}
