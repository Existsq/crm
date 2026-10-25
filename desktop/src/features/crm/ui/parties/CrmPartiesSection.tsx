import { Plus, Search, Users } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import {
  useCrmPartiesQuery,
  useWriteCrmPartyMutation,
} from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import { formatCrmDate } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { CrmPartyDialog } from "@/features/crm/ui/parties/CrmPartyDialog";
import { useDebouncedValue } from "@/features/crm/ui/products/useDebouncedValue";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type {
  CrmParty,
  CrmPartyFields,
  CrmPartyRole,
} from "@/shared/api/tauriCrmSales";
import { Badge } from "@/shared/ui/badge";
import { Button } from "@/shared/ui/button";
import { EmptyState } from "@/shared/ui/empty-state";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { SegmentedControl } from "@/shared/ui/segmented-control";
import { Skeleton } from "@/shared/ui/skeleton";
import { Switch } from "@/shared/ui/switch";

const SEARCH_DEBOUNCE_MS = 250;
const HEAD_CELL = "px-4 py-2.5 text-left text-xs font-medium";

type RoleFilter = "all" | CrmPartyRole;

type DialogState = {
  open: boolean;
  session: number;
  party: CrmParty | null;
  /** Edited party's id, or one chosen at opening for a create. */
  partyId: string;
};

function PartiesTable({
  busy,
  onOpen,
  parties,
}: {
  busy: boolean;
  onOpen: (party: CrmParty, opener: HTMLElement) => void;
  parties: CrmParty[];
}) {
  const columns = ru.parties.columns;
  return (
    <div
      aria-busy={busy || undefined}
      className="overflow-x-auto rounded-2xl border border-border/70"
    >
      <table
        className="w-full min-w-[44rem] border-collapse text-sm"
        data-testid="crm-parties-table"
      >
        <caption className="sr-only">{ru.parties.tableLabel}</caption>
        <thead className="bg-muted/40 text-muted-foreground">
          <tr>
            <th className={HEAD_CELL} scope="col">
              {columns.name}
            </th>
            <th className={HEAD_CELL} scope="col">
              {columns.type}
            </th>
            <th className={HEAD_CELL} scope="col">
              {columns.contacts}
            </th>
            <th className={`${HEAD_CELL} text-right`} scope="col">
              {columns.purchases}
            </th>
            <th className={HEAD_CELL} scope="col">
              {columns.lastSale}
            </th>
            <th className={HEAD_CELL} scope="col">
              <span className="sr-only">{columns.actions}</span>
            </th>
          </tr>
        </thead>
        <tbody className="divide-y divide-border/60">
          {parties.map((party) => (
            <tr data-testid={`crm-party-${party.id}`} key={party.id}>
              <th className="px-4 py-2.5 text-left font-normal" scope="row">
                <div className="flex min-w-0 flex-wrap items-center gap-2">
                  <span className="font-medium">{party.name}</span>
                  {party.archived ? (
                    <Badge variant="secondary">
                      {ru.parties.archivedBadge}
                    </Badge>
                  ) : null}
                </div>
                {party.inn ? (
                  <p className="font-mono text-xs text-muted-foreground">
                    {ru.parties.fields.inn} {party.inn}
                  </p>
                ) : null}
              </th>
              <td className="px-4 py-2.5">
                {ru.parties.types[party.partyType]}
              </td>
              <td className="px-4 py-2.5 text-muted-foreground">
                <div>{party.phone ?? "—"}</div>
                {party.email ? (
                  <div className="text-xs">{party.email}</div>
                ) : null}
              </td>
              <td className="whitespace-nowrap px-4 py-2.5 text-right tabular-nums">
                <div>{formatRubles(party.purchases.revenueMinor)}</div>
                <div className="text-xs text-muted-foreground">
                  {ru.parties.purchasesCount.replace(
                    "{count}",
                    String(party.purchases.salesCount),
                  )}
                </div>
              </td>
              <td className="whitespace-nowrap px-4 py-2.5 text-muted-foreground">
                {party.purchases.lastSaleAt
                  ? formatCrmDate(party.purchases.lastSaleAt)
                  : "—"}
              </td>
              <td className="px-2 py-1.5 text-right">
                <Button
                  aria-label={`${ru.parties.edit}: ${party.name}`}
                  onClick={(event) => onOpen(party, event.currentTarget)}
                  size="sm"
                  variant="ghost"
                >
                  {ru.parties.edit}
                </Button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

export function CrmPartiesSection({ space }: { space: CrmMemberSpace }) {
  const canEdit = space.capabilities.has("edit_parties");
  const [search, setSearch] = React.useState("");
  const [role, setRole] = React.useState<RoleFilter>("all");
  const [showArchived, setShowArchived] = React.useState(false);
  const q = useDebouncedValue(search.trim(), SEARCH_DEBOUNCE_MS);
  const query = useCrmPartiesQuery({
    q,
    role: role === "all" ? null : role,
    archived: showArchived,
  });
  const write = useWriteCrmPartyMutation();
  const [dialog, setDialog] = React.useState<DialogState>({
    open: false,
    session: 0,
    party: null,
    partyId: "",
  });
  const addButtonRef = React.useRef<HTMLButtonElement>(null);
  const returnFocusRef = React.useRef<HTMLElement | null>(null);

  const parties = React.useMemo(
    () => query.data?.pages.flatMap((page) => page.parties) ?? [],
    [query.data],
  );
  const dialogParty = dialog.party
    ? (parties.find((party) => party.id === dialog.party?.id) ?? dialog.party)
    : null;

  function openDialog(party: CrmParty | null, opener: HTMLElement) {
    returnFocusRef.current = opener;
    write.reset();
    setDialog((current) => ({
      open: true,
      session: current.session + 1,
      party,
      partyId: party?.id ?? crypto.randomUUID(),
    }));
  }

  async function save(fields: CrmPartyFields, expectedRevision: string | null) {
    try {
      await write.mutateAsync({
        ...fields,
        channelId: space.channelId,
        partyId: dialog.partyId,
        expectedRevision,
      });
      toast.success(dialogParty ? ru.parties.updated : ru.parties.created);
      setDialog((current) => ({ ...current, open: false }));
    } catch {
      // Rendered in the dialog through the mutation error state.
    }
  }

  const filtered = q.length > 0 || showArchived || role !== "all";
  const filter = ru.parties.filter;

  return (
    <section
      aria-labelledby="crm-parties-title"
      className="space-y-4"
      data-testid="crm-parties"
    >
      <SectionHeader
        action={
          canEdit ? (
            <Button
              data-testid="crm-party-add"
              onClick={(event) => openDialog(null, event.currentTarget)}
              ref={addButtonRef}
              size="sm"
            >
              <Plus aria-hidden="true" className="h-4 w-4" />
              {ru.parties.add}
            </Button>
          ) : undefined
        }
        description={canEdit ? undefined : ru.parties.readOnlyHint}
        title={<span id="crm-parties-title">{ru.parties.title}</span>}
      />

      <div className="flex flex-wrap items-center gap-x-6 gap-y-3">
        <div className="relative min-w-[14rem] flex-1">
          <label className="sr-only" htmlFor="crm-parties-search">
            {ru.parties.searchLabel}
          </label>
          <Search
            aria-hidden="true"
            className="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
          />
          <input
            autoCapitalize="none"
            autoCorrect="off"
            className="h-9 w-full rounded-lg border border-border/70 bg-background pl-9 pr-3 text-sm placeholder:text-muted-foreground focus-visible:outline-hidden focus-visible:ring-2 focus-visible:ring-ring"
            data-testid="crm-parties-search"
            id="crm-parties-search"
            onChange={(event) => setSearch(event.target.value)}
            placeholder={ru.parties.searchPlaceholder}
            spellCheck={false}
            type="search"
            value={search}
          />
        </div>
        <SegmentedControl<RoleFilter>
          legend={ru.parties.filterLabel}
          onValueChange={setRole}
          optionTestIdPrefix="crm-parties-role"
          options={[
            { value: "all", label: filter.all },
            { value: "customers", label: filter.customers },
            { value: "suppliers", label: filter.suppliers },
          ]}
          size="wide"
          testId="crm-parties-role"
          value={role}
        />
        <div className="flex items-center gap-2">
          <Switch
            checked={showArchived}
            data-testid="crm-parties-show-archived"
            id="crm-parties-show-archived"
            onCheckedChange={setShowArchived}
          />
          <label className="text-sm" htmlFor="crm-parties-show-archived">
            {ru.parties.showArchived}
          </label>
        </div>
      </div>

      {query.isPending ? (
        <div
          aria-busy="true"
          aria-label={ru.parties.loading}
          className="space-y-2"
          data-testid="crm-parties-loading"
          role="status"
        >
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      ) : query.isError && parties.length === 0 ? (
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
          testId="crm-parties-error"
          title={ru.parties.loadError}
        />
      ) : parties.length === 0 ? (
        <EmptyState
          description={
            filtered
              ? ru.parties.noResultsHint
              : canEdit
                ? ru.parties.emptyCanEdit
                : ru.parties.emptyReadOnly
          }
          icon={<Users className="h-10 w-10" />}
          testId="crm-parties-empty"
          title={filtered ? ru.parties.noResultsTitle : ru.parties.emptyTitle}
        />
      ) : (
        <>
          {query.isError ? <CrmErrorAlert error={query.error} /> : null}
          <PartiesTable
            busy={query.isPlaceholderData}
            onOpen={openDialog}
            parties={parties}
          />
        </>
      )}

      {query.hasNextPage && !query.isPlaceholderData ? (
        <div className="flex justify-center">
          <Button
            data-testid="crm-parties-load-more"
            disabled={query.isFetchingNextPage}
            onClick={() => void query.fetchNextPage()}
            size="sm"
            variant="outline"
          >
            {query.isFetchingNextPage
              ? ru.parties.loadingMore
              : ru.parties.loadMore}
          </Button>
        </div>
      ) : null}

      {!dialog.open ? (
        <CrmErrorAlert error={write.error} testId="crm-parties-write-error" />
      ) : null}

      <CrmPartyDialog
        canEdit={canEdit}
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
        party={dialogParty}
        pending={write.isPending}
      />
    </section>
  );
}
