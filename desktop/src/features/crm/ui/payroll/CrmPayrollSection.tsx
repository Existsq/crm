import { Wallet } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import { describeCrmError } from "@/features/crm/lib/crmErrors";
import { todayIn } from "@/features/crm/lib/analyticsPeriod";
import { formatRubles } from "@/features/crm/lib/money";
import {
  formatPercentBp,
  PAYROLL_PRESETS,
  type PayrollPreset,
  payrollPeriod,
} from "@/features/crm/lib/payroll";
import {
  useCrmPayrollQuery,
  useRecordCrmPayoutMutation,
  useSetCrmPayRatesMutation,
  useVoidCrmPayoutMutation,
} from "@/features/crm/payrollHooks";
import {
  CRM_ANALYTICS_CARD as CARD,
  CardTitle,
  fillTemplate,
} from "@/features/crm/ui/analytics/CrmAnalyticsParts";
import { formatCrmDate } from "@/features/crm/ui/CrmField";
import {
  CrmPayoutDialog,
  CrmPayRatesDialog,
} from "@/features/crm/ui/payroll/CrmPayrollDialogs";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmPayout, CrmStaffPayroll } from "@/shared/api/tauriCrmPayroll";
import { cn } from "@/shared/lib/cn";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";

const NUM = "whitespace-nowrap py-2 pl-3 text-right tabular-nums";
const HEAD_NUM = "pb-2 pl-3 text-right font-medium";

function formatDay(day: string): string {
  return formatCrmDate(`${day}T12:00:00Z`);
}

function ratesText(row: CrmStaffPayroll): string {
  const p = ru.payroll;
  const parts = [
    row.percentBp > 0 &&
      fillTemplate(p.ratePercent, { percent: formatPercentBp(row.percentBp) }),
    row.perVisitMinor > 0 &&
      fillTemplate(p.ratePerVisit, { amount: formatRubles(row.perVisitMinor) }),
    row.shiftMinor > 0 &&
      fillTemplate(p.rateShift, { amount: formatRubles(row.shiftMinor) }),
  ].filter(Boolean);
  return parts.length > 0 ? parts.join(" · ") : p.noRates;
}

type Editing =
  | { kind: "rates"; row: CrmStaffPayroll; key: string }
  | { kind: "payout"; row: CrmStaffPayroll; key: string; payoutId: string };

function PayrollTable({
  onEditRates,
  onPay,
  staff,
}: {
  onEditRates: (row: CrmStaffPayroll) => void;
  onPay: (row: CrmStaffPayroll) => void;
  staff: readonly CrmStaffPayroll[];
}) {
  const p = ru.payroll;
  const c = p.columns;
  const sum = (pick: (row: CrmStaffPayroll) => number) =>
    staff.reduce((total, row) => total + pick(row), 0);
  return (
    <div className="overflow-x-auto">
      <table
        aria-label={p.tableLabel}
        className="w-full min-w-[44rem] border-collapse text-sm"
        data-testid="crm-payroll-table"
      >
        <thead className="text-xs text-muted-foreground">
          <tr>
            <th className="pb-2 text-left font-medium" scope="col">
              {c.staff}
            </th>
            <th className={HEAD_NUM} scope="col">
              {c.visits}
            </th>
            <th className={HEAD_NUM} scope="col">
              {c.shifts}
            </th>
            <th className={HEAD_NUM} scope="col">
              {c.accrued}
            </th>
            <th className={HEAD_NUM} scope="col">
              {c.paid}
            </th>
            <th className={HEAD_NUM} scope="col">
              {c.balance}
            </th>
            <th className="pb-2 pl-3 text-right font-medium" scope="col">
              <span className="sr-only">{c.actions}</span>
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-border/50">
          {staff.map((row) => (
            <tr
              data-testid={`crm-payroll-row-${row.staffId}`}
              key={row.staffId}
            >
              <th className="py-2 text-left font-normal" scope="row">
                <span className="flex flex-wrap items-center gap-2">
                  {row.name}
                  {row.archived ? (
                    <Badge variant="secondary">{p.archivedBadge}</Badge>
                  ) : null}
                </span>
                <span className="block text-xs text-muted-foreground">
                  {ratesText(row)}
                </span>
              </th>
              <td className={NUM}>{row.visits}</td>
              <td className={NUM}>{row.shifts}</td>
              <td className={NUM}>{formatRubles(row.accruedMinor)}</td>
              <td className={cn(NUM, "text-muted-foreground")}>
                {formatRubles(row.paidMinor)}
              </td>
              <td
                className={cn(NUM, "font-medium")}
                data-testid={`crm-payroll-balance-${row.staffId}`}
              >
                {formatRubles(row.balanceMinor)}
              </td>
              <td className="whitespace-nowrap py-2 pl-3 text-right">
                <Button
                  aria-label={fillTemplate(p.editRatesLabel, {
                    name: row.name,
                  })}
                  data-testid={`crm-payroll-rates-${row.staffId}`}
                  onClick={() => onEditRates(row)}
                  size="sm"
                  variant="ghost"
                >
                  {p.editRates}
                </Button>
                <Button
                  aria-label={fillTemplate(p.payLabel, { name: row.name })}
                  data-testid={`crm-payroll-pay-${row.staffId}`}
                  onClick={() => onPay(row)}
                  size="sm"
                  variant="outline"
                >
                  {p.pay}
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
        <tfoot className="border-t border-border font-medium">
          <tr>
            <th className="pt-2 text-left" scope="row">
              {p.totals}
            </th>
            <td className={NUM}>{sum((row) => row.visits)}</td>
            <td className={NUM}>{sum((row) => row.shifts)}</td>
            <td className={NUM}>
              {formatRubles(sum((row) => row.accruedMinor))}
            </td>
            <td className={NUM}>{formatRubles(sum((row) => row.paidMinor))}</td>
            <td className={NUM} data-testid="crm-payroll-total-balance">
              {formatRubles(sum((row) => row.balanceMinor))}
            </td>
            <td />
          </tr>
        </tfoot>
      </table>
    </div>
  );
}

function PayoutList({
  names,
  onVoid,
  payouts,
}: {
  names: ReadonlyMap<string, string>;
  onVoid: (payout: CrmPayout) => void;
  payouts: readonly CrmPayout[];
}) {
  const p = ru.payroll;
  if (payouts.length === 0) {
    return <p className="text-sm text-muted-foreground">{p.payoutsEmpty}</p>;
  }
  return (
    <ul
      aria-label={p.payoutsLabel}
      className="divide-y divide-border/50 text-sm"
    >
      {payouts.map((payout) => (
        <li
          className={cn(
            "flex flex-wrap items-center gap-x-3 gap-y-1 py-2",
            payout.voided && "text-muted-foreground",
          )}
          data-testid={`crm-payout-${payout.id}`}
          key={payout.id}
        >
          <span className="w-24 tabular-nums">{formatDay(payout.date)}</span>
          <span className="min-w-0 flex-1 truncate">
            {names.get(payout.staffId) ?? "—"}
            {payout.note ? (
              <span className="text-muted-foreground"> · {payout.note}</span>
            ) : null}
          </span>
          <span className={cn("tabular-nums", payout.voided && "line-through")}>
            {formatRubles(payout.amountMinor)}
          </span>
          {payout.voided ? (
            <Badge variant="secondary">{p.voided}</Badge>
          ) : (
            <Button
              aria-label={fillTemplate(p.voidPayoutLabel, {
                amount: formatRubles(payout.amountMinor),
                date: formatDay(payout.date),
              })}
              data-testid={`crm-payout-void-${payout.id}`}
              onClick={() => onVoid(payout)}
              size="sm"
              variant="ghost"
            >
              {p.voidPayout}
            </Button>
          )}
        </li>
      ))}
    </ul>
  );
}

/** Accruals per staff member, rates and payouts (owners and admins). */
export function CrmPayrollSection({ space }: { space: CrmMemberSpace }) {
  const p = ru.payroll;
  const canManage = space.capabilities.has("manage_space");
  const [preset, setPreset] = React.useState<PayrollPreset>("month");
  const today = todayIn(space.settings.timezone);
  const period = payrollPeriod(preset, today);
  const query = useCrmPayrollQuery(period, canManage);
  const rates = useSetCrmPayRatesMutation();
  const payout = useRecordCrmPayoutMutation();
  const voidPayout = useVoidCrmPayoutMutation();
  const [editing, setEditing] = React.useState<Editing | null>(null);
  const [voiding, setVoiding] = React.useState<CrmPayout | null>(null);
  const data = query.data;
  const names = React.useMemo(
    () => new Map((data?.staff ?? []).map((row) => [row.staffId, row.name])),
    [data],
  );

  function open(next: Editing) {
    rates.reset();
    payout.reset();
    setEditing(next);
  }

  if (!canManage) {
    return (
      <EmptyState
        description={p.noAccessHint}
        icon={<Wallet className="h-10 w-10" />}
        testId="crm-payroll-no-access"
        title={p.noAccessTitle}
      />
    );
  }

  return (
    <section
      aria-labelledby="crm-payroll-title"
      className="space-y-4"
      data-testid="crm-payroll"
    >
      <SectionHeader
        action={
          <fieldset className="flex flex-wrap gap-0.5 rounded-lg bg-muted p-0.5">
            <legend className="sr-only">{p.periodLabel}</legend>
            {PAYROLL_PRESETS.map((item) => (
              <button
                aria-pressed={item === preset}
                className={cn(
                  "rounded-md px-3 py-1 text-sm transition-colors focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring",
                  item === preset
                    ? "bg-background font-medium text-foreground shadow-sm"
                    : "text-muted-foreground hover:text-foreground",
                )}
                data-testid={`crm-payroll-period-${item}`}
                key={item}
                onClick={() => setPreset(item)}
                type="button"
              >
                {p.periods[item]}
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
        title={<span id="crm-payroll-title">{p.title}</span>}
      />

      {query.isPending ? (
        <div aria-busy="true" aria-label={p.loading} role="status">
          <Skeleton className="h-48" />
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
          testId="crm-payroll-error"
          title={p.loadError}
        />
      ) : data && data.staff.length === 0 ? (
        <EmptyState
          description={p.emptyHint}
          icon={<Wallet className="h-10 w-10" />}
          testId="crm-payroll-empty"
          title={p.emptyTitle}
        />
      ) : data ? (
        <div
          aria-busy={query.isPlaceholderData || undefined}
          className="space-y-4"
        >
          <section aria-labelledby="crm-payroll-staff-title" className={CARD}>
            <CardTitle id="crm-payroll-staff-title">{p.tableLabel}</CardTitle>
            <PayrollTable
              onEditRates={(row) =>
                open({ kind: "rates", row, key: crypto.randomUUID() })
              }
              onPay={(row) =>
                open({
                  kind: "payout",
                  row,
                  key: crypto.randomUUID(),
                  payoutId: crypto.randomUUID(),
                })
              }
              staff={data.staff}
            />
            <p className="mt-3 text-xs text-muted-foreground">{p.note}</p>
          </section>
          <section aria-labelledby="crm-payroll-payouts-title" className={CARD}>
            <CardTitle id="crm-payroll-payouts-title">
              {p.payoutsTitle}
            </CardTitle>
            <PayoutList
              names={names}
              onVoid={setVoiding}
              payouts={data.payouts}
            />
          </section>
        </div>
      ) : null}

      {editing?.kind === "rates" ? (
        <CrmPayRatesDialog
          error={rates.error}
          key={editing.key}
          onOpenChange={(next) => !next && setEditing(null)}
          onSubmit={(next) =>
            rates.mutate(
              {
                channelId: space.channelId,
                staffId: editing.row.staffId,
                ...next,
              },
              {
                onSuccess: () => {
                  setEditing(null);
                  toast.success(p.ratesDialog.saved);
                },
              },
            )
          }
          pending={rates.isPending}
          row={editing.row}
        />
      ) : null}
      {editing?.kind === "payout" ? (
        <CrmPayoutDialog
          error={payout.error}
          key={editing.key}
          onOpenChange={(next) => !next && setEditing(null)}
          onSubmit={(next) =>
            payout.mutate(
              {
                channelId: space.channelId,
                payoutId: editing.payoutId,
                staffId: editing.row.staffId,
                ...next,
              },
              {
                onSuccess: () => {
                  setEditing(null);
                  toast.success(p.payoutDialog.saved);
                },
              },
            )
          }
          pending={payout.isPending}
          row={editing.row}
          today={today}
        />
      ) : null}

      <AlertDialog
        onOpenChange={(next) => !next && setVoiding(null)}
        open={voiding !== null}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{p.voidPayout}</AlertDialogTitle>
            <AlertDialogDescription>
              {fillTemplate(p.voidConfirm, {
                amount: formatRubles(voiding?.amountMinor ?? 0),
              })}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{ru.common.cancel}</AlertDialogCancel>
            <AlertDialogAction
              data-testid="crm-payout-void-confirm"
              onClick={() => {
                if (!voiding) return;
                voidPayout.mutate(
                  { channelId: space.channelId, payoutId: voiding.id },
                  {
                    onError: (error) =>
                      toast.error(
                        `${p.voidFailed}: ${describeCrmError(error).message}`,
                      ),
                  },
                );
                setVoiding(null);
              }}
            >
              {p.voidPayout}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}
