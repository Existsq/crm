import * as React from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { CRM_KINDS } from "@/features/crm/lib/crmKinds";
import { relayClient } from "@/shared/api/relayClient";
import {
  createCrmSpace,
  getCrmSpace,
  updateCrmSpace,
  writeCrmWarehouse,
  type CrmSettings,
  type CrmSpaceState,
  type CrmWarehouseWriteInput,
} from "@/shared/api/tauriCrm";

/** Root of every CRM query key; listed in `RELAY_QUERY_ROOTS`. */
export const crmQueryRoot = ["crm"] as const;
export const crmSpaceQueryKey = ["crm", "space"] as const;

/** Burst window for live-event invalidation. */
const LIVE_INVALIDATE_DEBOUNCE_MS = 300;

export function useCrmSpaceQuery() {
  return useQuery<CrmSpaceState>({
    queryKey: crmSpaceQueryKey,
    queryFn: getCrmSpace,
    staleTime: 30_000,
  });
}

function useInvalidateCrm() {
  const queryClient = useQueryClient();
  return React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: crmQueryRoot }),
    [queryClient],
  );
}

export function useCreateCrmSpaceMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: createCrmSpace,
    onSettled: invalidate,
  });
}

export function useUpdateCrmSettingsMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: {
      channelId: string;
      settings: CrmSettings;
      expectedRevision: string;
    }) => updateCrmSpace(input),
    onSettled: invalidate,
  });
}

export function useWriteCrmWarehouseMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmWarehouseWriteInput) => writeCrmWarehouse(input),
    onSettled: invalidate,
  });
}

/**
 * Keep CRM queries fresh while the CRM is open: any CRM event in the space
 * channel (from this or another client) invalidates the CRM query root,
 * debounced so a burst of postings refetches once.
 */
export function useCrmLiveUpdates(spaceChannelId: string | null): void {
  const queryClient = useQueryClient();

  React.useEffect(() => {
    if (!spaceChannelId) return;
    let disposed = false;
    let dispose: (() => Promise<void>) | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;

    const invalidate = () => {
      if (timer !== null) return;
      timer = setTimeout(() => {
        timer = null;
        void queryClient.invalidateQueries({ queryKey: crmQueryRoot });
      }, LIVE_INVALIDATE_DEBOUNCE_MS);
    };

    void relayClient
      .subscribeLive(
        { kinds: [...CRM_KINDS], "#h": [spaceChannelId], limit: 0 },
        invalidate,
      )
      .then((unsubscribe) => {
        if (disposed) {
          void unsubscribe();
        } else {
          dispose = unsubscribe;
        }
      })
      .catch((error) => {
        console.error("Couldn’t subscribe to CRM updates", error);
      });

    const unsubscribeReconnect = relayClient.subscribeToReconnects(invalidate);

    return () => {
      disposed = true;
      if (timer !== null) clearTimeout(timer);
      unsubscribeReconnect();
      if (dispose) void dispose();
    };
  }, [spaceChannelId, queryClient]);
}
