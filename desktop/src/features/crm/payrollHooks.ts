import * as React from "react";
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { crmQueryRoot } from "@/features/crm/hooks";
import {
  type CrmPayRatesInput,
  type CrmPayoutInput,
  getCrmPayroll,
  recordCrmPayout,
  setCrmPayRates,
  voidCrmPayout,
} from "@/shared/api/tauriCrmPayroll";

/** Payroll keys sit under the CRM root, so live CRM events refresh them. */
const payrollRoot = [...crmQueryRoot, "payroll"] as const;

/** Accruals and payouts for one period; idle for callers without access. */
export function useCrmPayrollQuery(
  period: { from: string; to: string },
  enabled: boolean,
) {
  return useQuery({
    queryKey: [...payrollRoot, period.from, period.to],
    queryFn: () => getCrmPayroll(period),
    enabled,
    placeholderData: keepPreviousData,
    staleTime: 15_000,
  });
}

function useInvalidateCrm() {
  const queryClient = useQueryClient();
  return React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: crmQueryRoot }),
    [queryClient],
  );
}

export function useSetCrmPayRatesMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmPayRatesInput) => setCrmPayRates(input),
    onSettled: invalidate,
  });
}

export function useRecordCrmPayoutMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmPayoutInput) => recordCrmPayout(input),
    onSettled: invalidate,
  });
}

export function useVoidCrmPayoutMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: { channelId: string; payoutId: string }) =>
      voidCrmPayout(input.channelId, input.payoutId),
    onSettled: invalidate,
  });
}
