import { CrmSettingsForm } from "@/features/crm/ui/CrmSettingsForm";
import { CrmWarehousesCard } from "@/features/crm/ui/CrmWarehousesCard";
import { CrmLoyaltyCard } from "@/features/crm/ui/loyalty/CrmLoyaltyCard";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";

export function CrmSettingsSection({ space }: { space: CrmMemberSpace }) {
  return (
    <div className="grid gap-6 [@container(min-width:56rem)]:grid-cols-2">
      <CrmSettingsForm space={space} />
      <CrmWarehousesCard space={space} />
      <CrmLoyaltyCard space={space} />
    </div>
  );
}
