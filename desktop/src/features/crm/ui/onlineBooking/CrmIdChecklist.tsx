import { ru } from "@/features/crm/i18n/ru";
import { toggleId } from "@/features/crm/lib/onlineBooking";
import { Checkbox } from "@/shared/ui/checkbox";

export type CrmChecklistItem = {
  id: string;
  label: string;
  detail?: string | null;
};

/**
 * A fieldset of checkboxes over ids where nothing checked means "all": the
 * online booking page offers every active item until the owner narrows it.
 */
export function CrmIdChecklist({
  disabled,
  emptyText,
  id,
  items,
  label,
  onChange,
  selected,
}: {
  disabled: boolean;
  emptyText: string;
  /** Base id for the fieldset, its hint and each checkbox. */
  id: string;
  /** `null` while loading. */
  items: readonly CrmChecklistItem[] | null;
  label: string;
  onChange: (ids: string[]) => void;
  selected: readonly string[];
}) {
  const hintId = `${id}-hint`;
  return (
    <fieldset
      aria-describedby={hintId}
      className="min-w-0 space-y-2"
      data-testid={id}
      disabled={disabled}
    >
      <legend className="text-sm font-medium">{label}</legend>
      <p className="text-xs text-muted-foreground" id={hintId}>
        {ru.onlineBooking.allWhenNoneHint}
      </p>
      {items === null ? (
        <p className="text-sm text-muted-foreground" role="status">
          {ru.onlineBooking.choicesLoading}
        </p>
      ) : items.length === 0 ? (
        <p className="text-sm text-muted-foreground">{emptyText}</p>
      ) : (
        <div className="grid max-h-56 gap-1 overflow-y-auto rounded-xl border border-border/70 p-2">
          {items.map((item) => {
            const checkboxId = `${id}-${item.id}`;
            return (
              <div
                className="flex items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-muted/50"
                key={item.id}
              >
                <Checkbox
                  checked={selected.includes(item.id)}
                  data-testid={checkboxId}
                  disabled={disabled}
                  id={checkboxId}
                  onCheckedChange={(value) =>
                    onChange(toggleId(selected, item.id, value === true))
                  }
                />
                <label className="min-w-0 flex-1 text-sm" htmlFor={checkboxId}>
                  <span className="block truncate">{item.label}</span>
                  {item.detail ? (
                    <span className="text-xs text-muted-foreground">
                      {item.detail}
                    </span>
                  ) : null}
                </label>
              </div>
            );
          })}
        </div>
      )}
      {items !== null && items.length > 0 ? (
        <p className="text-xs font-medium" role="status">
          {selected.length === 0
            ? ru.onlineBooking.allSelected
            : ru.onlineBooking.someSelected(
                items.filter((item) => selected.includes(item.id)).length,
              )}
        </p>
      ) : null}
    </fieldset>
  );
}
