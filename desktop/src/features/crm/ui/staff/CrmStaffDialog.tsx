import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { formatDuration } from "@/features/crm/lib/catalog";
import { describeCrmError } from "@/features/crm/lib/crmErrors";
import {
  emptyStaffDraft,
  sameStaffDraft,
  STAFF_DRAFT_FIELDS,
  type StaffDraft,
  staffDraftFrom,
  validateStaffDraft,
} from "@/features/crm/lib/staff";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmProduct } from "@/shared/api/tauriCrmProducts";
import type { CrmStaff, CrmStaffFields } from "@/shared/api/tauriCrmStaff";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { Checkbox } from "@/shared/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";

const fieldId = (field: string) => `crm-staff-${field}`;

/** Checkbox list of the services a staff member performs. */
function ServicePicker({
  onChange,
  selected,
  services,
}: {
  onChange: (serviceIds: string[]) => void;
  selected: readonly string[];
  /** `null` while the services are loading. */
  services: readonly CrmProduct[] | null;
}) {
  const fields = ru.staff.fields;
  const hintId = `${fieldId("services")}-hint`;
  return (
    <fieldset
      aria-describedby={hintId}
      className="space-y-2 sm:col-span-2"
      data-testid={fieldId("services")}
    >
      <legend className="text-sm font-medium">{fields.services}</legend>
      <p className="text-xs text-muted-foreground" id={hintId}>
        {fields.servicesHint}
      </p>
      {services === null ? (
        <p className="text-sm text-muted-foreground" role="status">
          {fields.servicesLoading}
        </p>
      ) : services.length === 0 ? (
        <p className="text-sm text-muted-foreground">{fields.noServicesYet}</p>
      ) : (
        <div className="grid max-h-56 gap-1 overflow-y-auto rounded-xl border border-border/70 p-2 sm:grid-cols-2">
          {services.map((service) => {
            const id = `${fieldId("service")}-${service.id}`;
            const checked = selected.includes(service.id);
            return (
              <div
                className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-muted/50"
                key={service.id}
              >
                <Checkbox
                  checked={checked}
                  data-testid={id}
                  id={id}
                  onCheckedChange={(value) =>
                    onChange(
                      value === true
                        ? [...selected, service.id]
                        : selected.filter((other) => other !== service.id),
                    )
                  }
                />
                <label className="min-w-0 flex-1 text-sm" htmlFor={id}>
                  <span className="block truncate">{service.name}</span>
                  {service.durationMin !== null ? (
                    <span className="text-xs text-muted-foreground">
                      {formatDuration(service.durationMin)}
                    </span>
                  ) : null}
                </label>
              </div>
            );
          })}
        </div>
      )}
    </fieldset>
  );
}

/**
 * Create or edit a staff member; the caller owns the mutation. Mount it with
 * a fresh `key` per opening so the draft starts from `staff`.
 */
export function CrmStaffDialog({
  error,
  onCloseAutoFocus,
  onDismissError,
  onOpenChange,
  onSubmit,
  open,
  pending,
  services,
  staff,
}: {
  error: unknown;
  onCloseAutoFocus: (event: Event) => void;
  onDismissError: () => void;
  onOpenChange: (open: boolean) => void;
  onSubmit: (fields: CrmStaffFields, expectedRevision: string | null) => void;
  open: boolean;
  pending: boolean;
  services: readonly CrmProduct[] | null;
  /** Live staff member being edited, `null` to create one. */
  staff: CrmStaff | null;
}) {
  const initialDraft = () =>
    staff ? staffDraftFrom(staff) : emptyStaffDraft();
  const [baseline, setBaseline] = React.useState<StaffDraft>(initialDraft);
  const [draft, setDraft] = React.useState<StaffDraft>(initialDraft);
  const [baseRevision, setBaseRevision] = React.useState(
    staff?.revision ?? null,
  );
  const [submitted, setSubmitted] = React.useState(false);

  // Adopt a staff member changed elsewhere unless the user has unsaved
  // edits; with edits the stale base revision makes saving a conflict.
  if (
    staff &&
    staff.revision !== baseRevision &&
    sameStaffDraft(draft, baseline)
  ) {
    adopt(staff);
  }
  const conflicted =
    staff !== null &&
    staff.revision !== baseRevision &&
    error !== null &&
    describeCrmError(error).kind === "conflict";

  function adopt(latest: CrmStaff) {
    const next = staffDraftFrom(latest);
    setBaseRevision(latest.revision);
    setBaseline(next);
    setDraft(next);
  }

  const set = (patch: Partial<StaffDraft>) =>
    setDraft((current) => ({ ...current, ...patch }));
  const validation = validateStaffDraft(draft);
  const errors = submitted && !validation.ok ? validation.errors : {};
  const fields = ru.staff.fields;

  // Archived services stay assigned but are not offered.
  const offered = React.useMemo(() => {
    if (services === null) return null;
    return services.filter(
      (service) => !service.archived || draft.serviceIds.includes(service.id),
    );
  }, [services, draft.serviceIds]);

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (!validation.ok) {
      const first = STAFF_DRAFT_FIELDS.find(
        (field) => validation.errors[field],
      );
      if (first) {
        event.currentTarget
          .querySelector<HTMLElement>(`#${fieldId(first)}`)
          ?.focus();
      }
      return;
    }
    onSubmit(validation.fields, baseRevision);
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="flex max-h-[calc(100dvh-2rem)] flex-col sm:max-w-2xl"
        data-testid="crm-staff-dialog"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <form
          className="flex min-h-0 flex-1 flex-col gap-5"
          noValidate
          onSubmit={handleSubmit}
        >
          <DialogHeader>
            <DialogTitle className="flex flex-wrap items-center gap-2">
              {staff ? ru.staff.editTitle : ru.staff.createTitle}
              {staff?.archived ? (
                <Badge variant="secondary">{ru.staff.archivedBadge}</Badge>
              ) : null}
            </DialogTitle>
          </DialogHeader>
          <div className="-mx-1 grid min-h-0 flex-1 gap-4 overflow-y-auto px-1 pb-1 sm:grid-cols-2">
            <CrmField
              className="sm:col-span-2"
              error={errors.name}
              hint={fields.nameHint}
              id={fieldId("name")}
              label={fields.name}
            >
              <Input
                {...crmControlProps(fieldId("name"), true, errors.name)}
                autoFocus
                maxLength={128}
                onChange={(event) => set({ name: event.target.value })}
                value={draft.name}
              />
            </CrmField>
            <CrmField
              error={errors.position}
              id={fieldId("position")}
              label={fields.position}
            >
              <Input
                {...crmControlProps(
                  fieldId("position"),
                  false,
                  errors.position,
                )}
                maxLength={64}
                onChange={(event) => set({ position: event.target.value })}
                placeholder={fields.positionPlaceholder}
                value={draft.position}
              />
            </CrmField>
            <CrmField
              error={errors.phone}
              id={fieldId("phone")}
              label={fields.phone}
            >
              <Input
                {...crmControlProps(fieldId("phone"), false, errors.phone)}
                inputMode="tel"
                maxLength={32}
                onChange={(event) => set({ phone: event.target.value })}
                type="tel"
                value={draft.phone}
              />
            </CrmField>
            <ServicePicker
              onChange={(serviceIds) => set({ serviceIds })}
              selected={draft.serviceIds}
              services={offered}
            />
            {staff ? (
              <div className="flex items-start gap-2 sm:col-span-2">
                <Checkbox
                  aria-describedby={`${fieldId("archived")}-hint`}
                  checked={draft.archived}
                  data-testid={fieldId("archived")}
                  id={fieldId("archived")}
                  onCheckedChange={(checked) =>
                    set({ archived: checked === true })
                  }
                />
                <div className="space-y-0.5">
                  <label className="text-sm" htmlFor={fieldId("archived")}>
                    {fields.archived}
                  </label>
                  <p
                    className="text-xs text-muted-foreground"
                    id={`${fieldId("archived")}-hint`}
                  >
                    {fields.archivedHint}
                  </p>
                </div>
              </div>
            ) : null}
          </div>
          <CrmErrorAlert error={error} testId="crm-staff-dialog-error" />
          <DialogFooter>
            {conflicted && staff ? (
              <Button
                data-testid="crm-staff-dialog-reload"
                onClick={() => {
                  onDismissError();
                  adopt(staff);
                }}
                type="button"
                variant="outline"
              >
                {ru.staff.reloadLatest}
              </Button>
            ) : null}
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-staff-dialog-submit"
              disabled={pending || conflicted}
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
