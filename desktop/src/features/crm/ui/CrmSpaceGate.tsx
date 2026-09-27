import type { UseQueryResult } from "@tanstack/react-query";
import { Lock, Store } from "lucide-react";
import type * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { CrmCreateSpaceForm } from "@/features/crm/ui/CrmCreateSpaceForm";
import type { CrmMemberSpace, CrmSpaceState } from "@/shared/api/tauriCrm";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { Skeleton } from "@/shared/ui/skeleton";

/**
 * Renders `children` only for members of the CRM space; every other state
 * (loading, error, no space, no access) gets an explicit screen so the user
 * always knows the next step.
 */
export function CrmSpaceGate({
  children,
  query,
}: {
  children: (space: CrmMemberSpace) => React.ReactNode;
  query: UseQueryResult<CrmSpaceState>;
}) {
  if (query.isPending) {
    return (
      <div
        aria-busy="true"
        aria-label={ru.gate.loading}
        className="space-y-3"
        data-testid="crm-gate-loading"
        role="status"
      >
        <Skeleton className="h-10 w-64" />
        <Skeleton className="h-40 w-full" />
      </div>
    );
  }

  if (query.isError) {
    return (
      <EmptyState
        action={
          <Button
            onClick={() => void query.refetch()}
            size="sm"
            variant="outline"
          >
            {ru.common.retry}
          </Button>
        }
        testId="crm-gate-error"
        title={ru.gate.loadError}
      />
    );
  }

  const space = query.data;
  switch (space.status) {
    case "none":
      return space.canCreate ? (
        <CrmCreateSpaceForm />
      ) : (
        <EmptyState
          description={ru.gate.noSpaceAskAdmin}
          icon={<Store className="h-10 w-10" />}
          testId="crm-gate-no-space"
          title={ru.gate.noSpaceTitle}
        />
      );
    case "no-access":
      return (
        <EmptyState
          description={ru.gate.noAccessBody}
          icon={<Lock className="h-10 w-10" />}
          testId="crm-gate-no-access"
          title={ru.gate.noAccessTitle}
        />
      );
    case "member":
      return <>{children(space)}</>;
  }
}
