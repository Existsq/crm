import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import { formatStockQty } from "@/features/crm/lib/stock";
import {
  useCrmStockDocQuery,
  useVoidCrmStockDocMutation,
} from "@/features/crm/stockHooks";
import {
  CrmField,
  crmControlProps,
  formatCrmDateTime,
} from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmStockDoc } from "@/shared/api/tauriCrmStock";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Skeleton } from "@/shared/ui/skeleton";

const CELL = "whitespace-nowrap px-3 py-2 text-right tabular-nums";
const HEAD = "px-3 py-2 text-right font-medium";

function DocFacts({ doc }: { doc: CrmStockDoc }) {
  const details = ru.stock.details;
  const facts: [string, string][] = [
    [details.postedAt, formatCrmDateTime(doc.postedAt)],
    [details.warehouse, doc.warehouseName],
  ];
  if (doc.toWarehouseName)
    facts.push([details.toWarehouse, doc.toWarehouseName]);
  if (doc.supplierName) facts.push([details.supplier, doc.supplierName]);
  if (doc.reference) facts.push([details.reference, doc.reference]);
  return (
    <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm sm:grid-cols-4">
      {facts.map(([label, value]) => (
        <div key={label}>
          <dt className="text-xs text-muted-foreground">{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

function DocLines({ doc }: { doc: CrmStockDoc }) {
  const dialog = ru.stock.dialog;
  const inventory = doc.type === "inventory";
  const receipt = doc.type === "receipt";
  return (
    <div className="overflow-x-auto rounded-xl border border-border/70">
      <table
        className="w-full min-w-[32rem] border-collapse text-sm"
        data-testid="crm-stock-doc-details-lines"
      >
        <caption className="sr-only">{ru.stock.details.linesLabel}</caption>
        <thead className="bg-muted/40 text-xs text-muted-foreground">
          <tr>
            <th className="px-3 py-2 text-left font-medium" scope="col">
              {dialog.product}
            </th>
            {inventory ? (
              <th className={HEAD} scope="col">
                {dialog.onHand}
              </th>
            ) : null}
            <th className={HEAD} scope="col">
              {inventory ? dialog.counted : dialog.qty}
            </th>
            {inventory ? (
              <th className={HEAD} scope="col">
                {dialog.difference}
              </th>
            ) : null}
            {receipt ? (
              <th className={HEAD} scope="col">
                {dialog.cost}
              </th>
            ) : null}
            <th className={HEAD} scope="col">
              {dialog.amount}
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-border/60">
          {(doc.lines ?? []).map((line) => (
            <tr key={line.productId}>
              <th className="px-3 py-2 text-left font-normal" scope="row">
                {line.productName}
              </th>
              {inventory ? (
                <td className={CELL}>
                  {line.bookQtyMilli === null
                    ? "—"
                    : formatStockQty(line.bookQtyMilli, line.unit)}
                </td>
              ) : null}
              <td className={CELL}>
                {formatStockQty(line.qtyMilli, line.unit)}
              </td>
              {inventory ? (
                <td className={CELL}>
                  {line.bookQtyMilli === null
                    ? "—"
                    : formatStockQty(
                        line.qtyMilli - line.bookQtyMilli,
                        line.unit,
                        {
                          signed: true,
                        },
                      )}
                </td>
              ) : null}
              {receipt ? (
                <td className={CELL}>
                  {line.costMinor === null ? "—" : formatRubles(line.costMinor)}
                </td>
              ) : null}
              <td className={CELL}>{formatRubles(line.amountMinor)}</td>
            </tr>
          ))}
        </tbody>
        <tfoot>
          <tr className="border-t border-border/70">
            <th
              className="px-3 py-2 text-left font-medium"
              colSpan={inventory ? 4 : receipt ? 3 : 2}
              scope="row"
            >
              {dialog.amount}
            </th>
            <td
              className={`${CELL} font-semibold`}
              data-testid="crm-stock-doc-details-total"
            >
              {formatRubles(doc.totalMinor)}
            </td>
          </tr>
        </tfoot>
      </table>
    </div>
  );
}

/** One stock document with its lines; void-capable roles can cancel it. */
export function CrmStockDocDetailsDialog({
  canVoid,
  channelId,
  docId,
  onCloseAutoFocus,
  onOpenChange,
  open,
}: {
  canVoid: boolean;
  channelId: string;
  docId: string | null;
  onCloseAutoFocus: (event: Event) => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
}) {
  const details = ru.stock.details;
  const query = useCrmStockDocQuery(open ? docId : null);
  const voidDoc = useVoidCrmStockDocMutation();
  const [voiding, setVoiding] = React.useState(false);
  const [reason, setReason] = React.useState("");
  const [submitted, setSubmitted] = React.useState(false);
  const doc = query.data;
  const reasonError =
    submitted && reason.trim() === "" ? details.reasonRequired : undefined;

  async function confirmVoid() {
    setSubmitted(true);
    if (!doc || reason.trim() === "") return;
    try {
      await voidDoc.mutateAsync({
        channelId,
        docId: doc.id,
        reason: reason.trim(),
      });
      toast.success(details.voidDone);
      setVoiding(false);
    } catch {
      // Shown below through the mutation error.
    }
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="flex max-h-[calc(100dvh-2rem)] flex-col sm:max-w-2xl"
        data-testid="crm-stock-doc-details"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <DialogHeader>
          <DialogTitle className="flex flex-wrap items-center gap-2">
            {doc
              ? details.title
                  .replace("{type}", ru.stock.docTypes[doc.type])
                  .replace("{number}", String(doc.number))
              : ru.stock.tabs.docs}
            {doc?.void ? (
              <Badge variant="destructive">{ru.stock.docs.voidedBadge}</Badge>
            ) : null}
          </DialogTitle>
        </DialogHeader>
        <div className="-mx-1 min-h-0 flex-1 space-y-4 overflow-y-auto px-1 pb-1">
          {query.isPending ? (
            <div aria-busy="true" className="space-y-2" role="status">
              <Skeleton className="h-6 w-1/2" />
              <Skeleton className="h-24 w-full" />
            </div>
          ) : query.isError || !doc ? (
            <CrmErrorAlert error={query.error ?? details.loadError} />
          ) : (
            <>
              {doc.void ? (
                <p
                  className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm"
                  data-testid="crm-stock-doc-voided"
                >
                  {details.voided
                    .replace("{date}", formatCrmDateTime(doc.void.voidedAt))
                    .replace("{reason}", doc.void.reason)}
                </p>
              ) : null}
              <DocFacts doc={doc} />
              <DocLines doc={doc} />
              {doc.note ? (
                <p className="text-sm">
                  <span className="text-muted-foreground">
                    {details.note}:{" "}
                  </span>
                  {doc.note}
                </p>
              ) : null}
              {voiding ? (
                <div className="space-y-2 rounded-xl border border-border/70 p-3">
                  <CrmField
                    error={reasonError}
                    hint={details.voidHint}
                    id="crm-stock-void-reason"
                    label={details.voidReason}
                  >
                    <Input
                      {...crmControlProps(
                        "crm-stock-void-reason",
                        true,
                        reasonError,
                      )}
                      autoFocus
                      maxLength={256}
                      onChange={(event) => setReason(event.target.value)}
                      placeholder={details.voidReasonPlaceholder}
                      value={reason}
                    />
                  </CrmField>
                  <CrmErrorAlert
                    error={voidDoc.error}
                    testId="crm-stock-void-error"
                  />
                  <div className="flex justify-end gap-2">
                    <Button
                      onClick={() => {
                        setVoiding(false);
                        voidDoc.reset();
                      }}
                      size="sm"
                      type="button"
                      variant="ghost"
                    >
                      {ru.common.cancel}
                    </Button>
                    <Button
                      data-testid="crm-stock-void-confirm"
                      disabled={voidDoc.isPending}
                      onClick={() => void confirmVoid()}
                      size="sm"
                      type="button"
                      variant="destructive"
                    >
                      {voidDoc.isPending
                        ? details.voiding
                        : details.voidConfirm}
                    </Button>
                  </div>
                </div>
              ) : null}
            </>
          )}
        </div>
        <DialogFooter>
          {canVoid && doc && !doc.void && !voiding ? (
            <Button
              data-testid="crm-stock-void"
              onClick={() => setVoiding(true)}
              type="button"
              variant="outline"
            >
              {details.void}
            </Button>
          ) : null}
          <Button onClick={() => onOpenChange(false)} type="button">
            {details.close}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
