import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { FieldLabel } from "@/shared/ui/native-select";

/** Create or rename a warehouse; the caller owns the mutation. */
export function CrmWarehouseDialog({
  error,
  initialName,
  mode,
  onOpenChange,
  onSubmit,
  open,
  pending,
}: {
  error: unknown;
  initialName: string;
  mode: "create" | "rename";
  onOpenChange: (open: boolean) => void;
  onSubmit: (name: string) => void;
  open: boolean;
  pending: boolean;
}) {
  const [name, setName] = React.useState(initialName);
  const [validation, setValidation] = React.useState<string | null>(null);

  React.useEffect(() => {
    if (open) {
      setName(initialName);
      setValidation(null);
    }
  }, [initialName, open]);

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) {
      setValidation(ru.errors.nameRequired);
      return;
    }
    onSubmit(trimmed);
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent data-testid="crm-warehouse-dialog">
        <form className="space-y-4" onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>
              {mode === "create"
                ? ru.warehouses.createTitle
                : ru.warehouses.renameTitle}
            </DialogTitle>
          </DialogHeader>
          <div className="space-y-1.5">
            <FieldLabel htmlFor="crm-warehouse-dialog-name">
              {ru.warehouses.nameLabel}
            </FieldLabel>
            <Input
              autoFocus
              data-testid="crm-warehouse-dialog-name"
              id="crm-warehouse-dialog-name"
              maxLength={128}
              onChange={(event) => setName(event.target.value)}
              value={name}
            />
          </div>
          {validation ? (
            <p className="text-xs text-destructive" role="alert">
              {validation}
            </p>
          ) : null}
          <CrmErrorAlert error={error} />
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-warehouse-dialog-submit"
              disabled={pending}
              type="submit"
            >
              {pending ? ru.common.saving : ru.common.save}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
