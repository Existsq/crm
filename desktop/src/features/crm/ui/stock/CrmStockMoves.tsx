import { ArrowLeftRight, X } from "lucide-react";
import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { formatStockQty } from "@/features/crm/lib/stock";
import { useCrmStockMovesQuery } from "@/features/crm/stockHooks";
import { formatCrmDateTime } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { StockProductFilter } from "@/features/crm/ui/stock/CrmStockSection";
import {
  STOCK_HEAD_CELL,
  StockLoadMore,
  StockQueryState,
  StockWarehouseFilter,
} from "@/features/crm/ui/stock/CrmStockParts";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmStockMove } from "@/shared/api/tauriCrmStock";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";

function moveSource(move: CrmStockMove): string {
  const moves = ru.stock.moves;
  const parts: string[] = [moves.sources[move.source]];
  if (move.docNumber !== null) {
    parts.push(moves.docNumber.replace("{number}", String(move.docNumber)));
  }
  if (move.reversal) parts.push(`(${moves.reversal})`);
  return parts.join(" ");
}

/** Every stock movement, newest first, optionally narrowed to one product. */
export function CrmStockMoves({
  onClearProduct,
  onWarehouseChange,
  product,
  space,
  warehouseId,
}: {
  onClearProduct: () => void;
  onWarehouseChange: (warehouseId: string | null) => void;
  product: StockProductFilter;
  space: CrmMemberSpace;
  warehouseId: string | null;
}) {
  const moves = ru.stock.moves;
  const query = useCrmStockMovesQuery({
    warehouseId,
    productId: product?.id ?? null,
  });
  const items = React.useMemo(
    () => query.data?.pages.flatMap((page) => page.moves) ?? [],
    [query.data],
  );

  return (
    <div className="space-y-4" data-testid="crm-stock-moves">
      <div className="flex flex-wrap items-end gap-3">
        <StockWarehouseFilter
          id="crm-stock-moves-warehouse"
          onChange={onWarehouseChange}
          space={space}
          value={warehouseId}
        />
        {product ? (
          <div
            className="flex h-9 items-center gap-1 rounded-md border border-border/70 pl-3 pr-1 text-sm"
            data-testid="crm-stock-moves-product"
          >
            <span className="truncate">
              {moves.productFilter.replace("{name}", product.name)}
            </span>
            <Button
              aria-label={moves.clearProduct}
              className="h-7 w-7"
              onClick={onClearProduct}
              size="icon"
              variant="ghost"
            >
              <X aria-hidden="true" className="h-3.5 w-3.5" />
            </Button>
          </div>
        ) : null}
      </div>

      <StockQueryState
        empty={
          <EmptyState
            description={moves.emptyBody}
            icon={<ArrowLeftRight className="h-10 w-10" />}
            testId="crm-stock-moves-empty"
            title={moves.emptyTitle}
          />
        }
        errorText={moves.loadError}
        isEmpty={items.length === 0}
        isError={query.isError}
        isPending={query.isPending}
        loadingText={moves.loading}
        onRetry={() => void query.refetch()}
        testId="crm-stock-moves"
      >
        {query.isError ? <CrmErrorAlert error={query.error} /> : null}
        <div
          aria-busy={query.isPlaceholderData || undefined}
          className="overflow-x-auto rounded-2xl border border-border/70"
        >
          <table
            className="w-full min-w-[46rem] border-collapse text-sm"
            data-testid="crm-stock-moves-table"
          >
            <caption className="sr-only">{moves.tableLabel}</caption>
            <thead className="bg-muted/40 text-muted-foreground">
              <tr>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {moves.columns.date}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {moves.columns.product}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {moves.columns.warehouse}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {moves.columns.source}
                </th>
                <th className={`${STOCK_HEAD_CELL} text-right`} scope="col">
                  {moves.columns.delta}
                </th>
                <th className={`${STOCK_HEAD_CELL} text-right`} scope="col">
                  {moves.columns.after}
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {items.map((move) => (
                <tr data-testid={`crm-stock-move-${move.id}`} key={move.id}>
                  <td className="whitespace-nowrap px-4 py-2.5">
                    {formatCrmDateTime(move.movedAt)}
                  </td>
                  <th className="px-4 py-2.5 text-left font-medium" scope="row">
                    {move.productName}
                  </th>
                  <td className="px-4 py-2.5">{move.warehouseName}</td>
                  <td className="px-4 py-2.5">{moveSource(move)}</td>
                  <td
                    className={cn(
                      "whitespace-nowrap px-4 py-2.5 text-right tabular-nums",
                      move.qtyDeltaMilli < 0
                        ? "text-destructive"
                        : "text-emerald-600 dark:text-emerald-400",
                    )}
                  >
                    {formatStockQty(move.qtyDeltaMilli, move.unit, {
                      signed: true,
                    })}
                  </td>
                  <td className="whitespace-nowrap px-4 py-2.5 text-right tabular-nums">
                    {formatStockQty(move.qtyAfterMilli, move.unit)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </StockQueryState>

      {query.hasNextPage && !query.isPlaceholderData ? (
        <StockLoadMore
          fetching={query.isFetchingNextPage}
          label={moves.loadMore}
          loadingLabel={moves.loadingMore}
          onClick={() => void query.fetchNextPage()}
          testId="crm-stock-moves-load-more"
        />
      ) : null}
    </div>
  );
}
