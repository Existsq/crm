import * as React from "react";

import { niceTicks } from "@/features/crm/lib/analyticsPeriod";
import { formatRublesCompact } from "@/features/crm/lib/money";
import { cn } from "@/shared/lib/cn";

/** One bar: `title` and `detail` fill the tooltip and the accessible name. */
export type CrmChartBar = {
  key: string;
  valueMinor: number;
  title: string;
  detail: string;
  /** Axis label under the bar, or null to leave it blank. */
  tick: string | null;
};

/** Series colour (categorical slot 1), stepped separately for dark mode. */
export const CRM_CHART_SERIES = "bg-[#2a78d6] dark:bg-[#3987e5]";

/**
 * Revenue bars on a zero baseline with a sparse money axis. Hovering or
 * arrowing through the bars shows a tooltip; for assistive technology the
 * chart is a listbox whose options carry the full text of each bar.
 */
export function CrmBarChart({
  bars,
  label,
  testId,
}: {
  bars: readonly CrmChartBar[];
  label: string;
  testId: string;
}) {
  const id = React.useId();
  const [active, setActive] = React.useState<number | null>(null);
  const max = Math.max(0, ...bars.map((bar) => bar.valueMinor));
  const ticks = niceTicks(max);
  const top = ticks.at(-1) || 1;
  const current = active !== null ? bars[active] : undefined;

  function onKeyDown(event: React.KeyboardEvent<HTMLDivElement>) {
    const last = bars.length - 1;
    const from = active ?? last;
    const next =
      event.key === "ArrowLeft"
        ? Math.max(0, from - 1)
        : event.key === "ArrowRight"
          ? Math.min(last, from + 1)
          : event.key === "Home"
            ? 0
            : event.key === "End"
              ? last
              : null;
    if (next === null) return;
    event.preventDefault();
    setActive(next);
  }

  return (
    <div className="flex gap-3">
      <div
        aria-hidden="true"
        className="relative h-44 w-14 shrink-0 text-2xs tabular-nums text-muted-foreground"
      >
        {ticks.map((tick) => (
          <span
            className="absolute right-0 translate-y-1/2"
            key={tick}
            style={{ bottom: `${(tick / top) * 100}%` }}
          >
            {formatRublesCompact(tick)}
          </span>
        ))}
      </div>
      <div className="relative min-w-0 flex-1">
        <div
          aria-hidden="true"
          className="pointer-events-none absolute inset-x-0 top-0 h-44"
        >
          {ticks.map((tick) => (
            <div
              className={cn(
                "absolute inset-x-0 border-t",
                tick === 0 ? "border-border" : "border-border/40",
              )}
              key={tick}
              style={{ bottom: `${(tick / top) * 100}%` }}
            />
          ))}
        </div>
        <div
          aria-activedescendant={
            active !== null ? `${id}-${active}` : undefined
          }
          aria-label={label}
          className="relative flex h-44 items-end gap-0.5 rounded-sm outline-hidden focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-4 focus-visible:ring-offset-background"
          data-testid={testId}
          onBlur={(event) => {
            if (!event.currentTarget.contains(event.relatedTarget)) {
              setActive(null);
            }
          }}
          onFocus={(event) => {
            if (event.target === event.currentTarget) {
              setActive((value) => value ?? bars.length - 1);
            }
          }}
          onKeyDown={onKeyDown}
          onPointerLeave={() => setActive(null)}
          onPointerMove={(event) => {
            // Track the column under the pointer, gaps included.
            const box = event.currentTarget.getBoundingClientRect();
            const ratio = (event.clientX - box.left) / box.width;
            setActive(
              Math.min(
                bars.length - 1,
                Math.max(0, Math.floor(ratio * bars.length)),
              ),
            );
          }}
          role="listbox"
          tabIndex={0}
        >
          {bars.map((bar, index) => (
            <div
              aria-label={`${bar.title}: ${bar.detail}`}
              aria-selected={index === active}
              className="flex h-full min-w-0 flex-1 flex-col justify-end"
              id={`${id}-${index}`}
              key={bar.key}
              role="option"
              tabIndex={-1}
            >
              <div
                className={cn(
                  "w-full rounded-t-[4px] transition-opacity duration-100 motion-reduce:transition-none",
                  CRM_CHART_SERIES,
                  active !== null && index !== active && "opacity-40",
                )}
                style={{
                  height: `${(bar.valueMinor / top) * 100}%`,
                  minHeight: bar.valueMinor > 0 ? "2px" : undefined,
                }}
              />
            </div>
          ))}
        </div>
        {current && active !== null ? (
          <div
            aria-hidden="true"
            className={cn(
              "pointer-events-none absolute z-10 -translate-y-full whitespace-nowrap rounded-lg border border-border/70 bg-popover px-2.5 py-1.5 text-xs text-popover-foreground shadow-md",
              active < bars.length * 0.2
                ? "translate-x-0"
                : active > bars.length * 0.8
                  ? "-translate-x-full"
                  : "-translate-x-1/2",
            )}
            data-testid={`${testId}-tooltip`}
            style={{
              left: `${((active + 0.5) / bars.length) * 100}%`,
              // 11rem is the plot height (h-44); sit just above the bar.
              top: `calc(${1 - current.valueMinor / top} * 11rem - 0.5rem)`,
            }}
          >
            <p className="font-medium">{current.title}</p>
            <p className="tabular-nums text-muted-foreground">
              {current.detail}
            </p>
          </div>
        ) : null}
        <div
          aria-hidden="true"
          className="mt-1.5 flex gap-0.5 text-2xs tabular-nums text-muted-foreground"
        >
          {bars.map((bar) => (
            <span
              className="min-w-0 flex-1 overflow-visible whitespace-nowrap text-center"
              key={bar.key}
            >
              {bar.tick ?? ""}
            </span>
          ))}
        </div>
      </div>
    </div>
  );
}
