import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { crmQueryRoot } from "@/features/crm/hooks";
import {
  adjustCrmBonuses,
  type CrmBonusAdjustInput,
  type CrmLoyaltyInput,
  getCrmLoyalty,
  setCrmLoyalty,
} from "@/shared/api/tauriCrmLoyalty";

/** Loyalty keys sit under the CRM root, so live CRM events refresh them. */
const loyaltyRoot = [...crmQueryRoot, "loyalty"] as const;

/** The bonus programme and, with a customer, their balance and history. */
export function useCrmLoyaltyQuery(customerId: string | null) {
  return useQuery({
    queryKey: [...loyaltyRoot, customerId],
    queryFn: () => getCrmLoyalty(customerId),
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

export function useSetCrmLoyaltyMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmLoyaltyInput) => setCrmLoyalty(input),
    onSettled: invalidate,
  });
}

export function useAdjustCrmBonusesMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmBonusAdjustInput) => adjustCrmBonuses(input),
    onSettled: invalidate,
  });
}
