import { ScanBarcode } from "lucide-react";
import * as React from "react";

import { useCrmProductsQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import { useDebouncedValue } from "@/features/crm/ui/products/useDebouncedValue";
import {
  type CrmProduct,
  listCrmProducts,
} from "@/shared/api/tauriCrmProducts";

const SEARCH_DEBOUNCE_MS = 200;
const PICKER_LIMIT = 8;
/** EAN-8 … GTIN-14: what a barcode scanner types before its Enter. */
const BARCODE = /^\d{8,14}$/;

/**
 * Product input for the cash desk. A barcode scanner types the code and
 * presses Enter faster than the search debounce, so Enter on a code looks the
 * barcode up exactly instead of waiting for the suggestions.
 */
export function CrmCashScanner({
  onPick,
}: {
  onPick: (product: CrmProduct) => void;
}) {
  const cart = ru.cash.cart;
  const id = "crm-cash-scan";
  const inputRef = React.useRef<HTMLInputElement>(null);
  const [text, setText] = React.useState("");
  const [notFound, setNotFound] = React.useState<string | null>(null);
  const q = useDebouncedValue(text.trim(), SEARCH_DEBOUNCE_MS);
  const products = useCrmProductsQuery({ q, archived: false });
  const matches =
    q.length > 0 && q === text.trim() && !products.isPlaceholderData
      ? (products.data?.pages[0]?.products ?? []).slice(0, PICKER_LIMIT)
      : [];

  function pick(product: CrmProduct) {
    onPick(product);
    setText("");
    setNotFound(null);
    inputRef.current?.focus();
  }

  async function handleEnter() {
    const typed = text.trim();
    if (typed === "") return;
    if (BARCODE.test(typed)) {
      const page = await listCrmProducts({
        barcode: typed,
        archived: false,
        limit: 1,
      }).catch(() => null);
      const product = page?.products[0];
      if (product) return pick(product);
    }
    if (matches[0]) return pick(matches[0]);
    setNotFound(typed);
  }

  return (
    <div className="space-y-1.5">
      <label
        className="block text-xs font-medium text-muted-foreground"
        htmlFor={id}
      >
        {cart.scanLabel}
      </label>
      <div className="relative">
        <ScanBarcode
          aria-hidden="true"
          className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
        />
        <input
          aria-controls={text.trim() ? `${id}-results` : undefined}
          aria-describedby={notFound ? `${id}-error` : undefined}
          autoCapitalize="none"
          autoComplete="off"
          autoCorrect="off"
          // The cash desk opens to ring up goods: the scanner types here.
          // biome-ignore lint/a11y/noAutofocus: primary input of the screen
          autoFocus
          className="h-10 w-full rounded-md border border-input bg-transparent pl-9 pr-3 text-sm shadow-xs placeholder:text-muted-foreground focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring"
          data-testid={id}
          id={id}
          onChange={(event) => {
            setText(event.target.value);
            setNotFound(null);
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void handleEnter();
            }
          }}
          placeholder={cart.scanPlaceholder}
          ref={inputRef}
          spellCheck={false}
          type="search"
          value={text}
        />
      </div>
      {notFound ? (
        <p
          className="text-xs text-destructive"
          data-testid={`${id}-error`}
          id={`${id}-error`}
          role="alert"
        >
          {ru.sales.dialog.noProducts}: {notFound}
        </p>
      ) : null}
      {text.trim() && !notFound ? (
        <ul
          aria-busy={products.isFetching || undefined}
          aria-label={cart.scanLabel}
          className="max-h-56 overflow-y-auto rounded-md border border-border/70"
          data-testid={`${id}-results`}
          id={`${id}-results`}
        >
          {matches.length === 0 ? (
            <li className="px-3 py-2 text-sm text-muted-foreground">
              {products.isFetching || q !== text.trim()
                ? "…"
                : ru.sales.dialog.noProducts}
            </li>
          ) : (
            matches.map((product) => (
              <li key={product.id}>
                <button
                  className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left text-sm hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:outline-hidden"
                  data-testid={`${id}-option`}
                  onClick={() => pick(product)}
                  type="button"
                >
                  <span className="truncate">{product.name}</span>
                  <span className="shrink-0 tabular-nums text-muted-foreground">
                    {formatRubles(product.priceMinor)}
                  </span>
                </button>
              </li>
            ))
          )}
        </ul>
      ) : null}
    </div>
  );
}
