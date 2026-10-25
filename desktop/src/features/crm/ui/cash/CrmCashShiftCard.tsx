import { RefreshCw, Settings } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import {
  useCrmCashDeviceQuery,
  useCrmCashReportXMutation,
  useCrmCashShiftMutation,
  useCrmCashShiftsQuery,
} from "@/features/crm/cashHooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import { CrmCashSettingsDialog } from "@/features/crm/ui/cash/CrmCashSettingsDialog";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmCashDeviceStatus } from "@/shared/api/tauriCrmCash";
import { cn } from "@/shared/lib/cn";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";

function statusLine(status: CrmCashDeviceStatus | undefined): {
  text: string;
  tone: "success" | "warning" | "destructive" | "secondary";
} {
  const device = ru.cash.device;
  if (!status) return { text: device.checking, tone: "secondary" };
  if (!status.reachable) {
    return {
      text: device.unreachable.replace("{error}", status.error ?? ""),
      tone: "destructive",
    };
  }
  const number = String(status.shiftNumber ?? "");
  switch (status.shiftState) {
    case "opened":
      return {
        text: device.opened.replace("{number}", number),
        tone: "success",
      };
    case "expired":
      return {
        text: device.expired.replace("{number}", number),
        tone: "warning",
      };
    case "closed":
      return { text: device.closed, tone: "secondary" };
    default:
      return { text: status.error ?? device.unknown, tone: "warning" };
  }
}

/**
 * The cash register header: whether the cash register answers, its shift, and the
 * shift operations (open, X-report, Z-report).
 */
export function CrmCashShiftCard({
  canOperate,
  channelId,
}: {
  canOperate: boolean;
  channelId: string;
}) {
  const device = ru.cash.device;
  const status = useCrmCashDeviceQuery();
  const shifts = useCrmCashShiftsQuery();
  const shift = useCrmCashShiftMutation();
  const reportX = useCrmCashReportXMutation();
  const [settingsOpen, setSettingsOpen] = React.useState(false);
  const line = statusLine(status.data);
  const state = status.data?.reachable ? status.data.shiftState : null;
  const lastShift = shifts.data?.[0];

  async function runShift(action: "open" | "close") {
    try {
      const result = await shift.mutateAsync({ channelId, action });
      const done = action === "open" ? device.openedDone : device.closedDone;
      toast.success(done.replace("{number}", String(result.shiftNumber)));
      if (result.recordError) {
        toast.error(device.recordError.replace("{error}", result.recordError));
      }
    } catch {
      // Shown below through the mutation error.
    }
  }

  async function runReportX() {
    try {
      await reportX.mutateAsync();
      toast.success(device.reportXDone);
    } catch {
      // Shown below through the mutation error.
    }
  }

  return (
    <div
      className="space-y-3 rounded-2xl border border-border/70 p-4"
      data-testid="crm-cash-shift"
    >
      <div className="flex flex-wrap items-center gap-3">
        <h3 className="text-sm font-medium">{device.title}</h3>
        <Badge data-testid="crm-cash-device-status" variant={line.tone}>
          <span aria-live="polite">{line.text}</span>
        </Badge>
        <div className="ml-auto flex items-center gap-1">
          <Button
            aria-label={ru.common.refresh}
            disabled={status.isFetching}
            onClick={() => void status.refetch()}
            size="icon"
            variant="ghost"
          >
            <RefreshCw
              aria-hidden="true"
              className={cn("h-4 w-4", status.isFetching && "animate-spin")}
            />
          </Button>
          <Button
            aria-label={device.settings}
            data-testid="crm-cash-settings"
            onClick={() => setSettingsOpen(true)}
            size="icon"
            variant="ghost"
          >
            <Settings aria-hidden="true" className="h-4 w-4" />
          </Button>
        </div>
      </div>

      {lastShift ? (
        <p
          className="text-sm text-muted-foreground"
          data-testid="crm-cash-last-shift"
        >
          {device.lastShift
            .replace("{number}", String(lastShift.shiftNumber))
            .replace("{count}", String(lastShift.sellCount))
            .replace("{total}", formatRubles(lastShift.sellTotalMinor))
            .replace("{cash}", formatRubles(lastShift.cashMinor))}
        </p>
      ) : null}

      {canOperate ? (
        <div className="flex flex-wrap gap-2">
          {state === "closed" ? (
            <Button
              data-testid="crm-cash-open-shift"
              disabled={shift.isPending}
              onClick={() => void runShift("open")}
              size="sm"
            >
              {shift.isPending ? device.opening : device.open}
            </Button>
          ) : null}
          {state === "opened" ? (
            <Button
              data-testid="crm-cash-report-x"
              disabled={reportX.isPending}
              onClick={() => void runReportX()}
              size="sm"
              variant="outline"
            >
              {device.reportX}
            </Button>
          ) : null}
          {state === "opened" || state === "expired" ? (
            <Button
              data-testid="crm-cash-close-shift"
              disabled={shift.isPending}
              onClick={() => void runShift("close")}
              size="sm"
              variant="outline"
            >
              {shift.isPending ? device.closing : device.close}
            </Button>
          ) : null}
        </div>
      ) : null}

      <CrmErrorAlert
        error={shift.error ?? reportX.error}
        testId="crm-cash-shift-error"
      />

      {settingsOpen ? (
        <CrmCashSettingsDialog onClose={() => setSettingsOpen(false)} />
      ) : null}
    </div>
  );
}
