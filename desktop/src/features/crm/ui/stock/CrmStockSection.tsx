import { ChevronDown, Plus } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import { ru } from "@/features/crm/i18n/ru";
import { CRM_STOCK_DOC_TYPES } from "@/features/crm/lib/stock";
import { usePostCrmStockDocMutation } from "@/features/crm/stockHooks";
import { CrmStockDocDetailsDialog } from "@/features/crm/ui/stock/CrmStockDocDetailsDialog";
import { CrmStockDocDialog } from "@/features/crm/ui/stock/CrmStockDocDialog";
import { CrmStockDocs } from "@/features/crm/ui/stock/CrmStockDocs";
import { CrmStockLevels } from "@/features/crm/ui/stock/CrmStockLevels";
import { CrmStockMoves } from "@/features/crm/ui/stock/CrmStockMoves";
import { CrmTechCards } from "@/features/crm/ui/stock/CrmTechCards";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import {
  type CrmStockDocInput,
  type CrmStockDocType,
  getCrmStockDoc,
} from "@/shared/api/tauriCrmStock";
import { Button } from "@/shared/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/shared/ui/dropdown-menu";
import { SectionHeader } from "@/shared/ui/PageHeader";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/shared/ui/tabs";

type StockTab = "levels" | "docs" | "moves" | "techCards";
const STOCK_TABS: readonly StockTab[] = [
  "levels",
  "docs",
  "moves",
  "techCards",
];

/** Product picked on the levels tab to narrow the movement journal. */
export type StockProductFilter = { id: string; name: string } | null;

/** The warehouse: stock levels, stock documents, movements and tech cards. */
export function CrmStockSection({ space }: { space: CrmMemberSpace }) {
  const canPost = space.capabilities.has("post_documents");
  const canVoid = space.capabilities.has("void_documents");
  const canEditCatalog = space.capabilities.has("edit_catalog");
  const [tab, setTab] = React.useState<StockTab>("levels");
  const [warehouseId, setWarehouseId] = React.useState<string | null>(null);
  const [moveProduct, setMoveProduct] =
    React.useState<StockProductFilter>(null);
  const post = usePostCrmStockDocMutation();
  const [create, setCreate] = React.useState<{
    open: boolean;
    session: number;
    docId: string;
    type: CrmStockDocType;
  }>({ open: false, session: 0, docId: "", type: "receipt" });
  const [details, setDetails] = React.useState<{
    open: boolean;
    docId: string | null;
  }>({ open: false, docId: null });
  const addButtonRef = React.useRef<HTMLButtonElement>(null);
  const returnFocusRef = React.useRef<HTMLElement | null>(null);

  function openCreate(type: CrmStockDocType) {
    returnFocusRef.current = addButtonRef.current;
    post.reset();
    setCreate((current) => ({
      open: true,
      session: current.session + 1,
      docId: crypto.randomUUID(),
      type,
    }));
  }

  async function submit(input: CrmStockDocInput) {
    try {
      await post.mutateAsync(input);
      setCreate((current) => ({ ...current, open: false }));
      // The number is assigned by the relay; read it back for the toast.
      const number = await getCrmStockDoc(input.docId)
        .then((doc) => doc.number)
        .catch(() => null);
      toast.success(
        number === null
          ? ru.stock.docs.createdNoNumber
          : ru.stock.docs.created
              .replace("{type}", ru.stock.docTypes[input.type])
              .replace("{number}", String(number)),
      );
    } catch {
      // Rendered in the dialog through the mutation error state.
    }
  }

  const restoreFocus = (event: Event) => {
    event.preventDefault();
    const opener = returnFocusRef.current;
    (opener?.isConnected ? opener : addButtonRef.current)?.focus();
  };

  return (
    <section
      aria-labelledby="crm-stock-title"
      className="space-y-4"
      data-testid="crm-stock"
    >
      <SectionHeader
        action={
          canPost ? (
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  aria-label={ru.stock.newDocLabel}
                  data-testid="crm-stock-new-doc"
                  ref={addButtonRef}
                  size="sm"
                >
                  <Plus aria-hidden="true" className="h-4 w-4" />
                  {ru.stock.newDoc}
                  <ChevronDown aria-hidden="true" className="h-4 w-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {CRM_STOCK_DOC_TYPES.map((type) => (
                  <DropdownMenuItem
                    data-testid={`crm-stock-new-${type}`}
                    key={type}
                    onSelect={() => openCreate(type)}
                  >
                    {ru.stock.docTypes[type]}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          ) : undefined
        }
        description={canPost ? undefined : ru.stock.readOnlyHint}
        title={<span id="crm-stock-title">{ru.stock.title}</span>}
      />

      <Tabs onValueChange={(value) => setTab(value as StockTab)} value={tab}>
        <TabsList aria-label={ru.stock.tabsLabel}>
          {STOCK_TABS.map((id) => (
            <TabsTrigger
              data-testid={`crm-stock-tab-${id}`}
              key={id}
              value={id}
            >
              {ru.stock.tabs[id]}
            </TabsTrigger>
          ))}
        </TabsList>
        <TabsContent className="mt-4" value="levels">
          <CrmStockLevels
            onShowMoves={(product) => {
              setMoveProduct(product);
              setTab("moves");
            }}
            onWarehouseChange={setWarehouseId}
            space={space}
            warehouseId={warehouseId}
          />
        </TabsContent>
        <TabsContent className="mt-4" value="docs">
          <CrmStockDocs
            onOpen={(docId, opener) => {
              returnFocusRef.current = opener;
              setDetails({ open: true, docId });
            }}
          />
        </TabsContent>
        <TabsContent className="mt-4" value="moves">
          <CrmStockMoves
            onClearProduct={() => setMoveProduct(null)}
            onWarehouseChange={setWarehouseId}
            product={moveProduct}
            space={space}
            warehouseId={warehouseId}
          />
        </TabsContent>
        <TabsContent className="mt-4" value="techCards">
          <CrmTechCards canEdit={canEditCatalog} space={space} />
        </TabsContent>
      </Tabs>

      {canPost && create.session > 0 ? (
        <CrmStockDocDialog
          docId={create.docId}
          error={post.error}
          key={create.session}
          onCloseAutoFocus={restoreFocus}
          onOpenChange={(open) => {
            if (open) return;
            if (!post.isPending) post.reset();
            setCreate((current) => ({ ...current, open: false }));
          }}
          onSubmit={(input) => void submit(input)}
          open={create.open}
          pending={post.isPending}
          space={space}
          type={create.type}
        />
      ) : null}

      <CrmStockDocDetailsDialog
        canVoid={canVoid}
        channelId={space.channelId}
        docId={details.docId}
        key={details.docId ?? "none"}
        onCloseAutoFocus={restoreFocus}
        onOpenChange={(open) => setDetails((current) => ({ ...current, open }))}
        open={details.open}
      />
    </section>
  );
}
