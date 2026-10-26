import * as React from "react";

import { useCrmSalesQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { describeCrmError } from "@/features/crm/lib/crmErrors";
import { formatRubles } from "@/features/crm/lib/money";
import {
  CRM_PARTY_TYPES,
  emptyPartyDraft,
  PARTY_DRAFT_FIELDS,
  type PartyDraft,
  partyDraftFrom,
  samePartyDraft,
  validatePartyDraft,
} from "@/features/crm/lib/parties";
import {
  CrmField,
  crmControlProps,
  formatCrmDate,
  formatCrmDateTime,
} from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { CrmPartyVisits } from "@/features/crm/ui/parties/CrmPartyVisits";
import type { CrmParty, CrmPartyFields } from "@/shared/api/tauriCrmSales";
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
import { NativeSelect } from "@/shared/ui/native-select";
import { Textarea } from "@/shared/ui/textarea";

const fieldId = (field: string) => `crm-party-${field}`;

/** Purchase totals and the latest sales of a customer. */
function PartyPurchases({ party }: { party: CrmParty }) {
  const stats = ru.parties.stats;
  const { salesCount, revenueMinor, lastSaleAt } = party.purchases;
  const recent = useCrmSalesQuery({
    customerId: party.id,
    voided: false,
    limit: 5,
  });
  const sales = recent.data?.pages[0]?.sales ?? [];
  const average = salesCount > 0 ? Math.round(revenueMinor / salesCount) : null;
  return (
    <section
      aria-labelledby="crm-party-stats-title"
      className="space-y-3 rounded-xl border border-border/70 p-3"
      data-testid="crm-party-stats"
    >
      <h3 className="text-sm font-medium" id="crm-party-stats-title">
        {stats.title}
      </h3>
      {salesCount === 0 ? (
        <p className="text-sm text-muted-foreground">{stats.never}</p>
      ) : (
        <>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm sm:grid-cols-4">
            <div>
              <dt className="text-xs text-muted-foreground">{stats.revenue}</dt>
              <dd className="tabular-nums" data-testid="crm-party-revenue">
                {formatRubles(revenueMinor)}
              </dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground">{stats.count}</dt>
              <dd className="tabular-nums">{salesCount}</dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground">{stats.average}</dt>
              <dd className="tabular-nums">
                {average === null ? "—" : formatRubles(average)}
              </dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground">{stats.last}</dt>
              <dd>{lastSaleAt ? formatCrmDate(lastSaleAt) : "—"}</dd>
            </div>
          </dl>
          {sales.length > 0 ? (
            <div className="space-y-1">
              <p className="text-xs text-muted-foreground">{stats.recent}</p>
              <ul className="divide-y divide-border/60 text-sm">
                {sales.map((sale) => (
                  <li
                    className="flex items-center justify-between gap-3 py-1.5"
                    key={sale.id}
                  >
                    <span>
                      {ru.sales.columns.number}
                      {sale.number} · {formatCrmDateTime(sale.soldAt)}
                    </span>
                    <span className="tabular-nums">
                      {formatRubles(sale.totalMinor)}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          ) : null}
        </>
      )}
    </section>
  );
}

/**
 * Create or edit a customer/supplier; the caller owns the mutation. Mount it
 * with a fresh `key` per opening so the draft starts from `party`.
 */
export function CrmPartyDialog({
  canEdit,
  error,
  onCloseAutoFocus,
  onDismissError,
  onOpenChange,
  onSubmit,
  open,
  party,
  pending,
}: {
  canEdit: boolean;
  error: unknown;
  onCloseAutoFocus: (event: Event) => void;
  onDismissError: () => void;
  onOpenChange: (open: boolean) => void;
  onSubmit: (fields: CrmPartyFields, expectedRevision: string | null) => void;
  open: boolean;
  /** Live party being edited, `null` to create one. */
  party: CrmParty | null;
  pending: boolean;
}) {
  const initialDraft = () =>
    party ? partyDraftFrom(party) : emptyPartyDraft();
  const [baseline, setBaseline] = React.useState<PartyDraft>(initialDraft);
  const [draft, setDraft] = React.useState<PartyDraft>(initialDraft);
  const [baseRevision, setBaseRevision] = React.useState(
    party?.revision ?? null,
  );
  const [submitted, setSubmitted] = React.useState(false);

  // Adopt a party changed elsewhere unless the user has unsaved edits; with
  // edits the stale base revision turns a save into a visible conflict.
  if (
    party &&
    party.revision !== baseRevision &&
    samePartyDraft(draft, baseline)
  ) {
    adopt(party);
  }
  const conflicted =
    party !== null &&
    party.revision !== baseRevision &&
    error !== null &&
    describeCrmError(error).kind === "conflict";

  function adopt(latest: CrmParty) {
    const next = partyDraftFrom(latest);
    setBaseRevision(latest.revision);
    setBaseline(next);
    setDraft(next);
  }

  const set = (patch: Partial<PartyDraft>) =>
    setDraft((current) => ({ ...current, ...patch }));
  const validation = validatePartyDraft(draft);
  const errors = submitted && !validation.ok ? validation.errors : {};
  const fields = ru.parties.fields;

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canEdit) return;
    setSubmitted(true);
    if (!validation.ok) {
      const first = PARTY_DRAFT_FIELDS.find(
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
        data-testid="crm-party-dialog"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <form
          className="flex min-h-0 flex-1 flex-col gap-5"
          noValidate
          onSubmit={handleSubmit}
        >
          <DialogHeader>
            <DialogTitle className="flex flex-wrap items-center gap-2">
              {party ? ru.parties.editTitle : ru.parties.createTitle}
              {party?.archived ? (
                <Badge variant="secondary">{ru.parties.archivedBadge}</Badge>
              ) : null}
            </DialogTitle>
          </DialogHeader>
          <div className="-mx-1 min-h-0 flex-1 space-y-4 overflow-y-auto px-1 pb-1">
            {party ? <PartyPurchases party={party} /> : null}
            {party && party.partyType !== "supplier" ? (
              <CrmPartyVisits customerId={party.id} />
            ) : null}
            <fieldset className="grid gap-4 sm:grid-cols-2" disabled={!canEdit}>
              <CrmField id={fieldId("type")} label={fields.type}>
                <NativeSelect
                  {...crmControlProps(fieldId("type"), false, undefined)}
                  onChange={(value) =>
                    set({ partyType: value as PartyDraft["partyType"] })
                  }
                  value={draft.partyType}
                >
                  {CRM_PARTY_TYPES.map((type) => (
                    <option key={type} value={type}>
                      {ru.parties.types[type]}
                    </option>
                  ))}
                </NativeSelect>
              </CrmField>
              <CrmField
                error={errors.name}
                id={fieldId("name")}
                label={fields.name}
              >
                <Input
                  {...crmControlProps(fieldId("name"), false, errors.name)}
                  autoFocus={!party}
                  maxLength={128}
                  onChange={(event) => set({ name: event.target.value })}
                  value={draft.name}
                />
              </CrmField>
              <CrmField
                error={errors.phone}
                hint={fields.phoneHint}
                id={fieldId("phone")}
                label={fields.phone}
              >
                <Input
                  {...crmControlProps(fieldId("phone"), true, errors.phone)}
                  inputMode="tel"
                  maxLength={32}
                  onChange={(event) => set({ phone: event.target.value })}
                  type="tel"
                  value={draft.phone}
                />
              </CrmField>
              <CrmField
                error={errors.email}
                id={fieldId("email")}
                label={fields.email}
              >
                <Input
                  {...crmControlProps(fieldId("email"), false, errors.email)}
                  autoCapitalize="none"
                  maxLength={254}
                  onChange={(event) => set({ email: event.target.value })}
                  spellCheck={false}
                  type="email"
                  value={draft.email}
                />
              </CrmField>
              <CrmField
                error={errors.inn}
                hint={fields.innHint}
                id={fieldId("inn")}
                label={fields.inn}
              >
                <Input
                  {...crmControlProps(fieldId("inn"), true, errors.inn)}
                  inputMode="numeric"
                  maxLength={12}
                  onChange={(event) => set({ inn: event.target.value })}
                  value={draft.inn}
                />
              </CrmField>
              <CrmField id={fieldId("address")} label={fields.address}>
                <Input
                  {...crmControlProps(fieldId("address"), false, undefined)}
                  maxLength={256}
                  onChange={(event) => set({ address: event.target.value })}
                  value={draft.address}
                />
              </CrmField>
              <CrmField
                className="sm:col-span-2"
                hint={fields.noteHint}
                id={fieldId("note")}
                label={fields.note}
              >
                <Textarea
                  {...crmControlProps(fieldId("note"), true, undefined)}
                  maxLength={2000}
                  onChange={(event) => set({ note: event.target.value })}
                  rows={3}
                  value={draft.note}
                />
              </CrmField>
              {party ? (
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
            </fieldset>
          </div>
          <CrmErrorAlert error={error} testId="crm-party-dialog-error" />
          <DialogFooter>
            {conflicted && party ? (
              <Button
                data-testid="crm-party-dialog-reload"
                onClick={() => {
                  onDismissError();
                  adopt(party);
                }}
                type="button"
                variant="outline"
              >
                {ru.parties.reloadLatest}
              </Button>
            ) : null}
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {canEdit ? ru.common.cancel : ru.sales.details.close}
            </Button>
            {canEdit ? (
              <Button
                data-testid="crm-party-dialog-submit"
                disabled={pending || conflicted}
                type="submit"
              >
                {pending ? ru.common.saving : ru.common.save}
              </Button>
            ) : null}
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
