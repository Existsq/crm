import * as React from "react";

import {
  useCrmCashQueueQuery,
  useProcessCrmCashQueueMutation,
  useResolveCrmCashJobMutation,
} from "@/features/crm/cashHooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmCashJob, CrmCashJobState } from "@/shared/api/tauriCrmCash";
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
import { SectionHeader } from "@/shared/ui/PageHeader";

const STATE_TONE: Record<
  CrmCashJobState,
  "secondary" | "info" | "destructive" | "warning"
> = {
  pending: "secondary",
  printed: "info",
  failed: "destructive",
  needs_attention: "warning",
};

const HEAD_CELL = "px-3 py-2 text-left text-xs font-medium";

/**
 * Receipts that are not printed and recorded yet. Pending ones are retried
 * on a timer; failed and unclear ones wait for the cashier's decision.
 */
export function CrmCashQueue({ canOperate }: { canOperate: boolean }) {
  const queue = ru.cash.queue;
  const query = useCrmCashQueueQuery();
  const process = useProcessCrmCashQueueMutation();
  const resolve = useResolveCrmCashJobMutation();
  const [discarding, setDiscarding] = React.useState<CrmCashJob | null>(null);
  const jobs = query.data ?? [];

  const act = (job: CrmCashJob, action: "retry" | "resume" | "discard") =>
    resolve.mutate({ operation: job.operation, saleId: job.saleId, action });

  return (
    <section
      aria-labelledby="crm-cash-queue-title"
      className="space-y-3"
      data-testid="crm-cash-queue"
    >
      <SectionHeader
        action={
          canOperate && jobs.length > 0 ? (
            <Button
              data-testid="crm-cash-queue-process"
              disabled={process.isPending}
              onClick={() => process.mutate()}
              size="sm"
              variant="outline"
            >
              {process.isPending ? queue.processing : queue.process}
            </Button>
          ) : undefined
        }
        description={queue.description}
        title={<span id="crm-cash-queue-title">{queue.title}</span>}
      />
      <CrmErrorAlert
        error={query.error ?? process.error ?? resolve.error}
        testId="crm-cash-queue-error"
      />
      {jobs.length === 0 ? (
        <p
          className="text-sm text-muted-foreground"
          data-testid="crm-cash-queue-empty"
        >
          {queue.empty}
        </p>
      ) : (
        <div className="overflow-x-auto rounded-2xl border border-border/70">
          <table className="w-full min-w-[40rem] border-collapse text-sm">
            <caption className="sr-only">{queue.title}</caption>
            <thead className="bg-muted/40 text-muted-foreground">
              <tr>
                <th className={HEAD_CELL} scope="col">
                  {queue.columns.sale}
                </th>
                <th className={HEAD_CELL} scope="col">
                  {queue.columns.operation}
                </th>
                <th className={`${HEAD_CELL} text-right`} scope="col">
                  {queue.columns.total}
                </th>
                <th className={HEAD_CELL} scope="col">
                  {queue.columns.state}
                </th>
                <th className={HEAD_CELL} scope="col">
                  <span className="sr-only">{queue.columns.actions}</span>
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {jobs.map((job) => (
                <tr
                  data-testid={`crm-cash-job-${job.saleNumber}-${job.operation}`}
                  key={`${job.operation}-${job.saleId}`}
                >
                  <th className="px-3 py-2 text-left font-medium" scope="row">
                    №{job.saleNumber}
                  </th>
                  <td className="px-3 py-2">
                    {queue.operations[job.operation]}
                  </td>
                  <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums">
                    {formatRubles(job.totalMinor)}
                  </td>
                  <td className="space-y-1 px-3 py-2">
                    <Badge variant={STATE_TONE[job.state]}>
                      {queue.states[job.state]}
                    </Badge>
                    {job.error ? (
                      <p className="text-xs text-muted-foreground">
                        {job.error}
                      </p>
                    ) : null}
                    {job.state === "needs_attention" ? (
                      <p className="text-xs text-muted-foreground">
                        {queue.attentionHint}
                      </p>
                    ) : null}
                  </td>
                  <td className="px-2 py-1.5">
                    {canOperate ? (
                      <div className="flex justify-end gap-1">
                        {job.state === "failed" ? (
                          <Button
                            disabled={resolve.isPending}
                            onClick={() => act(job, "retry")}
                            size="sm"
                            variant="ghost"
                          >
                            {queue.retry}
                          </Button>
                        ) : null}
                        {job.state === "needs_attention" ? (
                          <Button
                            disabled={resolve.isPending}
                            onClick={() => act(job, "resume")}
                            size="sm"
                            variant="ghost"
                          >
                            {queue.resume}
                          </Button>
                        ) : null}
                        {job.state === "failed" ||
                        job.state === "needs_attention" ? (
                          <Button
                            disabled={resolve.isPending}
                            onClick={() => setDiscarding(job)}
                            size="sm"
                            variant="ghost"
                          >
                            {queue.discard}
                          </Button>
                        ) : null}
                      </div>
                    ) : null}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <AlertDialog
        onOpenChange={(open) => !open && setDiscarding(null)}
        open={discarding !== null}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{queue.discard}</AlertDialogTitle>
            <AlertDialogDescription>
              {queue.discardConfirm.replace(
                "{number}",
                String(discarding?.saleNumber ?? ""),
              )}
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>{ru.common.cancel}</AlertDialogCancel>
            <AlertDialogAction
              onClick={() => {
                if (discarding) act(discarding, "discard");
                setDiscarding(null);
              }}
            >
              {queue.discard}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}
