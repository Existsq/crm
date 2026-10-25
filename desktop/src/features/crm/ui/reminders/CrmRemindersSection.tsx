import { FlaskConical, Send } from "lucide-react";

import { ru } from "@/features/crm/i18n/ru";
import { useCrmRemindersQuery } from "@/features/crm/reminderHooks";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { CrmReminderLog } from "@/features/crm/ui/reminders/CrmReminderLog";
import { CrmReminderSettingsForm } from "@/features/crm/ui/reminders/CrmReminderSettingsForm";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmReminderTransport } from "@/shared/api/tauriCrmReminders";
import { Button } from "@/shared/ui/button";
import { Skeleton } from "@/shared/ui/skeleton";

const r = ru.reminders;

/** Whether messages really reach Telegram or stay in the sandbox log. */
function TransportBanner({ transport }: { transport: CrmReminderTransport }) {
  if (transport.mode === "telegram") {
    return (
      <div
        className="flex items-start gap-3 rounded-xl border border-emerald-500/30 bg-emerald-500/5 px-4 py-3"
        data-testid="crm-reminders-transport"
      >
        <Send aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0" />
        <p className="text-sm">
          {r.transport.telegram.replace(
            "{value}",
            transport.botUsername ? `@${transport.botUsername}` : "",
          )}
        </p>
      </div>
    );
  }
  return (
    <div
      className="flex items-start gap-3 rounded-xl border border-amber-500/30 bg-amber-500/5 px-4 py-3"
      data-testid="crm-reminders-transport"
    >
      <FlaskConical aria-hidden="true" className="mt-0.5 h-4 w-4 shrink-0" />
      <div className="space-y-0.5">
        <p className="text-sm">{r.transport.sandbox}</p>
        <p className="text-xs text-muted-foreground">
          {r.transport.sandboxHint}
        </p>
      </div>
    </div>
  );
}

/** Reminder rules, the transport in use and the delivery log. */
export function CrmRemindersSection({ space }: { space: CrmMemberSpace }) {
  const query = useCrmRemindersQuery();

  if (query.isPending) {
    return (
      <div aria-label={r.loading} className="space-y-3" role="status">
        <Skeleton className="h-14 w-full" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }
  if (query.isError) {
    return (
      <div className="space-y-2" data-testid="crm-reminders-load-error">
        <p className="text-sm text-destructive">{r.loadError}</p>
        <CrmErrorAlert error={query.error} />
        <Button onClick={() => void query.refetch()} variant="outline">
          {ru.common.retry}
        </Button>
      </div>
    );
  }
  return (
    <div className="space-y-6">
      <TransportBanner transport={query.data.transport} />
      <CrmReminderSettingsForm reminders={query.data} space={space} />
      <CrmReminderLog />
    </div>
  );
}
