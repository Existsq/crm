import * as React from "react";
import { toast } from "sonner";

import { useCrmSaleQuery, useVoidCrmSaleMutation } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatQty, formatRubles } from "@/features/crm/lib/money";
import {
  CrmField,
  crmControlProps,
  formatCrmDateTime,
} from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
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

/** One sale with its lines; owners and admins can void it from here. */
export function CrmSaleDetailsDialog({
  canVoid,
  channelId,
  onCloseAutoFocus,
  onOpenChange,
  open,
  saleId,
}: {
  canVoid: boolean;
  channelId: string;
  onCloseAutoFocus: (event: Event) => void;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  saleId: string | null;
}) {
  const details = ru.sales.details;
  const query = useCrmSaleQuery(open ? saleId : null);
  const voidSale = useVoidCrmSaleMutation();
  const [voiding, setVoiding] = React.useState(false);
  const [reason, setReason] = React.useState("");
  const [submitted, setSubmitted] = React.useState(false);
  const sale = query.data;
  const reasonError =
    submitted && reason.trim() === "" ? details.reasonRequired : undefined;

  async function confirmVoid() {
    setSubmitted(true);
    if (!sale || reason.trim() === "") return;
    try {
      await voidSale.mutateAsync({
        channelId,
        saleId: sale.id,
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
        data-testid="crm-sale-details"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <DialogHeader>
          <DialogTitle className="flex flex-wrap items-center gap-2">
            {sale
              ? details.title.replace("{number}", String(sale.number))
              : ru.sales.title}
            {sale?.void ? (
              <Badge variant="destructive">{ru.sales.voidedBadge}</Badge>
            ) : null}
          </DialogTitle>
        </DialogHeader>
        <div className="-mx-1 min-h-0 flex-1 space-y-4 overflow-y-auto px-1 pb-1">
          {query.isPending ? (
            <div aria-busy="true" className="space-y-2" role="status">
              <Skeleton className="h-6 w-1/2" />
              <Skeleton className="h-24 w-full" />
            </div>
          ) : query.isError || !sale ? (
            <CrmErrorAlert error={query.error ?? details.loadError} />
          ) : (
            <>
              {sale.void ? (
                <p
                  className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm"
                  data-testid="crm-sale-voided"
                >
                  {details.voided
                    .replace("{date}", formatCrmDateTime(sale.void.voidedAt))
                    .replace("{reason}", sale.void.reason)}
                </p>
              ) : null}
              <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm sm:grid-cols-4">
                <div>
                  <dt className="text-xs text-muted-foreground">
                    {details.soldAt}
                  </dt>
                  <dd>{formatCrmDateTime(sale.soldAt)}</dd>
                </div>
                <div>
                  <dt className="text-xs text-muted-foreground">
                    {details.customer}
                  </dt>
                  <dd>{sale.customerName ?? ru.sales.anonymous}</dd>
                </div>
                <div>
                  <dt className="text-xs text-muted-foreground">
                    {details.payment}
                  </dt>
                  <dd>{ru.payments[sale.payment]}</dd>
                </div>
                <div>
                  <dt className="text-xs text-muted-foreground">
                    {details.warehouse}
                  </dt>
                  <dd>{sale.warehouseName}</dd>
                </div>
              </dl>
              <div className="overflow-x-auto rounded-xl border border-border/70">
                <table className="w-full min-w-[32rem] border-collapse text-sm">
                  <caption className="sr-only">{details.linesLabel}</caption>
                  <thead className="bg-muted/40 text-xs text-muted-foreground">
                    <tr>
                      <th
                        className="px-3 py-2 text-left font-medium"
                        scope="col"
                      >
                        {ru.products.columns.name}
                      </th>
                      <th
                        className="px-3 py-2 text-right font-medium"
                        scope="col"
                      >
                        {ru.sales.dialog.qty}
                      </th>
                      <th
                        className="px-3 py-2 text-right font-medium"
                        scope="col"
                      >
                        {ru.sales.dialog.price}
                      </th>
                      <th
                        className="px-3 py-2 text-right font-medium"
                        scope="col"
                      >
                        {ru.sales.dialog.discount}
                      </th>
                      <th
                        className="px-3 py-2 text-right font-medium"
                        scope="col"
                      >
                        {ru.sales.dialog.amount}
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/60">
                    {(sale.lines ?? []).map((line) => (
                      <tr key={line.productId}>
                        <th
                          className="px-3 py-2 text-left font-normal"
                          scope="row"
                        >
                          {line.productName}
                        </th>
                        <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums">
                          {formatQty(line.qtyMilli)} {ru.units.short[line.unit]}
                        </td>
                        <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums">
                          {formatRubles(line.priceMinor)}
                        </td>
                        <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums">
                          {line.discountMinor > 0
                            ? formatRubles(line.discountMinor)
                            : "—"}
                        </td>
                        <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums">
                          {formatRubles(line.amountMinor)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                  <tfoot>
                    <tr className="border-t border-border/70">
                      <th
                        className="px-3 py-2 text-left font-medium"
                        colSpan={4}
                        scope="row"
                      >
                        {ru.sales.dialog.total}
                      </th>
                      <td
                        className="whitespace-nowrap px-3 py-2 text-right font-semibold tabular-nums"
                        data-testid="crm-sale-details-total"
                      >
                        {formatRubles(sale.totalMinor)}
                      </td>
                    </tr>
                  </tfoot>
                </table>
              </div>
              {sale.note ? (
                <p className="text-sm">
                  <span className="text-muted-foreground">
                    {details.note}:{" "}
                  </span>
                  {sale.note}
                </p>
              ) : null}
              {voiding ? (
                <div className="space-y-2 rounded-xl border border-border/70 p-3">
                  <CrmField
                    error={reasonError}
                    hint={details.voidHint}
                    id="crm-sale-void-reason"
                    label={details.voidReason}
                  >
                    <Input
                      {...crmControlProps(
                        "crm-sale-void-reason",
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
                    error={voidSale.error}
                    testId="crm-sale-void-error"
                  />
                  <div className="flex justify-end gap-2">
                    <Button
                      onClick={() => {
                        setVoiding(false);
                        voidSale.reset();
                      }}
                      size="sm"
                      type="button"
                      variant="ghost"
                    >
                      {ru.common.cancel}
                    </Button>
                    <Button
                      data-testid="crm-sale-void-confirm"
                      disabled={voidSale.isPending}
                      onClick={() => void confirmVoid()}
                      size="sm"
                      type="button"
                      variant="destructive"
                    >
                      {voidSale.isPending
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
          {canVoid && sale && !sale.void && !voiding ? (
            <Button
              data-testid="crm-sale-void"
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
