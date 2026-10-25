import { Boxes } from "lucide-react";
import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import {
  averageCostMinor,
  formatStockQty,
  isLowStock,
} from "@/features/crm/lib/stock";
import { useCrmStockQuery } from "@/features/crm/stockHooks";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { useDebouncedValue } from "@/features/crm/ui/products/useDebouncedValue";
import type { StockProductFilter } from "@/features/crm/ui/stock/CrmStockSection";
import {
  STOCK_HEAD_CELL,
  StockQueryState,
  StockWarehouseFilter,
} from "@/features/crm/ui/stock/CrmStockParts";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmStockItem } from "@/shared/api/tauriCrmStock";
import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { Input } from "@/shared/ui/input";
import { FieldLabel } from "@/shared/ui/native-select";
import { Switch } from "@/shared/ui/switch";

const SEARCH_DEBOUNCE_MS = 250;

function LevelRow({
  item,
  onShowMoves,
}: {
  item: CrmStockItem;
  onShowMoves: (product: StockProductFilter) => void;
}) {
  const levels = ru.stock.levels;
  const low = isLowStock(item);
  const average = averageCostMinor(item);
  return (
    <tr
      className={cn(item.archived && "text-muted-foreground")}
      data-testid={`crm-stock-item-${item.productId}`}
    >
      <th className="px-4 py-2.5 text-left font-normal" scope="row">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-medium">{item.name}</span>
          {item.sku ? (
            <span className="text-xs text-muted-foreground">{item.sku}</span>
          ) : null}
          {item.qtyMilli < 0 ? (
            <Badge variant="destructive">{levels.negative}</Badge>
          ) : low ? (
            <Badge variant="secondary">{levels.low}</Badge>
          ) : null}
        </div>
      </th>
      <td
        className={cn(
          "whitespace-nowrap px-4 py-2.5 text-right tabular-nums",
          item.qtyMilli < 0 && "text-destructive",
        )}
        data-testid="crm-stock-qty"
      >
        {formatStockQty(item.qtyMilli, item.unit)}
      </td>
      <td className="whitespace-nowrap px-4 py-2.5 text-right tabular-nums text-muted-foreground">
        {item.minStockMilli > 0
          ? formatStockQty(item.minStockMilli, item.unit)
          : "—"}
      </td>
      <td className="whitespace-nowrap px-4 py-2.5 text-right tabular-nums">
        {average === null ? "—" : formatRubles(average)}
      </td>
      <td className="whitespace-nowrap px-4 py-2.5 text-right tabular-nums">
        {formatRubles(item.valueMinor)}
      </td>
      <td className="px-2 py-1.5 text-right">
        <Button
          aria-label={`${levels.history}: ${item.name}`}
          onClick={() => onShowMoves({ id: item.productId, name: item.name })}
          size="sm"
          variant="ghost"
        >
          {levels.history}
        </Button>
      </td>
    </tr>
  );
}

/** Stock on hand per product, on one warehouse or summed over all. */
export function CrmStockLevels({
  onShowMoves,
  onWarehouseChange,
  space,
  warehouseId,
}: {
  onShowMoves: (product: StockProductFilter) => void;
  onWarehouseChange: (warehouseId: string | null) => void;
  space: CrmMemberSpace;
  warehouseId: string | null;
}) {
  const levels = ru.stock.levels;
  const [search, setSearch] = React.useState("");
  const [lowOnly, setLowOnly] = React.useState(false);
  const q = useDebouncedValue(search.trim(), SEARCH_DEBOUNCE_MS);
  const query = useCrmStockQuery({ warehouseId, q, low: lowOnly });
  const items = query.data?.items ?? [];
  const filtered = q !== "" || lowOnly;

  return (
    <div className="space-y-4" data-testid="crm-stock-levels">
      <div className="flex flex-wrap items-end gap-3">
        <StockWarehouseFilter
          id="crm-stock-warehouse"
          onChange={onWarehouseChange}
          space={space}
          value={warehouseId}
        />
        <div className="w-full space-y-1.5 sm:w-64">
          <FieldLabel htmlFor="crm-stock-search">{ru.stock.search}</FieldLabel>
          <Input
            data-testid="crm-stock-search"
            id="crm-stock-search"
            onChange={(event) => setSearch(event.target.value)}
            placeholder={ru.stock.searchPlaceholder}
            type="search"
            value={search}
          />
        </div>
        <div className="flex h-9 items-center gap-2">
          <Switch
            checked={lowOnly}
            data-testid="crm-stock-low-only"
            id="crm-stock-low-only"
            onCheckedChange={setLowOnly}
          />
          <label className="text-sm" htmlFor="crm-stock-low-only">
            {ru.stock.lowOnly}
          </label>
        </div>
      </div>

      {query.data ? (
        <dl
          className="grid max-w-md grid-cols-2 gap-3"
          data-testid="crm-stock-totals"
        >
          <div className="rounded-xl border border-border/70 px-4 py-3">
            <dt className="text-xs text-muted-foreground">
              {ru.stock.totalValue}
            </dt>
            <dd
              className="text-lg font-semibold tabular-nums"
              data-testid="crm-stock-total-value"
            >
              {formatRubles(query.data.totalValueMinor)}
            </dd>
          </div>
          <div className="rounded-xl border border-border/70 px-4 py-3">
            <dt className="text-xs text-muted-foreground">
              {ru.stock.lowCount}
            </dt>
            <dd
              className={cn(
                "text-lg font-semibold tabular-nums",
                query.data.lowCount > 0 && "text-destructive",
              )}
              data-testid="crm-stock-low-count"
            >
              {query.data.lowCount}
            </dd>
          </div>
        </dl>
      ) : null}

      <StockQueryState
        empty={
          filtered ? (
            <EmptyState
              testId="crm-stock-no-matches"
              title={levels.noMatches}
            />
          ) : (
            <EmptyState
              description={levels.emptyBody}
              icon={<Boxes className="h-10 w-10" />}
              testId="crm-stock-empty"
              title={levels.emptyTitle}
            />
          )
        }
        errorText={levels.loadError}
        isEmpty={items.length === 0}
        isError={query.isError}
        isPending={query.isPending}
        loadingText={levels.loading}
        onRetry={() => void query.refetch()}
        testId="crm-stock"
      >
        {query.isError ? <CrmErrorAlert error={query.error} /> : null}
        <div
          aria-busy={query.isPlaceholderData || undefined}
          className="overflow-x-auto rounded-2xl border border-border/70"
        >
          <table
            className="w-full min-w-[44rem] border-collapse text-sm"
            data-testid="crm-stock-table"
          >
            <caption className="sr-only">{levels.tableLabel}</caption>
            <thead className="bg-muted/40 text-muted-foreground">
              <tr>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {levels.columns.name}
                </th>
                <th className={`${STOCK_HEAD_CELL} text-right`} scope="col">
                  {levels.columns.qty}
                </th>
                <th className={`${STOCK_HEAD_CELL} text-right`} scope="col">
                  {levels.columns.min}
                </th>
                <th className={`${STOCK_HEAD_CELL} text-right`} scope="col">
                  {levels.columns.avgCost}
                </th>
                <th className={`${STOCK_HEAD_CELL} text-right`} scope="col">
                  {levels.columns.value}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  <span className="sr-only">{levels.columns.actions}</span>
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {items.map((item) => (
                <LevelRow
                  item={item}
                  key={item.productId}
                  onShowMoves={onShowMoves}
                />
              ))}
            </tbody>
          </table>
        </div>
      </StockQueryState>
    </div>
  );
}
