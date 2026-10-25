import type * as React from "react";

import { cn } from "@/shared/lib/cn";
import { FieldLabel } from "@/shared/ui/native-select";

/**
 * Accessibility wiring for the control inside a {@link CrmField}: the id,
 * test id and `aria-describedby` for the hint and error texts.
 */
export function crmControlProps(
  id: string,
  hint: boolean,
  error: string | undefined,
) {
  const described = [hint && `${id}-hint`, error && `${id}-error`].filter(
    Boolean,
  );
  return {
    id,
    "data-testid": id,
    "aria-invalid": error ? true : undefined,
    "aria-describedby": described.length > 0 ? described.join(" ") : undefined,
  } as const;
}

/** A labelled form field with an optional hint and validation error. */
export function CrmField({
  children,
  className,
  error,
  hint,
  id,
  label,
}: {
  children: React.ReactNode;
  className?: string;
  error?: string;
  hint?: string;
  /** Id of the control (pair with {@link crmControlProps}). */
  id: string;
  label: string;
}) {
  return (
    <div className={cn("space-y-1.5", className)}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      {children}
      {hint ? (
        <p className="text-xs text-muted-foreground" id={`${id}-hint`}>
          {hint}
        </p>
      ) : null}
      {error ? (
        <p
          className="text-xs text-destructive"
          data-testid={`${id}-error`}
          id={`${id}-error`}
        >
          {error}
        </p>
      ) : null}
    </div>
  );
}

/** Format an ISO timestamp as a short Russian date and time. */
export function formatCrmDateTime(iso: string): string {
  return new Date(iso).toLocaleString("ru-RU", {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

/** Format an ISO timestamp as a short Russian date. */
export function formatCrmDate(iso: string): string {
  return new Date(iso).toLocaleDateString("ru-RU", {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  });
}
