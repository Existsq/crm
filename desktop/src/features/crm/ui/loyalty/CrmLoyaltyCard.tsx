import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import type { LoyaltyProgramme } from "@/features/crm/lib/loyalty";
import { formatPercentBp, parsePercentBp } from "@/features/crm/lib/payroll";
import {
  useCrmLoyaltyQuery,
  useSetCrmLoyaltyMutation,
} from "@/features/crm/loyaltyHooks";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Switch } from "@/shared/ui/switch";

/** Loyalty programme settings: cashback and the share payable with bonuses. */
export function CrmLoyaltyCard({ space }: { space: CrmMemberSpace }) {
  const query = useCrmLoyaltyQuery(null);
  const t = ru.loyalty;
  return (
    <section
      aria-labelledby="crm-loyalty-title"
      className="space-y-5 rounded-2xl border border-border/70 p-5"
      data-testid="crm-loyalty-card"
    >
      <SectionHeader
        description={t.description}
        title={<span id="crm-loyalty-title">{t.title}</span>}
      />
      {query.data ? (
        <LoyaltyForm
          // Remount with the saved programme after each change.
          canManage={space.capabilities.has("manage_space")}
          channelId={space.channelId}
          key={JSON.stringify(query.data.programme)}
          programme={query.data.programme}
        />
      ) : query.isError ? (
        <CrmErrorAlert error={query.error} testId="crm-loyalty-load-error" />
      ) : (
        <p className="text-sm text-muted-foreground">{t.loading}</p>
      )}
    </section>
  );
}

function LoyaltyForm({
  canManage,
  channelId,
  programme,
}: {
  canManage: boolean;
  channelId: string;
  programme: LoyaltyProgramme;
}) {
  const t = ru.loyalty;
  const save = useSetCrmLoyaltyMutation();
  const [enabled, setEnabled] = React.useState(programme.enabled);
  const [cashback, setCashback] = React.useState(
    formatPercentBp(programme.cashbackBp),
  );
  const [maxRedeem, setMaxRedeem] = React.useState(
    formatPercentBp(programme.maxRedeemBp),
  );
  const [submitted, setSubmitted] = React.useState(false);

  const cashbackParsed = parsePercentBp(cashback);
  const maxRedeemParsed = parsePercentBp(maxRedeem);
  const errors = submitted
    ? {
        cashback: cashbackParsed.ok ? undefined : t.percentInvalid,
        maxRedeem: maxRedeemParsed.ok ? undefined : t.percentInvalid,
      }
    : {};
  const dirty =
    enabled !== programme.enabled ||
    !cashbackParsed.ok ||
    !maxRedeemParsed.ok ||
    cashbackParsed.bp !== programme.cashbackBp ||
    maxRedeemParsed.bp !== programme.maxRedeemBp;

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (!cashbackParsed.ok || !maxRedeemParsed.ok) {
      event.currentTarget
        .querySelector<HTMLElement>(
          cashbackParsed.ok
            ? "#crm-loyalty-max-redeem"
            : "#crm-loyalty-cashback",
        )
        ?.focus();
      return;
    }
    try {
      await save.mutateAsync({
        channelId,
        enabled,
        cashbackBp: cashbackParsed.bp,
        maxRedeemBp: maxRedeemParsed.bp,
      });
      toast.success(t.saved);
    } catch {
      // Rendered through the mutation error state.
    }
  }

  return (
    <form
      className="space-y-5"
      noValidate
      onSubmit={(event) => void handleSubmit(event)}
    >
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-0.5">
          <label className="text-sm" htmlFor="crm-loyalty-enabled">
            {t.enabled}
          </label>
          <p
            className="text-xs text-muted-foreground"
            id="crm-loyalty-enabled-hint"
          >
            {t.enabledHint}
          </p>
        </div>
        <Switch
          aria-describedby="crm-loyalty-enabled-hint"
          checked={enabled}
          data-testid="crm-loyalty-enabled"
          disabled={!canManage}
          id="crm-loyalty-enabled"
          onCheckedChange={setEnabled}
        />
      </div>
      <div className="grid gap-4">
        <CrmField
          error={errors.cashback}
          hint={t.cashbackHint}
          id="crm-loyalty-cashback"
          label={t.cashback}
        >
          <Input
            {...crmControlProps("crm-loyalty-cashback", true, errors.cashback)}
            disabled={!canManage}
            inputMode="decimal"
            onChange={(event) => setCashback(event.target.value)}
            value={cashback}
          />
        </CrmField>
        <CrmField
          error={errors.maxRedeem}
          hint={t.maxRedeemHint}
          id="crm-loyalty-max-redeem"
          label={t.maxRedeem}
        >
          <Input
            {...crmControlProps(
              "crm-loyalty-max-redeem",
              true,
              errors.maxRedeem,
            )}
            disabled={!canManage}
            inputMode="decimal"
            onChange={(event) => setMaxRedeem(event.target.value)}
            value={maxRedeem}
          />
        </CrmField>
      </div>
      <CrmErrorAlert error={save.error} testId="crm-loyalty-error" />
      {canManage ? (
        <Button
          data-testid="crm-loyalty-save"
          disabled={!dirty || save.isPending}
          type="submit"
        >
          {save.isPending ? ru.common.saving : ru.common.save}
        </Button>
      ) : (
        <p className="text-xs text-muted-foreground">{t.readOnly}</p>
      )}
    </form>
  );
}
