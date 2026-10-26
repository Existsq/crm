import { FlaskConical } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { useCrmProductsQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatStockQty } from "@/features/crm/lib/stock";
import {
  useCrmTechCardsQuery,
  useWriteCrmTechCardMutation,
} from "@/features/crm/stockHooks";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import {
  STOCK_HEAD_CELL,
  StockLoadMore,
  StockQueryState,
} from "@/features/crm/ui/stock/CrmStockParts";
import { CrmTechCardDialog } from "@/features/crm/ui/stock/CrmTechCardDialog";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmProduct } from "@/shared/api/tauriCrmProducts";
import type { CrmTechCard, CrmTechCardInput } from "@/shared/api/tauriCrmStock";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";

/** Services with the consumables each one writes off when it is paid. */
export function CrmTechCards({
  canEdit,
  space,
}: {
  canEdit: boolean;
  space: CrmMemberSpace;
}) {
  const techCards = ru.stock.techCards;
  const services = useCrmProductsQuery({
    q: "",
    archived: false,
    kind: "services",
  });
  const cards = useCrmTechCardsQuery();
  const write = useWriteCrmTechCardMutation();
  const [editing, setEditing] = React.useState<{
    open: boolean;
    session: number;
    service: CrmProduct | null;
  }>({ open: false, session: 0, service: null });
  const returnFocusRef = React.useRef<HTMLElement | null>(null);

  const serviceList = React.useMemo(
    () => services.data?.pages.flatMap((page) => page.products) ?? [],
    [services.data],
  );
  const cardById = React.useMemo(() => {
    const map = new Map<string, CrmTechCard>();
    for (const card of cards.data ?? []) map.set(card.serviceId, card);
    return map;
  }, [cards.data]);
  const editingCard = editing.service
    ? (cardById.get(editing.service.id) ?? null)
    : null;

  async function save(input: CrmTechCardInput) {
    try {
      await write.mutateAsync(input);
      setEditing((current) => ({ ...current, open: false }));
      toast.success(techCards.saved);
    } catch {
      // Rendered in the dialog through the mutation error state.
    }
  }

  return (
    <div className="space-y-4" data-testid="crm-tech-cards">
      <div>
        <h2 className="text-base font-semibold">{techCards.title}</h2>
        <p className="text-sm text-muted-foreground">
          {canEdit ? techCards.description : techCards.readOnlyHint}
        </p>
      </div>

      <StockQueryState
        empty={
          <EmptyState
            description={techCards.emptyBody}
            icon={<FlaskConical className="h-10 w-10" />}
            testId="crm-tech-cards-empty"
            title={techCards.emptyTitle}
          />
        }
        errorText={techCards.loadError}
        isEmpty={serviceList.length === 0}
        isError={services.isError || cards.isError}
        isPending={services.isPending || cards.isPending}
        loadingText={techCards.loading}
        onRetry={() => {
          void services.refetch();
          void cards.refetch();
        }}
        testId="crm-tech-cards"
      >
        {cards.isError ? <CrmErrorAlert error={cards.error} /> : null}
        <div className="overflow-x-auto rounded-2xl border border-border/70">
          <table
            className="w-full min-w-[36rem] border-collapse text-sm"
            data-testid="crm-tech-cards-table"
          >
            <caption className="sr-only">{techCards.tableLabel}</caption>
            <thead className="bg-muted/40 text-muted-foreground">
              <tr>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {techCards.columns.service}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  {techCards.columns.items}
                </th>
                <th className={STOCK_HEAD_CELL} scope="col">
                  <span className="sr-only">{techCards.columns.actions}</span>
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {serviceList.map((service) => {
                const card = cardById.get(service.id);
                const items = card?.items ?? [];
                return (
                  <tr
                    data-testid={`crm-tech-card-${service.id}`}
                    key={service.id}
                  >
                    <th
                      className="px-4 py-2.5 text-left align-top font-medium"
                      scope="row"
                    >
                      {service.name}
                    </th>
                    <td className="px-4 py-2.5">
                      {items.length === 0 ? (
                        <span className="text-muted-foreground">
                          {techCards.none}
                        </span>
                      ) : (
                        <ul className="space-y-0.5">
                          {items.map((item) => (
                            <li key={item.productId}>
                              {item.productName}
                              <span className="text-muted-foreground">
                                {" · "}
                                {formatStockQty(item.qtyMilli, item.unit)}
                              </span>
                            </li>
                          ))}
                        </ul>
                      )}
                    </td>
                    <td className="px-2 py-1.5 text-right align-top">
                      {canEdit ? (
                        <Button
                          aria-label={`${card ? techCards.edit : techCards.create}: ${service.name}`}
                          data-testid={`crm-tech-card-edit-${service.id}`}
                          onClick={(event) => {
                            returnFocusRef.current = event.currentTarget;
                            write.reset();
                            setEditing((current) => ({
                              open: true,
                              session: current.session + 1,
                              service,
                            }));
                          }}
                          size="sm"
                          variant="ghost"
                        >
                          {card ? techCards.edit : techCards.create}
                        </Button>
                      ) : null}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </StockQueryState>

      {services.hasNextPage ? (
        <StockLoadMore
          fetching={services.isFetchingNextPage}
          label={techCards.loadMore}
          loadingLabel={ru.stock.docs.loadingMore}
          onClick={() => void services.fetchNextPage()}
          testId="crm-tech-cards-load-more"
        />
      ) : null}

      {canEdit && editing.service ? (
        <CrmTechCardDialog
          card={editingCard}
          channelId={space.channelId}
          error={write.error}
          key={editing.session}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            if (returnFocusRef.current?.isConnected) {
              returnFocusRef.current.focus();
            }
          }}
          onOpenChange={(open) => {
            if (open) return;
            if (!write.isPending) write.reset();
            setEditing((current) => ({ ...current, open: false }));
          }}
          onSubmit={(input) => void save(input)}
          open={editing.open}
          pending={write.isPending}
          service={editing.service}
        />
      ) : null}
    </div>
  );
}
