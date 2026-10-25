import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { deliveryLogSince, ruleKeyLabel } from "@/features/crm/lib/reminders";
import { useCrmReminderDeliveriesQuery } from "@/features/crm/reminderHooks";
import { formatCrmDateTime } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type {
  CrmDeliveryStatus,
  CrmReminderDelivery,
} from "@/shared/api/tauriCrmReminders";
import { Badge, type BadgeProps } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";

const log = ru.reminders.log;
const HEAD_CELL = "px-4 py-2 text-left text-xs font-medium";

const STATUS_VARIANT: Record<CrmDeliveryStatus, BadgeProps["variant"]> = {
  pending: "info",
  sent: "success",
  failed: "destructive",
  skipped: "secondary",
  cancelled: "outline",
};

function DeliveryRow({ delivery }: { delivery: CrmReminderDelivery }) {
  const [expanded, setExpanded] = React.useState(false);
  const textId = `crm-delivery-text-${delivery.id}`;
  return (
    <tr className="align-top" data-testid={`crm-delivery-${delivery.id}`}>
      <td className="whitespace-nowrap px-4 py-2.5 tabular-nums">
        {formatCrmDateTime(delivery.sentAt ?? delivery.createdAt)}
      </td>
      <th className="px-4 py-2.5 text-left font-normal" scope="row">
        {delivery.clientName}
      </th>
      <td className="px-4 py-2.5">
        <div>{ruleKeyLabel(delivery.ruleKey)}</div>
        <div className="text-xs text-muted-foreground">
          {log.visit.replace("{value}", formatCrmDateTime(delivery.visitAt))}
        </div>
      </td>
      <td className="px-4 py-2.5">
        <div className="flex flex-wrap items-center gap-1.5">
          <Badge
            data-testid={`crm-delivery-status-${delivery.id}`}
            variant={STATUS_VARIANT[delivery.status]}
          >
            {log.statuses[delivery.status]}
          </Badge>
          {delivery.transport === "sandbox" ? (
            <Badge variant="outline">{log.sandboxBadge}</Badge>
          ) : null}
        </div>
        {delivery.reason ? (
          <p className="mt-1 text-xs text-muted-foreground">
            {log.reasons[delivery.reason]}
          </p>
        ) : null}
        {delivery.lastError ? (
          <p className="mt-1 break-words text-xs text-destructive">
            {delivery.lastError}
          </p>
        ) : null}
        {delivery.attempts > 1 ? (
          <p className="mt-1 text-xs text-muted-foreground">
            {log.attempts.replace("{value}", String(delivery.attempts))}
          </p>
        ) : null}
      </td>
      <td className="max-w-[24rem] px-4 py-2.5">
        <Button
          aria-controls={textId}
          aria-expanded={expanded}
          className="h-auto px-0 py-0"
          onClick={() => setExpanded((value) => !value)}
          size="sm"
          type="button"
          variant="link"
        >
          {expanded ? log.hideText : log.showText}
        </Button>
        <p
          className={
            expanded
              ? "mt-1 whitespace-pre-wrap text-sm"
              : "mt-1 truncate text-sm text-muted-foreground"
          }
          id={textId}
        >
          {delivery.text}
        </p>
      </td>
    </tr>
  );
}

/** Messages planned in the last seven days, newest first. */
export function CrmReminderLog() {
  const [since] = React.useState(() => deliveryLogSince(new Date()));
  const query = useCrmReminderDeliveriesQuery(since);
  const deliveries = query.data ?? [];

  return (
    <section
      aria-labelledby="crm-reminder-log-title"
      className="space-y-4"
      data-testid="crm-reminder-log"
    >
      <SectionHeader
        description={log.description}
        title={<span id="crm-reminder-log-title">{log.title}</span>}
      />
      {query.isPending ? (
        <div aria-label={log.loading} className="space-y-2" role="status">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : query.isError ? (
        <div className="space-y-2">
          <p className="text-sm text-destructive">{log.loadError}</p>
          <CrmErrorAlert error={query.error} />
          <Button onClick={() => void query.refetch()} variant="outline">
            {ru.common.retry}
          </Button>
        </div>
      ) : deliveries.length === 0 ? (
        <EmptyState
          description={log.emptyHint}
          testId="crm-reminder-log-empty"
          title={log.emptyTitle}
        />
      ) : (
        <div
          aria-busy={query.isFetching || undefined}
          className="overflow-x-auto rounded-2xl border border-border/70"
        >
          <table className="w-full min-w-[48rem] border-collapse text-sm">
            <caption className="sr-only">{log.tableLabel}</caption>
            <thead className="bg-muted/40 text-muted-foreground">
              <tr>
                <th className={HEAD_CELL} scope="col">
                  {log.columns.time}
                </th>
                <th className={HEAD_CELL} scope="col">
                  {log.columns.client}
                </th>
                <th className={HEAD_CELL} scope="col">
                  {log.columns.rule}
                </th>
                <th className={HEAD_CELL} scope="col">
                  {log.columns.status}
                </th>
                <th className={HEAD_CELL} scope="col">
                  {log.columns.text}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {deliveries.map((delivery) => (
                <DeliveryRow delivery={delivery} key={delivery.id} />
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
