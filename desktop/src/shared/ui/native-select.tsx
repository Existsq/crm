import { ChevronDown } from "lucide-react";
import type * as React from "react";

import { cn } from "@/shared/lib/cn";

/**
 * Styled native `<select>`: keyboard, screen-reader and OS pickers for free.
 * Prefer it over a custom listbox for short, static option lists.
 */
export function NativeSelect({
  children,
  className,
  disabled,
  id,
  onChange,
  value,
  ...rest
}: {
  children: React.ReactNode;
  className?: string;
  disabled?: boolean;
  id?: string;
  onChange: (value: string) => void;
  value: string;
} & Pick<
  React.SelectHTMLAttributes<HTMLSelectElement>,
  "aria-describedby" | "aria-label" | "name" | "required"
> & { "data-testid"?: string }) {
  return (
    <div className={cn("relative", className)}>
      <select
        {...rest}
        className="flex h-9 w-full appearance-none rounded-md border border-input bg-transparent px-3 pr-8 text-sm shadow-xs transition-colors focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
        disabled={disabled}
        id={id}
        onChange={(event) => onChange(event.target.value)}
        value={value}
      >
        {children}
      </select>
      <ChevronDown
        aria-hidden="true"
        className="pointer-events-none absolute right-2 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
      />
    </div>
  );
}

/** Small muted field label paired with a form control by `htmlFor`. */
export function FieldLabel({
  children,
  htmlFor,
}: {
  children: React.ReactNode;
  htmlFor?: string;
}) {
  return (
    <label
      className="block text-xs font-medium text-muted-foreground"
      htmlFor={htmlFor}
    >
      {children}
    </label>
  );
}
