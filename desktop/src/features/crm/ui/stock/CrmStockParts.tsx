import type * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { FieldLabel, NativeSelect } from "@/shared/ui/native-select";
import { Skeleton } from "@/shared/ui/skeleton";

export const STOCK_HEAD_CELL = "px-4 py-2.5 text-left text-xs font-medium";

/** Live warehouses of the space, default first. */
export function liveWarehouses(space: CrmMemberSpace) {
  return space.warehouses
    .filter((warehouse) => !warehouse.archived)
    .sort((a, b) => Number(b.isDefault) - Number(a.isDefault));
}

/** "All warehouses" or one of them; `null` means all. */
export function StockWarehouseFilter({
  id,
  onChange,
  space,
  value,
}: {
  id: string;
  onChange: (warehouseId: string | null) => void;
  space: CrmMemberSpace;
  value: string | null;
}) {
  return (
    <div className="w-full space-y-1.5 sm:w-56">
      <FieldLabel htmlFor={id}>{ru.stock.warehouse}</FieldLabel>
      <NativeSelect
        data-testid={id}
        id={id}
        onChange={(next) => onChange(next === "" ? null : next)}
        value={value ?? ""}
      >
        <option value="">{ru.stock.allWarehouses}</option>
        {liveWarehouses(space).map((warehouse) => (
          <option key={warehouse.id} value={warehouse.id}>
            {warehouse.name}
          </option>
        ))}
      </NativeSelect>
    </div>
  );
}

/** Loading skeleton, load error with retry, or `children`. */
export function StockQueryState({
  children,
  empty,
  errorText,
  isEmpty,
  isError,
  isPending,
  loadingText,
  onRetry,
  testId,
}: {
  children: React.ReactNode;
  empty: React.ReactNode;
  errorText: string;
  isEmpty: boolean;
  isError: boolean;
  isPending: boolean;
  loadingText: string;
  onRetry: () => void;
  testId: string;
}) {
  if (isPending) {
    return (
      <div
        aria-busy="true"
        aria-label={loadingText}
        className="space-y-2"
        data-testid={`${testId}-loading`}
        role="status"
      >
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
      </div>
    );
  }
  if (isError && isEmpty) {
    return (
      <EmptyState
        action={
          <Button onClick={onRetry} size="sm" variant="outline">
            {ru.common.retry}
          </Button>
        }
        testId={`${testId}-error`}
        title={errorText}
      />
    );
  }
  return isEmpty ? empty : children;
}

/** "Load more" under an infinite list. */
export function StockLoadMore({
  fetching,
  label,
  loadingLabel,
  onClick,
  testId,
}: {
  fetching: boolean;
  label: string;
  loadingLabel: string;
  onClick: () => void;
  testId: string;
}) {
  return (
    <div className="flex justify-center">
      <Button
        data-testid={testId}
        disabled={fetching}
        onClick={onClick}
        size="sm"
        variant="outline"
      >
        {fetching ? loadingLabel : label}
      </Button>
    </div>
  );
}
