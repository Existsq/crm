import { ArrowDownRight, ArrowRight, ArrowUpRight } from "lucide-react";
import type * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { changePercent, percent } from "@/features/crm/lib/analyticsPeriod";
import { formatRubles } from "@/features/crm/lib/money";
import type {
  CrmPaymentMethod,
  CrmRevenueBucket,
} from "@/shared/api/tauriCrmSales";
import { cn } from "@/shared/lib/cn";

export const CRM_ANALYTICS_CARD =
  "rounded-2xl border border-border/60 bg-card p-5";

/** Fixed colour per payment method (categorical slots 1–3). */
const PAYMENT_COLORS: Record<CrmPaymentMethod, string> = {
  cash: "bg-[#2a78d6] dark:bg-[#3987e5]",
  card: "bg-[#eb6834] dark:bg-[#d95926]",
  transfer: "bg-[#1baf7a] dark:bg-[#199e70]",
};

export function fillTemplate(
  template: string,
  values: Record<string, string>,
): string {
  return Object.entries(values).reduce(
    (text, [key, value]) => text.replace(`{${key}}`, value),
    template,
  );
}

export function CardTitle({
  aside,
  children,
  id,
}: {
  aside?: React.ReactNode;
  children: React.ReactNode;
  id: string;
}) {
  return (
    <div className="mb-4 flex items-baseline justify-between gap-3">
      <h3 className="text-sm font-medium" id={id}>
        {children}
      </h3>
      {aside ? (
        <p className="text-xs tabular-nums text-muted-foreground">{aside}</p>
      ) : null}
    </div>
  );
}

/** Change against the previous period: arrow + text, never colour alone. */
function Change({
  current,
  previous,
}: {
  current: number;
  previous: number | undefined;
}) {
  const a = ru.analytics;
  if (previous === undefined) {
    return <span className="invisible">{a.vsPrevious}</span>;
  }
  const change = changePercent(current, previous);
  if (change === null) {
    return <span className="text-muted-foreground">{a.noPrevious}</span>;
  }
  const Icon =
    change > 0 ? ArrowUpRight : change < 0 ? ArrowDownRight : ArrowRight;
  const spoken =
    change === 0
      ? a.changeFlat
      : (change > 0 ? a.changeUp : a.changeDown).replace(
          "{value}",
          String(Math.abs(change)),
        );
  const shown =
    change > 0 ? `+${change}%` : change < 0 ? `\u2212${-change}%` : "0%";
  return (
    <span className="flex flex-wrap items-center gap-x-1.5">
      <span
        className={cn(
          "inline-flex items-center gap-0.5 whitespace-nowrap font-medium tabular-nums",
          change > 0 && "text-emerald-700 dark:text-emerald-400",
          change < 0 && "text-red-700 dark:text-red-400",
          change === 0 && "text-muted-foreground",
        )}
      >
        <Icon aria-hidden="true" className="h-3.5 w-3.5" />
        <span aria-hidden="true">{shown}</span>
        <span className="sr-only">{spoken}</span>
      </span>
      <span className="whitespace-nowrap text-muted-foreground">
        {a.vsPrevious}
      </span>
    </span>
  );
}

export function Kpi({
  label,
  note,
  previous,
  testId,
  value,
  text,
}: {
  label: string;
  note?: string;
  previous: number | undefined;
  testId: string;
  value: number;
  text: string;
}) {
  return (
    <div className={cn(CRM_ANALYTICS_CARD, "flex flex-col gap-1")}>
      <dt className="text-xs font-medium text-muted-foreground">{label}</dt>
      <dd
        className="text-2xl font-semibold tracking-tight tabular-nums"
        data-testid={testId}
      >
        {text}
      </dd>
      <dd className="text-xs" data-testid={`${testId}-change`}>
        <Change current={value} previous={previous} />
      </dd>
      {note ? <dd className="text-xs text-muted-foreground">{note}</dd> : null}
    </div>
  );
}

/** Payment methods as one stacked bar plus a labelled legend. */
export function PaymentMix({
  buckets,
  totalMinor,
}: {
  buckets: readonly CrmRevenueBucket<CrmPaymentMethod>[];
  totalMinor: number;
}) {
  const a = ru.analytics;
  const rows = [...buckets].sort((x, y) => y.revenueMinor - x.revenueMinor);
  return (
    <div className="space-y-4" data-testid="crm-analytics-by-payment">
      <div aria-hidden="true" className="flex h-3 gap-0.5 overflow-hidden">
        {rows.map((bucket) => (
          <div
            className={cn(
              "h-full first:rounded-l-full last:rounded-r-full",
              PAYMENT_COLORS[bucket.key],
            )}
            key={bucket.key}
            style={{ flexGrow: bucket.revenueMinor, flexBasis: 0 }}
          />
        ))}
      </div>
      <ul className="space-y-2.5">
        {rows.map((bucket) => (
          <li className="flex items-center gap-3 text-sm" key={bucket.key}>
            <span
              aria-hidden="true"
              className={cn(
                "h-2.5 w-2.5 shrink-0 rounded-full",
                PAYMENT_COLORS[bucket.key],
              )}
            />
            <span className="min-w-0 flex-1">
              {ru.payments[bucket.key]}
              <span className="ml-2 text-xs text-muted-foreground">
                {a.paymentCount.replace("{count}", String(bucket.salesCount))}
              </span>
            </span>
            <span className="tabular-nums">
              {formatRubles(bucket.revenueMinor)}
            </span>
            <span className="w-10 text-right text-xs tabular-nums text-muted-foreground">
              {percent(bucket.revenueMinor, totalMinor)}%
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/**
 * A ranked table whose rows carry a thin bar scaled to the leader, so the
 * gap between first and fifth place reads at a glance.
 */
export function RankedTable<T>({
  columns,
  rows,
  testId,
  name,
  revenue,
  middle,
  rowKey,
}: {
  columns: { name: string; middle: string; revenue: string };
  rows: readonly T[];
  testId: string;
  name: (row: T) => string;
  revenue: (row: T) => number;
  middle: (row: T) => string;
  rowKey: (row: T) => string;
}) {
  const max = Math.max(1, ...rows.map(revenue));
  return (
    <table className="w-full border-collapse text-sm" data-testid={testId}>
      <thead className="text-xs text-muted-foreground">
        <tr>
          <th className="w-6 pb-2 text-left font-medium" scope="col">
            <span className="sr-only">№</span>
          </th>
          <th className="pb-2 text-left font-medium" scope="col">
            {columns.name}
          </th>
          <th className="pb-2 pl-3 text-right font-medium" scope="col">
            {columns.middle}
          </th>
          <th className="pb-2 pl-3 text-right font-medium" scope="col">
            {columns.revenue}
          </th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row, index) => (
          <tr key={rowKey(row)}>
            <td className="py-2 align-top text-xs tabular-nums text-muted-foreground">
              {index + 1}
            </td>
            <th className="py-2 text-left font-normal" scope="row">
              <span className="block truncate">{name(row)}</span>
              <span
                aria-hidden="true"
                className="mt-1.5 block h-1 rounded-full bg-muted"
              >
                <span
                  className="block h-1 rounded-full bg-[#2a78d6] dark:bg-[#3987e5]"
                  style={{ width: `${(revenue(row) / max) * 100}%` }}
                />
              </span>
            </th>
            <td className="whitespace-nowrap py-2 pl-3 text-right align-top tabular-nums text-muted-foreground">
              {middle(row)}
            </td>
            <td className="whitespace-nowrap py-2 pl-3 text-right align-top font-medium tabular-nums">
              {formatRubles(revenue(row))}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="mt-0.5 text-xl font-semibold tabular-nums">{value}</dd>
    </div>
  );
}
