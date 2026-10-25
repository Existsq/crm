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
  todayIn,
} from "@/features/crm/lib/analyticsPeriod";
import { formatQty, formatRubles } from "@/features/crm/lib/money";
import { formatCrmDate } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type {
  CrmAnalytics,
  CrmRevenueBucket,
} from "@/shared/api/tauriCrmSales";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";

const CARD = "rounded-2xl border border-border/70 p-4";

function formatDay(day: string): string {
  return formatCrmDate(`${day}T12:00:00Z`);
}

function Kpi({
  label,
  testId,
  value,
}: {
  label: string;
  testId: string;
  value: string;
}) {
  return (
    <div className={CARD}>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd
        className="mt-1 text-2xl font-semibold tabular-nums"
        data-testid={testId}
      >
        {value}
      </dd>
    </div>
  );
}

/**
 * Vertical revenue bars. Each bar is a list item with a full text label, so
 * the chart reads as a list for assistive technology.
 */
function Bars<K>({
  buckets,
  describe,
  label,
  tick,
  testId,
}: {
  buckets: readonly CrmRevenueBucket<K>[];
  describe: (bucket: CrmRevenueBucket<K>) => string;
  label: string;
  tick: (bucket: CrmRevenueBucket<K>, index: number) => string | null;
  testId: string;
}) {
  const max = Math.max(1, ...buckets.map((bucket) => bucket.revenueMinor));
  return (
    <ul
      aria-label={label}
      className="flex h-40 items-end gap-px"
      data-testid={testId}
    >
      {buckets.map((bucket, index) => {
        const height = Math.round((bucket.revenueMinor / max) * 100);
        const text = tick(bucket, index);
        return (
          <li
            aria-label={describe(bucket)}
            className="group relative flex h-full min-w-0 flex-1 flex-col justify-end"
            key={String(bucket.key)}
            title={describe(bucket)}
          >
            <div
              className="w-full rounded-t-sm bg-primary/70 transition-colors group-hover:bg-primary"
              style={{
                height: `${Math.max(height, bucket.revenueMinor > 0 ? 2 : 0)}%`,
              }}
            />
            <span
              aria-hidden="true"
              className="mt-1 h-4 truncate text-center text-2xs text-muted-foreground"
            >
              {text ?? ""}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

function ShareBar({ share }: { share: number }) {
  return (
    <div aria-hidden="true" className="h-1.5 w-full rounded-full bg-muted">
      <div
        className="h-1.5 rounded-full bg-primary/70"
        style={{ width: `${share}%` }}
      />
    </div>
  );
}

function AnalyticsBody({ data }: { data: CrmAnalytics }) {
  const a = ru.analytics;
  const { totals, customers } = data;
  const days = fillDays(data.from, data.to, data.byDay);
  const tickEvery = Math.max(1, Math.ceil(days.length / 8));
  const hours: CrmRevenueBucket<number>[] = Array.from(
    { length: 24 },
    (_, hour) =>
      data.byHour.find((bucket) => bucket.key === hour) ?? {
        key: hour,
        revenueMinor: 0,
        salesCount: 0,
      },
  );
  const fill = (template: string, values: Record<string, string>) =>
    Object.entries(values).reduce(
      (text, [key, value]) => text.replace(`{${key}}`, value),
      template,
    );

  return (
    <div className="space-y-4">
      <dl className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Kpi
          label={a.kpi.revenue}
          testId="crm-analytics-revenue"
          value={formatRubles(totals.revenueMinor)}
        />
        <Kpi
          label={a.kpi.sales}
          testId="crm-analytics-sales"
          value={String(totals.salesCount)}
        />
        <Kpi
          label={a.kpi.avgCheck}
          testId="crm-analytics-avg-check"
          value={formatRubles(totals.avgCheckMinor)}
        />
        <Kpi
          label={a.kpi.discount}
          testId="crm-analytics-discount"
          value={formatRubles(totals.discountMinor)}
        />
      </dl>

      <section aria-labelledby="crm-analytics-by-day" className={CARD}>
        <h3 className="mb-3 text-sm font-medium" id="crm-analytics-by-day">
          {a.byDay}
        </h3>
        <Bars
          buckets={days}
          describe={(bucket) =>
            fill(a.byDayLabel, {
              date: formatDay(bucket.key),
              amount: formatRubles(bucket.revenueMinor),
              count: String(bucket.salesCount),
            })
          }
          label={a.byDay}
          testId="crm-analytics-by-day-chart"
          tick={(bucket, index) =>
            index % tickEvery === 0 ? bucket.key.slice(8, 10) : null
          }
        />
      </section>

      <div className="grid gap-4 lg:grid-cols-2">
        <section aria-labelledby="crm-analytics-by-hour" className={CARD}>
          <h3 className="mb-3 text-sm font-medium" id="crm-analytics-by-hour">
            {a.byHour}
          </h3>
          <Bars
            buckets={hours}
            describe={(bucket) =>
              fill(a.byHourLabel, {
                hour: String(bucket.key).padStart(2, "0"),
                amount: formatRubles(bucket.revenueMinor),
                count: String(bucket.salesCount),
              })
            }
            label={a.byHour}
            testId="crm-analytics-by-hour-chart"
            tick={(bucket) =>
              bucket.key % 6 === 0 ? String(bucket.key) : null
            }
          />
        </section>

        <section aria-labelledby="crm-analytics-by-payment" className={CARD}>
          <h3
            className="mb-3 text-sm font-medium"
            id="crm-analytics-by-payment"
          >
            {a.byPayment}
          </h3>
          <ul className="space-y-3" data-testid="crm-analytics-by-payment">
            {data.byPayment.map((bucket) => {
              const share = percent(bucket.revenueMinor, totals.revenueMinor);
              return (
                <li className="space-y-1" key={bucket.key}>
                  <div className="flex justify-between gap-3 text-sm">
                    <span>{ru.payments[bucket.key]}</span>
                    <span className="tabular-nums">
                      {formatRubles(bucket.revenueMinor)} · {share}%
                    </span>
                  </div>
                  <ShareBar share={share} />
                </li>
              );
            })}
          </ul>
        </section>
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <section aria-labelledby="crm-analytics-top-products" className={CARD}>
          <h3
            className="mb-3 text-sm font-medium"
            id="crm-analytics-top-products"
          >
            {a.topProducts}
          </h3>
          <table
            className="w-full border-collapse text-sm"
            data-testid="crm-analytics-top-products"
          >
            <thead className="text-xs text-muted-foreground">
              <tr>
                <th className="pb-2 text-left font-medium" scope="col">
                  {a.columns.name}
                </th>
                <th className="pb-2 text-right font-medium" scope="col">
                  {a.columns.qty}
                </th>
                <th className="pb-2 text-right font-medium" scope="col">
                  {a.columns.revenue}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {data.topProducts.map((row) => (
                <tr key={row.productId}>
                  <th className="py-1.5 text-left font-normal" scope="row">
                    {row.name}
                  </th>
                  <td className="whitespace-nowrap py-1.5 text-right tabular-nums">
                    {formatQty(row.qtyMilli)} {ru.units.short[row.unit]}
                  </td>
                  <td className="whitespace-nowrap py-1.5 text-right tabular-nums">
                    {formatRubles(row.revenueMinor)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>

        <section aria-labelledby="crm-analytics-top-customers" className={CARD}>
          <h3
            className="mb-3 text-sm font-medium"
            id="crm-analytics-top-customers"
          >
            {a.topCustomers}
          </h3>
          {data.topCustomers.length === 0 ? (
            <p className="text-sm text-muted-foreground">{a.noCustomers}</p>
          ) : (
            <table
              className="w-full border-collapse text-sm"
              data-testid="crm-analytics-top-customers"
            >
              <thead className="text-xs text-muted-foreground">
                <tr>
                  <th className="pb-2 text-left font-medium" scope="col">
                    {a.columns.name}
                  </th>
                  <th className="pb-2 text-right font-medium" scope="col">
                    {a.columns.sales}
                  </th>
                  <th className="pb-2 text-right font-medium" scope="col">
                    {a.columns.revenue}
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border/60">
                {data.topCustomers.map((row) => (
                  <tr key={row.customerId}>
                    <th className="py-1.5 text-left font-normal" scope="row">
                      {row.name}
                    </th>
                    <td className="py-1.5 text-right tabular-nums">
                      {row.salesCount}
                    </td>
                    <td className="whitespace-nowrap py-1.5 text-right tabular-nums">
                      {formatRubles(row.revenueMinor)}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>
      </div>

      <section aria-labelledby="crm-analytics-customers" className={CARD}>
        <h3 className="mb-3 text-sm font-medium" id="crm-analytics-customers">
          {a.customers.title}
        </h3>
        <dl
          className="grid grid-cols-2 gap-4 text-sm sm:grid-cols-4"
          data-testid="crm-analytics-customers"
        >
          <div>
            <dt className="text-xs text-muted-foreground">
              {a.customers.buyers}
            </dt>
            <dd className="text-lg font-semibold tabular-nums">
              {customers.buyersCount}
            </dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">
              {a.customers.newBuyers}
            </dt>
            <dd className="text-lg font-semibold tabular-nums">
              {customers.newBuyersCount}
            </dd>
          </div>
          <div>
            <dt className="text-xs text-muted-foreground">
              {a.customers.repeat}
            </dt>
            <dd className="text-lg font-semibold tabular-nums">
              {customers.repeatBuyersCount}
            </dd>
          </div>
          <div className="space-y-1">
            <dt className="text-xs text-muted-foreground">
              {a.customers.identifiedShare}
            </dt>
            <dd className="text-lg font-semibold tabular-nums">
              {percent(customers.identifiedRevenueMinor, totals.revenueMinor)}%
            </dd>
            <dd className="text-xs text-muted-foreground">
              {a.customers.anonymous}:{" "}
              {formatRubles(customers.anonymousRevenueMinor)}
            </dd>
          </div>
        </dl>
      </section>
    </div>
  );
}

export function CrmAnalyticsSection({ space }: { space: CrmMemberSpace }) {
  const [preset, setPreset] = React.useState<AnalyticsPreset>("d30");
  const today = todayIn(space.settings.timezone);
  const period = presetPeriod(preset, today);
  const query = useCrmAnalyticsQuery(period);
  const a = ru.analytics;
  const data = query.data;

  return (
    <section
      aria-labelledby="crm-analytics-title"
      className="space-y-4"
      data-testid="crm-analytics"
    >
      <SectionHeader
        description={
          data
            ? a.periodRange
                .replace("{from}", formatDay(data.from))
                .replace("{to}", formatDay(data.to))
            : undefined
        }
        title={<span id="crm-analytics-title">{a.title}</span>}
      />
      <fieldset className="flex flex-wrap gap-1">
        <legend className="sr-only">{a.periodLabel}</legend>
        {ANALYTICS_PRESETS.map((item) => (
          <Button
            aria-pressed={item === preset}
            data-testid={`crm-analytics-period-${item}`}
            key={item}
            onClick={() => setPreset(item)}
            size="sm"
            type="button"
            variant={item === preset ? "default" : "outline"}
          >
            {a.periods[item]}
          </Button>
        ))}
      </fieldset>

      {query.isPending ? (
        <div
          aria-busy="true"
          aria-label={a.loading}
          className="grid grid-cols-2 gap-3 lg:grid-cols-4"
          data-testid="crm-analytics-loading"
          role="status"
        >
          <Skeleton className="h-20" />
          <Skeleton className="h-20" />
          <Skeleton className="h-20" />
          <Skeleton className="h-20" />
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
          <AnalyticsBody data={data} />
        </div>
      ) : null}
    </section>
  );
}
