import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import { parseSignedRubles } from "@/features/crm/lib/loyalty";
import { formatRubles } from "@/features/crm/lib/money";
import {
  useAdjustCrmBonusesMutation,
  useCrmLoyaltyQuery,
} from "@/features/crm/loyaltyHooks";
import { fillTemplate } from "@/features/crm/ui/analytics/CrmAnalyticsParts";
import {
  CrmField,
  crmControlProps,
  formatCrmDateTime,
} from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
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

/** Moves listed under the balance. */
const RECENT_MOVES = 8;

function signed(minor: number) {
  return minor > 0 ? `+${formatRubles(minor)}` : formatRubles(minor);
}

/** A customer's bonus balance and history, with manual adjustment. */
export function CrmPartyBonuses({
  canAdjust,
  channelId,
  customerId,
}: {
  canAdjust: boolean;
  channelId: string;
  customerId: string;
}) {
  const t = ru.loyalty.bonuses;
  const query = useCrmLoyaltyQuery(customerId);
  const [adjustKey, setAdjustKey] = React.useState<string | null>(null);

  if (query.isPending) return null;
  if (query.isError) {
    return (
      <CrmErrorAlert error={query.error} testId="crm-party-bonuses-error" />
    );
  }
  const { programme, customer } = query.data;
  const balance = customer?.balanceMinor ?? 0;
  const moves = customer?.moves ?? [];
  // Nothing to show for a space that never ran a programme.
  if (!programme.enabled && moves.length === 0) return null;

  return (
    <section
      aria-labelledby="crm-party-bonuses-title"
      className="space-y-3 rounded-xl border border-border/70 p-3"
      data-testid="crm-party-bonuses"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-medium" id="crm-party-bonuses-title">
          {t.title}
        </h3>
        <span
          className="text-sm font-medium tabular-nums"
          data-testid="crm-party-bonus-balance"
        >
          {fillTemplate(t.balance, { amount: formatRubles(balance) })}
        </span>
      </div>
      {programme.enabled ? null : (
        <p className="text-xs text-muted-foreground">{t.off}</p>
      )}
      {moves.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t.empty}</p>
      ) : (
        <ul
          aria-label={t.historyLabel}
          className="divide-y divide-border/60 text-sm"
        >
          {moves.slice(0, RECENT_MOVES).map((move) => (
            <li
              className="flex items-center justify-between gap-3 py-1.5"
              key={move.id}
            >
              <span className="min-w-0">
                {t.sources[move.source]}
                {move.reason ? ` · ${move.reason}` : ""}
                <span className="block text-xs text-muted-foreground">
                  {formatCrmDateTime(move.createdAt)}
                </span>
              </span>
              <span
                className={
                  move.deltaMinor < 0
                    ? "tabular-nums text-muted-foreground"
                    : "tabular-nums"
                }
              >
                {signed(move.deltaMinor)}
              </span>
            </li>
          ))}
        </ul>
      )}
      {canAdjust ? (
        <Button
          data-testid="crm-party-bonus-adjust"
          onClick={() => setAdjustKey(crypto.randomUUID())}
          size="sm"
          type="button"
          variant="outline"
        >
          {t.adjustTitle}
        </Button>
      ) : null}
      {adjustKey ? (
        <BonusAdjustDialog
          adjustmentId={adjustKey}
          balanceMinor={balance}
          channelId={channelId}
          customerId={customerId}
          key={adjustKey}
          onClose={() => setAdjustKey(null)}
        />
      ) : null}
    </section>
  );
}

function BonusAdjustDialog({
  adjustmentId,
  balanceMinor,
  channelId,
  customerId,
  onClose,
}: {
  adjustmentId: string;
  balanceMinor: number;
  channelId: string;
  customerId: string;
  onClose: () => void;
}) {
  const t = ru.loyalty.bonuses;
  const adjust = useAdjustCrmBonusesMutation();
  const [amount, setAmount] = React.useState("");
  const [reason, setReason] = React.useState("");
  const [submitted, setSubmitted] = React.useState(false);

  const deltaMinor = parseSignedRubles(amount);
  const errors = submitted
    ? {
        amount: deltaMinor === null ? t.amountInvalid : undefined,
        reason: reason.trim() ? undefined : t.reasonInvalid,
      }
    : {};

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    // The dialog renders in a portal, but React still bubbles the submit
    // to the party form around it.
    event.stopPropagation();
    setSubmitted(true);
    if (deltaMinor === null || !reason.trim()) {
      event.currentTarget
        .querySelector<HTMLElement>(
          deltaMinor === null
            ? "#crm-bonus-adjust-amount"
            : "#crm-bonus-adjust-reason",
        )
        ?.focus();
      return;
    }
    try {
      await adjust.mutateAsync({
        channelId,
        adjustmentId,
        customerId,
        deltaMinor,
        reason: reason.trim(),
      });
      toast.success(t.saved);
      onClose();
    } catch {
      // Rendered through the mutation error state.
    }
  }

  return (
    <Dialog onOpenChange={(open) => (open ? undefined : onClose())} open>
      <DialogContent
        className="sm:max-w-md"
        data-testid="crm-bonus-adjust-dialog"
      >
        <form
          className="space-y-5"
          noValidate
          onSubmit={(event) => void handleSubmit(event)}
        >
          <DialogHeader>
            <DialogTitle>{t.adjustTitle}</DialogTitle>
            <DialogDescription>
              {fillTemplate(t.balance, { amount: formatRubles(balanceMinor) })}
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-4">
            <CrmField
              error={errors.amount}
              hint={t.amountHint}
              id="crm-bonus-adjust-amount"
              label={t.amount}
            >
              <Input
                {...crmControlProps(
                  "crm-bonus-adjust-amount",
                  true,
                  errors.amount,
                )}
                autoFocus
                inputMode="decimal"
                onChange={(event) => setAmount(event.target.value)}
                value={amount}
              />
            </CrmField>
            <CrmField
              error={errors.reason}
              id="crm-bonus-adjust-reason"
              label={t.reason}
            >
              <Input
                {...crmControlProps(
                  "crm-bonus-adjust-reason",
                  false,
                  errors.reason,
                )}
                maxLength={256}
                onChange={(event) => setReason(event.target.value)}
                placeholder={t.reasonPlaceholder}
                value={reason}
              />
            </CrmField>
          </div>
          <CrmErrorAlert error={adjust.error} testId="crm-bonus-adjust-error" />
          <DialogFooter>
            <Button onClick={onClose} type="button" variant="ghost">
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-bonus-adjust-submit"
              disabled={adjust.isPending}
              type="submit"
            >
              {adjust.isPending ? ru.common.saving : t.submit}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
