import { toast } from "sonner";

import { useFiscalizeCrmSaleMutation } from "@/features/crm/cashHooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmFiscalRecord, CrmSale } from "@/shared/api/tauriCrmSales";
import { Button } from "@/shared/ui/button";

function describeReceipt(template: string, record: CrmFiscalRecord) {
  return template
    .replace("{fd}", String(record.documentNumber))
    .replace("{fp}", record.documentSign)
    .replace("{shift}", String(record.shiftNumber));
}

/**
 * How a sale was paid and which fiscal receipts it has. A sale without a
 * receipt can be printed from here; a voided sale whose receipt was printed
 * gets its return receipt (sell_return).
 */
export function CrmSaleFiscal({
  canFiscalize,
  channelId,
  sale,
}: {
  canFiscalize: boolean;
  channelId: string;
  sale: CrmSale;
}) {
  const fiscal = ru.fiscal;
  const print = useFiscalizeCrmSaleMutation();
  const { sell, sellReturn } = sale.fiscal;
  const operation: "sell" | "sell_return" | null = sale.void
    ? sell && !sellReturn
      ? "sell_return"
      : null
    : sell
      ? null
      : "sell";
  const cashPart =
    sale.payments.find((part) => part.method === "cash")?.amountMinor ?? 0;

  async function run(op: "sell" | "sell_return") {
    try {
      const job = await print.mutateAsync({
        channelId,
        saleId: sale.id,
        operation: op,
      });
      if (job === null) toast.success(fiscal.printed);
      else {
        toast.warning(
          fiscal.queued.replace(
            "{error}",
            job.error ?? ru.cash.queue.states[job.state],
          ),
        );
      }
    } catch {
      // Shown below through the mutation error.
    }
  }

  return (
    <div
      className="space-y-2 rounded-xl border border-border/70 p-3 text-sm"
      data-testid="crm-sale-fiscal"
    >
      {sale.payments.length > 1 ? (
        <p data-testid="crm-sale-split">
          {sale.payments
            .map((part) =>
              fiscal.split
                .replace("{method}", ru.payments[part.method])
                .replace("{amount}", formatRubles(part.amountMinor)),
            )
            .join(" · ")}
        </p>
      ) : null}
      {sale.cashTenderedMinor !== null ? (
        <p className="text-muted-foreground">
          {fiscal.tendered
            .replace("{tendered}", formatRubles(sale.cashTenderedMinor))
            .replace(
              "{change}",
              formatRubles(Math.max(0, sale.cashTenderedMinor - cashPart)),
            )}
        </p>
      ) : null}
      <p className="text-xs font-medium text-muted-foreground">
        {fiscal.title}
      </p>
      {sell ? (
        <p data-testid="crm-sale-fiscal-sell">
          {describeReceipt(fiscal.sell, sell)}
        </p>
      ) : (
        <p className="text-muted-foreground">{fiscal.none}</p>
      )}
      {sellReturn ? (
        <p data-testid="crm-sale-fiscal-return">
          {describeReceipt(fiscal.sellReturn, sellReturn)}
        </p>
      ) : null}
      {canFiscalize && operation ? (
        <Button
          data-testid="crm-sale-fiscalize"
          disabled={print.isPending}
          onClick={() => void run(operation)}
          size="sm"
          variant="outline"
        >
          {print.isPending
            ? fiscal.printing
            : operation === "sell"
              ? fiscal.print
              : fiscal.printReturn}
        </Button>
      ) : null}
      <CrmErrorAlert error={print.error} testId="crm-sale-fiscal-error" />
    </div>
  );
}
