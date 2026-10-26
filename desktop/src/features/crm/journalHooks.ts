import * as React from "react";
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { crmQueryRoot } from "@/features/crm/hooks";
import {
  type CrmAppointmentWriteInput,
  getCrmSlots,
  getCrmVisitAnalytics,
  listCrmAppointments,
  listCrmCustomerAppointments,
  writeCrmAppointment,
} from "@/shared/api/tauriCrmAppointments";

/** Journal keys sit under the CRM root, so live CRM events refresh them. */
const journalRoot = [...crmQueryRoot, "journal"] as const;

/** Visits `from..=to` of every staff member. */
export function useCrmAppointmentsQuery(from: string, to: string) {
  return useQuery({
    queryKey: [...journalRoot, "range", from, to],
    queryFn: () => listCrmAppointments({ from, to }),
    placeholderData: keepPreviousData,
    staleTime: 15_000,
  });
}

/** Every visit of one client. */
export function useCrmCustomerAppointmentsQuery(customerId: string | null) {
  return useQuery({
    queryKey: [...journalRoot, "customer", customerId],
    queryFn: () => listCrmCustomerAppointments(customerId ?? ""),
    enabled: customerId !== null,
    staleTime: 15_000,
  });
}

/** Free start times for `serviceIds` on `date`; idle until services are picked. */
export function useCrmSlotsQuery(request: {
  date: string;
  serviceIds: readonly string[];
  staffId: string | null;
}) {
  const enabled =
    request.date.length > 0 &&
    request.serviceIds.length > 0 &&
    request.staffId !== null;
  return useQuery({
    queryKey: [
      ...journalRoot,
      "slots",
      request.date,
      request.staffId,
      [...request.serviceIds],
    ],
    queryFn: () =>
      getCrmSlots({
        date: request.date,
        serviceIds: [...request.serviceIds],
        staffId: request.staffId,
      }),
    enabled,
    staleTime: 5_000,
  });
}

export function useWriteCrmAppointmentMutation() {
  const queryClient = useQueryClient();
  const invalidate = React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: crmQueryRoot }),
    [queryClient],
  );
  return useMutation({
    mutationFn: (input: CrmAppointmentWriteInput) => writeCrmAppointment(input),
    onSettled: invalidate,
  });
}

/** Visit analytics for one period. */
export function useCrmVisitAnalyticsQuery(period: {
  from: string | null;
  to: string | null;
}) {
  return useQuery({
    queryKey: [...journalRoot, "analytics", period.from, period.to],
    queryFn: () => getCrmVisitAnalytics(period),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}
