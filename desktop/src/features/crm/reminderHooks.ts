import * as React from "react";
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { crmQueryRoot } from "@/features/crm/hooks";
import {
  type CrmClientTelegramWriteInput,
  type CrmReminderSettingsWriteInput,
  getCrmClientTelegram,
  getCrmReminders,
  listCrmReminderDeliveries,
  writeCrmClientTelegram,
  writeCrmReminderSettings,
} from "@/shared/api/tauriCrmReminders";

/** Reminder keys sit under the CRM root, so live CRM events refresh them. */
const remindersRoot = [...crmQueryRoot, "reminders"] as const;

/** The space's reminder rules and the relay's transport. */
export function useCrmRemindersQuery() {
  return useQuery({
    queryKey: [...remindersRoot, "settings"],
    queryFn: getCrmReminders,
    staleTime: 30_000,
  });
}

/** The delivery log planned since `since`. */
export function useCrmReminderDeliveriesQuery(since: string) {
  return useQuery({
    queryKey: [...remindersRoot, "deliveries", since],
    queryFn: () => listCrmReminderDeliveries({ since, limit: 200 }),
    placeholderData: keepPreviousData,
    // The worker sends on its own; poll so new messages show up.
    refetchInterval: 30_000,
    staleTime: 10_000,
  });
}

/**
 * Where one customer gets reminders; idle until a customer is open. The bot
 * links a client without a CRM event, so an unlinked card polls until the
 * client opens their invite.
 */
export function useCrmClientTelegramQuery(partyId: string | null) {
  return useQuery({
    queryKey: [...remindersRoot, "telegram", partyId],
    queryFn: () => getCrmClientTelegram(partyId ?? ""),
    enabled: partyId !== null,
    staleTime: 15_000,
    refetchInterval: (query) => (query.state.data?.linked ? false : 5_000),
  });
}

function useInvalidateCrm() {
  const queryClient = useQueryClient();
  return React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: crmQueryRoot }),
    [queryClient],
  );
}

export function useWriteCrmReminderSettingsMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmReminderSettingsWriteInput) =>
      writeCrmReminderSettings(input),
    onSettled: invalidate,
  });
}

export function useWriteCrmClientTelegramMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmClientTelegramWriteInput) =>
      writeCrmClientTelegram(input),
    onSettled: invalidate,
  });
}
