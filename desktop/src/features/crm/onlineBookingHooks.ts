import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { crmQueryRoot } from "@/features/crm/hooks";
import {
  type CrmOnlineBookingWriteInput,
  getCrmOnlineBooking,
  writeCrmOnlineBooking,
} from "@/shared/api/tauriCrmOnlineBooking";

/** Online booking keys sit under the CRM root, so live CRM events refresh them. */
const onlineBookingKey = [...crmQueryRoot, "online-booking"] as const;

/** The space's online booking settings, revision and public page URL. */
export function useCrmOnlineBookingQuery() {
  return useQuery({
    queryKey: onlineBookingKey,
    queryFn: getCrmOnlineBooking,
    staleTime: 30_000,
  });
}

export function useWriteCrmOnlineBookingMutation() {
  const queryClient = useQueryClient();
  const invalidate = React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: crmQueryRoot }),
    [queryClient],
  );
  return useMutation({
    mutationFn: (input: CrmOnlineBookingWriteInput) =>
      writeCrmOnlineBooking(input),
    onSettled: invalidate,
  });
}
