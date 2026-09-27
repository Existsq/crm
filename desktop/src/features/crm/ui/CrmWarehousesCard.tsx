import { Archive, MoreHorizontal, Pencil, Plus, Star } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { useWriteCrmWarehouseMutation } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { CrmWarehouseDialog } from "@/features/crm/ui/CrmWarehouseDialog";
import type { CrmMemberSpace, CrmWarehouse } from "@/shared/api/tauriCrm";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { SectionHeader } from "@/shared/ui/PageHeader";

type DialogState =
  | { mode: "closed" }
  // The id is chosen once per dialog so a retried create reuses it.
  | { mode: "create"; warehouseId: string }
  | { mode: "rename"; warehouse: CrmWarehouse };

export function CrmWarehousesCard({ space }: { space: CrmMemberSpace }) {
  const canManage = space.capabilities.has("manage_warehouses");
  const write = useWriteCrmWarehouseMutation();
  const [dialog, setDialog] = React.useState<DialogState>({ mode: "closed" });

  async function save(
    warehouseId: string,
    warehouse: CrmWarehouse | null,
    patch: { name?: string; isDefault?: boolean; archived?: boolean },
    successMessage: string,
  ) {
    try {
      await write.mutateAsync({
        channelId: space.channelId,
        warehouseId,
        name: patch.name ?? warehouse?.name ?? "",
        isDefault: patch.isDefault ?? warehouse?.isDefault ?? false,
        archived: patch.archived ?? warehouse?.archived ?? false,
        expectedRevision: warehouse?.revision ?? null,
      });
      toast.success(successMessage);
      setDialog({ mode: "closed" });
    } catch {
      // Rendered through the mutation error state.
    }
  }

  const dialogOpen = dialog.mode !== "closed";

  return (
    <section
      aria-labelledby="crm-warehouses-title"
      className="space-y-4 rounded-2xl border border-border/70 p-5"
      data-testid="crm-warehouses"
    >
      <SectionHeader
        action={
          canManage ? (
            <Button
              data-testid="crm-warehouse-add"
              onClick={() => {
                write.reset();
                setDialog({
                  mode: "create",
                  warehouseId: crypto.randomUUID(),
                });
              }}
              size="sm"
              variant="outline"
            >
              <Plus aria-hidden="true" className="h-4 w-4" />
              {ru.warehouses.add}
            </Button>
          ) : undefined
        }
        title={<span id="crm-warehouses-title">{ru.warehouses.title}</span>}
      />

      {space.warehouses.length === 0 ? (
        <p className="text-sm text-muted-foreground">{ru.warehouses.empty}</p>
      ) : (
        <ul className="divide-y divide-border/60">
          {space.warehouses.map((warehouse) => (
            <li
              className="flex items-center justify-between gap-3 py-2.5"
              data-testid={`crm-warehouse-${warehouse.id}`}
              key={warehouse.id}
            >
              <div className="flex min-w-0 items-center gap-2">
                <span className="truncate text-sm">{warehouse.name}</span>
                {warehouse.isDefault ? (
                  <Badge variant="secondary">
                    {ru.warehouses.defaultBadge}
                  </Badge>
                ) : null}
              </div>
              {canManage ? (
                <DropdownMenu>
                  <DropdownMenuTrigger asChild>
                    <Button
                      aria-label={`${ru.warehouses.actionsLabel}: ${warehouse.name}`}
                      size="icon-xs"
                      variant="ghost"
                    >
                      <MoreHorizontal className="h-4 w-4" />
                    </Button>
                  </DropdownMenuTrigger>
                  <DropdownMenuContent align="end">
                    <DropdownMenuItem
                      onSelect={() => {
                        write.reset();
                        setDialog({ mode: "rename", warehouse });
                      }}
                    >
                      <Pencil aria-hidden="true" className="h-4 w-4" />
                      {ru.warehouses.rename}
                    </DropdownMenuItem>
                    {!warehouse.isDefault ? (
                      <>
                        <DropdownMenuItem
                          data-testid="crm-warehouse-make-default"
                          onSelect={() =>
                            void save(
                              warehouse.id,
                              warehouse,
                              { isDefault: true },
                              ru.warehouses.updated,
                            )
                          }
                        >
                          <Star aria-hidden="true" className="h-4 w-4" />
                          {ru.warehouses.makeDefault}
                        </DropdownMenuItem>
                        <DropdownMenuItem
                          onSelect={() =>
                            void save(
                              warehouse.id,
                              warehouse,
                              { archived: true },
                              ru.warehouses.archived,
                            )
                          }
                        >
                          <Archive aria-hidden="true" className="h-4 w-4" />
                          {ru.warehouses.archive}
                        </DropdownMenuItem>
                      </>
                    ) : null}
                  </DropdownMenuContent>
                </DropdownMenu>
              ) : null}
            </li>
          ))}
        </ul>
      )}

      {!dialogOpen ? (
        <CrmErrorAlert error={write.error} testId="crm-warehouses-error" />
      ) : null}

      <CrmWarehouseDialog
        error={dialogOpen ? write.error : null}
        initialName={dialog.mode === "rename" ? dialog.warehouse.name : ""}
        mode={dialog.mode === "rename" ? "rename" : "create"}
        onOpenChange={(open) => {
          if (!open) setDialog({ mode: "closed" });
        }}
        onSubmit={(name) => {
          if (dialog.mode === "rename") {
            void save(
              dialog.warehouse.id,
              dialog.warehouse,
              { name },
              ru.warehouses.updated,
            );
          } else if (dialog.mode === "create") {
            void save(
              dialog.warehouseId,
              null,
              { name },
              ru.warehouses.created,
            );
          }
        }}
        open={dialogOpen}
        pending={write.isPending}
      />
    </section>
  );
}
