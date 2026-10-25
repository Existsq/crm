import { Minus, Plus, Trash2 } from "lucide-react";

import { ru } from "@/features/crm/i18n/ru";
import { formatQty, formatRubles, parseQty } from "@/features/crm/lib/money";
import { evaluateLine, type SaleLineDraft } from "@/features/crm/lib/saleDraft";
import { crmControlProps } from "@/features/crm/ui/CrmField";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";

/** One unit up or down; never below one unit (remove the line instead). */
export function stepQty(qty: string, delta: 1 | -1): string {
  const parsed = parseQty(qty, { wholeOnly: false });
  if (!parsed.ok) return qty;
  return formatQty(Math.max(1000, parsed.milli + delta * 1000));
}

/** The receipt being rung up: products, quantities and line sums. */
export function CrmCashCart({
  lines,
  onChange,
  onRemove,
  showErrors,
}: {
  lines: SaleLineDraft[];
  onChange: (index: number, patch: Partial<SaleLineDraft>) => void;
  onRemove: (index: number) => void;
  showErrors: boolean;
}) {
  const dialog = ru.sales.dialog;
  if (lines.length === 0) {
    return (
      <p
        className={
          showErrors
            ? "text-sm text-destructive"
            : "text-sm text-muted-foreground"
        }
        data-testid="crm-cash-cart-empty"
        role={showErrors ? "alert" : undefined}
      >
        {ru.cash.cart.empty}
      </p>
    );
  }
  return (
    <ul
      aria-label={ru.cash.cart.title}
      className="divide-y divide-border/60 rounded-xl border border-border/70"
      data-testid="crm-cash-cart"
    >
      {lines.map((line, index) => {
        const evaluated = evaluateLine(line);
        const error = showErrors && !evaluated.ok ? evaluated.errors.qty : null;
        const id = `crm-cash-line-${index}-qty`;
        return (
          <li
            className="flex flex-wrap items-center gap-2 px-3 py-2"
            data-testid={`crm-cash-line-${line.product.id}`}
            key={line.product.id}
          >
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium">
                {line.product.name}
              </p>
              <p className="text-xs text-muted-foreground tabular-nums">
                {formatRubles(line.product.priceMinor)} /{" "}
                {ru.units.short[line.product.unit]}
              </p>
            </div>
            <div className="flex items-center gap-1">
              <Button
                aria-label={`${dialog.qty} −1: ${line.product.name}`}
                className="h-8 w-8"
                onClick={() => onChange(index, { qty: stepQty(line.qty, -1) })}
                size="icon"
                type="button"
                variant="ghost"
              >
                <Minus aria-hidden="true" className="h-3.5 w-3.5" />
              </Button>
              <Input
                {...crmControlProps(id, false, error ?? undefined)}
                aria-label={`${dialog.qty}: ${line.product.name}`}
                className="h-8 w-20 text-right tabular-nums"
                inputMode="decimal"
                onChange={(event) =>
                  onChange(index, { qty: event.target.value })
                }
                value={line.qty}
              />
              <Button
                aria-label={`${dialog.qty} +1: ${line.product.name}`}
                className="h-8 w-8"
                onClick={() => onChange(index, { qty: stepQty(line.qty, 1) })}
                size="icon"
                type="button"
                variant="ghost"
              >
                <Plus aria-hidden="true" className="h-3.5 w-3.5" />
              </Button>
            </div>
            <span className="w-24 text-right text-sm tabular-nums">
              {evaluated.ok ? formatRubles(evaluated.amountMinor) : "—"}
            </span>
            <Button
              aria-label={`${dialog.remove}: ${line.product.name}`}
              onClick={() => onRemove(index)}
              size="icon"
              type="button"
              variant="ghost"
            >
              <Trash2 aria-hidden="true" className="h-4 w-4" />
            </Button>
            {error ? (
              <p className="w-full text-xs text-destructive" id={`${id}-error`}>
                {error}
              </p>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}
