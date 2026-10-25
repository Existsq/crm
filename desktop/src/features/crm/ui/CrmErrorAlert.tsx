import { describeCrmError } from "@/features/crm/lib/crmErrors";

/**
 * Inline error for a failed CRM action: the Russian message, plus the relay's
 * own reason in muted text so support can see what the server said.
 */
export function CrmErrorAlert({
  error,
  testId,
}: {
  error: unknown;
  testId?: string;
}) {
  if (!error) return null;
  const described = describeCrmError(error);
  return (
    <div
      className="space-y-0.5 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2"
      data-testid={testId}
      role="alert"
    >
      <p className="text-sm text-destructive">{described.message}</p>
      {described.detail ? (
        <p className="text-xs text-muted-foreground">{described.detail}</p>
      ) : null}
    </div>
  );
}
