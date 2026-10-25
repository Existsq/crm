import { Pencil } from "lucide-react";

import { ru } from "@/features/crm/i18n/ru";
import { formatDuration } from "@/features/crm/lib/catalog";
import { formatRubles } from "@/features/crm/lib/money";
import type { CrmProduct, CrmProductKind } from "@/shared/api/tauriCrmProducts";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";

const HEAD_CELL = "px-4 py-2.5 text-left text-xs font-medium";

export function CrmProductsTable({
  busy,
  canEdit,
  kind,
  onEdit,
  products,
}: {
  /** A new search or refresh is loading over the rows shown. */
  busy: boolean;
  canEdit: boolean;
  kind: CrmProductKind;
  /** `opener` is the row's button, to return focus to after editing. */
  onEdit: (product: CrmProduct, opener: HTMLElement) => void;
  products: CrmProduct[];
}) {
  const columns = ru.products.columns;
  const services = kind === "services";
  return (
    <div
      aria-busy={busy || undefined}
      className="overflow-x-auto rounded-2xl border border-border/70"
    >
      <table
        className="w-full min-w-[40rem] border-collapse text-sm"
        data-testid={services ? "crm-services-table" : "crm-products-table"}
      >
        <caption className="sr-only">
          {services ? ru.services.tableLabel : ru.products.tableLabel}
        </caption>
        <thead className="bg-muted/40 text-muted-foreground">
          <tr>
            <th className={HEAD_CELL} scope="col">
              {services ? ru.services.columns.name : columns.name}
            </th>
            {services ? (
              <th className={HEAD_CELL} scope="col">
                {ru.services.columns.duration}
              </th>
            ) : (
              <>
                <th className={HEAD_CELL} scope="col">
                  {columns.sku}
                </th>
                <th className={HEAD_CELL} scope="col">
                  {columns.unit}
                </th>
              </>
            )}
            <th className={`${HEAD_CELL} text-right`} scope="col">
              {columns.price}
            </th>
            <th className={HEAD_CELL} scope="col">
              {columns.vat}
            </th>
            {canEdit ? (
              <th className={HEAD_CELL} scope="col">
                <span className="sr-only">{columns.actions}</span>
              </th>
            ) : null}
          </tr>
        </thead>
        <tbody className="divide-y divide-border/60">
          {products.map((product) => (
            <tr data-testid={`crm-product-${product.id}`} key={product.id}>
              <th className="px-4 py-2.5 text-left font-normal" scope="row">
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <span className="font-medium">{product.name}</span>
                  {product.archived ? (
                    <Badge variant="secondary">
                      {ru.products.archivedBadge}
                    </Badge>
                  ) : null}
                  {product.requiresMarking ? (
                    <Badge variant="warning">{ru.products.markingBadge}</Badge>
                  ) : null}
                </div>
                {product.category ? (
                  <p className="text-xs text-muted-foreground">
                    {product.category}
                  </p>
                ) : null}
              </th>
              {services ? (
                <td className="whitespace-nowrap px-4 py-2.5 tabular-nums">
                  {product.durationMin === null ? (
                    <span className="text-muted-foreground">
                      {ru.services.noDuration}
                    </span>
                  ) : (
                    formatDuration(product.durationMin)
                  )}
                </td>
              ) : (
                <>
                  <td className="px-4 py-2.5 font-mono text-xs text-muted-foreground">
                    {product.sku ?? "—"}
                  </td>
                  <td className="px-4 py-2.5">
                    {ru.units.short[product.unit]}
                  </td>
                </>
              )}
              <td className="whitespace-nowrap px-4 py-2.5 text-right tabular-nums">
                {formatRubles(product.priceMinor)}
              </td>
              <td className="whitespace-nowrap px-4 py-2.5">
                {ru.vat[product.vat]}
              </td>
              {canEdit ? (
                <td className="px-2 py-1.5 text-right">
                  <Button
                    aria-label={`${ru.products.edit}: ${product.name}`}
                    onClick={(event) => onEdit(product, event.currentTarget)}
                    size="sm"
                    variant="ghost"
                  >
                    <Pencil aria-hidden="true" className="h-3.5 w-3.5" />
                    {ru.products.edit}
                  </Button>
                </td>
              ) : null}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
