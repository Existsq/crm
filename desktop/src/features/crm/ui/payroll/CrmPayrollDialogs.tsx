import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import {
  formatRubles,
  formatRublesInput,
  parseRubles,
} from "@/features/crm/lib/money";
import { formatPercentBp, parsePercentBp } from "@/features/crm/lib/payroll";
import { fillTemplate } from "@/features/crm/ui/analytics/CrmAnalyticsParts";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmStaffPayroll } from "@/shared/api/tauriCrmPayroll";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";

export type PayRates = {
  percentBp: number;
  perVisitMinor: number;
  shiftMinor: number;
};

function rublesOrZero(text: string): number | null {
  if (text.trim().length === 0) return 0;
  const parsed = parseRubles(text);
  return parsed.ok ? parsed.minor : null;
}

/** Edit a staff member's pay rates; mount with a fresh `key` per opening. */
export function CrmPayRatesDialog({
  error,
  onOpenChange,
  onSubmit,
  pending,
  row,
}: {
  error: unknown;
  onOpenChange: (open: boolean) => void;
  onSubmit: (rates: PayRates) => void;
  pending: boolean;
  row: CrmStaffPayroll;
}) {
  const t = ru.payroll.ratesDialog;
  const [percent, setPercent] = React.useState(formatPercentBp(row.percentBp));
  const [perVisit, setPerVisit] = React.useState(
    formatRublesInput(row.perVisitMinor),
  );
  const [shift, setShift] = React.useState(formatRublesInput(row.shiftMinor));
  const [submitted, setSubmitted] = React.useState(false);

  const percentParsed = parsePercentBp(percent);
  const perVisitMinor = rublesOrZero(perVisit);
  const shiftMinor = rublesOrZero(shift);
  const errors = submitted
    ? {
        percent: percentParsed.ok ? undefined : t.percentInvalid,
        perVisit: perVisitMinor === null ? t.amountInvalid : undefined,
        shift: shiftMinor === null ? t.amountInvalid : undefined,
      }
    : {};

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (!percentParsed.ok || perVisitMinor === null || shiftMinor === null) {
      const first = !percentParsed.ok
        ? "percent"
        : perVisitMinor === null
          ? "per-visit"
          : "shift";
      event.currentTarget
        .querySelector<HTMLElement>(`#crm-pay-rates-${first}`)
        ?.focus();
      return;
    }
    onSubmit({ percentBp: percentParsed.bp, perVisitMinor, shiftMinor });
  }

  return (
    <Dialog onOpenChange={onOpenChange} open>
      <DialogContent className="sm:max-w-md" data-testid="crm-pay-rates-dialog">
        <form className="space-y-5" noValidate onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>
              {fillTemplate(t.title, { name: row.name })}
            </DialogTitle>
            <DialogDescription>{t.description}</DialogDescription>
          </DialogHeader>
          <div className="grid gap-4">
            <CrmField
              error={errors.percent}
              hint={t.percentHint}
              id="crm-pay-rates-percent"
              label={t.percent}
            >
              <Input
                {...crmControlProps(
                  "crm-pay-rates-percent",
                  true,
                  errors.percent,
                )}
                autoFocus
                inputMode="decimal"
                onChange={(event) => setPercent(event.target.value)}
                value={percent}
              />
            </CrmField>
            <CrmField
              error={errors.perVisit}
              id="crm-pay-rates-per-visit"
              label={t.perVisit}
            >
              <Input
                {...crmControlProps(
                  "crm-pay-rates-per-visit",
                  false,
                  errors.perVisit,
                )}
                inputMode="decimal"
                onChange={(event) => setPerVisit(event.target.value)}
                value={perVisit}
              />
            </CrmField>
            <CrmField
              error={errors.shift}
              hint={t.shiftHint}
              id="crm-pay-rates-shift"
              label={t.shift}
            >
              <Input
                {...crmControlProps("crm-pay-rates-shift", true, errors.shift)}
                inputMode="decimal"
                onChange={(event) => setShift(event.target.value)}
                value={shift}
              />
            </CrmField>
          </div>
          <CrmErrorAlert error={error} testId="crm-pay-rates-error" />
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-pay-rates-submit"
              disabled={pending}
              type="submit"
            >
              {pending ? ru.common.saving : ru.common.save}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export type PayoutDraft = {
  amountMinor: number;
  date: string;
  note: string | null;
};

/** Record a payout to a staff member; mount with a fresh `key` per opening. */
export function CrmPayoutDialog({
  error,
  onOpenChange,
  onSubmit,
  pending,
  row,
  today,
}: {
  error: unknown;
  onOpenChange: (open: boolean) => void;
  onSubmit: (payout: PayoutDraft) => void;
  pending: boolean;
  row: CrmStaffPayroll;
  today: string;
}) {
  const t = ru.payroll.payoutDialog;
  const [amount, setAmount] = React.useState(
    row.balanceMinor > 0 ? formatRublesInput(row.balanceMinor) : "",
  );
  const [date, setDate] = React.useState(today);
  const [note, setNote] = React.useState("");
  const [submitted, setSubmitted] = React.useState(false);

  const parsed = parseRubles(amount);
  const amountMinor = parsed.ok && parsed.minor > 0 ? parsed.minor : null;
  const errors = submitted
    ? {
        amount: amountMinor === null ? t.amountInvalid : undefined,
        date: date ? undefined : t.dateInvalid,
      }
    : {};

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (amountMinor === null || !date) {
      event.currentTarget
        .querySelector<HTMLElement>(
          amountMinor === null ? "#crm-payout-amount" : "#crm-payout-date",
        )
        ?.focus();
      return;
    }
    onSubmit({ amountMinor, date, note: note.trim() || null });
  }

  return (
    <Dialog onOpenChange={onOpenChange} open>
      <DialogContent className="sm:max-w-md" data-testid="crm-payout-dialog">
        <form className="space-y-5" noValidate onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>
              {fillTemplate(t.title, { name: row.name })}
            </DialogTitle>
            <DialogDescription>
              {fillTemplate(t.balance, {
                amount: formatRubles(row.balanceMinor),
              })}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-4 sm:grid-cols-2">
            <CrmField
              error={errors.amount}
              id="crm-payout-amount"
              label={t.amount}
            >
              <Input
                {...crmControlProps("crm-payout-amount", false, errors.amount)}
                autoFocus
                inputMode="decimal"
                onChange={(event) => setAmount(event.target.value)}
                value={amount}
              />
            </CrmField>
            <CrmField error={errors.date} id="crm-payout-date" label={t.date}>
              <Input
                {...crmControlProps("crm-payout-date", false, errors.date)}
                onChange={(event) => setDate(event.target.value)}
                type="date"
                value={date}
              />
            </CrmField>
            <CrmField
              className="sm:col-span-2"
              id="crm-payout-note"
              label={t.note}
            >
              <Input
                {...crmControlProps("crm-payout-note", false, undefined)}
                maxLength={256}
                onChange={(event) => setNote(event.target.value)}
                placeholder={t.notePlaceholder}
                value={note}
              />
            </CrmField>
          </div>
          <CrmErrorAlert error={error} testId="crm-payout-error" />
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-payout-submit"
              disabled={pending}
              type="submit"
            >
              {pending ? ru.common.saving : t.submit}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
