import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import { describeCrmError } from "@/features/crm/lib/crmErrors";
import {
  draftFromSettings,
  minNoticeOptions,
  ONLINE_BOOKING_ABOUT_MAX,
  ONLINE_BOOKING_ADDRESS_MAX,
  ONLINE_BOOKING_HORIZON_MAX,
  ONLINE_BOOKING_HORIZON_MIN,
  ONLINE_BOOKING_TITLE_MAX,
  type OnlineBookingDraft,
  type OnlineBookingField,
  sameDraft,
  settingsFromDraft,
  stepOptions,
} from "@/features/crm/lib/onlineBooking";
import { useWriteCrmOnlineBookingMutation } from "@/features/crm/onlineBookingHooks";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import {
  type CrmChecklistItem,
  CrmIdChecklist,
} from "@/features/crm/ui/onlineBooking/CrmIdChecklist";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmOnlineBooking } from "@/shared/api/tauriCrmOnlineBooking";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { NativeSelect } from "@/shared/ui/native-select";
import { Switch } from "@/shared/ui/switch";
import { Textarea } from "@/shared/ui/textarea";

const fieldId = (field: string) => `crm-online-booking-${field}`;

type FieldError = { field: OnlineBookingField; message: string } | null;

export function CrmOnlineBookingForm({
  booking,
  canManage,
  services,
  space,
  staff,
}: {
  booking: CrmOnlineBooking;
  canManage: boolean;
  /** `null` while loading. */
  services: readonly CrmChecklistItem[] | null;
  space: CrmMemberSpace;
  /** `null` while loading. */
  staff: readonly CrmChecklistItem[] | null;
}) {
  const t = ru.onlineBooking;
  const saved = React.useMemo(
    () => draftFromSettings(booking.settings),
    [booking.settings],
  );
  const [draft, setDraft] = React.useState<OnlineBookingDraft>(saved);
  // The revision the draft is based on, sent as the CAS key: a change saved
  // elsewhere while the user edits is a conflict, never a silent overwrite.
  const [base, setBase] = React.useState({
    draft: saved,
    revision: booking.revision,
  });
  const [fieldError, setFieldError] = React.useState<FieldError>(null);
  const write = useWriteCrmOnlineBookingMutation();
  const dirty = !sameDraft(draft, base.draft) || base.revision === null;
  const conflicted =
    write.error !== null && describeCrmError(write.error).kind === "conflict";

  // The server revision last adopted. Only a newer server revision is
  // adopted, so a refetch still holding the pre-save revision cannot roll
  // the form back after a successful save.
  const [seenRevision, setSeenRevision] = React.useState(booking.revision);

  // Adopt settings changed elsewhere unless the user has unsaved edits.
  if (booking.revision !== seenRevision) {
    setSeenRevision(booking.revision);
    if (sameDraft(draft, base.draft)) {
      setBase({ draft: saved, revision: booking.revision });
      setDraft(saved);
    }
  }

  function update(patch: Partial<OnlineBookingDraft>) {
    setDraft((current) => ({ ...current, ...patch }));
    if (fieldError && Object.hasOwn(patch, fieldError.field)) {
      setFieldError(null);
    }
  }

  function reloadLatest() {
    write.reset();
    setFieldError(null);
    setSeenRevision(booking.revision);
    setBase({ draft: saved, revision: booking.revision });
    setDraft(saved);
  }

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const parsed = settingsFromDraft(draft);
    if (!parsed.ok) {
      setFieldError({ field: parsed.field, message: parsed.message });
      document.getElementById(fieldId(parsed.field))?.focus();
      return;
    }
    const saving = draft;
    try {
      const result = await write.mutateAsync({
        ...parsed.settings,
        channelId: space.channelId,
        expectedRevision: base.revision,
      });
      setBase({ draft: saving, revision: result.eventId });
      toast.success(t.saved);
    } catch {
      // Rendered through the mutation error state.
    }
  }

  const errorFor = (field: OnlineBookingField) =>
    fieldError?.field === field ? fieldError.message : undefined;
  const disabled = !canManage;

  return (
    <form
      aria-labelledby="crm-online-booking-heading"
      className="space-y-6 rounded-2xl border border-border/70 p-5"
      data-testid="crm-online-booking-form"
      noValidate
      onSubmit={(event) => void handleSubmit(event)}
    >
      <div className="flex items-start justify-between gap-4">
        <div className="space-y-0.5">
          <label className="text-sm font-medium" htmlFor={fieldId("enabled")}>
            {t.enabledLabel}
          </label>
          <p
            className="text-xs text-muted-foreground"
            id={`${fieldId("enabled")}-hint`}
          >
            {t.enabledHint}
          </p>
        </div>
        <Switch
          aria-describedby={`${fieldId("enabled")}-hint`}
          checked={draft.enabled}
          data-testid={fieldId("enabled")}
          disabled={disabled}
          id={fieldId("enabled")}
          onCheckedChange={(enabled) => update({ enabled })}
        />
      </div>

      <fieldset className="grid gap-4 sm:grid-cols-2" disabled={disabled}>
        <legend className="mb-3 text-sm font-medium">{t.pageSection}</legend>
        <CrmField
          className="sm:col-span-2"
          error={errorFor("title")}
          hint={t.titleHint}
          id={fieldId("title")}
          label={t.titleLabel}
        >
          <Input
            {...crmControlProps(fieldId("title"), true, errorFor("title"))}
            maxLength={ONLINE_BOOKING_TITLE_MAX}
            onChange={(event) => update({ title: event.target.value })}
            required
            value={draft.title}
          />
        </CrmField>
        <CrmField
          className="sm:col-span-2"
          id={fieldId("about")}
          label={t.aboutLabel}
        >
          <Textarea
            {...crmControlProps(fieldId("about"), false, undefined)}
            maxLength={ONLINE_BOOKING_ABOUT_MAX}
            onChange={(event) => update({ about: event.target.value })}
            rows={3}
            value={draft.about}
          />
        </CrmField>
        <CrmField id={fieldId("address")} label={t.addressLabel}>
          <Input
            {...crmControlProps(fieldId("address"), false, undefined)}
            maxLength={ONLINE_BOOKING_ADDRESS_MAX}
            onChange={(event) => update({ address: event.target.value })}
            value={draft.address}
          />
        </CrmField>
        <CrmField id={fieldId("phone")} label={t.phoneLabel}>
          <Input
            {...crmControlProps(fieldId("phone"), false, undefined)}
            autoComplete="tel"
            inputMode="tel"
            onChange={(event) => update({ phone: event.target.value })}
            type="tel"
            value={draft.phone}
          />
        </CrmField>
      </fieldset>

      <fieldset className="grid gap-4 sm:grid-cols-3" disabled={disabled}>
        <legend className="mb-3 text-sm font-medium">{t.rulesSection}</legend>
        <CrmField
          error={errorFor("horizonDays")}
          hint={t.horizonHint}
          id={fieldId("horizonDays")}
          label={t.horizonLabel}
        >
          <Input
            {...crmControlProps(
              fieldId("horizonDays"),
              true,
              errorFor("horizonDays"),
            )}
            inputMode="numeric"
            max={ONLINE_BOOKING_HORIZON_MAX}
            min={ONLINE_BOOKING_HORIZON_MIN}
            onChange={(event) => update({ horizonDays: event.target.value })}
            type="number"
            value={draft.horizonDays}
          />
        </CrmField>
        <CrmField
          hint={t.minNoticeHint}
          id={fieldId("minNotice")}
          label={t.minNoticeLabel}
        >
          <NativeSelect
            aria-describedby={`${fieldId("minNotice")}-hint`}
            data-testid={fieldId("minNotice")}
            disabled={disabled}
            id={fieldId("minNotice")}
            onChange={(value) => update({ minNoticeMin: Number(value) })}
            value={String(draft.minNoticeMin)}
          >
            {minNoticeOptions(draft.minNoticeMin).map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </NativeSelect>
        </CrmField>
        <CrmField hint={t.stepHint} id={fieldId("step")} label={t.stepLabel}>
          <NativeSelect
            aria-describedby={`${fieldId("step")}-hint`}
            data-testid={fieldId("step")}
            disabled={disabled}
            id={fieldId("step")}
            onChange={(value) => update({ stepMin: Number(value) })}
            value={String(draft.stepMin)}
          >
            {stepOptions(draft.stepMin).map((step) => (
              <option key={step} value={step}>
                {t.stepOption.replace("{m}", String(step))}
              </option>
            ))}
          </NativeSelect>
        </CrmField>
      </fieldset>

      <div className="grid gap-4 sm:grid-cols-2">
        <CrmIdChecklist
          disabled={disabled}
          emptyText={t.staffEmpty}
          id={fieldId("staff")}
          items={staff}
          label={t.staffLabel}
          onChange={(staffIds) => update({ staffIds })}
          selected={draft.staffIds}
        />
        <CrmIdChecklist
          disabled={disabled}
          emptyText={t.servicesEmpty}
          id={fieldId("services")}
          items={services}
          label={t.servicesLabel}
          onChange={(serviceIds) => update({ serviceIds })}
          selected={draft.serviceIds}
        />
      </div>

      <CrmErrorAlert error={write.error} testId="crm-online-booking-error" />

      {canManage ? (
        <div className="flex flex-wrap gap-2">
          <Button
            data-testid="crm-online-booking-save"
            disabled={!dirty || write.isPending || conflicted}
            type="submit"
          >
            {write.isPending ? ru.common.saving : ru.common.save}
          </Button>
          {conflicted ? (
            <Button
              data-testid="crm-online-booking-reload"
              onClick={reloadLatest}
              type="button"
              variant="outline"
            >
              {t.reloadLatest}
            </Button>
          ) : null}
        </div>
      ) : null}
    </form>
  );
}
