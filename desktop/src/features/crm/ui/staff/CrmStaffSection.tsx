import { Contact, Pencil, Plus } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import {
  useCrmServiceChoicesQuery,
  useCrmStaffQuery,
  useWriteCrmStaffMutation,
} from "@/features/crm/staffHooks";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { CrmStaffDialog } from "@/features/crm/ui/staff/CrmStaffDialog";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmStaff, CrmStaffFields } from "@/shared/api/tauriCrmStaff";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Skeleton } from "@/shared/ui/skeleton";
import { Switch } from "@/shared/ui/switch";

const HEAD_CELL = "px-4 py-2.5 text-left text-xs font-medium";
/** Service names shown per row before "and N more". */
const SERVICES_SHOWN = 3;

type DialogState = {
  open: boolean;
  session: number;
  staff: CrmStaff | null;
  /** Id written by this dialog, chosen at opening for a create. */
  staffId: string;
};

export function CrmStaffSection({ space }: { space: CrmMemberSpace }) {
  const canEdit = space.capabilities.has("edit_catalog");
  const [showArchived, setShowArchived] = React.useState(false);
  const query = useCrmStaffQuery(showArchived);
  const services = useCrmServiceChoicesQuery(true);
  const write = useWriteCrmStaffMutation();
  const [dialog, setDialog] = React.useState<DialogState>({
    open: false,
    session: 0,
    staff: null,
    staffId: "",
  });
  const addButtonRef = React.useRef<HTMLButtonElement>(null);
  const returnFocusRef = React.useRef<HTMLElement | null>(null);

  const staff = query.data ?? [];
  const serviceNames = React.useMemo(
    () =>
      new Map(
        (services.data ?? []).map((service) => [service.id, service.name]),
      ),
    [services.data],
  );
  const dialogStaff = dialog.staff
    ? (staff.find((s) => s.id === dialog.staff?.id) ?? dialog.staff)
    : null;

  function openDialog(member: CrmStaff | null, opener: HTMLElement) {
    returnFocusRef.current = opener;
    write.reset();
    setDialog((current) => ({
      open: true,
      session: current.session + 1,
      staff: member,
      staffId: member?.id ?? crypto.randomUUID(),
    }));
  }

  async function save(fields: CrmStaffFields, expectedRevision: string | null) {
    try {
      await write.mutateAsync({
        ...fields,
        channelId: space.channelId,
        staffId: dialog.staffId,
        expectedRevision,
      });
      toast.success(dialogStaff ? ru.staff.updated : ru.staff.created);
      setDialog((current) => ({ ...current, open: false }));
    } catch {
      // Rendered in the dialog through the mutation error state.
    }
  }

  return (
    <section
      aria-labelledby="crm-staff-title"
      className="space-y-4"
      data-testid="crm-staff"
    >
      <SectionHeader
        action={
          canEdit ? (
            <Button
              data-testid="crm-staff-add"
              onClick={(event) => openDialog(null, event.currentTarget)}
              ref={addButtonRef}
              size="sm"
            >
              <Plus aria-hidden="true" className="h-4 w-4" />
              {ru.staff.add}
            </Button>
          ) : undefined
        }
        description={canEdit ? undefined : ru.staff.readOnlyHint}
        title={<span id="crm-staff-title">{ru.staff.title}</span>}
      />

      <div className="flex items-center gap-2">
        <Switch
          checked={showArchived}
          data-testid="crm-staff-show-archived"
          id="crm-staff-show-archived"
          onCheckedChange={setShowArchived}
        />
        <label className="text-sm" htmlFor="crm-staff-show-archived">
          {ru.staff.showArchived}
        </label>
      </div>

      {query.isPending ? (
        <div
          aria-busy="true"
          aria-label={ru.staff.loading}
          className="space-y-2"
          role="status"
        >
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : query.isError ? (
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
          testId="crm-staff-error"
          title={ru.staff.loadError}
        />
      ) : staff.length === 0 ? (
        <EmptyState
          description={canEdit ? ru.staff.emptyCanEdit : ru.staff.emptyReadOnly}
          icon={<Contact className="h-10 w-10" />}
          testId="crm-staff-empty"
          title={ru.staff.emptyTitle}
        />
      ) : (
        <div className="overflow-x-auto rounded-2xl border border-border/70">
          <table
            className="w-full min-w-[36rem] border-collapse text-sm"
            data-testid="crm-staff-table"
          >
            <caption className="sr-only">{ru.staff.tableLabel}</caption>
            <thead className="bg-muted/40 text-muted-foreground">
              <tr>
                <th className={HEAD_CELL} scope="col">
                  {ru.staff.columns.name}
                </th>
                <th className={HEAD_CELL} scope="col">
                  {ru.staff.columns.phone}
                </th>
                <th className={HEAD_CELL} scope="col">
                  {ru.staff.columns.services}
                </th>
                {canEdit ? (
                  <th className={HEAD_CELL} scope="col">
                    <span className="sr-only">{ru.staff.columns.actions}</span>
                  </th>
                ) : null}
              </tr>
            </thead>
            <tbody className="divide-y divide-border/60">
              {staff.map((member) => {
                const names = member.serviceIds
                  .map((id) => serviceNames.get(id))
                  .filter((name): name is string => Boolean(name));
                const hidden = names.length - SERVICES_SHOWN;
                return (
                  <tr data-testid={`crm-staff-${member.id}`} key={member.id}>
                    <th
                      className="px-4 py-2.5 text-left font-normal"
                      scope="row"
                    >
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium">{member.name}</span>
                        {member.archived ? (
                          <Badge variant="secondary">
                            {ru.staff.archivedBadge}
                          </Badge>
                        ) : null}
                      </div>
                      {member.position ? (
                        <p className="text-xs text-muted-foreground">
                          {member.position}
                        </p>
                      ) : null}
                    </th>
                    <td className="whitespace-nowrap px-4 py-2.5 tabular-nums text-muted-foreground">
                      {member.phone ?? "—"}
                    </td>
                    <td className="px-4 py-2.5">
                      {names.length === 0 ? (
                        <span className="text-muted-foreground">
                          {ru.staff.noServices}
                        </span>
                      ) : (
                        <span>
                          {names.slice(0, SERVICES_SHOWN).join(", ")}
                          {hidden > 0 ? (
                            <span className="text-muted-foreground">
                              {", "}
                              {ru.staff.moreServices.replace(
                                "{count}",
                                String(hidden),
                              )}
                            </span>
                          ) : null}
                        </span>
                      )}
                    </td>
                    {canEdit ? (
                      <td className="px-2 py-1.5 text-right">
                        <Button
                          aria-label={`${ru.staff.edit}: ${member.name}`}
                          onClick={(event) =>
                            openDialog(member, event.currentTarget)
                          }
                          size="sm"
                          variant="ghost"
                        >
                          <Pencil aria-hidden="true" className="h-3.5 w-3.5" />
                          {ru.staff.edit}
                        </Button>
                      </td>
                    ) : null}
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}

      {!dialog.open ? (
        <CrmErrorAlert error={write.error} testId="crm-staff-write-error" />
      ) : null}

      {canEdit ? (
        <CrmStaffDialog
          error={write.error}
          key={dialog.session}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            const opener = returnFocusRef.current;
            (opener?.isConnected ? opener : addButtonRef.current)?.focus();
          }}
          onDismissError={() => write.reset()}
          onOpenChange={(open) => {
            if (open) return;
            if (!write.isPending) write.reset();
            setDialog((current) => ({ ...current, open: false }));
          }}
          onSubmit={(fields, expectedRevision) =>
            void save(fields, expectedRevision)
          }
          open={dialog.open}
          pending={write.isPending}
          services={services.data ?? (services.isPending ? null : [])}
          staff={dialogStaff}
        />
      ) : null}
    </section>
  );
}
