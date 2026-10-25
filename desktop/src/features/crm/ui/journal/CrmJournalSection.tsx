import { ChevronLeft, ChevronRight, Plus } from "lucide-react";
import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { holdsTime, journalWindow } from "@/features/crm/lib/journal";
import { formatRubles } from "@/features/crm/lib/money";
import { addDays, fromIsoDate, toIsoDate } from "@/features/crm/lib/schedule";
import { useCrmAppointmentsQuery } from "@/features/crm/journalHooks";
import {
  useCrmScheduleQuery,
  useCrmServiceChoicesQuery,
  useCrmStaffQuery,
} from "@/features/crm/staffHooks";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import {
  type AppointmentDialogTarget,
  CrmAppointmentDialog,
} from "@/features/crm/ui/journal/CrmAppointmentDialog";
import {
  CrmJournalGrid,
  type JournalColumn,
} from "@/features/crm/ui/journal/CrmJournalGrid";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmAppointment } from "@/shared/api/tauriCrmAppointments";
import type { CrmStaff } from "@/shared/api/tauriCrmStaff";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { Input } from "@/shared/ui/input";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";

/** The day journal: visits of every staff member, booked from the grid. */
export function CrmJournalSection({ space }: { space: CrmMemberSpace }) {
  const j = ru.journal;
  const canBook = space.capabilities.has("post_documents");
  const today = toIsoDate(new Date());
  const [date, setDate] = React.useState(today);
  const staffQuery = useCrmStaffQuery(false);
  const scheduleQuery = useCrmScheduleQuery(date, date);
  const appointmentsQuery = useCrmAppointmentsQuery(date, date);
  const [target, setTarget] = React.useState<AppointmentDialogTarget | null>(
    null,
  );
  const [dialogOpen, setDialogOpen] = React.useState(false);
  const [session, setSession] = React.useState(0);
  const servicesQuery = useCrmServiceChoicesQuery(target !== null);
  const returnFocusRef = React.useRef<HTMLElement | null>(null);

  const staff = staffQuery.data ?? [];
  const appointments = appointmentsQuery.data ?? [];
  const columns: JournalColumn[] = React.useMemo(() => {
    const days = scheduleQuery.data ?? [];
    return (staffQuery.data ?? []).map((member) => ({
      staff: member,
      intervals:
        days.find((day) => day.staffId === member.id && day.date === date)
          ?.intervals ?? null,
      appointments: (appointmentsQuery.data ?? []).filter(
        (a) => a.staffId === member.id,
      ),
    }));
  }, [staffQuery.data, scheduleQuery.data, appointmentsQuery.data, date]);
  const hours = journalWindow(
    columns.flatMap((column) => column.intervals ?? []),
    appointments.filter((a) => a.status !== "cancelled"),
  );
  const active = appointments.filter((a) => holdsTime(a.status));
  const cancelled = appointments.length - active.length;
  const total = active.reduce((sum, a) => sum + a.totalMinor, 0);

  function open(opener: HTMLElement, next: AppointmentDialogTarget) {
    returnFocusRef.current = opener;
    setSession((value) => value + 1);
    setTarget(next);
    setDialogOpen(true);
  }

  function book(
    member: CrmStaff,
    start: number | null,
    working: boolean,
    opener: HTMLElement,
  ) {
    open(opener, {
      mode: "create",
      appointmentId: crypto.randomUUID(),
      staffId: member.id,
      date,
      start,
      outsideHours: !working,
    });
  }

  const dateLabel = fromIsoDate(date).toLocaleDateString("ru-RU", {
    weekday: "long",
    day: "numeric",
    month: "long",
  });
  const loading =
    staffQuery.isPending ||
    scheduleQuery.isPending ||
    appointmentsQuery.isPending;
  const error =
    staffQuery.error ?? scheduleQuery.error ?? appointmentsQuery.error;

  return (
    <section
      aria-labelledby="crm-journal-title"
      className="space-y-4"
      data-testid="crm-journal"
    >
      <SectionHeader
        action={
          canBook && staff.length > 0 ? (
            <Button
              data-testid="crm-journal-new"
              onClick={(event) =>
                book(staff[0], null, true, event.currentTarget)
              }
              size="sm"
            >
              <Plus aria-hidden="true" className="h-4 w-4" />
              {j.newAppointment}
            </Button>
          ) : undefined
        }
        description={canBook ? j.description : j.readOnlyHint}
        title={<span id="crm-journal-title">{j.title}</span>}
      />

      <div className="flex flex-wrap items-center gap-2">
        <Button
          aria-label={j.prevDay}
          className="h-8 w-8"
          data-testid="crm-journal-prev"
          onClick={() => setDate((value) => addDays(value, -1))}
          size="icon"
          variant="outline"
        >
          <ChevronLeft aria-hidden="true" className="h-4 w-4" />
        </Button>
        <Button
          data-testid="crm-journal-today"
          disabled={date === today}
          onClick={() => setDate(today)}
          size="sm"
          variant="outline"
        >
          {j.today}
        </Button>
        <Button
          aria-label={j.nextDay}
          className="h-8 w-8"
          data-testid="crm-journal-next"
          onClick={() => setDate((value) => addDays(value, 1))}
          size="icon"
          variant="outline"
        >
          <ChevronRight aria-hidden="true" className="h-4 w-4" />
        </Button>
        <Input
          aria-label={j.dateLabel}
          className="h-8 w-auto"
          data-testid="crm-journal-date"
          onChange={(event) => {
            if (event.target.value) setDate(event.target.value);
          }}
          type="date"
          value={date}
        />
        <span
          className="text-sm font-medium first-letter:uppercase"
          data-testid="crm-journal-date-label"
        >
          {dateLabel}
        </span>
        {!loading && !error ? (
          <span
            className="ml-auto text-xs text-muted-foreground"
            data-testid="crm-journal-summary"
          >
            {j.summary
              .replace("{count}", String(active.length))
              .replace("{total}", formatRubles(total))}
            {cancelled > 0
              ? ` · ${j.cancelledCount.replace("{count}", String(cancelled))}`
              : ""}
          </span>
        ) : null}
      </div>

      {error ? (
        <CrmErrorAlert error={error} testId="crm-journal-error" />
      ) : loading ? (
        <div aria-label={j.loading} className="space-y-2" role="status">
          <Skeleton className="h-12 w-full" />
          <Skeleton className="h-96 w-full" />
        </div>
      ) : staff.length === 0 ? (
        <EmptyState description={j.emptyHint} title={j.emptyTitle} />
      ) : (
        <CrmJournalGrid
          columns={columns}
          dateLabel={dateLabel}
          onBook={
            canBook
              ? (member, start, working, opener) =>
                  book(member, start, working, opener)
              : null
          }
          onOpen={(appointment: CrmAppointment, opener) =>
            open(opener, { mode: "edit", appointment })
          }
          window={hours}
        />
      )}

      {target ? (
        <CrmAppointmentDialog
          key={session}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            returnFocusRef.current?.focus();
          }}
          onOpenChange={setDialogOpen}
          open={dialogOpen}
          services={servicesQuery.data ?? null}
          space={space}
          staff={staff}
          target={target}
        />
      ) : null}
    </section>
  );
}
