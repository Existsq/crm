import { X } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { useCrmPartiesQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatDuration } from "@/features/crm/lib/catalog";
import {
  type AppointmentDraft,
  draftFromAppointment,
  MAX_APPOINTMENT_SERVICES,
  servicesTotal,
  staffServices,
  startChoices,
  statusWrite,
  validateAppointmentDraft,
} from "@/features/crm/lib/journal";
import { formatRubles } from "@/features/crm/lib/money";
import { clockMinutes } from "@/features/crm/lib/journal";
import { formatClock } from "@/features/crm/lib/schedule";
import {
  useCrmSlotsQuery,
  useWriteCrmAppointmentMutation,
} from "@/features/crm/journalHooks";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { useDebouncedValue } from "@/features/crm/ui/products/useDebouncedValue";
import { CrmSearchPicker } from "@/features/crm/ui/sales/CrmSearchPicker";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import {
  CRM_APPOINTMENT_STATUSES,
  type CrmAppointment,
  type CrmAppointmentStatus,
} from "@/shared/api/tauriCrmAppointments";
import type { CrmProduct } from "@/shared/api/tauriCrmProducts";
import type { CrmStaff } from "@/shared/api/tauriCrmStaff";
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
import { NativeSelect } from "@/shared/ui/native-select";
import { Textarea } from "@/shared/ui/textarea";

const SEARCH_DEBOUNCE_MS = 200;
const PICKER_LIMIT = 8;
const fieldId = (field: string) => `crm-appointment-${field}`;

/** What the dialog opens on: a new visit at a grid cell, or a loaded one. */
export type AppointmentDialogTarget =
  | {
      mode: "create";
      appointmentId: string;
      staffId: string;
      date: string;
      start: number | null;
      outsideHours: boolean;
    }
  | { mode: "edit"; appointment: CrmAppointment };

function initialDraft(target: AppointmentDialogTarget): AppointmentDraft {
  if (target.mode === "edit") return draftFromAppointment(target.appointment);
  return {
    staffId: target.staffId,
    date: target.date,
    start: target.start,
    services: [],
    customer: null,
    clientName: "",
    clientPhone: "",
    status: "booked",
    note: "",
    outsideHours: target.outsideHours,
  };
}

function ClientFields({
  draft,
  invalid,
  setDraft,
}: {
  draft: AppointmentDraft;
  invalid: boolean;
  setDraft: React.Dispatch<React.SetStateAction<AppointmentDraft>>;
}) {
  const d = ru.journal.dialog;
  const [search, setSearch] = React.useState("");
  const q = useDebouncedValue(search.trim(), SEARCH_DEBOUNCE_MS);
  const customers = useCrmPartiesQuery({
    q,
    role: "customers",
    archived: false,
  });
  // Only results for what is typed now, never a stale page.
  const matches =
    q.length > 0 && q === search.trim() && !customers.isPlaceholderData
      ? (customers.data?.pages[0]?.parties ?? []).slice(0, PICKER_LIMIT)
      : [];

  if (draft.customer) {
    const customer = draft.customer;
    return (
      <fieldset className="space-y-1.5">
        <legend className="text-xs font-medium text-muted-foreground">
          {d.client}
        </legend>
        <div
          className="flex items-center justify-between gap-2 rounded-md border border-border/70 px-3 py-2"
          data-testid={fieldId("customer")}
        >
          <span className="min-w-0 truncate text-sm">
            {customer.name}
            {customer.phone ? (
              <span className="ml-2 text-muted-foreground">
                {customer.phone}
              </span>
            ) : null}
          </span>
          <Button
            aria-label={d.clearCustomer.replace("{name}", customer.name)}
            className="h-7 w-7"
            data-testid={fieldId("customer-clear")}
            onClick={() =>
              setDraft((current) => ({ ...current, customer: null }))
            }
            size="icon"
            type="button"
            variant="ghost"
          >
            <X aria-hidden="true" className="h-4 w-4" />
          </Button>
        </div>
      </fieldset>
    );
  }

  return (
    <fieldset className="space-y-3">
      <legend className="sr-only">{d.client}</legend>
      <CrmSearchPicker
        emptyText={d.noCustomers}
        getKey={(party) => party.id}
        id={fieldId("customer-search")}
        items={matches}
        label={d.customerSearch}
        loading={customers.isFetching || q !== search.trim()}
        onPick={(party) => {
          setDraft((current) => ({
            ...current,
            customer: { id: party.id, name: party.name, phone: party.phone },
          }));
          setSearch("");
        }}
        onQueryChange={setSearch}
        placeholder={d.customerSearchPlaceholder}
        query={search}
        renderItem={(party) => (
          <>
            <span className="truncate">{party.name}</span>
            <span className="shrink-0 text-xs text-muted-foreground">
              {party.phone ?? ""}
            </span>
          </>
        )}
      />
      <p className="text-xs text-muted-foreground">{d.orNewClient}</p>
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-1.5">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor={fieldId("client-name")}
          >
            {d.clientName}
          </label>
          <Input
            aria-describedby={invalid ? fieldId("client-error") : undefined}
            aria-invalid={invalid || undefined}
            autoComplete="off"
            data-testid={fieldId("client-name")}
            id={fieldId("client-name")}
            onChange={(event) =>
              setDraft((current) => ({
                ...current,
                clientName: event.target.value,
              }))
            }
            value={draft.clientName}
          />
        </div>
        <div className="space-y-1.5">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor={fieldId("client-phone")}
          >
            {d.clientPhone}
          </label>
          <Input
            autoComplete="off"
            data-testid={fieldId("client-phone")}
            id={fieldId("client-phone")}
            inputMode="tel"
            onChange={(event) =>
              setDraft((current) => ({
                ...current,
                clientPhone: event.target.value,
              }))
            }
            value={draft.clientPhone}
          />
        </div>
      </div>
    </fieldset>
  );
}

/**
 * Book a client or edit a booked visit. Mount with a fresh `key` per opening.
 * A paid visit keeps its staff member, time and services.
 */
export function CrmAppointmentDialog({
  onCloseAutoFocus,
  onOpenChange,
  open,
  services,
  space,
  staff,
  target,
}: {
  onCloseAutoFocus: (event: Event) => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  /** Active bookable services; `null` while loading. */
  services: readonly CrmProduct[] | null;
  space: CrmMemberSpace;
  staff: readonly CrmStaff[];
  target: AppointmentDialogTarget;
}) {
  const d = ru.journal.dialog;
  const editing = target.mode === "edit" ? target.appointment : null;
  const paid = editing?.saleId != null;
  const initial = React.useMemo(() => initialDraft(target), [target]);
  const [draft, setDraft] = React.useState(initial);
  const [submitted, setSubmitted] = React.useState(false);
  const write = useWriteCrmAppointmentMutation();

  const member = staff.find((s) => s.id === draft.staffId) ?? null;
  const offered = staffServices(services ?? [], member?.serviceIds ?? []);
  // Booked services stay listed even if the staff member no longer offers them.
  const booked = (editing?.services ?? [])
    .map((service, index) => ({
      id: service.productId,
      name: editing?.serviceNames[index] ?? "",
      durationMin: service.durationMin,
      priceMinor: service.priceMinor,
    }))
    .filter((service) => !offered.some((o) => o.id === service.id));
  const choices = [
    ...offered.map((service) => ({
      id: service.id,
      name: service.name,
      durationMin: service.durationMin ?? 0,
      priceMinor: service.priceMinor,
    })),
    ...booked,
  ];
  const serviceIds = draft.services.map((service) => service.productId);

  const slots = useCrmSlotsQuery({
    date: draft.date,
    serviceIds,
    staffId: draft.outsideHours ? null : draft.staffId,
  });
  // The visit's own time, or the clicked cell, while staff and day are unchanged.
  const keep =
    draft.staffId === initial.staffId && draft.date === initial.date
      ? initial.start
      : null;
  const starts = startChoices(
    slots.data?.staff.find((s) => s.staffId === draft.staffId)?.starts ?? [],
    keep,
  );
  const start =
    draft.outsideHours ||
    paid ||
    (draft.start !== null && starts.includes(draft.start))
      ? draft.start
      : null;
  const validation = validateAppointmentDraft({ ...draft, start });
  const invalidField = submitted && !validation.ok ? validation.field : null;
  const totals = servicesTotal(draft.services);

  function toggleService(choice: (typeof choices)[number], checked: boolean) {
    setDraft((current) => ({
      ...current,
      services: checked
        ? [
            ...current.services,
            {
              productId: choice.id,
              durationMin: choice.durationMin,
              priceMinor: choice.priceMinor,
            },
          ]
        : current.services.filter((s) => s.productId !== choice.id),
    }));
  }

  function changeStaff(staffId: string) {
    const next = staff.find((s) => s.id === staffId);
    setDraft((current) => ({
      ...current,
      staffId,
      services: current.services.filter((s) =>
        next?.serviceIds.includes(s.productId),
      ),
    }));
  }

  async function save(
    fields: Parameters<typeof write.mutateAsync>[0],
    message: string,
  ) {
    try {
      await write.mutateAsync(fields);
      toast.success(message);
      onOpenChange(false);
    } catch {
      // Rendered through the mutation error state.
    }
  }

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (!validation.ok) {
      event.currentTarget
        .querySelector<HTMLElement>(`[data-field="${validation.field}"]`)
        ?.focus();
      return;
    }
    void save(
      {
        ...validation.fields,
        channelId: space.channelId,
        appointmentId:
          target.mode === "edit" ? target.appointment.id : target.appointmentId,
        expectedRevision: editing?.revision ?? null,
      },
      editing ? d.saved : d.created,
    );
  }

  const fieldError = (field: string) =>
    invalidField === field && !validation.ok ? (
      <p
        className="text-xs text-destructive"
        data-testid={fieldId(`${field}-error`)}
        id={fieldId(`${field}-error`)}
      >
        {validation.error}
      </p>
    ) : null;

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-h-[90vh] overflow-y-auto sm:max-w-xl"
        data-testid="crm-appointment-dialog"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <form className="space-y-5" noValidate onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>{editing ? d.editTitle : d.createTitle}</DialogTitle>
          </DialogHeader>

          <div data-field="client" tabIndex={-1}>
            <ClientFields
              draft={draft}
              invalid={invalidField === "client"}
              setDraft={setDraft}
            />
            {fieldError("client")}
          </div>

          <div className="grid gap-3 sm:grid-cols-2">
            <div className="space-y-1.5">
              <label
                className="text-xs font-medium text-muted-foreground"
                htmlFor={fieldId("staff")}
              >
                {d.staff}
              </label>
              <NativeSelect
                data-testid={fieldId("staff")}
                disabled={paid}
                id={fieldId("staff")}
                onChange={changeStaff}
                value={draft.staffId}
              >
                {staff.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.name}
                  </option>
                ))}
              </NativeSelect>
            </div>
            <div className="space-y-1.5">
              <label
                className="text-xs font-medium text-muted-foreground"
                htmlFor={fieldId("date")}
              >
                {d.date}
              </label>
              <Input
                data-testid={fieldId("date")}
                disabled={paid}
                id={fieldId("date")}
                onChange={(event) => {
                  const date = event.target.value;
                  if (date) setDraft((current) => ({ ...current, date }));
                }}
                type="date"
                value={draft.date}
              />
            </div>
          </div>

          <fieldset
            aria-describedby={
              invalidField === "services"
                ? fieldId("services-error")
                : undefined
            }
            className="space-y-2"
            data-field="services"
            tabIndex={-1}
          >
            <legend className="text-xs font-medium text-muted-foreground">
              {d.services}
            </legend>
            {choices.length === 0 ? (
              <p className="text-sm text-muted-foreground">{d.noServices}</p>
            ) : (
              <ul className="divide-y divide-border/60 rounded-md border border-border/70">
                {choices.map((choice) => {
                  const id = fieldId(`service-${choice.id}`);
                  const checked = serviceIds.includes(choice.id);
                  return (
                    <li
                      className="flex items-center gap-3 px-3 py-2"
                      key={choice.id}
                    >
                      <Checkbox
                        checked={checked}
                        data-testid={id}
                        disabled={
                          paid ||
                          (!checked &&
                            draft.services.length >= MAX_APPOINTMENT_SERVICES)
                        }
                        id={id}
                        onCheckedChange={(value) =>
                          toggleService(choice, value === true)
                        }
                      />
                      <label
                        className="min-w-0 flex-1 truncate text-sm"
                        htmlFor={id}
                      >
                        {choice.name}
                      </label>
                      <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
                        {formatDuration(choice.durationMin)} ·{" "}
                        {formatRubles(choice.priceMinor)}
                      </span>
                    </li>
                  );
                })}
              </ul>
            )}
            {fieldError("services")}
          </fieldset>

          <div className="space-y-2">
            <div className="space-y-1.5" data-field="time" tabIndex={-1}>
              <label
                className="text-xs font-medium text-muted-foreground"
                htmlFor={fieldId("time")}
              >
                {d.time}
              </label>
              {draft.outsideHours ? (
                <Input
                  aria-invalid={invalidField === "time" || undefined}
                  data-testid={fieldId("time")}
                  disabled={paid}
                  id={fieldId("time")}
                  onChange={(event) =>
                    setDraft((current) => ({
                      ...current,
                      start: clockMinutes(event.target.value),
                    }))
                  }
                  step={300}
                  type="time"
                  value={draft.start === null ? "" : formatClock(draft.start)}
                />
              ) : (
                <NativeSelect
                  aria-invalid={invalidField === "time" || undefined}
                  data-testid={fieldId("time")}
                  disabled={paid || serviceIds.length === 0}
                  id={fieldId("time")}
                  onChange={(value) =>
                    setDraft((current) => ({
                      ...current,
                      start: value === "" ? null : Number(value),
                    }))
                  }
                  value={start === null ? "" : String(start)}
                >
                  <option value="">—</option>
                  {starts.map((minutes) => (
                    <option key={minutes} value={String(minutes)}>
                      {formatClock(minutes)}
                    </option>
                  ))}
                </NativeSelect>
              )}
              {!draft.outsideHours && !paid ? (
                <p
                  className="text-xs text-muted-foreground"
                  data-testid={fieldId("time-hint")}
                >
                  {serviceIds.length === 0
                    ? d.pickServicesFirst
                    : slots.isFetching
                      ? d.slotsLoading
                      : starts.length === 0
                        ? d.noSlots
                        : null}
                </p>
              ) : null}
              {fieldError("time")}
            </div>
            <div className="flex items-start gap-2">
              <Checkbox
                aria-describedby={fieldId("outside-hours-hint")}
                checked={draft.outsideHours}
                data-testid={fieldId("outside-hours")}
                disabled={paid}
                id={fieldId("outside-hours")}
                onCheckedChange={(value) =>
                  setDraft((current) => ({
                    ...current,
                    outsideHours: value === true,
                    start: start ?? current.start,
                  }))
                }
              />
              <div className="space-y-0.5">
                <label className="text-sm" htmlFor={fieldId("outside-hours")}>
                  {d.outsideHours}
                </label>
                <p
                  className="text-xs text-muted-foreground"
                  id={fieldId("outside-hours-hint")}
                >
                  {d.outsideHoursHint}
                </p>
              </div>
            </div>
          </div>

          {editing ? (
            <div className="space-y-1.5">
              <label
                className="text-xs font-medium text-muted-foreground"
                htmlFor={fieldId("status")}
              >
                {d.status}
              </label>
              <NativeSelect
                data-testid={fieldId("status")}
                id={fieldId("status")}
                onChange={(value) =>
                  setDraft((current) => ({
                    ...current,
                    status: value as CrmAppointmentStatus,
                  }))
                }
                value={draft.status}
              >
                {CRM_APPOINTMENT_STATUSES.map((status) => (
                  <option key={status} value={status}>
                    {ru.journal.statuses[status]}
                  </option>
                ))}
              </NativeSelect>
            </div>
          ) : null}

          <div className="space-y-1.5">
            <label
              className="text-xs font-medium text-muted-foreground"
              htmlFor={fieldId("note")}
            >
              {d.note}
            </label>
            <Textarea
              data-testid={fieldId("note")}
              id={fieldId("note")}
              onChange={(event) =>
                setDraft((current) => ({
                  ...current,
                  note: event.target.value,
                }))
              }
              rows={2}
              value={draft.note}
            />
          </div>

          {draft.services.length > 0 ? (
            <p className="text-sm font-medium" data-testid={fieldId("total")}>
              {d.total
                .replace("{duration}", formatDuration(totals.durationMin))
                .replace("{total}", formatRubles(totals.totalMinor))}
            </p>
          ) : null}

          <CrmErrorAlert error={write.error} testId="crm-appointment-error" />
          <DialogFooter className="sm:justify-between">
            {editing && editing.status !== "cancelled" ? (
              <Button
                data-testid="crm-appointment-cancel-visit"
                disabled={write.isPending}
                onClick={() =>
                  void save(
                    statusWrite(editing, space.channelId, "cancelled"),
                    d.cancelled,
                  )
                }
                type="button"
                variant="outline"
              >
                {d.cancelVisit}
              </Button>
            ) : (
              <span />
            )}
            <div className="flex flex-col-reverse gap-2 sm:flex-row">
              <Button
                onClick={() => onOpenChange(false)}
                type="button"
                variant="ghost"
              >
                {ru.common.cancel}
              </Button>
              <Button
                data-testid="crm-appointment-submit"
                disabled={write.isPending}
                type="submit"
              >
                {write.isPending
                  ? ru.common.saving
                  : editing
                    ? ru.common.save
                    : d.submitCreate}
              </Button>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
