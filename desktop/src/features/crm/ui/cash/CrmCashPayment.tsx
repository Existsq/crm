import { ru } from "@/features/crm/i18n/ru";
import {
  CASH_PAYMENT_MODES,
  type CashPaymentDraft,
  type EvaluatedCashPayment,
} from "@/features/crm/lib/cashPayment";
import { formatRubles } from "@/features/crm/lib/money";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { cn } from "@/shared/lib/cn";
import { Input } from "@/shared/ui/input";

/** Payment mode, cash handed over and the change to give back. */
export function CrmCashPayment({
  draft,
  evaluated,
  onChange,
  showErrors,
  totalMinor,
}: {
  draft: CashPaymentDraft;
  evaluated: EvaluatedCashPayment;
  onChange: (patch: Partial<CashPaymentDraft>) => void;
  showErrors: boolean;
  totalMinor: number;
}) {
  const pay = ru.cash.pay;
  const errors = !evaluated.ok ? evaluated.errors : {};
  // Checked on submit, so a half-typed amount is not flagged; the change
  // reads "—" until the amount covers the cash part.
  const tenderedError = showErrors ? errors.tendered : undefined;
  const cashPartError = showErrors ? errors.cashPart : undefined;
  return (
    <div className="space-y-4" data-testid="crm-cash-payment">
      <fieldset>
        <legend className="mb-1.5 text-xs font-medium text-muted-foreground">
          {pay.modeLabel}
        </legend>
        <div className="grid grid-cols-3 gap-1 rounded-lg bg-muted/50 p-1">
          {CASH_PAYMENT_MODES.map((mode) => (
            <label
              className={cn(
                "cursor-pointer rounded-md px-2 py-1.5 text-center text-sm transition-colors focus-within:ring-2 focus-within:ring-ring",
                draft.mode === mode
                  ? "bg-background font-medium shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
              key={mode}
            >
              <input
                checked={draft.mode === mode}
                className="sr-only"
                data-testid={`crm-cash-mode-${mode}`}
                name="crm-cash-mode"
                onChange={() => onChange({ mode })}
                type="radio"
                value={mode}
              />
              {pay.modes[mode]}
            </label>
          ))}
        </div>
      </fieldset>

      {draft.mode === "mixed" ? (
        <CrmField
          error={cashPartError}
          id="crm-cash-cash-part"
          label={pay.cashPart}
        >
          <Input
            {...crmControlProps("crm-cash-cash-part", false, cashPartError)}
            className="text-right tabular-nums"
            inputMode="decimal"
            onChange={(event) => onChange({ cashPart: event.target.value })}
            value={draft.cashPart}
          />
        </CrmField>
      ) : null}

      {draft.mode !== "card" ? (
        <CrmField
          error={tenderedError}
          id="crm-cash-tendered"
          label={pay.tendered}
        >
          <Input
            {...crmControlProps("crm-cash-tendered", false, tenderedError)}
            className="text-right tabular-nums"
            inputMode="decimal"
            onChange={(event) => onChange({ tendered: event.target.value })}
            placeholder={pay.tenderedPlaceholder}
            value={draft.tendered}
          />
        </CrmField>
      ) : null}

      <dl
        className="grid grid-cols-2 gap-y-1 text-sm"
        data-testid="crm-cash-summary"
      >
        <dt className="font-medium">{pay.total}</dt>
        <dd
          className="text-right text-xl font-semibold tabular-nums"
          data-testid="crm-cash-total"
        >
          {formatRubles(totalMinor)}
        </dd>
        {draft.mode === "mixed" && evaluated.ok ? (
          <>
            <dt className="text-muted-foreground">{pay.cardPart}</dt>
            <dd
              className="text-right tabular-nums"
              data-testid="crm-cash-card-part"
            >
              {formatRubles(evaluated.cardMinor)}
            </dd>
          </>
        ) : null}
        {draft.mode !== "card" ? (
          <>
            <dt className="text-muted-foreground">{pay.change}</dt>
            <dd
              aria-live="polite"
              className="text-right text-base font-semibold tabular-nums"
              data-testid="crm-cash-change"
            >
              {evaluated.ok ? formatRubles(evaluated.changeMinor) : "—"}
            </dd>
          </>
        ) : null}
      </dl>
    </div>
  );
}
