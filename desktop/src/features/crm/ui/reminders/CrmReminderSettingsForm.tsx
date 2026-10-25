import { Plus, Trash2 } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import { describeCrmError } from "@/features/crm/lib/crmErrors";
import {
  defaultReminderSettings,
  formatOffset,
  MAX_TEMPLATE_CHARS,
  nextReminderRule,
  REMINDER_OFFSET_CHOICES,
  REMINDER_PLACEHOLDERS,
  type ReminderContext,
  type ReminderPlaceholder,
  renderReminder,
  ruleKeyLabel,
  ruleKey,
  sameReminderSettings,
  sampleReminderContext,
  validateReminderDraft,
} from "@/features/crm/lib/reminders";
import { useWriteCrmReminderSettingsMutation } from "@/features/crm/reminderHooks";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type {
  CrmReminderRule,
  CrmReminderSettings,
  CrmReminders,
} from "@/shared/api/tauriCrmReminders";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { NativeSelect } from "@/shared/ui/native-select";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Switch } from "@/shared/ui/switch";
import { Textarea } from "@/shared/ui/textarea";

const r = ru.reminders;

function settingsOf(reminders: CrmReminders): CrmReminderSettings {
  return reminders.settings ?? defaultReminderSettings();
}

/** One message rule: when it fires, its text, chips and a live preview. */
function ReminderRuleCard({
  canManage,
  context,
  error,
  index,
  onChange,
  onRemove,
  rule,
  takenKeys,
}: {
  canManage: boolean;
  context: ReminderContext;
  error: string | undefined;
  index: number;
  onChange: (rule: CrmReminderRule) => void;
  onRemove: () => void;
  rule: CrmReminderRule;
  /** Keys of the other rules, which this one's offset may not reuse. */
  takenKeys: ReadonlySet<string>;
}) {
  const textareaRef = React.useRef<HTMLTextAreaElement>(null);
  const templateId = `crm-reminder-template-${index}`;
  const offsetId = `crm-reminder-offset-${index}`;
  const label = ruleKeyLabel(ruleKey(rule));
  const offsets =
    rule.offsetMin !== null && !REMINDER_OFFSET_CHOICES.includes(rule.offsetMin)
      ? [rule.offsetMin, ...REMINDER_OFFSET_CHOICES]
      : REMINDER_OFFSET_CHOICES;

  function insert(placeholder: ReminderPlaceholder) {
    const token = `{${placeholder}}`;
    const element = textareaRef.current;
    const start = element?.selectionStart ?? rule.template.length;
    const end = element?.selectionEnd ?? rule.template.length;
    onChange({
      ...rule,
      template:
        rule.template.slice(0, start) + token + rule.template.slice(end),
    });
    requestAnimationFrame(() => {
      element?.focus();
      element?.setSelectionRange(start + token.length, start + token.length);
    });
  }

  return (
    <li
      className="space-y-3 rounded-xl border border-border/70 p-4"
      data-testid={`crm-reminder-rule-${index}`}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        {rule.trigger === "booked" ? (
          <div className="space-y-0.5">
            <h4 className="text-sm font-medium">{r.rule.booked}</h4>
            <p className="text-xs text-muted-foreground">{r.rule.bookedHint}</p>
          </div>
        ) : (
          <CrmField className="min-w-48" id={offsetId} label={r.offsetLabel}>
            <NativeSelect
              {...crmControlProps(offsetId, false, undefined)}
              disabled={!canManage}
              onChange={(value) =>
                onChange({ ...rule, offsetMin: Number(value) })
              }
              value={String(rule.offsetMin ?? "")}
            >
              {offsets.map((offset) => (
                <option
                  disabled={
                    offset !== rule.offsetMin &&
                    takenKeys.has(`before:${offset}`)
                  }
                  key={offset}
                  value={offset}
                >
                  {r.offsetOption.replace("{value}", formatOffset(offset))}
                </option>
              ))}
            </NativeSelect>
          </CrmField>
        )}
        {canManage ? (
          <Button
            aria-label={r.removeRule.replace("{value}", label)}
            data-testid={`crm-reminder-remove-${index}`}
            onClick={onRemove}
            size="icon"
            type="button"
            variant="ghost"
          >
            <Trash2 aria-hidden="true" className="h-4 w-4" />
          </Button>
        ) : null}
      </div>
      <CrmField error={error} id={templateId} label={r.templateLabel}>
        <Textarea
          {...crmControlProps(templateId, false, error)}
          disabled={!canManage}
          maxLength={MAX_TEMPLATE_CHARS}
          onChange={(event) =>
            onChange({ ...rule, template: event.target.value })
          }
          ref={textareaRef}
          rows={3}
          value={rule.template}
        />
      </CrmField>
      {canManage ? (
        <fieldset className="space-y-1.5">
          <legend className="text-xs text-muted-foreground">
            {r.placeholdersLabel}
          </legend>
          <div className="flex flex-wrap gap-1.5">
            {REMINDER_PLACEHOLDERS.map((placeholder) => (
              <button
                className="rounded-full border border-border/70 px-2.5 py-0.5 text-xs hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                data-testid={`crm-reminder-chip-${index}-${placeholder}`}
                key={placeholder}
                onClick={() => insert(placeholder)}
                type="button"
              >
                <span className="font-mono">{`{${placeholder}}`}</span>{" "}
                <span className="text-muted-foreground">
                  {r.placeholders[placeholder]}
                </span>
              </button>
            ))}
          </div>
        </fieldset>
      ) : null}
      <figure className="space-y-1">
        <figcaption className="text-xs text-muted-foreground">
          {r.previewLabel}
        </figcaption>
        <p
          className="whitespace-pre-wrap rounded-lg bg-muted/50 px-3 py-2 text-sm"
          data-testid={`crm-reminder-preview-${index}`}
        >
          {renderReminder(rule.template, context)}
        </p>
      </figure>
    </li>
  );
}

/** Reminder rules of the space, saved as one kind 42150 event (CAS). */
export function CrmReminderSettingsForm({
  reminders,
  space,
}: {
  reminders: CrmReminders;
  space: CrmMemberSpace;
}) {
  const canManage = space.capabilities.has("manage_space");
  const [draft, setDraft] = React.useState<CrmReminderSettings>(() =>
    settingsOf(reminders),
  );
  // The revision the draft is based on: a change saved elsewhere while the
  // user edits turns their save into a visible conflict.
  const [base, setBase] = React.useState({
    settings: settingsOf(reminders),
    revision: reminders.revision,
  });
  // The revision this form replaced with its own save: until the refetch
  // lands the query still reports it, and it must not be adopted back.
  const [replaced, setReplaced] = React.useState<string | null>(null);
  const [submitted, setSubmitted] = React.useState(false);
  const write = useWriteCrmReminderSettingsMutation();
  const dirty =
    reminders.settings === null ||
    !sameReminderSettings(draft, settingsOf(reminders));
  const conflicted =
    write.error !== null && describeCrmError(write.error).kind === "conflict";

  if (
    base.revision !== reminders.revision &&
    reminders.revision !== replaced &&
    sameReminderSettings(draft, base.settings)
  ) {
    adoptLatest();
  }

  function adoptLatest() {
    const latest = settingsOf(reminders);
    setBase({ settings: latest, revision: reminders.revision });
    setDraft(latest);
  }

  const validation = validateReminderDraft(draft);
  const errors = submitted && !validation.ok ? validation.errors : null;
  const context = sampleReminderContext(draft);
  const next = nextReminderRule(draft.rules);
  const keys = draft.rules.map(ruleKey);

  const setRule = (index: number, rule: CrmReminderRule) =>
    setDraft((current) => ({
      ...current,
      rules: current.rules.map((existing, i) =>
        i === index ? rule : existing,
      ),
    }));

  async function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!canManage) return;
    setSubmitted(true);
    if (!validation.ok) return;
    const saving = validation.settings;
    try {
      const result = await write.mutateAsync({
        ...saving,
        channelId: space.channelId,
        expectedRevision: base.revision,
      });
      setReplaced(base.revision);
      setBase({ settings: saving, revision: result.eventId });
      setDraft(saving);
      setSubmitted(false);
      toast.success(r.saved);
    } catch {
      // Rendered through the mutation error state.
    }
  }

  return (
    <form
      aria-labelledby="crm-reminders-settings-title"
      className="space-y-5 rounded-2xl border border-border/70 p-5"
      data-testid="crm-reminders-form"
      noValidate
      onSubmit={(event) => void handleSubmit(event)}
    >
      <SectionHeader
        description={canManage ? r.description : r.readOnlyHint}
        title={<span id="crm-reminders-settings-title">{r.title}</span>}
      />

      <div className="flex items-start justify-between gap-4">
        <div className="space-y-0.5">
          <label className="text-sm" htmlFor="crm-reminders-enabled">
            {r.enabledLabel}
          </label>
          <p
            className="text-xs text-muted-foreground"
            id="crm-reminders-enabled-hint"
          >
            {r.enabledHint}
          </p>
        </div>
        <Switch
          aria-describedby="crm-reminders-enabled-hint"
          checked={draft.enabled}
          data-testid="crm-reminders-enabled"
          disabled={!canManage}
          id="crm-reminders-enabled"
          onCheckedChange={(enabled) =>
            setDraft((current) => ({ ...current, enabled }))
          }
        />
      </div>

      <fieldset className="grid gap-4 sm:grid-cols-2" disabled={!canManage}>
        <CrmField
          error={errors?.salonName}
          hint={r.salonNameHint}
          id="crm-reminders-salon"
          label={r.salonName}
        >
          <Input
            {...crmControlProps("crm-reminders-salon", true, errors?.salonName)}
            maxLength={128}
            onChange={(event) =>
              setDraft((current) => ({
                ...current,
                salonName: event.target.value,
              }))
            }
            value={draft.salonName ?? ""}
          />
        </CrmField>
        <CrmField
          error={errors?.address}
          hint={r.addressHint}
          id="crm-reminders-address"
          label={r.address}
        >
          <Input
            {...crmControlProps("crm-reminders-address", true, errors?.address)}
            maxLength={200}
            onChange={(event) =>
              setDraft((current) => ({
                ...current,
                address: event.target.value,
              }))
            }
            value={draft.address ?? ""}
          />
        </CrmField>
      </fieldset>

      <section
        aria-labelledby="crm-reminders-rules-title"
        className="space-y-3"
      >
        <h3 className="text-sm font-medium" id="crm-reminders-rules-title">
          {r.rulesTitle}
        </h3>
        <ul className="space-y-3">
          {draft.rules.map((rule, index) => (
            <ReminderRuleCard
              canManage={canManage}
              context={context}
              error={errors?.rules[index]}
              index={index}
              // Rules have no ids of their own; the index is their identity
              // within the draft.
              // biome-ignore lint/suspicious/noArrayIndexKey: see above
              key={index}
              onChange={(next) => setRule(index, next)}
              onRemove={() =>
                setDraft((current) => ({
                  ...current,
                  rules: current.rules.filter((_, i) => i !== index),
                }))
              }
              rule={rule}
              takenKeys={new Set(keys.filter((_, i) => i !== index))}
            />
          ))}
        </ul>
        {canManage ? (
          <Button
            data-testid="crm-reminders-add"
            disabled={next === null}
            onClick={() => {
              if (next) {
                setDraft((current) => ({
                  ...current,
                  rules: [...current.rules, next],
                }));
              }
            }}
            type="button"
            variant="outline"
          >
            <Plus aria-hidden="true" className="h-4 w-4" />
            {r.addRule}
          </Button>
        ) : null}
      </section>

      <CrmErrorAlert error={write.error} testId="crm-reminders-error" />

      {canManage ? (
        <div className="flex flex-wrap gap-2">
          <Button
            data-testid="crm-reminders-save"
            disabled={!dirty || write.isPending || conflicted}
            type="submit"
          >
            {write.isPending ? ru.common.saving : ru.common.save}
          </Button>
          {conflicted ? (
            <Button
              data-testid="crm-reminders-reload"
              onClick={() => {
                write.reset();
                adoptLatest();
              }}
              type="button"
              variant="outline"
            >
              {r.reloadLatest}
            </Button>
          ) : null}
        </div>
      ) : null}
    </form>
  );
}
