import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { crmQueryRoot } from "@/features/crm/hooks";
import { listCrmProducts } from "@/shared/api/tauriCrmProducts";
import {
  getCrmSchedule,
  listCrmStaff,
  setCrmSchedule,
  writeCrmStaff,
  type CrmScheduleInput,
  type CrmStaffWriteInput,
} from "@/shared/api/tauriCrmStaff";

/** Staff keys sit under the CRM root, so live CRM events refresh them. */
const staffRoot = [...crmQueryRoot, "staff"] as const;

/** Most services a staff dialog offers (the relay's page bound). */
const SERVICE_CHOICES_LIMIT = 200;

function useInvalidateCrm() {
  const queryClient = useQueryClient();
  return React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: crmQueryRoot }),
    [queryClient],
  );
}

export function useCrmStaffQuery(archived: boolean) {
  return useQuery({
    queryKey: [...staffRoot, "list", archived],
    queryFn: () => listCrmStaff(archived),
    staleTime: 30_000,
  });
}

/** Working days `from..=to` of every staff member. */
export function useCrmScheduleQuery(from: string, to: string) {
  return useQuery({
    queryKey: [...staffRoot, "schedule", from, to],
    queryFn: () => getCrmSchedule(from, to),
    staleTime: 30_000,
  });
}

/** Active services, for assigning them to staff. */
export function useCrmServiceChoicesQuery(enabled: boolean) {
  return useQuery({
    queryKey: [...crmQueryRoot, "products", "service-choices"],
    queryFn: async () =>
      (
        await listCrmProducts({
          kind: "services",
          limit: SERVICE_CHOICES_LIMIT,
        })
      ).products,
    enabled,
    staleTime: 30_000,
  });
}

export function useWriteCrmStaffMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmStaffWriteInput) => writeCrmStaff(input),
    onSettled: invalidate,
  });
}

export function useSetCrmScheduleMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmScheduleInput) => setCrmSchedule(input),
    onSettled: invalidate,
  });
}
