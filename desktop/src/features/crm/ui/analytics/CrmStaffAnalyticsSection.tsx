import { TrendingUp } from "lucide-react";
import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import {
  ANALYTICS_PRESETS,
  type AnalyticsPreset,
  percent,
  presetPeriod,
  previousPeriod,
  todayIn,
} from "@/features/crm/lib/analyticsPeriod";
import { formatDuration } from "@/features/crm/lib/catalog";
import { formatRubles } from "@/features/crm/lib/money";
import { useCrmVisitAnalyticsQuery } from "@/features/crm/journalHooks";
import {
  CRM_ANALYTICS_CARD as CARD,
  CardTitle,
  fillTemplate,
  Kpi,
  RankedTable,
} from "@/features/crm/ui/analytics/CrmAnalyticsParts";
import { formatCrmDate } from "@/features/crm/ui/CrmField";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type {
  CrmStaffVisitStats,
  CrmVisitAnalytics,
} from "@/shared/api/tauriCrmAppointments";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";

const NUM = "whitespace-nowrap py-2 pl-3 text-right tabular-nums";

function formatDay(day: string): string {
  return formatCrmDate(`${day}T12:00:00Z`);
}

function totalsOf(data: CrmVisitAnalytics) {
  const sum = (pick: (s: CrmStaffVisitStats) => number) =>
    data.staff.reduce((total, s) => total + pick(s), 0);
  return {
    visits: sum((s) => s.visits),
    done: sum((s) => s.done),
    noShows: sum((s) => s.noShows),
    revenueMinor: sum((s) => s.revenueMinor),
    bookedMin: sum((s) => s.bookedMin),
    workedMin: sum((s) => s.workedMin),
  };
}

function StaffTable({ staff }: { staff: readonly CrmStaffVisitStats[] }) {
  const c = ru.staffAnalytics.columns;
  return (
    <div className="overflow-x-auto">
      <table
        aria-label={ru.staffAnalytics.staffTable}
        className="w-full min-w-[36rem] border-collapse text-sm"
        data-testid="crm-staff-analytics-table"
      >
        <thead className="text-xs text-muted-foreground">
          <tr>
            <th className="pb-2 text-left font-medium" scope="col">
              {c.staff}
            </th>
            <th className="pb-2 pl-3 text-right font-medium" scope="col">
              {c.visits}
            </th>
            <th className="pb-2 pl-3 text-right font-medium" scope="col">
              {c.noShows}
            </th>
            <th className="w-40 pb-2 pl-3 text-left font-medium" scope="col">
              {c.utilisation}
            </th>
            <th className="pb-2 pl-3 text-right font-medium" scope="col">
              {c.avgCheck}
            </th>
            <th className="pb-2 pl-3 text-right font-medium" scope="col">
              {c.revenue}
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-border/50">
          {staff.map((s) => {
            const load = percent(s.bookedMin, s.workedMin);
            return (
              <tr
                data-testid={`crm-staff-analytics-${s.staffId}`}
                key={s.staffId}
              >
                <th className="py-2 text-left font-normal" scope="row">
                  {s.name}
                </th>
                <td className={NUM}>{s.visits}</td>
                <td className={cn(NUM, "text-muted-foreground")}>
                  {s.noShows}
                </td>
                <td className="py-2 pl-3">
                  {s.workedMin > 0 ? (
                    <span className="flex items-center gap-2">
                      <span
                        aria-hidden="true"
                        className="block h-1.5 flex-1 rounded-full bg-muted"
                      >
                        <span
                          className="block h-1.5 rounded-full bg-[#2a78d6] dark:bg-[#3987e5]"
                          style={{ width: `${Math.min(100, load)}%` }}
                        />
                      </span>
                      <span className="w-10 text-right tabular-nums">
                        {load}%
                      </span>
                    </span>
                  ) : (
                    <span className="text-xs text-muted-foreground">
                      {ru.staffAnalytics.noSchedule}
                    </span>
                  )}
                </td>
                <td className={cn(NUM, "text-muted-foreground")}>
                  {s.done > 0
                    ? formatRubles(Math.round(s.revenueMinor / s.done))
                    : "—"}
                </td>
                <td className={cn(NUM, "font-medium")}>
                  {formatRubles(s.revenueMinor)}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function AnalyticsBody({
  data,
  previous,
}: {
  data: CrmVisitAnalytics;
  previous: CrmVisitAnalytics | undefined;
}) {
  const a = ru.staffAnalytics;
  const totals = totalsOf(data);
  const prev = previous ? totalsOf(previous) : undefined;
  const load = percent(totals.bookedMin, totals.workedMin);
  return (
    <div className="space-y-4">
      <dl className="grid grid-cols-2 gap-3 xl:grid-cols-4">
        <Kpi
          label={a.kpi.visits}
          previous={prev?.visits}
          testId="crm-staff-analytics-visits"
          text={String(totals.visits)}
          value={totals.visits}
        />
        <Kpi
          label={a.kpi.revenue}
          note={a.revenueNote}
          previous={prev?.revenueMinor}
          testId="crm-staff-analytics-revenue"
          text={formatRubles(totals.revenueMinor)}
          value={totals.revenueMinor}
        />
        <Kpi
          label={a.kpi.utilisation}
          note={
            totals.workedMin > 0
              ? fillTemplate(a.utilisationNote, {
                  booked: formatDuration(totals.bookedMin),
                  worked: formatDuration(totals.workedMin),
                })
              : undefined
          }
          previous={prev ? percent(prev.bookedMin, prev.workedMin) : undefined}
          testId="crm-staff-analytics-utilisation"
          text={`${load}%`}
          value={load}
        />
        <Kpi
          label={a.kpi.newClients}
          note={fillTemplate(a.newClientsNote, {
            clients: String(data.clients.clients),
            walkIns: String(data.clients.walkIns),
          })}
          previous={previous?.clients.newClients}
          testId="crm-staff-analytics-new-clients"
          text={String(data.clients.newClients)}
          value={data.clients.newClients}
        />
      </dl>

      <section
        aria-labelledby="crm-staff-analytics-staff-title"
        className={CARD}
      >
        <CardTitle
          aside={
            totals.noShows > 0
              ? `${a.kpi.noShows}: ${totals.noShows} · ${fillTemplate(
                  a.noShowNote,
                  {
                    percent: String(
                      percent(totals.noShows, totals.visits + totals.noShows),
                    ),
                  },
                )}`
              : undefined
          }
          id="crm-staff-analytics-staff-title"
        >
          {a.staffTitle}
        </CardTitle>
        <StaffTable staff={data.staff} />
      </section>

      {data.services.length > 0 ? (
        <section
          aria-labelledby="crm-staff-analytics-services-title"
          className={CARD}
        >
          <CardTitle id="crm-staff-analytics-services-title">
            {a.servicesTitle}
          </CardTitle>
          <RankedTable
            columns={{
              name: a.columns.service,
              middle: a.columns.booked,
              revenue: a.columns.revenue,
            }}
            middle={(row) => String(row.visits)}
            name={(row) => row.name}
            revenue={(row) => row.revenueMinor}
            rowKey={(row) => row.productId}
            rows={data.services}
            testId="crm-staff-analytics-services"
          />
        </section>
      ) : null}
    </div>
  );
}

/** Staff and service figures from the appointment journal. */
export function CrmStaffAnalyticsSection({ space }: { space: CrmMemberSpace }) {
  const [preset, setPreset] = React.useState<AnalyticsPreset>("d30");
  const today = todayIn(space.settings.timezone);
  const period = presetPeriod(preset, today);
  const query = useCrmVisitAnalyticsQuery(period);
  const previous = useCrmVisitAnalyticsQuery(previousPeriod(period));
  const previousData = previous.isPlaceholderData ? undefined : previous.data;
  const a = ru.staffAnalytics;
  const data = query.data;
  const empty =
    data !== undefined &&
    data.staff.every((s) => s.visits === 0 && s.noShows === 0);

  return (
    <section
      aria-labelledby="crm-staff-analytics-title"
      className="space-y-4"
      data-testid="crm-staff-analytics"
    >
      <SectionHeader
        action={
          <fieldset className="flex flex-wrap gap-0.5 rounded-lg bg-muted p-0.5">
            <legend className="sr-only">{ru.analytics.periodLabel}</legend>
            {ANALYTICS_PRESETS.map((item) => (
              <button
                aria-pressed={item === preset}
                className={cn(
                  "rounded-md px-3 py-1 text-sm transition-colors focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
                  item === preset
                    ? "bg-background font-medium text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground",
                )}
                data-testid={`crm-staff-analytics-period-${item}`}
                key={item}
                onClick={() => setPreset(item)}
                type="button"
              >
                {ru.analytics.periods[item]}
              </button>
            ))}
          </fieldset>
        }
        className="flex-wrap"
        description={
          data
            ? ru.analytics.periodRange
                .replace("{from}", formatDay(data.from))
                .replace("{to}", formatDay(data.to))
            : undefined
        }
        title={<span id="crm-staff-analytics-title">{a.title}</span>}
      />

      {query.isPending ? (
        <div
          aria-busy="true"
          aria-label={a.loading}
          className="grid grid-cols-2 gap-3 xl:grid-cols-4"
          role="status"
        >
          <Skeleton className="h-28" />
          <Skeleton className="h-28" />
          <Skeleton className="h-28" />
          <Skeleton className="h-28" />
        </div>
      ) : query.isError && !data ? (
        <EmptyState
          action={
            <Button
              onClick={() => void query.refetch()}
              size="sm"
              variant="outline"
            >
              {ru.common.retry}
            </Button>
          }
          testId="crm-staff-analytics-error"
          title={a.loadError}
        />
      ) : empty ? (
        <EmptyState
          description={a.emptyHint}
          icon={<TrendingUp className="h-10 w-10" />}
          testId="crm-staff-analytics-empty"
          title={a.emptyTitle}
        />
      ) : data ? (
        <div aria-busy={query.isPlaceholderData || undefined}>
          <AnalyticsBody data={data} previous={previousData} />
        </div>
      ) : null}
    </section>
  );
}
