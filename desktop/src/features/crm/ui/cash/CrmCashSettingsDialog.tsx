import * as React from "react";
import { toast } from "sonner";

import {
  useCrmCashSettingsQuery,
  useSaveCrmCashSettingsMutation,
} from "@/features/crm/cashHooks";
import { ru } from "@/features/crm/i18n/ru";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import type { CrmCashSettings } from "@/shared/api/tauriCrmCash";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";

function SettingsForm({
  initial,
  onClose,
}: {
  initial: CrmCashSettings;
  onClose: () => void;
}) {
  const copy = ru.cash.settings;
  const save = useSaveCrmCashSettingsMutation();
  const [baseUrl, setBaseUrl] = React.useState(initial.baseUrl);
  const [operatorName, setOperatorName] = React.useState(initial.operatorName);

  async function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      await save.mutateAsync({ baseUrl, operatorName });
      toast.success(copy.saved);
      onClose();
    } catch {
      // Shown below through the mutation error.
    }
  }

  return (
    <form className="space-y-4" noValidate onSubmit={(e) => void submit(e)}>
      <CrmField
        hint={copy.baseUrlHint}
        id="crm-cash-base-url"
        label={copy.baseUrl}
      >
        <Input
          {...crmControlProps("crm-cash-base-url", true, undefined)}
          inputMode="url"
          onChange={(event) => setBaseUrl(event.target.value)}
          value={baseUrl}
        />
      </CrmField>
      <CrmField id="crm-cash-operator" label={copy.operatorName}>
        <Input
          {...crmControlProps("crm-cash-operator", false, undefined)}
          maxLength={64}
          onChange={(event) => setOperatorName(event.target.value)}
          value={operatorName}
        />
      </CrmField>
      <CrmErrorAlert error={save.error} testId="crm-cash-settings-error" />
      <DialogFooter>
        <Button onClick={onClose} type="button" variant="ghost">
          {ru.common.cancel}
        </Button>
        <Button
          data-testid="crm-cash-settings-save"
          disabled={save.isPending}
          type="submit"
        >
          {save.isPending ? ru.common.saving : ru.common.save}
        </Button>
      </DialogFooter>
    </form>
  );
}

/** Where the DTO10 web server runs and the cashier name on receipts. */
export function CrmCashSettingsDialog({ onClose }: { onClose: () => void }) {
  const settings = useCrmCashSettingsQuery();
  return (
    <Dialog onOpenChange={(open) => !open && onClose()} open>
      <DialogContent data-testid="crm-cash-settings-dialog">
        <DialogHeader>
          <DialogTitle>{ru.cash.settings.title}</DialogTitle>
        </DialogHeader>
        {settings.data ? (
          <SettingsForm initial={settings.data} onClose={onClose} />
        ) : (
          <CrmErrorAlert error={settings.error} />
        )}
      </DialogContent>
    </Dialog>
  );
}
