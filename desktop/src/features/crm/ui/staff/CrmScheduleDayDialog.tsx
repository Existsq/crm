import { Plus, Trash2 } from "lucide-react";
import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import {
  formatClock,
  type IntervalDraft,
  MAX_DAY_INTERVALS,
  validateIntervals,
} from "@/features/crm/lib/schedule";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmWorkInterval } from "@/shared/api/tauriCrmStaff";
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

/** Hours a day without a schedule starts from in the editor. */
const DEFAULT_DAY: IntervalDraft = { start: "10:00", end: "20:00" };

function draftsFrom(intervals: readonly CrmWorkInterval[] | null) {
  if (!intervals || intervals.length === 0) return [DEFAULT_DAY];
  return intervals.map((i) => ({
    start: formatClock(i.start),
    end: formatClock(i.end),
  }));
}

/**
 * Edit one staff member's hours on one day. Mount with a fresh `key` per
 * opening; `intervals` is the day as loaded (`null`: not scheduled yet).
 */
export function CrmScheduleDayDialog({
  dateLabel,
  error,
  intervals,
  onCloseAutoFocus,
  onOpenChange,
  onSubmit,
  open,
  pending,
  staffName,
}: {
  dateLabel: string;
  error: unknown;
  intervals: readonly CrmWorkInterval[] | null;
  onCloseAutoFocus: (event: Event) => void;
  onOpenChange: (open: boolean) => void;
  /** An empty list makes the day a day off. */
  onSubmit: (intervals: CrmWorkInterval[]) => void;
  open: boolean;
  pending: boolean;
  staffName: string;
}) {
  const d = ru.schedule.day;
  const [dayOff, setDayOff] = React.useState(
    intervals !== null && intervals.length === 0,
  );
  const [drafts, setDrafts] = React.useState<IntervalDraft[]>(() =>
    draftsFrom(intervals),
  );
  const [submitted, setSubmitted] = React.useState(false);
  const validation = validateIntervals(drafts);
  const invalid = submitted && !dayOff && !validation.ok ? validation : null;

  function update(index: number, patch: Partial<IntervalDraft>) {
    setDrafts((current) =>
      current.map((draft, i) => (i === index ? { ...draft, ...patch } : draft)),
    );
  }

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (dayOff) {
      onSubmit([]);
      return;
    }
    setSubmitted(true);
    if (!validation.ok) {
      event.currentTarget
        .querySelector<HTMLElement>(
          `#crm-schedule-interval-${validation.index}-start`,
        )
        ?.focus();
      return;
    }
    onSubmit(validation.intervals);
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        data-testid="crm-schedule-day-dialog"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <form className="space-y-5" noValidate onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>{staffName}</DialogTitle>
            <DialogDescription>{dateLabel}</DialogDescription>
          </DialogHeader>

          {dayOff ? (
            <p className="text-sm text-muted-foreground">{d.dayOffNote}</p>
          ) : (
            <div className="space-y-3">
              {drafts.map((draft, index) => {
                const n = String(index + 1);
                const label = d.interval.replace("{n}", n);
                const errorId =
                  invalid?.index === index
                    ? "crm-schedule-interval-error"
                    : undefined;
                return (
                  <fieldset
                    className="flex items-end gap-2"
                    // biome-ignore lint/suspicious/noArrayIndexKey: rows have no identity besides their position
                    key={index}
                  >
                    <legend className="sr-only">{label}</legend>
                    {(["start", "end"] as const).map((edge) => {
                      const id = `crm-schedule-interval-${index}-${edge}`;
                      return (
                        <div className="flex-1 space-y-1.5" key={edge}>
                          <label
                            className="text-xs text-muted-foreground"
                            htmlFor={id}
                          >
                            {d[edge]}
                          </label>
                          <Input
                            aria-describedby={errorId}
                            aria-invalid={errorId ? true : undefined}
                            data-testid={id}
                            id={id}
                            onChange={(event) =>
                              update(index, { [edge]: event.target.value })
                            }
                            step={300}
                            type="time"
                            value={draft[edge]}
                          />
                        </div>
                      );
                    })}
                    <Button
                      aria-label={d.removeInterval.replace("{n}", n)}
                      disabled={drafts.length === 1}
                      onClick={() =>
                        setDrafts((current) =>
                          current.filter((_, i) => i !== index),
                        )
                      }
                      size="icon"
                      type="button"
                      variant="ghost"
                    >
                      <Trash2 aria-hidden="true" className="h-4 w-4" />
                    </Button>
                  </fieldset>
                );
              })}
              {invalid ? (
                <p
                  className="text-xs text-destructive"
                  data-testid="crm-schedule-interval-error"
                  id="crm-schedule-interval-error"
                >
                  {invalid.error}
                </p>
              ) : null}
              {drafts.length < MAX_DAY_INTERVALS ? (
                <Button
                  data-testid="crm-schedule-add-interval"
                  onClick={() => {
                    const last = drafts.at(-1);
                    setDrafts((current) => [
                      ...current,
                      { start: last?.end ?? DEFAULT_DAY.start, end: "" },
                    ]);
                  }}
                  size="sm"
                  type="button"
                  variant="outline"
                >
                  <Plus aria-hidden="true" className="h-4 w-4" />
                  {d.addInterval}
                </Button>
              ) : null}
            </div>
          )}

          <CrmErrorAlert error={error} testId="crm-schedule-day-error" />
          <DialogFooter className="sm:justify-between">
            <Button
              data-testid="crm-schedule-toggle-day-off"
              onClick={() => {
                setDayOff((value) => !value);
                setSubmitted(false);
              }}
              type="button"
              variant="outline"
            >
              {dayOff ? d.makeWorking : d.makeDayOff}
            </Button>
            <div className="flex flex-col-reverse gap-2 sm:flex-row">
              <Button
                onClick={() => onOpenChange(false)}
                type="button"
                variant="ghost"
              >
                {ru.common.cancel}
              </Button>
              <Button
                data-testid="crm-schedule-day-submit"
                disabled={pending}
                type="submit"
              >
                {pending ? ru.common.saving : ru.common.save}
              </Button>
            </div>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
