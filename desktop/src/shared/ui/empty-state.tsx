import type * as React from "react";

import { cn } from "@/shared/lib/cn";

/**
 * Centered empty/blocked state: decorative icon, title, supporting text and an
 * optional action. The icon is hidden from assistive technology; the title and
 * description carry the meaning.
 */
export function EmptyState({
  action,
  className,
  description,
  icon,
  testId,
  title,
}: {
  action?: React.ReactNode;
  className?: string;
  description?: React.ReactNode;
  icon?: React.ReactNode;
  testId?: string;
  title: React.ReactNode;
}) {
  return (
    <div
      className={cn(
        "flex flex-1 flex-col items-center justify-center gap-3 px-4 py-16 text-center",
        className,
      )}
      data-testid={testId}
    >
      {icon ? (
        <span aria-hidden="true" className="text-muted-foreground/40">
          {icon}
        </span>
      ) : null}
      <div className="max-w-md space-y-1">
        <p className="text-sm font-medium text-foreground">{title}</p>
        {description ? (
          <p className="text-sm text-muted-foreground">{description}</p>
        ) : null}
      </div>
      {action}
    </div>
  );
}
