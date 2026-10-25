import { ru } from "@/features/crm/i18n/ru";
import {
  assignLanes,
  clientLabel,
  holdsTime,
  isWorking,
  JOURNAL_ROW_MIN,
  journalRows,
} from "@/features/crm/lib/journal";
import { formatClock } from "@/features/crm/lib/schedule";
import type {
  CrmAppointment,
  CrmAppointmentStatus,
} from "@/shared/api/tauriCrmAppointments";
import type { CrmStaff, CrmWorkInterval } from "@/shared/api/tauriCrmStaff";
import { cn } from "@/shared/lib/cn";

/** Height of one grid row, in rem so the grid follows zoom. */
const ROW_REM = 2.5;

const STATUS_STYLES: Record<CrmAppointmentStatus, string> = {
  booked: "border-sky-500/60 bg-sky-500/10",
  confirmed: "border-indigo-500/60 bg-indigo-500/10",
  arrived: "border-amber-500/70 bg-amber-500/15",
  done: "border-emerald-500/60 bg-emerald-500/10",
  no_show: "border-dashed border-rose-500/60 bg-rose-500/5 opacity-70",
  cancelled: "border-dashed border-border bg-muted/40 opacity-60",
};

export type JournalColumn = {
  staff: CrmStaff;
  /** `null` when the day has no schedule. */
  intervals: CrmWorkInterval[] | null;
  appointments: CrmAppointment[];
};

function rem(minutes: number): string {
  return `${(minutes / JOURNAL_ROW_MIN) * ROW_REM}rem`;
}

function AppointmentCard({
  appointment,
  lane,
  lanes,
  onOpen,
  windowStart,
}: {
  appointment: CrmAppointment;
  lane: number;
  lanes: number;
  onOpen: (appointment: CrmAppointment, opener: HTMLElement) => void;
  windowStart: number;
}) {
  const j = ru.journal;
  const time = `${formatClock(appointment.start)}–${formatClock(appointment.end)}`;
  const client = clientLabel(appointment);
  const services = appointment.serviceNames.join(", ");
  return (
    <button
      aria-label={j.openAppointment
        .replace("{time}", time)
        .replace("{client}", client)
        .replace("{services}", services)}
      className={cn(
        "absolute z-10 overflow-hidden rounded-md border-l-4 border px-2 py-1 text-left text-xs shadow-xs transition-colors hover:brightness-95 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
        STATUS_STYLES[appointment.status],
      )}
      data-status={appointment.status}
      data-testid={`crm-journal-appointment-${appointment.id}`}
      onClick={(event) => onOpen(appointment, event.currentTarget)}
      style={{
        top: rem(appointment.start - windowStart),
        height: rem(appointment.end - appointment.start),
        left: `calc(${(lane / lanes) * 100}% + 2px)`,
        width: `calc(${100 / lanes}% - 4px)`,
      }}
      type="button"
    >
      <span className="flex items-center justify-between gap-1 font-medium tabular-nums">
        <span className="truncate">{time}</span>
        <span className="shrink-0 text-2xs font-normal text-muted-foreground">
          {j.statuses[appointment.status]}
        </span>
      </span>
      <span className="block truncate font-medium">{client}</span>
      <span className="block truncate text-muted-foreground">{services}</span>
      {appointment.outsideHours ||
      appointment.source === "online" ||
      appointment.saleId ? (
        <span className="block truncate text-2xs text-muted-foreground">
          {[
            appointment.saleId ? j.paidBadge : null,
            appointment.source === "online" ? j.onlineBadge : null,
            appointment.outsideHours ? j.outsideHoursBadge : null,
          ]
            .filter(Boolean)
            .join(" · ")}
        </span>
      ) : null}
    </button>
  );
}

/**
 * One day of the journal: a column per staff member, working time light,
 * the rest shaded. Empty rows are buttons that book at that time when
 * `onBook` is given.
 */
export function CrmJournalGrid({
  columns,
  dateLabel,
  onBook,
  onOpen,
  window,
}: {
  columns: readonly JournalColumn[];
  dateLabel: string;
  onBook:
    | ((
        staff: CrmStaff,
        start: number,
        working: boolean,
        opener: HTMLElement,
      ) => void)
    | null;
  onOpen: (appointment: CrmAppointment, opener: HTMLElement) => void;
  window: { start: number; end: number };
}) {
  const j = ru.journal;
  const rows = journalRows(window);
  const height = rem(window.end - window.start);
  return (
    <section
      aria-label={j.gridLabel.replace("{date}", dateLabel)}
      className="overflow-x-auto rounded-xl border border-border/70 bg-card"
      data-testid="crm-journal-grid"
    >
      <div className="flex min-w-max">
        <div
          aria-hidden="true"
          className="sticky left-0 z-20 w-14 shrink-0 border-r border-border/70 bg-card"
        >
          <div className="h-12 border-b border-border/70" />
          <div className="relative" style={{ height }}>
            {rows.map((row) => (
              <div
                className="absolute right-2 -translate-y-1/2 text-2xs tabular-nums text-muted-foreground first:translate-y-0"
                key={row}
                style={{ top: rem(row - window.start) }}
              >
                {row % 60 === 0 ? formatClock(row) : ""}
              </div>
            ))}
          </div>
        </div>
        {columns.map((column) => {
          const intervals = column.intervals ?? [];
          const visible = column.appointments.filter(
            (a) => holdsTime(a.status) || a.status === "no_show",
          );
          const status =
            column.intervals === null
              ? j.notScheduled
              : intervals.length === 0
                ? j.dayOff
                : null;
          return (
            <section
              aria-label={column.staff.name}
              className="w-44 shrink-0 border-r border-border/70 last:border-r-0 sm:w-auto sm:min-w-44 sm:flex-1"
              data-testid={`crm-journal-column-${column.staff.id}`}
              key={column.staff.id}
            >
              <header className="flex h-12 flex-col justify-center border-b border-border/70 px-3">
                <span className="truncate text-sm font-medium">
                  {column.staff.name}
                </span>
                <span className="truncate text-2xs text-muted-foreground">
                  {status ?? column.staff.position ?? ""}
                </span>
              </header>
              <div className="relative" style={{ height }}>
                {rows.map((row) => {
                  const end = row + JOURNAL_ROW_MIN;
                  const working = isWorking(intervals, row, end);
                  const cell = cn(
                    "absolute inset-x-0 border-b border-border/40",
                    row % 60 === 0 ? "" : "border-dashed",
                    working ? "bg-background" : "bg-muted/50",
                  );
                  const style = {
                    top: rem(row - window.start),
                    height: rem(JOURNAL_ROW_MIN),
                  };
                  return onBook ? (
                    <button
                      aria-label={j.bookAt
                        .replace("{name}", column.staff.name)
                        .replace("{time}", formatClock(row))}
                      className={cn(
                        cell,
                        "hover:bg-accent focus-visible:z-10 focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring",
                      )}
                      data-testid={`crm-journal-cell-${column.staff.id}-${formatClock(row)}`}
                      data-working={working || undefined}
                      key={row}
                      onClick={(event) =>
                        onBook(column.staff, row, working, event.currentTarget)
                      }
                      style={style}
                      type="button"
                    />
                  ) : (
                    <div
                      aria-hidden="true"
                      className={cell}
                      key={row}
                      style={style}
                    />
                  );
                })}
                {assignLanes(visible).map(({ item, lane, lanes }) => (
                  <AppointmentCard
                    appointment={item}
                    key={item.id}
                    lane={lane}
                    lanes={lanes}
                    onOpen={onOpen}
                    windowStart={window.start}
                  />
                ))}
              </div>
            </section>
          );
        })}
      </div>
    </section>
  );
}
