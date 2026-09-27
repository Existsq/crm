import * as React from "react";
import { toast } from "sonner";

import { useUpdateCrmSettingsMutation } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { describeCrmError } from "@/features/crm/lib/crmErrors";
import { crmTimezoneOptions } from "@/features/crm/lib/crmTimezones";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type {
  CrmFiscalization,
  CrmMemberSpace,
  CrmSettings,
  CrmVatRate,
} from "@/shared/api/tauriCrm";
import { Button } from "@/shared/ui/button";
import { FieldLabel, NativeSelect } from "@/shared/ui/native-select";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Switch } from "@/shared/ui/switch";

/** VAT rates offered as a default for new products (historical 20 % excluded). */
const DEFAULT_VAT_CHOICES: CrmVatRate[] = [
  "vat22",
  "vat10",
  "vat7",
  "vat5",
  "vat0",
  "none",
];

function sameSettings(a: CrmSettings, b: CrmSettings) {
  return (
    a.timezone === b.timezone &&
    a.fiscalization === b.fiscalization &&
    a.defaultVat === b.defaultVat &&
    a.allowNegativeStock === b.allowNegativeStock
  );
}

export function CrmSettingsForm({ space }: { space: CrmMemberSpace }) {
  const canManage = space.capabilities.has("manage_space");
  const [draft, setDraft] = React.useState<CrmSettings>(space.settings);
  // The revision the draft is based on. Saving sends it as the CAS key, so a
  // change made elsewhere while the user edits is a conflict, never a silent
  // overwrite.
  const [base, setBase] = React.useState({
    settings: space.settings,
    revision: space.revision,
  });
  const update = useUpdateCrmSettingsMutation();
  const dirty = !sameSettings(draft, space.settings);
  const conflicted =
    update.error !== null && describeCrmError(update.error).kind === "conflict";

  // Adopt settings changed elsewhere unless the user has unsaved edits.
  if (base.revision !== space.revision && sameSettings(draft, base.settings)) {
    setBase({ settings: space.settings, revision: space.revision });
    setDraft(space.settings);
  }

  function reloadLatest() {
    update.reset();
    setBase({ settings: space.settings, revision: space.revision });
    setDraft(space.settings);
  }

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const saving = draft;
    try {
      const result = await update.mutateAsync({
        channelId: space.channelId,
        settings: saving,
        expectedRevision: base.revision,
      });
      setBase({ settings: saving, revision: result.eventId });
      toast.success(ru.settings.saved);
    } catch {
      // Rendered through the mutation error state.
    }
  }

  const vatChoices = DEFAULT_VAT_CHOICES.includes(draft.defaultVat)
    ? DEFAULT_VAT_CHOICES
    : [draft.defaultVat, ...DEFAULT_VAT_CHOICES];

  return (
    <form
      aria-labelledby="crm-settings-title"
      className="space-y-5 rounded-2xl border border-border/70 p-5"
      data-testid="crm-settings-form"
      onSubmit={(event) => void handleSubmit(event)}
    >
      <SectionHeader
        description={canManage ? undefined : ru.settings.readOnlyHint}
        title={<span id="crm-settings-title">{ru.settings.title}</span>}
      />

      <div className="space-y-1.5">
        <FieldLabel htmlFor="crm-settings-timezone">
          {ru.settings.timezoneLabel}
        </FieldLabel>
        <NativeSelect
          aria-describedby="crm-settings-timezone-hint"
          disabled={!canManage}
          id="crm-settings-timezone"
          onChange={(timezone) => setDraft({ ...draft, timezone })}
          value={draft.timezone}
        >
          {crmTimezoneOptions(draft.timezone).map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </NativeSelect>
        <p
          className="text-xs text-muted-foreground"
          id="crm-settings-timezone-hint"
        >
          {ru.settings.timezoneHint}
        </p>
      </div>

      <div className="space-y-1.5">
        <FieldLabel htmlFor="crm-settings-fiscalization">
          {ru.settings.fiscalizationLabel}
        </FieldLabel>
        <NativeSelect
          aria-describedby="crm-settings-fiscalization-hint"
          data-testid="crm-settings-fiscalization"
          disabled={!canManage}
          id="crm-settings-fiscalization"
          onChange={(value) =>
            setDraft({ ...draft, fiscalization: value as CrmFiscalization })
          }
          value={draft.fiscalization}
        >
          {(Object.keys(ru.settings.fiscalization) as CrmFiscalization[]).map(
            (mode) => (
              <option key={mode} value={mode}>
                {ru.settings.fiscalization[mode]}
              </option>
            ),
          )}
        </NativeSelect>
        <p
          className="text-xs text-muted-foreground"
          id="crm-settings-fiscalization-hint"
        >
          {ru.settings.fiscalizationHint[draft.fiscalization]}
        </p>
      </div>

      <div className="space-y-1.5">
        <FieldLabel htmlFor="crm-settings-vat">
          {ru.settings.vatLabel}
        </FieldLabel>
        <NativeSelect
          disabled={!canManage}
          id="crm-settings-vat"
          onChange={(value) =>
            setDraft({ ...draft, defaultVat: value as CrmVatRate })
          }
          value={draft.defaultVat}
        >
          {vatChoices.map((rate) => (
            <option key={rate} value={rate}>
              {ru.vat[rate]}
            </option>
          ))}
        </NativeSelect>
      </div>

      <div className="flex items-start justify-between gap-4">
        <div className="space-y-0.5">
          <label className="text-sm" htmlFor="crm-settings-negative">
            {ru.settings.negativeStockLabel}
          </label>
          <p
            className="text-xs text-muted-foreground"
            id="crm-settings-negative-hint"
          >
            {ru.settings.negativeStockHint}
          </p>
        </div>
        <Switch
          aria-describedby="crm-settings-negative-hint"
          checked={draft.allowNegativeStock}
          disabled={!canManage}
          id="crm-settings-negative"
          onCheckedChange={(allowNegativeStock) =>
            setDraft({ ...draft, allowNegativeStock })
          }
        />
      </div>

      <CrmErrorAlert error={update.error} testId="crm-settings-error" />

      {canManage ? (
        <div className="flex flex-wrap gap-2">
          <Button
            data-testid="crm-settings-save"
            disabled={!dirty || update.isPending || conflicted}
            type="submit"
          >
            {update.isPending ? ru.common.saving : ru.common.save}
          </Button>
          {conflicted ? (
            <Button
              data-testid="crm-settings-reload"
              onClick={reloadLatest}
              type="button"
              variant="outline"
            >
              {ru.settings.reloadLatest}
            </Button>
          ) : null}
        </div>
      ) : null}
    </form>
  );
}
