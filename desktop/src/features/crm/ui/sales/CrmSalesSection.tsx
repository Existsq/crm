import { Plus, Receipt } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { useCrmSalesQuery, usePostCrmSaleMutation } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import { formatCrmDateTime } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { CrmSaleDetailsDialog } from "@/features/crm/ui/sales/CrmSaleDetailsDialog";
import { CrmSaleDialog } from "@/features/crm/ui/sales/CrmSaleDialog";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmSale, CrmSaleInput } from "@/shared/api/tauriCrmSales";
import { getCrmSale } from "@/shared/api/tauriCrmSales";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";
import { Switch } from "@/shared/ui/switch";
import { cn } from "@/shared/lib/cn";

const HEAD_CELL = "px-4 py-2.5 text-left text-xs font-medium";

function SalesTable({
  busy,
  onOpen,
  sales,
}: {
  busy: boolean;
  onOpen: (sale: CrmSale, opener: HTMLElement) => void;
  sales: CrmSale[];
}) {
  const columns = ru.sales.columns;
  return (
    <div
      aria-busy={busy || undefined}
      className="overflow-x-auto rounded-2xl border border-border/70"
    >
      <table
        className="w-full min-w-[40rem] border-collapse text-sm"
        data-testid="crm-sales-table"
      >
        <caption className="sr-only">{ru.sales.tableLabel}</caption>
        <thead className="bg-muted/40 text-muted-foreground">
          <tr>
            <th className={HEAD_CELL} scope="col">
              {columns.number}
            </th>
            <th className={HEAD_CELL} scope="col">
              {columns.date}
            </th>
            <th className={HEAD_CELL} scope="col">
              {columns.customer}
            </th>
            <th className={HEAD_CELL} scope="col">
              {columns.payment}
            </th>
            <th className={`${HEAD_CELL} text-right`} scope="col">
              {columns.total}
            </th>
            <th className={HEAD_CELL} scope="col">
              <span className="sr-only">{columns.actions}</span>
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-border/60">
          {sales.map((sale) => (
            <tr
              className={cn(sale.void && "text-muted-foreground")}
              data-testid={`crm-sale-${sale.number}`}
              key={sale.id}
            >
              <th className="px-4 py-2.5 text-left font-medium" scope="row">
                <div className="flex items-center gap-2">
                  <span className="tabular-nums">{sale.number}</span>
                  {sale.void ? (
                    <Badge variant="destructive">{ru.sales.voidedBadge}</Badge>
                  ) : null}
                </div>
              </th>
              <td className="whitespace-nowrap px-4 py-2.5">
                {formatCrmDateTime(sale.soldAt)}
              </td>
              <td className="px-4 py-2.5">
                {sale.customerName ?? ru.sales.anonymous}
              </td>
              <td className="px-4 py-2.5">{ru.payments[sale.payment]}</td>
              <td
                className={cn(
                  "whitespace-nowrap px-4 py-2.5 text-right tabular-nums",
                  sale.void && "line-through",
                )}
              >
                {formatRubles(sale.totalMinor)}
              </td>
              <td className="px-2 py-1.5 text-right">
                <Button
                  aria-label={`${ru.sales.open}: ${ru.sales.columns.number}${sale.number}`}
                  onClick={(event) => onOpen(sale, event.currentTarget)}
                  size="sm"
                  variant="ghost"
                >
                  {ru.sales.open}
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function CrmSalesSection({ space }: { space: CrmMemberSpace }) {
  const canPost = space.capabilities.has("post_documents");
  const canVoid = space.capabilities.has("void_documents");
  const [hideVoided, setHideVoided] = React.useState(false);
  const query = useCrmSalesQuery({ customerId: null, voided: !hideVoided });
  const post = usePostCrmSaleMutation();
  const [create, setCreate] = React.useState({
    open: false,
    session: 0,
    saleId: "",
  });
  const [details, setDetails] = React.useState<{
    open: boolean;
    saleId: string | null;
  }>({ open: false, saleId: null });
  const addButtonRef = React.useRef<HTMLButtonElement>(null);
  const returnFocusRef = React.useRef<HTMLElement | null>(null);
  const sales = React.useMemo(
    () => query.data?.pages.flatMap((page) => page.sales) ?? [],
    [query.data],
  );

  function openCreate(opener: HTMLElement) {
    returnFocusRef.current = opener;
    post.reset();
    setCreate((current) => ({
      open: true,
      session: current.session + 1,
      saleId: crypto.randomUUID(),
    }));
  }

  async function submit(input: CrmSaleInput) {
    try {
      await post.mutateAsync(input);
      setCreate((current) => ({ ...current, open: false }));
      // The number is assigned by the relay; read it back for the toast.
      const number = await getCrmSale(input.saleId)
        .then((sale) => sale.number)
        .catch(() => null);
      toast.success(
        number === null
          ? ru.sales.createdNoNumber
          : ru.sales.created.replace("{number}", String(number)),
      );
    } catch {
      // Rendered in the dialog through the mutation error state.
    }
  }

  const restoreFocus = (event: Event) => {
    event.preventDefault();
    const opener = returnFocusRef.current;
    (opener?.isConnected ? opener : addButtonRef.current)?.focus();
  };

  return (
    <section
      aria-labelledby="crm-sales-title"
      className="space-y-4"
      data-testid="crm-sales"
    >
      <SectionHeader
        action={
          canPost ? (
            <Button
              data-testid="crm-sale-add"
              onClick={(event) => openCreate(event.currentTarget)}
              ref={addButtonRef}
              size="sm"
            >
              <Plus aria-hidden="true" className="h-4 w-4" />
              {ru.sales.add}
            </Button>
          ) : undefined
        }
        description={canPost ? undefined : ru.sales.readOnlyHint}
        title={<span id="crm-sales-title">{ru.sales.title}</span>}
      />

      <div className="flex items-center gap-2">
        <Switch
          checked={hideVoided}
          data-testid="crm-sales-hide-voided"
          id="crm-sales-hide-voided"
          onCheckedChange={setHideVoided}
        />
        <label className="text-sm" htmlFor="crm-sales-hide-voided">
          {ru.sales.hideVoided}
        </label>
      </div>

      {query.isPending ? (
        <div
          aria-busy="true"
          aria-label={ru.sales.loading}
          className="space-y-2"
          data-testid="crm-sales-loading"
          role="status"
        >
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : query.isError && sales.length === 0 ? (
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
          testId="crm-sales-error"
          title={ru.sales.loadError}
        />
      ) : sales.length === 0 ? (
        <EmptyState
          description={canPost ? ru.sales.emptyCanPost : ru.sales.emptyReadOnly}
          icon={<Receipt className="h-10 w-10" />}
          testId="crm-sales-empty"
          title={ru.sales.emptyTitle}
        />
      ) : (
        <>
          {query.isError ? <CrmErrorAlert error={query.error} /> : null}
          <SalesTable
            busy={query.isPlaceholderData}
            onOpen={(sale, opener) => {
              returnFocusRef.current = opener;
              setDetails({ open: true, saleId: sale.id });
            }}
            sales={sales}
          />
        </>
      )}

      {query.hasNextPage && !query.isPlaceholderData ? (
        <div className="flex justify-center">
          <Button
            data-testid="crm-sales-load-more"
            disabled={query.isFetchingNextPage}
            onClick={() => void query.fetchNextPage()}
            size="sm"
            variant="outline"
          >
            {query.isFetchingNextPage
              ? ru.sales.loadingMore
              : ru.sales.loadMore}
          </Button>
        </div>
      ) : null}

      {canPost && create.session > 0 ? (
        <CrmSaleDialog
          error={post.error}
          key={create.session}
          onCloseAutoFocus={restoreFocus}
          onOpenChange={(open) => {
            if (open) return;
            if (!post.isPending) post.reset();
            setCreate((current) => ({ ...current, open: false }));
          }}
          onSubmit={(input) => void submit(input)}
          open={create.open}
          pending={post.isPending}
          saleId={create.saleId}
          space={space}
        />
      ) : null}

      <CrmSaleDetailsDialog
        canVoid={canVoid}
        channelId={space.channelId}
        key={details.saleId ?? "none"}
        onCloseAutoFocus={restoreFocus}
        onOpenChange={(open) => setDetails((current) => ({ ...current, open }))}
        open={details.open}
        saleId={details.saleId}
      />
    </section>
  );
}
