import * as React from "react";
import { toast } from "sonner";

import {
  useChannelsQuery,
  useCreateChannelMutation,
} from "@/features/channels/hooks";
import { useCreateCrmSpaceMutation } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { crmTimezoneOptions } from "@/features/crm/lib/crmTimezones";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmFiscalization, CrmSettings } from "@/shared/api/tauriCrm";
import type { Channel } from "@/shared/api/types";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { FieldLabel, NativeSelect } from "@/shared/ui/native-select";
import { SectionHeader } from "@/shared/ui/PageHeader";

const DEFAULT_SETTINGS: CrmSettings = {
  currency: "RUB",
  timezone: "Europe/Moscow",
  allowNegativeStock: false,
  fiscalization: "required",
  defaultVat: "vat22",
};

/** Channels the relay will accept as a CRM space (it re-checks the role). */
export function eligibleCrmChannels(channels: readonly Channel[]): Channel[] {
  return channels.filter(
    (channel) =>
      channel.isMember &&
      channel.channelType === "stream" &&
      channel.visibility === "private" &&
      channel.archivedAt === null &&
      channel.ttlSeconds === null,
  );
}

type ChannelChoice = "new" | "existing";

export function CrmCreateSpaceForm() {
  const channelsQuery = useChannelsQuery();
  const createChannel = useCreateChannelMutation();
  const createSpace = useCreateCrmSpaceMutation();

  const eligible = React.useMemo(
    () => eligibleCrmChannels(channelsQuery.data ?? []),
    [channelsQuery.data],
  );
  const [choice, setChoice] = React.useState<ChannelChoice>("new");
  const [existingId, setExistingId] = React.useState("");
  const [warehouseName, setWarehouseName] = React.useState<string>(
    ru.create.warehouseDefault,
  );
  const [timezone, setTimezone] = React.useState(DEFAULT_SETTINGS.timezone);
  const [fiscalization, setFiscalization] = React.useState<CrmFiscalization>(
    DEFAULT_SETTINGS.fiscalization,
  );
  const [validation, setValidation] = React.useState<string | null>(null);
  // A channel created by a previous attempt is reused on retry, so a failed
  // space binding never leaves a trail of orphan "crm" channels.
  const [createdChannelId, setCreatedChannelId] = React.useState<string | null>(
    null,
  );

  const effectiveChoice: ChannelChoice = eligible.length === 0 ? "new" : choice;
  const pending = createChannel.isPending || createSpace.isPending;
  const error = createChannel.error ?? createSpace.error;

  async function resolveChannelId(): Promise<string> {
    if (effectiveChoice === "existing") return existingId;
    if (createdChannelId) return createdChannelId;
    const channel = await createChannel.mutateAsync({
      name: ru.create.newChannelName,
      channelType: "stream",
      visibility: "private",
      description: ru.create.newChannelDescription,
    });
    setCreatedChannelId(channel.id);
    return channel.id;
  }

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setValidation(null);
    if (warehouseName.trim().length === 0) {
      setValidation(ru.errors.nameRequired);
      return;
    }
    if (effectiveChoice === "existing" && !existingId) {
      setValidation(ru.errors.channelRequired);
      return;
    }
    createSpace.reset();
    try {
      const channelId = await resolveChannelId();
      await createSpace.mutateAsync({
        channelId,
        settings: { ...DEFAULT_SETTINGS, timezone, fiscalization },
        warehouseName: warehouseName.trim(),
      });
      toast.success(ru.create.created);
    } catch {
      // Rendered below through the mutation error state.
    }
  }

  return (
    <form
      aria-labelledby="crm-create-title"
      className="max-w-2xl space-y-6 rounded-2xl border border-border/70 p-6"
      data-testid="crm-create-space"
      onSubmit={(event) => void handleSubmit(event)}
    >
      <SectionHeader
        description={ru.create.description}
        title={<span id="crm-create-title">{ru.create.title}</span>}
      />

      <fieldset className="space-y-2">
        <legend className="text-xs font-medium text-muted-foreground">
          {ru.create.channelLegend}
        </legend>
        <label className="flex items-center gap-2 text-sm">
          <input
            checked={effectiveChoice === "new"}
            name="crm-channel-choice"
            onChange={() => setChoice("new")}
            type="radio"
          />
          {ru.create.newChannel}
        </label>
        <label className="flex items-center gap-2 text-sm">
          <input
            checked={effectiveChoice === "existing"}
            disabled={eligible.length === 0}
            name="crm-channel-choice"
            onChange={() => setChoice("existing")}
            type="radio"
          />
          {ru.create.existingChannel}
        </label>
        {eligible.length === 0 ? (
          <p className="text-xs text-muted-foreground">
            {ru.create.noEligibleChannels}
          </p>
        ) : null}
        {effectiveChoice === "existing" ? (
          <div className="space-y-1.5 pt-1">
            <FieldLabel htmlFor="crm-existing-channel">
              {ru.create.channelLabel}
            </FieldLabel>
            <NativeSelect
              data-testid="crm-existing-channel"
              id="crm-existing-channel"
              onChange={setExistingId}
              value={existingId}
            >
              <option value="">—</option>
              {eligible.map((channel) => (
                <option key={channel.id} value={channel.id}>
                  {channel.name}
                </option>
              ))}
            </NativeSelect>
          </div>
        ) : null}
      </fieldset>

      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-1.5">
          <FieldLabel htmlFor="crm-warehouse-name">
            {ru.create.warehouseLabel}
          </FieldLabel>
          <Input
            data-testid="crm-warehouse-name"
            id="crm-warehouse-name"
            maxLength={128}
            onChange={(event) => setWarehouseName(event.target.value)}
            value={warehouseName}
          />
        </div>
        <div className="space-y-1.5">
          <FieldLabel htmlFor="crm-create-timezone">
            {ru.settings.timezoneLabel}
          </FieldLabel>
          <NativeSelect
            id="crm-create-timezone"
            onChange={setTimezone}
            value={timezone}
          >
            {crmTimezoneOptions(timezone).map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </NativeSelect>
        </div>
        <div className="space-y-1.5 sm:col-span-2">
          <FieldLabel htmlFor="crm-create-fiscalization">
            {ru.settings.fiscalizationLabel}
          </FieldLabel>
          <NativeSelect
            aria-describedby="crm-create-fiscalization-hint"
            id="crm-create-fiscalization"
            onChange={(value) => setFiscalization(value as CrmFiscalization)}
            value={fiscalization}
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
            id="crm-create-fiscalization-hint"
          >
            {ru.settings.fiscalizationHint[fiscalization]}
          </p>
        </div>
      </div>

      {validation ? (
        <p className="text-xs text-destructive" role="alert">
          {validation}
        </p>
      ) : null}
      <CrmErrorAlert error={error} testId="crm-create-error" />

      <Button data-testid="crm-create-submit" disabled={pending} type="submit">
        {pending ? ru.create.submitting : ru.create.submit}
      </Button>
    </form>
  );
}
