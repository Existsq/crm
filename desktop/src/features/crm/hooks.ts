import * as React from "react";
import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

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
import {
  listCrmProducts,
  writeCrmProduct,
  type CrmProductKind,
  type CrmProductWriteInput,
} from "@/shared/api/tauriCrmProducts";
import {
  getCrmAnalytics,
  getCrmSale,
  listCrmParties,
  listCrmSales,
  postCrmSale,
  voidCrmSale,
  writeCrmParty,
  type CrmPartyRole,
  type CrmPartyWriteInput,
  type CrmSaleInput,
} from "@/shared/api/tauriCrmSales";

/** Root of every CRM query key; listed in `RELAY_QUERY_ROOTS`. */
export const crmQueryRoot = ["crm"] as const;
export const crmSpaceQueryKey = ["crm", "space"] as const;

/** Burst window for live-event invalidation. */
const LIVE_INVALIDATE_DEBOUNCE_MS = 300;
/** Products per `GET /crm/products` page ("Load more" fetches the next). */
export const CRM_PRODUCTS_PAGE_SIZE = 50;
/** Parties and sales per page ("Load more" fetches the next). */
export const CRM_PARTIES_PAGE_SIZE = 50;
export const CRM_SALES_PAGE_SIZE = 50;

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
 * Catalog pages for a search. Keyed under the CRM root, so live CRM events
 * and writes refetch it; the previous result stays on screen while a new
 * search loads.
 */
export function useCrmProductsQuery(filter: {
  q: string;
  archived: boolean;
  kind?: CrmProductKind;
}) {
  return useInfiniteQuery({
    queryKey: [
      "crm",
      "products",
      filter.kind ?? "all",
      filter.q,
      filter.archived,
    ],
    queryFn: ({ pageParam }) =>
      listCrmProducts({
        query: filter.q,
        archived: filter.archived,
        kind: filter.kind,
        limit: CRM_PRODUCTS_PAGE_SIZE,
        after: pageParam,
      }),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.next ?? undefined,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

export function useWriteCrmProductMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmProductWriteInput) => writeCrmProduct(input),
    onSettled: invalidate,
  });
}

/** Customer/supplier pages for a search and role filter. */
export function useCrmPartiesQuery(filter: {
  q: string;
  role: CrmPartyRole | null;
  archived: boolean;
}) {
  return useInfiniteQuery({
    queryKey: ["crm", "parties", filter.q, filter.role, filter.archived],
    queryFn: ({ pageParam }) =>
      listCrmParties({
        query: filter.q,
        role: filter.role,
        archived: filter.archived,
        limit: CRM_PARTIES_PAGE_SIZE,
        after: pageParam,
      }),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.next ?? undefined,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

export function useWriteCrmPartyMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmPartyWriteInput) => writeCrmParty(input),
    onSettled: invalidate,
  });
}

/** The sale journal, newest first; `customerId` narrows to one buyer. */
export function useCrmSalesQuery(filter: {
  customerId: string | null;
  voided: boolean;
  limit?: number;
}) {
  return useInfiniteQuery({
    queryKey: ["crm", "sales", filter.customerId, filter.voided, filter.limit],
    queryFn: ({ pageParam }) =>
      listCrmSales({
        customerId: filter.customerId,
        voided: filter.voided,
        limit: filter.limit ?? CRM_SALES_PAGE_SIZE,
        before: pageParam,
      }),
    initialPageParam: null as number | null,
    getNextPageParam: (lastPage) => lastPage.next ?? undefined,
    placeholderData: keepPreviousData,
    staleTime: 30_000,
  });
}

/** One sale with its lines. */
export function useCrmSaleQuery(saleId: string | null) {
  return useQuery({
    queryKey: ["crm", "sale", saleId],
    queryFn: () => getCrmSale(saleId as string),
    enabled: saleId !== null,
    staleTime: 30_000,
  });
}

export function usePostCrmSaleMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmSaleInput) => postCrmSale(input),
    onSettled: invalidate,
  });
}

export function useVoidCrmSaleMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: {
      channelId: string;
      saleId: string;
      reason: string;
    }) => voidCrmSale(input),
    onSettled: invalidate,
  });
}

/** Sales and customer analytics for whole days in the space timezone. */
export function useCrmAnalyticsQuery(period: {
  from: string | null;
  to: string | null;
}) {
  return useQuery({
    queryKey: ["crm", "analytics", period.from, period.to],
    queryFn: () => getCrmAnalytics(period),
    placeholderData: keepPreviousData,
    staleTime: 30_000,
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
