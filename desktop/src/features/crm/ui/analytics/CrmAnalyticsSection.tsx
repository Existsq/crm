import { BarChart3 } from "lucide-react";
import * as React from "react";

import { useCrmAnalyticsQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import {
  ANALYTICS_PRESETS,
  type AnalyticsPreset,
  fillDays,
  percent,
  presetPeriod,
  previousPeriod,
  todayIn,
} from "@/features/crm/lib/analyticsPeriod";
import { formatQty, formatRubles } from "@/features/crm/lib/money";
import {
  CRM_ANALYTICS_CARD as CARD,
  CardTitle,
  fillTemplate,
  Kpi,
  PaymentMix,
  RankedTable,
  Stat,
} from "@/features/crm/ui/analytics/CrmAnalyticsParts";
import {
  CRM_CHART_SERIES,
  CrmBarChart,
} from "@/features/crm/ui/analytics/CrmBarChart";
import { formatCrmDate } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type {
  CrmAnalytics,
  CrmRevenueBucket,
} from "@/shared/api/tauriCrmSales";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";

function formatDay(day: string): string {
  return formatCrmDate(`${day}T12:00:00Z`);
}

/** Hours that had sales, padded to at least a working day. */
function hourBuckets(
  byHour: readonly CrmRevenueBucket<number>[],
): CrmRevenueBucket<number>[] {
  const active = byHour.filter((bucket) => bucket.salesCount > 0);
  const first = Math.min(8, ...active.map((bucket) => bucket.key));
  const last = Math.max(20, ...active.map((bucket) => bucket.key));
  return Array.from(
    { length: last - first + 1 },
    (_, index) =>
      byHour.find((bucket) => bucket.key === first + index) ?? {
        key: first + index,
        revenueMinor: 0,
        salesCount: 0,
      },
  );
}

function AnalyticsBody({
  data,
  previous,
}: {
  data: CrmAnalytics;
  previous: CrmAnalytics | undefined;
}) {
  const a = ru.analytics;
  const { totals, customers } = data;
  const prev = previous?.totals;
  const days = fillDays(data.from, data.to, data.byDay);
  const tickEvery = Math.max(1, Math.ceil(days.length / 10));
  const hours = hourBuckets(data.byHour);
  const peak = data.byHour.reduce<CrmRevenueBucket<number> | null>(
    (best, bucket) =>
      bucket.salesCount > 0 &&
      (best === null || bucket.revenueMinor > best.revenueMinor)
        ? bucket
        : best,
    null,
  );
  const salesCount = (count: number) =>
    a.salesCount.replace("{count}", String(count));
  const known = percent(customers.identifiedRevenueMinor, totals.revenueMinor);

  return (
    <div className="space-y-4">
      <dl className="grid grid-cols-2 gap-3 xl:grid-cols-4">
        <Kpi
          label={a.kpi.revenue}
          note={
            totals.discountMinor > 0
              ? a.discountNote.replace(
                  "{amount}",
                  formatRubles(totals.discountMinor),
                )
              : undefined
          }
          previous={prev?.revenueMinor}
          testId="crm-analytics-revenue"
          text={formatRubles(totals.revenueMinor)}
          value={totals.revenueMinor}
        />
        <Kpi
          label={a.kpi.sales}
          previous={prev?.salesCount}
          testId="crm-analytics-sales"
          text={String(totals.salesCount)}
          value={totals.salesCount}
        />
        <Kpi
          label={a.kpi.avgCheck}
          previous={prev?.avgCheckMinor}
          testId="crm-analytics-avg-check"
          text={formatRubles(totals.avgCheckMinor)}
          value={totals.avgCheckMinor}
        />
        <Kpi
          label={a.kpi.buyers}
          previous={previous?.customers.buyersCount}
          testId="crm-analytics-buyers"
          text={String(customers.buyersCount)}
          value={customers.buyersCount}
        />
      </dl>

      <section aria-labelledby="crm-analytics-by-day" className={CARD}>
        <CardTitle
          aside={a.periodTotal.replace(
            "{amount}",
            formatRubles(totals.revenueMinor),
          )}
          id="crm-analytics-by-day"
        >
          {a.byDay}
        </CardTitle>
        <CrmBarChart
          bars={days.map((bucket, index) => ({
            key: bucket.key,
            valueMinor: bucket.revenueMinor,
            title: formatDay(bucket.key),
            detail: `${formatRubles(bucket.revenueMinor)} · ${salesCount(bucket.salesCount)}`,
            tick: index % tickEvery === 0 ? bucket.key.slice(8, 10) : null,
          }))}
          label={a.byDay}
          testId="crm-analytics-by-day-chart"
        />
      </section>

      <div className="grid gap-4 lg:grid-cols-5">
        <section
          aria-labelledby="crm-analytics-by-hour"
          className={cn(CARD, "lg:col-span-3")}
        >
          <CardTitle
            aside={
              peak
                ? a.peakHour.replace(
                    "{hour}",
                    String(peak.key).padStart(2, "0"),
                  )
                : undefined
            }
            id="crm-analytics-by-hour"
          >
            {a.byHour}
          </CardTitle>
          <CrmBarChart
            bars={hours.map((bucket) => {
              const hour = String(bucket.key).padStart(2, "0");
              return {
                key: hour,
                valueMinor: bucket.revenueMinor,
                title: `${hour}:00–${hour}:59`,
                detail: `${formatRubles(bucket.revenueMinor)} · ${salesCount(bucket.salesCount)}`,
                tick: bucket.key % 2 === 0 ? hour : null,
              };
            })}
            label={a.byHour}
            testId="crm-analytics-by-hour-chart"
          />
        </section>

        <section
          aria-labelledby="crm-analytics-payment-title"
          className={cn(CARD, "lg:col-span-2")}
        >
          <CardTitle id="crm-analytics-payment-title">{a.byPayment}</CardTitle>
          <PaymentMix
            buckets={data.byPayment}
            totalMinor={totals.revenueMinor}
          />
        </section>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <section
          aria-labelledby="crm-analytics-top-products-title"
          className={CARD}
        >
          <CardTitle id="crm-analytics-top-products-title">
            {a.topProducts}
          </CardTitle>
          <RankedTable
            columns={{
              name: a.columns.name,
              middle: a.columns.qty,
              revenue: a.columns.revenue,
            }}
            middle={(row) =>
              `${formatQty(row.qtyMilli)} ${ru.units.short[row.unit]}`
            }
            name={(row) => row.name}
            revenue={(row) => row.revenueMinor}
            rowKey={(row) => row.productId}
            rows={data.topProducts}
            testId="crm-analytics-top-products"
          />
        </section>

        <section
          aria-labelledby="crm-analytics-top-customers-title"
          className={CARD}
        >
          <CardTitle id="crm-analytics-top-customers-title">
            {a.topCustomers}
          </CardTitle>
          {data.topCustomers.length === 0 ? (
            <p className="text-sm text-muted-foreground">{a.noCustomers}</p>
          ) : (
            <RankedTable
              columns={{
                name: a.columns.name,
                middle: a.columns.sales,
                revenue: a.columns.revenue,
              }}
              middle={(row) => String(row.salesCount)}
              name={(row) => row.name}
              revenue={(row) => row.revenueMinor}
              rowKey={(row) => row.customerId}
              rows={data.topCustomers}
              testId="crm-analytics-top-customers"
            />
          )}
        </section>
      </div>

      <section aria-labelledby="crm-analytics-customers-title" className={CARD}>
        <CardTitle id="crm-analytics-customers-title">
          {a.customers.title}
        </CardTitle>
        <dl
          className="grid grid-cols-2 gap-4 sm:grid-cols-4"
          data-testid="crm-analytics-customers"
        >
          <Stat
            label={a.customers.buyers}
            value={String(customers.buyersCount)}
          />
          <Stat
            label={a.customers.newBuyers}
            value={String(customers.newBuyersCount)}
          />
          <Stat
            label={a.customers.repeat}
            value={String(customers.repeatBuyersCount)}
          />
          <Stat
            label={a.customers.anonymous}
            value={formatRubles(customers.anonymousRevenueMinor)}
          />
        </dl>
        <div className="mt-5 space-y-2">
          <div aria-hidden="true" className="flex h-2 gap-0.5">
            <div
              className={cn("h-full rounded-l-full", CRM_CHART_SERIES)}
              style={{ flexGrow: known, flexBasis: 0 }}
            />
            <div
              className="h-full rounded-r-full bg-muted-foreground/30"
              style={{ flexGrow: 100 - known, flexBasis: 0 }}
            />
          </div>
          <p className="text-xs text-muted-foreground">
            {fillTemplate(a.identifiedSplit, {
              known: String(known),
              anonymous: String(100 - known),
            })}
          </p>
        </div>
      </section>
    </div>
  );
}

export function CrmAnalyticsSection({ space }: { space: CrmMemberSpace }) {
  const [preset, setPreset] = React.useState<AnalyticsPreset>("d30");
  const today = todayIn(space.settings.timezone);
  const period = presetPeriod(preset, today);
  const query = useCrmAnalyticsQuery(period);
  const previous = useCrmAnalyticsQuery(previousPeriod(period));
  // A placeholder from another period would compare against the wrong days.
  const previousData = previous.isPlaceholderData ? undefined : previous.data;
  const a = ru.analytics;
  const data = query.data;

  return (
    <section
      aria-labelledby="crm-analytics-title"
      className="space-y-4"
      data-testid="crm-analytics"
    >
      <SectionHeader
        action={
          <fieldset className="flex flex-wrap gap-0.5 rounded-lg bg-muted p-0.5">
            <legend className="sr-only">{a.periodLabel}</legend>
            {ANALYTICS_PRESETS.map((item) => (
              <button
                aria-pressed={item === preset}
                className={cn(
                  "rounded-md px-3 py-1 text-sm transition-colors focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
                  item === preset
                    ? "bg-background font-medium text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground",
                )}
                data-testid={`crm-analytics-period-${item}`}
                key={item}
                onClick={() => setPreset(item)}
                type="button"
              >
                {a.periods[item]}
              </button>
            ))}
          </fieldset>
        }
        className="flex-wrap"
        description={
          data
            ? a.periodRange
                .replace("{from}", formatDay(data.from))
                .replace("{to}", formatDay(data.to))
            : undefined
        }
        title={<span id="crm-analytics-title">{a.title}</span>}
      />

      {query.isPending ? (
        <div
          aria-busy="true"
          aria-label={a.loading}
          className="grid grid-cols-2 gap-3 xl:grid-cols-4"
          data-testid="crm-analytics-loading"
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
          testId="crm-analytics-error"
          title={a.loadError}
        />
      ) : data && data.totals.salesCount === 0 ? (
        <EmptyState
          description={a.emptyHint}
          icon={<BarChart3 className="h-10 w-10" />}
          testId="crm-analytics-empty"
          title={a.emptyTitle}
        />
      ) : data ? (
        <div aria-busy={query.isPlaceholderData || undefined}>
          {query.isError ? <CrmErrorAlert error={query.error} /> : null}
          <AnalyticsBody data={data} previous={previousData} />
        </div>
      ) : null}
    </section>
  );
}
