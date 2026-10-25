import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { crmQueryRoot } from "@/features/crm/hooks";
import {
  closeCrmCashShift,
  fiscalizeCrmSale,
  getCrmCashDeviceStatus,
  getCrmCashSettings,
  listCrmCashQueue,
  listCrmCashShifts,
  openCrmCashShift,
  printCrmCashReportX,
  processCrmCashQueue,
  resolveCrmCashJob,
  setCrmCashSettings,
  type CrmCashJob,
  type CrmCashJobAction,
  type CrmCashSettings,
} from "@/shared/api/tauriCrmCash";

/** Cash-desk keys sit under the CRM root, so any CRM change refreshes them. */
const cashRoot = [...crmQueryRoot, "cash"] as const;
const cashKeys = {
  settings: [...cashRoot, "settings"] as const,
  device: [...cashRoot, "device"] as const,
  queue: [...cashRoot, "queue"] as const,
  shifts: [...cashRoot, "shifts"] as const,
};

/** How often the queue retries pending receipts while the screen is open. */
const QUEUE_RETRY_MS = 30_000;

function useInvalidateCash() {
  const queryClient = useQueryClient();
  return React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: crmQueryRoot }),
    [queryClient],
  );
}

export function useCrmCashSettingsQuery() {
  return useQuery({
    queryKey: cashKeys.settings,
    queryFn: getCrmCashSettings,
    staleTime: Number.POSITIVE_INFINITY,
  });
}

export function useSaveCrmCashSettingsMutation() {
  const invalidate = useInvalidateCash();
  return useMutation({
    mutationFn: (settings: CrmCashSettings) => setCrmCashSettings(settings),
    onSettled: invalidate,
  });
}

export function useCrmCashDeviceQuery() {
  return useQuery({
    queryKey: cashKeys.device,
    queryFn: getCrmCashDeviceStatus,
    staleTime: 10_000,
  });
}

export function useCrmCashShiftsQuery() {
  return useQuery({
    queryKey: cashKeys.shifts,
    queryFn: listCrmCashShifts,
    staleTime: 30_000,
  });
}

/**
 * The receipt queue. While mounted it drives pending and printed receipts
 * every `QUEUE_RETRY_MS`, so a receipt interrupted by a crash or a lost
 * connection is finished without a click (AGENTS.md rule 1).
 */
export function useCrmCashQueueQuery() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: cashKeys.queue,
    queryFn: listCrmCashQueue,
    staleTime: 5_000,
  });
  const hasWork = (query.data ?? []).some(
    (job) => job.state === "pending" || job.state === "printed",
  );
  React.useEffect(() => {
    if (!hasWork) return;
    const timer = window.setInterval(() => {
      void processCrmCashQueue()
        .then((jobs) => queryClient.setQueryData(cashKeys.queue, jobs))
        .catch(() => undefined);
    }, QUEUE_RETRY_MS);
    return () => window.clearInterval(timer);
  }, [hasWork, queryClient]);
  return query;
}

export function useCrmCashShiftMutation() {
  const invalidate = useInvalidateCash();
  return useMutation({
    mutationFn: (input: { channelId: string; action: "open" | "close" }) =>
      input.action === "open"
        ? openCrmCashShift(input.channelId)
        : closeCrmCashShift(input.channelId),
    onSettled: invalidate,
  });
}

export function useCrmCashReportXMutation() {
  return useMutation({ mutationFn: printCrmCashReportX });
}

export function useFiscalizeCrmSaleMutation() {
  const invalidate = useInvalidateCash();
  return useMutation({
    mutationFn: (input: {
      channelId: string;
      saleId: string;
      operation: CrmCashJob["operation"];
    }) => fiscalizeCrmSale(input),
    onSettled: invalidate,
  });
}

export function useProcessCrmCashQueueMutation() {
  const invalidate = useInvalidateCash();
  return useMutation({
    mutationFn: processCrmCashQueue,
    onSettled: invalidate,
  });
}

export function useResolveCrmCashJobMutation() {
  const invalidate = useInvalidateCash();
  return useMutation({
    mutationFn: (input: {
      operation: CrmCashJob["operation"];
      saleId: string;
      action: CrmCashJobAction;
    }) => resolveCrmCashJob(input),
    onSettled: invalidate,
  });
}
