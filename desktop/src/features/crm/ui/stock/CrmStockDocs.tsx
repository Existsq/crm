import { FileText } from "lucide-react";
import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import { CRM_STOCK_DOC_TYPES } from "@/features/crm/lib/stock";
import { useCrmStockDocsQuery } from "@/features/crm/stockHooks";
import { formatCrmDateTime } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import {
  STOCK_HEAD_CELL,
  StockLoadMore,
  StockQueryState,
} from "@/features/crm/ui/stock/CrmStockParts";
import type { CrmStockDoc, CrmStockDocType } from "@/shared/api/tauriCrmStock";
import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { FieldLabel, NativeSelect } from "@/shared/ui/native-select";

/** Where the goods went: warehouse, transfer target or supplier. */
export function stockDocRoute(doc: CrmStockDoc): string {
  if (doc.toWarehouseName)
    return `${doc.warehouseName} → ${doc.toWarehouseName}`;
  if (doc.supplierName) return `${doc.warehouseName} · ${doc.supplierName}`;
  return doc.warehouseName;
}

/** The stock document journal, newest first. */
export function CrmStockDocs({
  onOpen,
}: {
  onOpen: (docId: string, opener: HTMLElement) => void;
}) {
  const docs = ru.stock.docs;
  const [type, setType] = React.useState<CrmStockDocType | null>(null);
  const query = useCrmStockDocsQuery(type);
  const items = React.useMemo(
    () => query.data?.pages.flatMap((page) => page.docs) ?? [],
    [query.data],
  );

  return (
    <div className="space-y-4" data-testid="crm-stock-docs">
      <div className="w-full space-y-1.5 sm:w-56">
        <FieldLabel htmlFor="crm-stock-docs-type">{docs.typeFilter}</FieldLabel>
        <NativeSelect
          data-testid="crm-stock-docs-type"
          id="crm-stock-docs-type"
          onChange={(value) =>
            setType(value === "" ? null : (value as CrmStockDocType))
          }
          value={type ?? ""}
        >
          <option value="">{docs.allTypes}</option>
          {CRM_STOCK_DOC_TYPES.map((value) => (
            <option key={value} value={value}>
              {ru.stock.docTypes[value]}
            </option>
          ))}
        </NativeSelect>
      </div>

      <StockQueryState
        empty={
          <EmptyState
            description={docs.emptyBody}
            icon={<FileText className="h-10 w-10" />}
            testId="crm-stock-docs-empty"
            title={docs.emptyTitle}
          />
        }
        errorText={docs.loadError}
        isEmpty={items.length === 0}
        isError={query.isError}
        isPending={query.isPending}
        loadingText={docs.loading}
        onRetry={() => void query.refetch()}
        testId="crm-stock-docs"
      >
        {query.isError ? <CrmErrorAlert error={query.error} /> : null}
        <div
          aria-busy={query.isPlaceholderData || undefined}
          className="overflow-x-auto rounded-2xl border border-border/70"
        >
          <table
            className="w-full min-w-[44rem] border-collapse text-sm"
            data-testid="crm-stock-docs-table"
          >
            <caption className="sr-only">{docs.tableLabel}</caption>
            <thead className="bg-muted/40 text-muted-foreground">
              <tr>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {docs.columns.number}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {docs.columns.date}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {docs.columns.type}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {docs.columns.details}
                </th>
                <th className={`${STOCK_HEAD_CELL} text-right`} scope="col">
                  {docs.columns.total}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  <span className="sr-only">{docs.columns.actions}</span>
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {items.map((doc) => (
                <tr
                  className={cn(doc.void && "text-muted-foreground")}
                  data-testid={`crm-stock-doc-${doc.number}`}
                  key={doc.id}
                >
                  <th className="px-4 py-2.5 text-left font-medium" scope="row">
                    <div className="flex items-center gap-2">
                      <span className="tabular-nums">{doc.number}</span>
                      {doc.void ? (
                        <Badge variant="destructive">{docs.voidedBadge}</Badge>
                      ) : null}
                    </div>
                  </th>
                  <td className="whitespace-nowrap px-4 py-2.5">
                    {formatCrmDateTime(doc.postedAt)}
                  </td>
                  <td className="px-4 py-2.5">{ru.stock.docTypes[doc.type]}</td>
                  <td className="px-4 py-2.5">{stockDocRoute(doc)}</td>
                  <td
                    className={cn(
                      "whitespace-nowrap px-4 py-2.5 text-right tabular-nums",
                      doc.void && "line-through",
                    )}
                  >
                    {formatRubles(doc.totalMinor)}
                  </td>
                  <td className="px-2 py-1.5 text-right">
                    <Button
                      aria-label={`${docs.open}: ${ru.stock.docTypes[doc.type]} ${docs.columns.number}${doc.number}`}
                      onClick={(event) => onOpen(doc.id, event.currentTarget)}
                      size="sm"
                      variant="ghost"
                    >
                      {docs.open}
                    </Button>
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
          label={docs.loadMore}
          loadingLabel={docs.loadingMore}
          onClick={() => void query.fetchNextPage()}
          testId="crm-stock-docs-load-more"
        />
      ) : null}
    </div>
  );
}
