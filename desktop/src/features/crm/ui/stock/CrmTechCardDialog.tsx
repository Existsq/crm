import { Trash2 } from "lucide-react";
import * as React from "react";

import { useCrmProductsQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatQty } from "@/features/crm/lib/money";
import {
  evaluateTechCardItem,
  isStockedProduct,
  MAX_TECH_CARD_ITEMS,
  type TechCardDraft,
} from "@/features/crm/lib/stock";
import { crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { useDebouncedValue } from "@/features/crm/ui/products/useDebouncedValue";
import { CrmSearchPicker } from "@/features/crm/ui/sales/CrmSearchPicker";
import type { CrmProduct } from "@/shared/api/tauriCrmProducts";
import type { CrmTechCard, CrmTechCardInput } from "@/shared/api/tauriCrmStock";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";

const SEARCH_DEBOUNCE_MS = 200;
const PICKER_LIMIT = 8;

/** Edit the consumables one service writes off per unit sold. */
export function CrmTechCardDialog({
  card,
  channelId,
  error,
  onCloseAutoFocus,
  onOpenChange,
  onSubmit,
  open,
  pending,
  service,
}: {
  card: CrmTechCard | null;
  channelId: string;
  error: unknown;
  onCloseAutoFocus: (event: Event) => void;
  onOpenChange: (open: boolean) => void;
  onSubmit: (input: CrmTechCardInput) => void;
  open: boolean;
  pending: boolean;
  service: CrmProduct;
}) {
  const techCards = ru.stock.techCards;
  const [items, setItems] = React.useState<TechCardDraft[]>(
    () =>
      card?.items.map((item) => ({
        product: {
          id: item.productId,
          name: item.productName,
          unit: item.unit,
        },
        qty: formatQty(item.qtyMilli),
      })) ?? [],
  );
  const [submitted, setSubmitted] = React.useState(false);
  const [search, setSearch] = React.useState("");
  const q = useDebouncedValue(search.trim(), SEARCH_DEBOUNCE_MS);
  const products = useCrmProductsQuery({ q, archived: false, kind: "goods" });
  const matches =
    q.length > 0 && q === search.trim() && !products.isPlaceholderData
      ? (products.data?.pages[0]?.products ?? [])
          .filter(isStockedProduct)
          .slice(0, PICKER_LIMIT)
      : [];

  const evaluated = items.map(evaluateTechCardItem);
  const tooMany = items.length > MAX_TECH_CARD_ITEMS;

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (tooMany || !evaluated.every((item) => item.ok)) return;
    onSubmit({
      channelId,
      serviceId: service.id,
      expectedRevision: card?.revision ?? null,
      items: evaluated.flatMap((item) => (item.ok ? [item.item] : [])),
    });
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="flex max-h-[calc(100dvh-2rem)] flex-col sm:max-w-xl"
        data-testid="crm-tech-card-dialog"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <form
          className="flex min-h-0 flex-1 flex-col gap-5"
          noValidate
          onSubmit={handleSubmit}
        >
          <DialogHeader>
            <DialogTitle>
              {techCards.dialogTitle.replace("{service}", service.name)}
            </DialogTitle>
            <DialogDescription>{techCards.dialogHint}</DialogDescription>
          </DialogHeader>
          <div className="-mx-1 min-h-0 flex-1 space-y-4 overflow-y-auto px-1 pb-1">
            <CrmSearchPicker
              emptyText={ru.stock.dialog.noProducts}
              getKey={(product) => product.id}
              id="crm-tech-card-product-search"
              items={matches}
              label={ru.stock.dialog.productSearch}
              loading={products.isFetching || q !== search.trim()}
              onPick={(product) => {
                setItems((current) =>
                  current.some((item) => item.product.id === product.id)
                    ? current
                    : [
                        ...current,
                        {
                          product: {
                            id: product.id,
                            name: product.name,
                            unit: product.unit,
                          },
                          qty: "",
                        },
                      ],
                );
                setSearch("");
              }}
              onQueryChange={setSearch}
              placeholder={ru.stock.dialog.productSearchPlaceholder}
              query={search}
              renderItem={(product) => (
                <span className="truncate">{product.name}</span>
              )}
            />
            {items.length === 0 ? (
              <p
                className="text-sm text-muted-foreground"
                data-testid="crm-tech-card-empty"
              >
                {techCards.empty}
              </p>
            ) : (
              <ul
                aria-label={techCards.itemsLabel}
                className="divide-y divide-border/60 rounded-xl border border-border/70"
                data-testid="crm-tech-card-items"
              >
                {items.map((item, index) => {
                  const id = `crm-tech-card-item-${index}`;
                  const result = evaluated[index];
                  const itemError =
                    submitted && result && !result.ok
                      ? result.error
                      : undefined;
                  return (
                    <li
                      className="flex items-start gap-3 px-3 py-2"
                      data-testid={`crm-tech-card-item-${item.product.id}`}
                      key={item.product.id}
                    >
                      <span className="min-w-0 flex-1 pt-1.5 text-sm">
                        {item.product.name}
                      </span>
                      <div>
                        <div className="flex items-center gap-1.5">
                          <Input
                            {...crmControlProps(id, false, itemError)}
                            aria-label={`${techCards.perService}: ${item.product.name}`}
                            className="h-8 w-24 text-right tabular-nums"
                            inputMode="decimal"
                            onChange={(event) =>
                              setItems((current) =>
                                current.map((row, i) =>
                                  i === index
                                    ? { ...row, qty: event.target.value }
                                    : row,
                                ),
                              )
                            }
                            value={item.qty}
                          />
                          <span className="w-8 text-xs text-muted-foreground">
                            {ru.units.short[item.product.unit]}
                          </span>
                        </div>
                        {itemError ? (
                          <p
                            className="mt-1 max-w-[10rem] text-xs text-destructive"
                            id={`${id}-error`}
                          >
                            {itemError}
                          </p>
                        ) : null}
                      </div>
                      <Button
                        aria-label={`${ru.stock.dialog.remove}: ${item.product.name}`}
                        onClick={() =>
                          setItems((current) =>
                            current.filter((_, i) => i !== index),
                          )
                        }
                        size="icon"
                        type="button"
                        variant="ghost"
                      >
                        <Trash2 aria-hidden="true" className="h-4 w-4" />
                      </Button>
                    </li>
                  );
                })}
              </ul>
            )}
            {tooMany ? (
              <p className="text-sm text-destructive" role="alert">
                {techCards.tooMany}
              </p>
            ) : null}
          </div>
          <CrmErrorAlert error={error} testId="crm-tech-card-error" />
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-tech-card-submit"
              disabled={pending}
              type="submit"
            >
              {pending ? ru.common.saving : ru.common.save}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
