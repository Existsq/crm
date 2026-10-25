import { Search } from "lucide-react";
import type * as React from "react";

/**
 * A search field with its matches listed as buttons underneath. Used to pick
 * products and customers in the sale dialog: typing filters through the
 * relay, Enter picks the first match.
 */
export function CrmSearchPicker<T>({
  emptyText,
  getKey,
  id,
  items,
  label,
  loading,
  onPick,
  onQueryChange,
  placeholder,
  query,
  renderItem,
}: {
  emptyText: string;
  getKey: (item: T) => string;
  id: string;
  items: readonly T[];
  label: string;
  loading: boolean;
  onPick: (item: T) => void;
  onQueryChange: (query: string) => void;
  placeholder: string;
  query: string;
  renderItem: (item: T) => React.ReactNode;
}) {
  const showResults = query.trim().length > 0;
  return (
    <div className="space-y-1.5">
      <label
        className="block text-xs font-medium text-muted-foreground"
        htmlFor={id}
      >
        {label}
      </label>
      <div className="relative">
        <Search
          aria-hidden="true"
          className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
        />
        <input
          aria-controls={showResults ? `${id}-results` : undefined}
          autoCapitalize="none"
          autoComplete="off"
          autoCorrect="off"
          className="h-9 w-full rounded-md border border-input bg-transparent pl-9 pr-3 text-sm shadow-xs placeholder:text-muted-foreground focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring"
          data-testid={id}
          id={id}
          onChange={(event) => onQueryChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              // Inside the sale form: pick instead of submitting the sale.
              event.preventDefault();
              if (items[0]) onPick(items[0]);
            }
          }}
          placeholder={placeholder}
          spellCheck={false}
          type="search"
          value={query}
        />
      </div>
      {showResults ? (
        <ul
          aria-busy={loading || undefined}
          aria-label={label}
          className="max-h-48 overflow-y-auto rounded-md border border-border/70"
          data-testid={`${id}-results`}
          id={`${id}-results`}
        >
          {items.length === 0 ? (
            <li className="px-3 py-2 text-sm text-muted-foreground">
              {loading ? "…" : emptyText}
            </li>
          ) : (
            items.map((item) => (
              <li key={getKey(item)}>
                <button
                  className="flex w-full items-center justify-between gap-3 px-3 py-2 text-left text-sm hover:bg-muted/60 focus-visible:bg-muted/60 focus-visible:outline-hidden"
                  data-testid={`${id}-option`}
                  onClick={() => onPick(item)}
                  type="button"
                >
                  {renderItem(item)}
                </button>
              </li>
            ))
          )}
        </ul>
      ) : null}
    </div>
  );
}
