import { Package, Plus, Search } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import {
  useCrmProductsQuery,
  useWriteCrmProductMutation,
} from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { CrmProductDialog } from "@/features/crm/ui/products/CrmProductDialog";
import { CrmProductsTable } from "@/features/crm/ui/products/CrmProductsTable";
import { useDebouncedValue } from "@/features/crm/ui/products/useDebouncedValue";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type {
  CrmProduct,
  CrmProductFields,
} from "@/shared/api/tauriCrmProducts";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";
import { Switch } from "@/shared/ui/switch";

/** Typing pause before the search reaches the relay (CRMC-04 AC2). */
const SEARCH_DEBOUNCE_MS = 250;

type DialogState = {
  open: boolean;
  /** Remounts the dialog so each opening starts from a fresh draft. */
  session: number;
  /** Product being edited, `null` to create one. */
  product: CrmProduct | null;
  /** Id written by this dialog: the edited product's, or one chosen at
   * opening for a create so a retried save cannot create a duplicate. */
  productId: string;
};

export function CrmProductsSection({ space }: { space: CrmMemberSpace }) {
  const canEdit = space.capabilities.has("edit_catalog");
  const [search, setSearch] = React.useState("");
  const [showArchived, setShowArchived] = React.useState(false);
  const q = useDebouncedValue(search.trim(), SEARCH_DEBOUNCE_MS);
  const query = useCrmProductsQuery({ q, archived: showArchived });
  const write = useWriteCrmProductMutation();
  const [dialog, setDialog] = React.useState<DialogState>({
    open: false,
    session: 0,
    product: null,
    productId: "",
  });
  const addButtonRef = React.useRef<HTMLButtonElement>(null);
  const returnFocusRef = React.useRef<HTMLElement | null>(null);

  const products = React.useMemo(
    () => query.data?.pages.flatMap((page) => page.products) ?? [],
    [query.data],
  );
  // Edit the freshest copy of the product while the dialog is open.
  const dialogProduct = dialog.product
    ? (products.find((product) => product.id === dialog.product?.id) ??
      dialog.product)
    : null;

  /** `opener` gets focus back on close (WebKit does not focus clicked buttons). */
  function openDialog(product: CrmProduct | null, opener: HTMLElement) {
    returnFocusRef.current = opener;
    write.reset();
    setDialog((current) => ({
      open: true,
      session: current.session + 1,
      product,
      productId: product?.id ?? crypto.randomUUID(),
    }));
  }

  async function save(
    fields: CrmProductFields,
    expectedRevision: string | null,
  ) {
    try {
      await write.mutateAsync({
        ...fields,
        channelId: space.channelId,
        productId: dialog.productId,
        expectedRevision,
      });
      toast.success(dialogProduct ? ru.products.updated : ru.products.created);
      setDialog((current) => ({ ...current, open: false }));
    } catch {
      // Rendered in the dialog through the mutation error state.
    }
  }

  const filtered = q.length > 0 || showArchived;

  return (
    <section
      aria-labelledby="crm-products-title"
      className="space-y-4"
      data-testid="crm-products"
    >
      <SectionHeader
        action={
          canEdit ? (
            <Button
              data-testid="crm-product-add"
              onClick={(event) => openDialog(null, event.currentTarget)}
              ref={addButtonRef}
              size="sm"
            >
              <Plus aria-hidden="true" className="h-4 w-4" />
              {ru.products.add}
            </Button>
          ) : undefined
        }
        description={canEdit ? undefined : ru.products.readOnlyHint}
        title={<span id="crm-products-title">{ru.products.title}</span>}
      />

      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <div className="relative min-w-[14rem] flex-1">
          <label className="sr-only" htmlFor="crm-products-search">
            {ru.products.searchLabel}
          </label>
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
          />
          <input
            autoCapitalize="none"
            autoCorrect="off"
            className="h-9 w-full rounded-lg border border-border/70 bg-background pl-9 pr-3 text-sm placeholder:text-muted-foreground focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring"
            data-testid="crm-products-search"
            id="crm-products-search"
            onChange={(event) => setSearch(event.target.value)}
            placeholder={ru.products.searchPlaceholder}
            spellCheck={false}
            type="search"
            value={search}
          />
        </div>
        <div className="flex items-center gap-2">
          <Switch
            checked={showArchived}
            data-testid="crm-products-show-archived"
            id="crm-products-show-archived"
            onCheckedChange={setShowArchived}
          />
          <label className="text-sm" htmlFor="crm-products-show-archived">
            {ru.products.showArchived}
          </label>
        </div>
      </div>

      {query.isPending ? (
        <div
          aria-busy="true"
          aria-label={ru.products.loading}
          className="space-y-2"
          data-testid="crm-products-loading"
          role="status"
        >
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : query.isError && products.length === 0 ? (
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
          testId="crm-products-error"
          title={ru.products.loadError}
        />
      ) : products.length === 0 ? (
        <EmptyState
          description={
            filtered
              ? ru.products.noResultsHint
              : canEdit
                ? ru.products.emptyCanEdit
                : ru.products.emptyReadOnly
          }
          icon={<Package className="h-10 w-10" />}
          testId="crm-products-empty"
          title={filtered ? ru.products.noResultsTitle : ru.products.emptyTitle}
        />
      ) : (
        <>
          {query.isError ? <CrmErrorAlert error={query.error} /> : null}
          <CrmProductsTable
            busy={query.isPlaceholderData}
            canEdit={canEdit}
            onEdit={openDialog}
            products={products}
          />
        </>
      )}

      {query.hasNextPage && !query.isPlaceholderData ? (
        <div className="flex justify-center">
          <Button
            data-testid="crm-products-load-more"
            disabled={query.isFetchingNextPage}
            onClick={() => void query.fetchNextPage()}
            size="sm"
            variant="outline"
          >
            {query.isFetchingNextPage
              ? ru.products.loadingMore
              : ru.products.loadMore}
          </Button>
        </div>
      ) : null}

      {!dialog.open ? (
        // A save that fails after its dialog was closed still reports here.
        <CrmErrorAlert error={write.error} testId="crm-products-write-error" />
      ) : null}

      {canEdit ? (
        <CrmProductDialog
          defaultVat={space.settings.defaultVat}
          error={write.error}
          key={dialog.session}
          onCloseAutoFocus={(event) => {
            // Back to the button that opened the dialog, or to "Add" when
            // that row is gone (archived, filtered out).
            event.preventDefault();
            const opener = returnFocusRef.current;
            (opener?.isConnected ? opener : addButtonRef.current)?.focus();
          }}
          onOpenChange={(open) => {
            if (open) return;
            // Drop an error the user has seen; keep an in-flight save's.
            if (!write.isPending) write.reset();
            setDialog((current) => ({ ...current, open: false }));
          }}
          onDismissError={() => write.reset()}
          onSubmit={(fields, expectedRevision) =>
            void save(fields, expectedRevision)
          }
          open={dialog.open}
          pending={write.isPending}
          product={dialogProduct}
        />
      ) : null}
    </section>
  );
}
