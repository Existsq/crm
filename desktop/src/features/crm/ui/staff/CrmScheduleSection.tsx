import { CalendarDays, ChevronLeft, ChevronRight, Wand2 } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import { formatDuration } from "@/features/crm/lib/catalog";
import {
  addDays,
  formatIntervals,
  fromIsoDate,
  toIsoDate,
  weekDays,
  weekStart,
  workedMinutes,
} from "@/features/crm/lib/schedule";
import {
  useCrmScheduleQuery,
  useCrmStaffQuery,
  useSetCrmScheduleMutation,
} from "@/features/crm/staffHooks";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { CrmScheduleDayDialog } from "@/features/crm/ui/staff/CrmScheduleDayDialog";
import { CrmScheduleTemplateDialog } from "@/features/crm/ui/staff/CrmScheduleTemplateDialog";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmStaff, CrmWorkInterval } from "@/shared/api/tauriCrmStaff";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";

const HEAD_CELL = "px-3 py-2.5 text-left text-xs font-medium";

function formatDate(iso: string, options: Intl.DateTimeFormatOptions) {
  return fromIsoDate(iso).toLocaleDateString("ru-RU", options);
}

type DayEdit = {
  staff: CrmStaff;
  date: string;
  /** The day as loaded; `null` when it has no schedule yet. */
  intervals: CrmWorkInterval[] | null;
};

/** Working hours of every staff member, a week at a time. */
export function CrmScheduleSection({ space }: { space: CrmMemberSpace }) {
  const canEdit = space.capabilities.has("edit_catalog");
  const today = toIsoDate(new Date());
  const [monday, setMonday] = React.useState(() => weekStart(today));
  const days = weekDays(monday);
  const sunday = days[6];
  const staffQuery = useCrmStaffQuery(false);
  const scheduleQuery = useCrmScheduleQuery(monday, sunday);
  const write = useSetCrmScheduleMutation();
  const [session, setSession] = React.useState(0);
  const [dayEdit, setDayEdit] = React.useState<DayEdit | null>(null);
  const [templateOpen, setTemplateOpen] = React.useState(false);
  const returnFocusRef = React.useRef<HTMLElement | null>(null);

  const staff = staffQuery.data ?? [];
  const byCell = React.useMemo(() => {
    const map = new Map<string, CrmWorkInterval[]>();
    for (const day of scheduleQuery.data ?? []) {
      map.set(`${day.staffId}/${day.date}`, day.intervals);
    }
    return map;
  }, [scheduleQuery.data]);

  function open(opener: HTMLElement, edit: DayEdit | null) {
    returnFocusRef.current = opener;
    write.reset();
    setSession((value) => value + 1);
    if (edit) setDayEdit(edit);
    else setTemplateOpen(true);
  }

  function close() {
    if (!write.isPending) write.reset();
    setDayEdit(null);
    setTemplateOpen(false);
  }

  async function save(
    staffId: string,
    scheduleDays: Array<{ date: string; intervals: CrmWorkInterval[] }>,
    message: string,
  ) {
    try {
      await write.mutateAsync({
        channelId: space.channelId,
        staffId,
        days: scheduleDays,
      });
      toast.success(message);
      setDayEdit(null);
      setTemplateOpen(false);
    } catch {
      // Rendered in the dialog through the mutation error state.
    }
  }

  const restoreFocus = (event: Event) => {
    event.preventDefault();
    returnFocusRef.current?.focus();
  };
  const rangeLabel = `${formatDate(monday, { day: "numeric", month: "long" })} – ${formatDate(sunday, { day: "numeric", month: "long", year: "numeric" })}`;
  const loading = staffQuery.isPending || scheduleQuery.isPending;
  const failed = staffQuery.isError || scheduleQuery.isError;

  return (
    <section
      aria-labelledby="crm-schedule-title"
      className="space-y-4"
      data-testid="crm-schedule"
    >
      <SectionHeader
        action={
          canEdit && staff.length > 0 ? (
            <Button
              data-testid="crm-schedule-fill"
              onClick={(event) => open(event.currentTarget, null)}
              size="sm"
            >
              <Wand2 aria-hidden="true" className="h-4 w-4" />
              {ru.schedule.fill}
            </Button>
          ) : undefined
        }
        description={
          canEdit ? ru.schedule.description : ru.schedule.readOnlyHint
        }
        title={<span id="crm-schedule-title">{ru.schedule.title}</span>}
      />

      <div className="flex flex-wrap items-center gap-2">
        <Button
          aria-label={ru.schedule.prevWeek}
          className="h-8 w-8"
          data-testid="crm-schedule-prev"
          onClick={() => setMonday((value) => addDays(value, -7))}
          size="icon"
          variant="outline"
        >
          <ChevronLeft aria-hidden="true" className="h-4 w-4" />
        </Button>
        <Button
          aria-label={ru.schedule.nextWeek}
          className="h-8 w-8"
          data-testid="crm-schedule-next"
          onClick={() => setMonday((value) => addDays(value, 7))}
          size="icon"
          variant="outline"
        >
          <ChevronRight aria-hidden="true" className="h-4 w-4" />
        </Button>
        <Button
          disabled={monday === weekStart(today)}
          onClick={() => setMonday(weekStart(today))}
          size="sm"
          variant="ghost"
        >
          {ru.schedule.thisWeek}
        </Button>
        <p
          aria-live="polite"
          className="text-sm font-medium tabular-nums"
          data-testid="crm-schedule-range"
        >
          {rangeLabel}
        </p>
      </div>

      {loading ? (
        <div
          aria-busy="true"
          aria-label={ru.schedule.loading}
          className="space-y-2"
          role="status"
        >
          <Skeleton className="h-12 w-full" />
          <Skeleton className="h-12 w-full" />
        </div>
      ) : failed ? (
        <EmptyState
          action={
            <Button
              onClick={() => {
                void staffQuery.refetch();
                void scheduleQuery.refetch();
              }}
              size="sm"
              variant="outline"
            >
              {ru.common.retry}
            </Button>
          }
          testId="crm-schedule-error"
          title={ru.schedule.loadError}
        />
      ) : staff.length === 0 ? (
        <EmptyState
          description={ru.schedule.emptyHint}
          icon={<CalendarDays className="h-10 w-10" />}
          testId="crm-schedule-empty"
          title={ru.schedule.emptyTitle}
        />
      ) : (
        <div className="overflow-x-auto rounded-2xl border border-border/70">
          <table
            className="w-full min-w-[56rem] table-fixed border-collapse text-sm"
            data-testid="crm-schedule-table"
          >
            <caption className="sr-only">
              {ru.schedule.tableLabel
                .replace(
                  "{from}",
                  formatDate(monday, { day: "numeric", month: "long" }),
                )
                .replace(
                  "{to}",
                  formatDate(sunday, { day: "numeric", month: "long" }),
                )}
            </caption>
            <thead className="bg-muted/40 text-muted-foreground">
              <tr>
                <th className={cn(HEAD_CELL, "w-44")} scope="col">
                  {ru.schedule.staffColumn}
                </th>
                {days.map((date, index) => (
                  <th
                    className={cn(
                      HEAD_CELL,
                      date === today && "text-foreground",
                      index >= 5 && "bg-muted/40",
                    )}
                    key={date}
                    scope="col"
                  >
                    <abbr
                      className="no-underline"
                      title={ru.schedule.weekdaysLong[index]}
                    >
                      {ru.schedule.weekdaysShort[index]}
                    </abbr>{" "}
                    <span
                      className={cn(
                        "tabular-nums",
                        date === today &&
                          "rounded-full bg-primary px-1.5 py-0.5 text-primary-foreground",
                      )}
                    >
                      {formatDate(date, { day: "numeric" })}
                    </span>
                    {date === today ? (
                      <span className="sr-only">, {ru.schedule.today}</span>
                    ) : null}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {staff.map((member) => {
                const total = days.reduce(
                  (sum, date) =>
                    sum +
                    workedMinutes(byCell.get(`${member.id}/${date}`) ?? []),
                  0,
                );
                return (
                  <tr key={member.id}>
                    <th
                      className="px-3 py-2 text-left align-top font-normal"
                      scope="row"
                    >
                      <span className="block truncate font-medium">
                        {member.name}
                      </span>
                      {member.position ? (
                        <span className="block truncate text-xs text-muted-foreground">
                          {member.position}
                        </span>
                      ) : null}
                      <span className="block text-xs tabular-nums text-muted-foreground">
                        {ru.schedule.hoursTotal.replace(
                          "{hours}",
                          formatDuration(total),
                        )}
                      </span>
                    </th>
                    {days.map((date, index) => {
                      const intervals =
                        byCell.get(`${member.id}/${date}`) ?? null;
                      const dateLabel = formatDate(date, {
                        weekday: "long",
                        day: "numeric",
                        month: "long",
                      });
                      const content =
                        intervals === null ? (
                          <span className="text-muted-foreground/70">
                            {ru.schedule.notSet}
                          </span>
                        ) : intervals.length === 0 ? (
                          <span className="text-muted-foreground">
                            {ru.schedule.dayOff}
                          </span>
                        ) : (
                          intervals.map((interval) => (
                            <span
                              className="block whitespace-nowrap font-medium tabular-nums"
                              key={interval.start}
                            >
                              {formatIntervals([interval])}
                            </span>
                          ))
                        );
                      const testId = `crm-schedule-cell-${member.id}-${date}`;
                      return (
                        <td
                          className={cn(
                            "p-1 align-top",
                            index >= 5 && "bg-muted/20",
                          )}
                          key={date}
                        >
                          {canEdit ? (
                            <button
                              aria-label={`${ru.schedule.editDay
                                .replace("{name}", member.name)
                                .replace("{date}", dateLabel)}: ${
                                intervals === null
                                  ? ru.schedule.notSet
                                  : intervals.length === 0
                                    ? ru.schedule.dayOff
                                    : formatIntervals(intervals)
                              }`}
                              className={cn(
                                "flex min-h-12 w-full flex-col items-start justify-center rounded-lg px-2 py-1.5 text-left text-xs transition-colors hover:bg-muted focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
                                intervals !== null &&
                                  intervals.length > 0 &&
                                  "bg-[#2a78d6]/10 dark:bg-[#3987e5]/15",
                              )}
                              data-testid={testId}
                              onClick={(event) =>
                                open(event.currentTarget, {
                                  staff: member,
                                  date,
                                  intervals,
                                })
                              }
                              type="button"
                            >
                              {content}
                            </button>
                          ) : (
                            <div
                              className="flex min-h-12 flex-col justify-center px-2 py-1.5 text-xs"
                              data-testid={testId}
                            >
                              {content}
                            </div>
                          )}
                        </td>
                      );
                    })}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {dayEdit === null && !templateOpen ? (
        <CrmErrorAlert error={write.error} testId="crm-schedule-write-error" />
      ) : null}

      {canEdit && dayEdit ? (
        <CrmScheduleDayDialog
          dateLabel={formatDate(dayEdit.date, {
            weekday: "long",
            day: "numeric",
            month: "long",
            year: "numeric",
          })}
          error={write.error}
          intervals={dayEdit.intervals}
          key={session}
          onCloseAutoFocus={restoreFocus}
          onOpenChange={(value) => {
            if (!value) close();
          }}
          onSubmit={(intervals) =>
            void save(
              dayEdit.staff.id,
              [{ date: dayEdit.date, intervals }],
              ru.schedule.day.saved,
            )
          }
          open
          pending={write.isPending}
          staffName={dayEdit.staff.name}
        />
      ) : null}

      {canEdit && templateOpen ? (
        <CrmScheduleTemplateDialog
          defaultFrom={monday}
          error={write.error}
          key={session}
          onCloseAutoFocus={restoreFocus}
          onOpenChange={(value) => {
            if (!value) close();
          }}
          onSubmit={(staffId, scheduleDays) =>
            void save(staffId, scheduleDays, ru.schedule.template.saved)
          }
          open
          pending={write.isPending}
          staff={staff}
        />
      ) : null}
    </section>
  );
}
