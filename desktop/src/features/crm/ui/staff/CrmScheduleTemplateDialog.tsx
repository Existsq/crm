import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import {
  buildTemplateDays,
  MAX_SHIFT_DAYS,
  MAX_TEMPLATE_WEEKS,
  type TemplateField,
} from "@/features/crm/lib/schedule";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmStaff, CrmWorkInterval } from "@/shared/api/tauriCrmStaff";
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

const fieldId = (field: string) => `crm-schedule-template-${field}`;
const WORKWEEK = [0, 1, 2, 3, 4];

type PatternType = "weekdays" | "shift";

/**
 * Fill weeks of one staff member's schedule from a pattern: fixed weekdays
 * or a shift rotation such as 2/2. Mount with a fresh `key` per opening.
 */
export function CrmScheduleTemplateDialog({
  defaultFrom,
  error,
  onCloseAutoFocus,
  onOpenChange,
  onSubmit,
  open,
  pending,
  staff,
}: {
  /** First date offered: the Monday of the week on screen. */
  defaultFrom: string;
  error: unknown;
  onCloseAutoFocus: (event: Event) => void;
  onOpenChange: (open: boolean) => void;
  onSubmit: (
    staffId: string,
    days: Array<{ date: string; intervals: CrmWorkInterval[] }>,
  ) => void;
  open: boolean;
  pending: boolean;
  staff: readonly CrmStaff[];
}) {
  const t = ru.schedule.template;
  const [staffId, setStaffId] = React.useState(staff[0]?.id ?? "");
  const [patternType, setPatternType] = React.useState<PatternType>("weekdays");
  const [weekdays, setWeekdays] = React.useState<number[]>(WORKWEEK);
  const [work, setWork] = React.useState("2");
  const [off, setOff] = React.useState("2");
  const [from, setFrom] = React.useState(defaultFrom);
  const [weeks, setWeeks] = React.useState("4");
  const [start, setStart] = React.useState("10:00");
  const [end, setEnd] = React.useState("20:00");
  const [breakStart, setBreakStart] = React.useState("");
  const [breakEnd, setBreakEnd] = React.useState("");
  const [submitted, setSubmitted] = React.useState(false);

  const result = buildTemplateDays({
    from,
    weeks: Number(weeks),
    pattern:
      patternType === "weekdays"
        ? { type: "weekdays", weekdays }
        : { type: "shift", work: Number(work), off: Number(off) },
    start,
    end,
    breakStart,
    breakEnd,
  });
  const errors: Partial<Record<TemplateField, string>> =
    submitted && !result.ok ? result.errors : {};

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (!result.ok) {
      const first = Object.keys(result.errors)[0];
      event.currentTarget
        .querySelector<HTMLElement>(`[data-field="${first}"]`)
        ?.focus();
      return;
    }
    if (staffId) onSubmit(staffId, result.days);
  }

  const timeInput = (
    field: TemplateField,
    id: string,
    value: string,
    set: (value: string) => void,
  ) => (
    <Input
      {...crmControlProps(id, field === "break", errors[field])}
      data-field={field}
      onChange={(event) => set(event.target.value)}
      step={300}
      type="time"
      value={value}
    />
  );

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="flex max-h-[calc(100dvh-2rem)] flex-col sm:max-w-xl"
        data-testid="crm-schedule-template-dialog"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <form
          className="flex min-h-0 flex-1 flex-col gap-5"
          noValidate
          onSubmit={handleSubmit}
        >
          <DialogHeader>
            <DialogTitle>{t.title}</DialogTitle>
          </DialogHeader>
          <div className="-mx-1 grid min-h-0 flex-1 gap-4 overflow-y-auto px-1 pb-1 sm:grid-cols-2">
            <CrmField id={fieldId("staff")} label={t.staff}>
              <NativeSelect
                {...crmControlProps(fieldId("staff"), false, undefined)}
                onChange={setStaffId}
                value={staffId}
              >
                {staff.map((member) => (
                  <option key={member.id} value={member.id}>
                    {member.name}
                  </option>
                ))}
              </NativeSelect>
            </CrmField>
            <CrmField id={fieldId("pattern")} label={t.pattern}>
              <NativeSelect
                {...crmControlProps(fieldId("pattern"), false, undefined)}
                onChange={(value) => setPatternType(value as PatternType)}
                value={patternType}
              >
                <option value="weekdays">{t.patterns.weekdays}</option>
                <option value="shift">{t.patterns.shift}</option>
              </NativeSelect>
            </CrmField>

            {patternType === "weekdays" ? (
              <fieldset
                aria-describedby={
                  errors.weekdays ? `${fieldId("weekdays")}-error` : undefined
                }
                className="space-y-2 sm:col-span-2"
              >
                <legend className="text-sm font-medium">{t.weekdays}</legend>
                <div className="flex flex-wrap gap-x-4 gap-y-2">
                  {ru.schedule.weekdaysShort.map((label, index) => {
                    const id = `${fieldId("weekday")}-${index}`;
                    return (
                      <div className="flex items-center gap-1.5" key={label}>
                        <Checkbox
                          aria-label={ru.schedule.weekdaysLong[index]}
                          checked={weekdays.includes(index)}
                          data-field={index === 0 ? "weekdays" : undefined}
                          data-testid={id}
                          id={id}
                          onCheckedChange={(value) =>
                            setWeekdays((current) =>
                              value === true
                                ? [...current, index].sort()
                                : current.filter((day) => day !== index),
                            )
                          }
                        />
                        <label
                          aria-hidden="true"
                          className="text-sm"
                          htmlFor={id}
                        >
                          {label}
                        </label>
                      </div>
                    );
                  })}
                </div>
                {errors.weekdays ? (
                  <p
                    className="text-xs text-destructive"
                    id={`${fieldId("weekdays")}-error`}
                  >
                    {errors.weekdays}
                  </p>
                ) : null}
              </fieldset>
            ) : (
              <>
                <CrmField
                  error={errors.work}
                  id={fieldId("work")}
                  label={t.work}
                >
                  <Input
                    {...crmControlProps(fieldId("work"), false, errors.work)}
                    data-field="work"
                    inputMode="numeric"
                    max={MAX_SHIFT_DAYS}
                    min={1}
                    onChange={(event) => setWork(event.target.value)}
                    type="number"
                    value={work}
                  />
                </CrmField>
                <CrmField error={errors.off} id={fieldId("off")} label={t.off}>
                  <Input
                    {...crmControlProps(fieldId("off"), false, errors.off)}
                    data-field="off"
                    inputMode="numeric"
                    max={MAX_SHIFT_DAYS}
                    min={1}
                    onChange={(event) => setOff(event.target.value)}
                    type="number"
                    value={off}
                  />
                </CrmField>
              </>
            )}

            <CrmField error={errors.from} id={fieldId("from")} label={t.from}>
              <Input
                {...crmControlProps(fieldId("from"), false, errors.from)}
                data-field="from"
                onChange={(event) => setFrom(event.target.value)}
                type="date"
                value={from}
              />
            </CrmField>
            <CrmField
              error={errors.weeks}
              hint={t.weeksHint}
              id={fieldId("weeks")}
              label={t.weeks}
            >
              <Input
                {...crmControlProps(fieldId("weeks"), true, errors.weeks)}
                data-field="weeks"
                inputMode="numeric"
                max={MAX_TEMPLATE_WEEKS}
                min={1}
                onChange={(event) => setWeeks(event.target.value)}
                type="number"
                value={weeks}
              />
            </CrmField>
            <CrmField
              error={errors.start}
              id={fieldId("start")}
              label={t.start}
            >
              {timeInput("start", fieldId("start"), start, setStart)}
            </CrmField>
            <CrmField error={errors.end} id={fieldId("end")} label={t.end}>
              {timeInput("end", fieldId("end"), end, setEnd)}
            </CrmField>
            <CrmField
              error={errors.break}
              hint={t.breakHint}
              id={fieldId("break-start")}
              label={t.breakStart}
            >
              {timeInput(
                "break",
                fieldId("break-start"),
                breakStart,
                setBreakStart,
              )}
            </CrmField>
            <CrmField id={fieldId("break-end")} label={t.breakEnd}>
              <Input
                {...crmControlProps(fieldId("break-end"), false, undefined)}
                onChange={(event) => setBreakEnd(event.target.value)}
                step={300}
                type="time"
                value={breakEnd}
              />
            </CrmField>
          </div>
          {result.ok ? (
            <p
              aria-live="polite"
              className="text-sm text-muted-foreground"
              data-testid="crm-schedule-template-summary"
            >
              {t.summary
                .replace("{days}", String(result.days.length))
                .replace("{work}", String(result.workDays))}
            </p>
          ) : null}
          <CrmErrorAlert error={error} testId="crm-schedule-template-error" />
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-schedule-template-submit"
              disabled={pending || !staffId}
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
