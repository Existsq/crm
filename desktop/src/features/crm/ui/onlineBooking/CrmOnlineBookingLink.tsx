import { openUrl } from "@tauri-apps/plugin-opener";
import { Copy, ExternalLink } from "lucide-react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import { writeTextToClipboard } from "@/shared/lib/clipboard";
import { Button } from "@/shared/ui/button";

/** The public booking page link with copy and open actions. */
export function CrmOnlineBookingLink({
  enabled,
  saved,
  url,
}: {
  /** Whether the saved settings accept bookings. */
  enabled: boolean;
  /** Whether settings were ever saved. */
  saved: boolean;
  url: string | null;
}) {
  const t = ru.onlineBooking;
  const hint = !saved ? t.notSavedHint : !enabled ? t.disabledHint : null;
  return (
    <section
      aria-labelledby="crm-online-booking-link-title"
      className="space-y-3 rounded-2xl border border-border/70 p-5"
      data-testid="crm-online-booking-link"
    >
      <h3 className="text-sm font-medium" id="crm-online-booking-link-title">
        {t.linkLabel}
      </h3>
      {url ? (
        <>
          <p
            className="break-all rounded-md bg-muted/50 px-3 py-2 font-mono text-sm"
            data-testid="crm-online-booking-url"
          >
            {url}
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              data-testid="crm-online-booking-copy"
              onClick={() =>
                void writeTextToClipboard(url).then(
                  () => toast.success(t.copied),
                  () => toast.error(t.copyFailed),
                )
              }
              size="sm"
              type="button"
              variant="outline"
            >
              <Copy aria-hidden="true" className="h-4 w-4" />
              {t.copyLink}
            </Button>
            <Button
              data-testid="crm-online-booking-open"
              onClick={() =>
                void openUrl(url).catch(() => toast.error(t.openFailed))
              }
              size="sm"
              type="button"
              variant="outline"
            >
              <ExternalLink aria-hidden="true" className="h-4 w-4" />
              {t.openLink}
            </Button>
          </div>
        </>
      ) : (
        <p className="text-sm text-muted-foreground">{t.linkUnavailable}</p>
      )}
      {hint ? (
        <p
          className="rounded-md border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm"
          data-testid="crm-online-booking-disabled-hint"
        >
          {hint}
        </p>
      ) : null}
    </section>
  );
}
