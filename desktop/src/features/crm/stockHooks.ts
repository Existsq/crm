import * as React from "react";
import {
  keepPreviousData,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { crmQueryRoot } from "@/features/crm/hooks";
import {
  getCrmStockDoc,
  listCrmStock,
  listCrmStockDocs,
  listCrmStockMoves,
  listCrmTechCards,
  postCrmStockDoc,
  voidCrmStockDoc,
  writeCrmTechCard,
  type CrmStockDocInput,
  type CrmStockDocType,
  type CrmTechCardInput,
} from "@/shared/api/tauriCrmStock";

/** Stock keys sit under the CRM root, so live CRM events refresh them. */
const stockRoot = [...crmQueryRoot, "stock"] as const;

function useInvalidateCrm() {
  const queryClient = useQueryClient();
  return React.useCallback(
    () => queryClient.invalidateQueries({ queryKey: crmQueryRoot }),
    [queryClient],
  );
}

export function useCrmStockQuery(filter: {
  warehouseId: string | null;
  q: string;
  low: boolean;
  enabled?: boolean;
}) {
  return useQuery({
    queryKey: [
      ...stockRoot,
      "levels",
      filter.warehouseId,
      filter.q,
      filter.low,
    ],
    queryFn: () =>
      listCrmStock({
        warehouseId: filter.warehouseId,
        query: filter.q,
        low: filter.low,
      }),
    enabled: filter.enabled ?? true,
    placeholderData: keepPreviousData,
    staleTime: 15_000,
  });
}

/** The movement journal, newest first. */
export function useCrmStockMovesQuery(filter: {
  warehouseId: string | null;
  productId: string | null;
}) {
  return useInfiniteQuery({
    queryKey: [...stockRoot, "moves", filter.warehouseId, filter.productId],
    queryFn: ({ pageParam }) =>
      listCrmStockMoves({
        warehouseId: filter.warehouseId,
        productId: filter.productId,
        before: pageParam,
      }),
    initialPageParam: null as number | null,
    getNextPageParam: (lastPage) => lastPage.next ?? undefined,
    placeholderData: keepPreviousData,
    staleTime: 15_000,
  });
}

export function useCrmStockDocsQuery(type: CrmStockDocType | null) {
  return useInfiniteQuery({
    queryKey: [...stockRoot, "docs", type],
    queryFn: ({ pageParam }) => listCrmStockDocs({ type, before: pageParam }),
    initialPageParam: null as number | null,
    getNextPageParam: (lastPage) => lastPage.next ?? undefined,
    placeholderData: keepPreviousData,
    staleTime: 15_000,
  });
}

export function useCrmStockDocQuery(docId: string | null) {
  return useQuery({
    queryKey: [...stockRoot, "doc", docId],
    queryFn: () => getCrmStockDoc(docId as string),
    enabled: docId !== null,
    staleTime: 15_000,
  });
}

export function useCrmTechCardsQuery() {
  return useQuery({
    queryKey: [...stockRoot, "tech-cards"],
    queryFn: listCrmTechCards,
    staleTime: 30_000,
  });
}

export function usePostCrmStockDocMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmStockDocInput) => postCrmStockDoc(input),
    onSettled: invalidate,
  });
}

export function useVoidCrmStockDocMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: { channelId: string; docId: string; reason: string }) =>
      voidCrmStockDoc(input),
    onSettled: invalidate,
  });
}

export function useWriteCrmTechCardMutation() {
  const invalidate = useInvalidateCrm();
  return useMutation({
    mutationFn: (input: CrmTechCardInput) => writeCrmTechCard(input),
    onSettled: invalidate,
  });
}
