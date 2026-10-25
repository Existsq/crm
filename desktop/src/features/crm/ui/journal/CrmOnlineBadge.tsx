import { Globe } from "lucide-react";

import { ru } from "@/features/crm/i18n/ru";
import { cn } from "@/shared/lib/cn";

/**
 * Marks a visit the client booked on the public page. The short visible text
 * is hidden from assistive tech in favour of the full label, so screen
 * readers hear one stop: "the client booked online".
 */
export function CrmOnlineBadge({
  className,
  testId,
}: {
  className?: string;
  testId?: string;
}) {
  const j = ru.journal;
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-0.5 rounded-full bg-violet-500/15 px-1.5 py-px text-2xs font-medium text-violet-700 dark:text-violet-300",
        className,
      )}
      data-testid={testId}
      title={j.onlineBadgeLabel}
    >
      <Globe aria-hidden="true" className="h-3 w-3" />
      <span aria-hidden="true">{j.onlineBadge}</span>
      <span className="sr-only">{j.onlineBadgeLabel}</span>
    </span>
  );
}
