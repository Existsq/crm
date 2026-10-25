import { ru } from "@/features/crm/i18n/ru";
import { holdsTime } from "@/features/crm/lib/journal";
import { formatRubles } from "@/features/crm/lib/money";
import {
  formatClock,
  fromIsoDate,
  toIsoDate,
} from "@/features/crm/lib/schedule";
import { useCrmCustomerAppointmentsQuery } from "@/features/crm/journalHooks";
import type { CrmAppointment } from "@/shared/api/tauriCrmAppointments";

/** Visits shown under the totals. */
const RECENT_VISITS = 8;

function formatDay(iso: string) {
  return fromIsoDate(iso).toLocaleDateString("ru-RU", {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

function visitLabel(visit: CrmAppointment) {
  return ru.visits.item
    .replace("{date}", formatDay(visit.date))
    .replace("{time}", formatClock(visit.start))
    .replace("{services}", visit.serviceNames.join(", "));
}

/** A customer's booked visits: counts, the next one, the latest ones. */
export function CrmPartyVisits({ customerId }: { customerId: string }) {
  const v = ru.visits;
  const query = useCrmCustomerAppointmentsQuery(customerId);
  const visits = query.data ?? [];
  const today = toIsoDate(new Date());
  const kept = visits.filter((a) => a.status !== "cancelled");
  const past = kept.filter((a) => a.date < today || a.status === "done");
  const upcoming = kept
    .filter(
      (a) => a.date >= today && holdsTime(a.status) && a.status !== "done",
    )
    .sort((a, b) => a.date.localeCompare(b.date) || a.start - b.start)[0];
  const last = [...past]
    .filter((a) => a.status !== "no_show")
    .sort((a, b) => b.date.localeCompare(a.date) || b.start - a.start)[0];
  const recent = [...visits]
    .sort((a, b) => b.date.localeCompare(a.date) || b.start - a.start)
    .slice(0, RECENT_VISITS);
  const noShows = visits.filter((a) => a.status === "no_show").length;

  if (query.isPending) return null;
  return (
    <section
      aria-labelledby="crm-party-visits-title"
      className="space-y-3 rounded-xl border border-border/70 p-3"
      data-testid="crm-party-visits"
    >
      <h3 className="text-sm font-medium" id="crm-party-visits-title">
        {v.title}
      </h3>
      {visits.length === 0 ? (
        <p className="text-sm text-muted-foreground">{v.never}</p>
      ) : (
        <>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm sm:grid-cols-4">
            <div>
              <dt className="text-xs text-muted-foreground">{v.count}</dt>
              <dd className="tabular-nums" data-testid="crm-party-visit-count">
                {past.filter((a) => a.status !== "no_show").length}
              </dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground">{v.noShows}</dt>
              <dd className="tabular-nums">{noShows}</dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground">{v.last}</dt>
              <dd>{last ? formatDay(last.date) : "—"}</dd>
            </div>
            <div>
              <dt className="text-xs text-muted-foreground">{v.next}</dt>
              <dd data-testid="crm-party-next-visit">
                {upcoming
                  ? `${formatDay(upcoming.date)}, ${formatClock(upcoming.start)}`
                  : "—"}
              </dd>
            </div>
          </dl>
          <div className="space-y-1">
            <p className="text-xs text-muted-foreground">{v.recent}</p>
            <ul className="divide-y divide-border/60 text-sm">
              {recent.map((visit) => (
                <li
                  className="flex items-center justify-between gap-3 py-1.5"
                  data-testid={`crm-party-visit-${visit.id}`}
                  key={visit.id}
                >
                  <span className="min-w-0 truncate">{visitLabel(visit)}</span>
                  <span className="shrink-0 text-xs text-muted-foreground">
                    {ru.journal.statuses[visit.status]} ·{" "}
                    <span className="tabular-nums">
                      {formatRubles(visit.totalMinor)}
                    </span>
                  </span>
                </li>
              ))}
            </ul>
          </div>
        </>
      )}
    </section>
  );
}
