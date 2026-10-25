import { Trash2, X } from "lucide-react";
import * as React from "react";

import { useCrmPartiesQuery, useCrmProductsQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import {
  addProductToStockLines,
  averageCostMinor,
  evaluateStockLine,
  formatStockQty,
  inventoryDifference,
  isStockedProduct,
  MAX_STOCK_DOC_LINES,
  newStockLine,
  type StockLineDraft,
} from "@/features/crm/lib/stock";
import { useCrmStockQuery } from "@/features/crm/stockHooks";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { useDebouncedValue } from "@/features/crm/ui/products/useDebouncedValue";
import { CrmSearchPicker } from "@/features/crm/ui/sales/CrmSearchPicker";
import { liveWarehouses } from "@/features/crm/ui/stock/CrmStockParts";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmParty } from "@/shared/api/tauriCrmSales";
import type {
  CrmStockDocInput,
  CrmStockDocType,
  CrmStockItem,
} from "@/shared/api/tauriCrmStock";
import { cn } from "@/shared/lib/cn";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { NativeSelect } from "@/shared/ui/native-select";
import { Textarea } from "@/shared/ui/textarea";

const SEARCH_DEBOUNCE_MS = 200;
const PICKER_LIMIT = 8;
const LINE_HEAD = "px-2 py-2 text-left font-medium";

/** Fresh results only: stale matches would put the wrong row on the document. */
function freshMatches<T>(
  typed: string,
  debounced: string,
  placeholder: boolean,
  items: readonly T[] | undefined,
): T[] {
  return debounced.length > 0 && debounced === typed.trim() && !placeholder
    ? (items ?? []).slice(0, PICKER_LIMIT)
    : [];
}

function LineRow({
  index,
  line,
  onChange,
  onRemove,
  showErrors,
  type,
}: {
  index: number;
  line: StockLineDraft;
  onChange: (patch: Partial<StockLineDraft>) => void;
  onRemove: () => void;
  showErrors: boolean;
  type: CrmStockDocType;
}) {
  const dialog = ru.stock.dialog;
  const evaluated = evaluateStockLine(type, line);
  const errors = showErrors && !evaluated.ok ? evaluated.errors : {};
  const id = (field: string) => `crm-stock-line-${index}-${field}`;
  const unit = line.product.unit;
  const difference = type === "inventory" ? inventoryDifference(line) : null;
  const fields: ("qty" | "cost")[] =
    type === "receipt" || type === "inventory" ? ["qty", "cost"] : ["qty"];
  return (
    <tr data-testid={`crm-stock-line-${line.product.id}`}>
      <th className="px-3 py-2 text-left font-normal" scope="row">
        <span className="font-medium">{line.product.name}</span>
        <span className="ml-1 text-xs text-muted-foreground">
          {ru.units.short[unit]}
        </span>
      </th>
      <td
        className="whitespace-nowrap px-2 py-2 text-right tabular-nums text-muted-foreground"
        data-testid="crm-stock-line-on-hand"
      >
        {line.onHandMilli === null
          ? "—"
          : formatStockQty(line.onHandMilli, unit)}
      </td>
      {fields.map((field) => {
        const label = field === "cost" ? costLabel(type) : qtyLabel(type);
        return (
          <td className="px-2 py-1.5 align-top" key={field}>
            <Input
              {...crmControlProps(id(field), false, errors[field])}
              aria-label={`${label}: ${line.product.name}`}
              className="h-8 w-24 text-right tabular-nums"
              inputMode="decimal"
              onChange={(event) => onChange({ [field]: event.target.value })}
              value={line[field]}
            />
            {errors[field] ? (
              <p
                className="mt-1 max-w-[9rem] text-xs text-destructive"
                id={`${id(field)}-error`}
              >
                {errors[field]}
              </p>
            ) : null}
          </td>
        );
      })}
      {type === "inventory" ? (
        <td
          className={cn(
            "whitespace-nowrap px-3 py-2 text-right tabular-nums",
            difference !== null && difference < 0 && "text-destructive",
          )}
          data-testid="crm-stock-line-difference"
        >
          {difference === null
            ? "—"
            : formatStockQty(difference, unit, { signed: true })}
        </td>
      ) : null}
      {type === "receipt" ? (
        <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums">
          {evaluated.ok ? formatRubles(evaluated.amountMinor) : "—"}
        </td>
      ) : null}
      <td className="px-1 py-1.5">
        <Button
          aria-label={`${dialog.remove}: ${line.product.name}`}
          onClick={onRemove}
          size="icon"
          type="button"
          variant="ghost"
        >
          <Trash2 aria-hidden="true" className="h-4 w-4" />
        </Button>
      </td>
    </tr>
  );
}

function costLabel(type: CrmStockDocType): string {
  return type === "inventory"
    ? ru.stock.dialog.surplusCost
    : ru.stock.dialog.cost;
}

function qtyLabel(type: CrmStockDocType): string {
  return type === "inventory" ? ru.stock.dialog.counted : ru.stock.dialog.qty;
}

/**
 * Post a receipt, write-off, inventory count or transfer. The caller owns the
 * mutation and picks `docId` once per opening, so a retried submit after a
 * lost response can never post a second document.
 */
export function CrmStockDocDialog({
  docId,
  error,
  onCloseAutoFocus,
  onOpenChange,
  onSubmit,
  open,
  pending,
  space,
  type,
}: {
  docId: string;
  error: unknown;
  onCloseAutoFocus: (event: Event) => void;
  onOpenChange: (open: boolean) => void;
  onSubmit: (input: CrmStockDocInput) => void;
  open: boolean;
  pending: boolean;
  space: CrmMemberSpace;
  type: CrmStockDocType;
}) {
  const dialog = ru.stock.dialog;
  const warehouses = liveWarehouses(space);
  const [warehouseId, setWarehouseId] = React.useState(
    () => warehouses[0]?.id ?? "",
  );
  const [toWarehouseId, setToWarehouseId] = React.useState(
    () => warehouses[1]?.id ?? "",
  );
  const [supplier, setSupplier] = React.useState<CrmParty | null>(null);
  const [reference, setReference] = React.useState("");
  const [note, setNote] = React.useState("");
  const [lines, setLines] = React.useState<StockLineDraft[]>([]);
  const [submitted, setSubmitted] = React.useState(false);

  // Book quantities of the source warehouse, re-read when it changes.
  const levels = useCrmStockQuery({ warehouseId, q: "", low: false });
  const stockById = React.useMemo(() => {
    const map = new Map<string, CrmStockItem>();
    if (levels.data?.warehouseId === warehouseId) {
      for (const item of levels.data.items) map.set(item.productId, item);
    }
    return map;
  }, [levels.data, warehouseId]);
  const known = levels.data?.warehouseId === warehouseId;
  const withBook = lines.map((line) => ({
    ...line,
    onHandMilli: known ? (stockById.get(line.product.id)?.qtyMilli ?? 0) : null,
  }));

  const [productSearch, setProductSearch] = React.useState("");
  const productQ = useDebouncedValue(productSearch.trim(), SEARCH_DEBOUNCE_MS);
  const products = useCrmProductsQuery({
    q: productQ,
    archived: false,
    kind: "goods",
  });
  const productMatches = freshMatches(
    productSearch,
    productQ,
    products.isPlaceholderData,
    products.data?.pages[0]?.products.filter(isStockedProduct),
  );

  const [supplierSearch, setSupplierSearch] = React.useState("");
  const supplierQ = useDebouncedValue(
    supplierSearch.trim(),
    SEARCH_DEBOUNCE_MS,
  );
  const suppliers = useCrmPartiesQuery({
    q: type === "receipt" ? supplierQ : "",
    role: "suppliers",
    archived: false,
  });
  const supplierMatches = freshMatches(
    supplierSearch,
    supplierQ,
    suppliers.isPlaceholderData,
    suppliers.data?.pages[0]?.parties,
  );

  const evaluated = withBook.map((line) => evaluateStockLine(type, line));
  const linesValid = lines.length > 0 && evaluated.every((line) => line.ok);
  const tooMany = lines.length > MAX_STOCK_DOC_LINES;
  const transfer = type === "transfer";
  const noSecondWarehouse = transfer && warehouses.length < 2;
  const sameWarehouse = transfer && toWarehouseId === warehouseId;
  const receiptTotal = evaluated.reduce(
    (sum, line) => sum + (line.ok ? line.amountMinor : 0),
    0,
  );

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (!linesValid || tooMany || warehouseId === "") return;
    if (transfer && (noSecondWarehouse || sameWarehouse)) return;
    onSubmit({
      channelId: space.channelId,
      docId,
      type,
      warehouseId,
      toWarehouseId: transfer ? toWarehouseId : null,
      supplierId: type === "receipt" ? (supplier?.id ?? null) : null,
      reference:
        type === "receipt" && reference.trim() !== "" ? reference.trim() : null,
      lines: evaluated.flatMap((line) => (line.ok ? [line.line] : [])),
      note: note.trim() === "" ? null : note.trim(),
    });
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="flex max-h-[calc(100dvh-2rem)] flex-col sm:max-w-3xl"
        data-testid="crm-stock-doc-dialog"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <form
          className="flex min-h-0 flex-1 flex-col gap-5"
          noValidate
          onSubmit={handleSubmit}
        >
          <DialogHeader>
            <DialogTitle>{dialog.title[type]}</DialogTitle>
            <DialogDescription>{dialog.hint[type]}</DialogDescription>
          </DialogHeader>
          <div className="-mx-1 min-h-0 flex-1 space-y-4 overflow-y-auto px-1 pb-1">
            <div className="grid gap-4 sm:grid-cols-3">
              <CrmField
                id="crm-stock-doc-warehouse"
                label={transfer ? dialog.fromWarehouse : dialog.warehouse}
              >
                <NativeSelect
                  {...crmControlProps(
                    "crm-stock-doc-warehouse",
                    false,
                    undefined,
                  )}
                  onChange={setWarehouseId}
                  value={warehouseId}
                >
                  {warehouses.map((warehouse) => (
                    <option key={warehouse.id} value={warehouse.id}>
                      {warehouse.name}
                    </option>
                  ))}
                </NativeSelect>
              </CrmField>
              {transfer && !noSecondWarehouse ? (
                <CrmField
                  error={
                    submitted && sameWarehouse
                      ? dialog.sameWarehouse
                      : undefined
                  }
                  id="crm-stock-doc-to-warehouse"
                  label={dialog.toWarehouse}
                >
                  <NativeSelect
                    {...crmControlProps(
                      "crm-stock-doc-to-warehouse",
                      false,
                      submitted && sameWarehouse
                        ? dialog.sameWarehouse
                        : undefined,
                    )}
                    onChange={setToWarehouseId}
                    value={toWarehouseId}
                  >
                    {warehouses.map((warehouse) => (
                      <option key={warehouse.id} value={warehouse.id}>
                        {warehouse.name}
                      </option>
                    ))}
                  </NativeSelect>
                </CrmField>
              ) : null}
              {type === "receipt" ? (
                <>
                  <div className="space-y-1.5">
                    <p className="text-xs font-medium text-muted-foreground">
                      {dialog.supplier}
                    </p>
                    <div
                      className="flex h-9 items-center justify-between gap-2 rounded-md border border-border/70 px-3 text-sm"
                      data-testid="crm-stock-doc-supplier"
                    >
                      <span className="truncate">
                        {supplier?.name ?? dialog.supplierNone}
                      </span>
                      {supplier ? (
                        <Button
                          aria-label={`${dialog.supplierNone}: ${supplier.name}`}
                          className="h-6 w-6"
                          onClick={() => setSupplier(null)}
                          size="icon"
                          type="button"
                          variant="ghost"
                        >
                          <X aria-hidden="true" className="h-3.5 w-3.5" />
                        </Button>
                      ) : null}
                    </div>
                  </div>
                  <CrmField
                    id="crm-stock-doc-reference"
                    label={dialog.reference}
                  >
                    <Input
                      {...crmControlProps(
                        "crm-stock-doc-reference",
                        false,
                        undefined,
                      )}
                      maxLength={64}
                      onChange={(event) => setReference(event.target.value)}
                      value={reference}
                    />
                  </CrmField>
                </>
              ) : null}
            </div>
            {noSecondWarehouse ? (
              <p
                className="text-sm text-destructive"
                data-testid="crm-stock-doc-no-second-warehouse"
                role="alert"
              >
                {dialog.needSecondWarehouse}
              </p>
            ) : null}

            <div className="grid gap-4 sm:grid-cols-2">
              <CrmSearchPicker
                emptyText={dialog.noProducts}
                getKey={(product) => product.id}
                id="crm-stock-doc-product-search"
                items={productMatches}
                label={dialog.productSearch}
                loading={
                  products.isFetching || productQ !== productSearch.trim()
                }
                onPick={(product) => {
                  const stock = stockById.get(product.id);
                  const draft = newStockLine(
                    type,
                    { id: product.id, name: product.name, unit: product.unit },
                    known ? (stock?.qtyMilli ?? 0) : null,
                    stock ? averageCostMinor(stock) : null,
                  );
                  setLines((current) => addProductToStockLines(current, draft));
                  setProductSearch("");
                }}
                onQueryChange={setProductSearch}
                placeholder={dialog.productSearchPlaceholder}
                query={productSearch}
                renderItem={(product) => (
                  <>
                    <span className="truncate">{product.name}</span>
                    <span className="shrink-0 tabular-nums text-muted-foreground">
                      {known
                        ? formatStockQty(
                            stockById.get(product.id)?.qtyMilli ?? 0,
                            product.unit,
                          )
                        : ""}
                    </span>
                  </>
                )}
              />
              {type === "receipt" ? (
                <CrmSearchPicker
                  emptyText={ru.parties.noResultsTitle}
                  getKey={(party) => party.id}
                  id="crm-stock-doc-supplier-search"
                  items={supplierMatches}
                  label={dialog.supplierSearch}
                  loading={
                    suppliers.isFetching || supplierQ !== supplierSearch.trim()
                  }
                  onPick={(party) => {
                    setSupplier(party);
                    setSupplierSearch("");
                  }}
                  onQueryChange={setSupplierSearch}
                  placeholder={dialog.supplierSearchPlaceholder}
                  query={supplierSearch}
                  renderItem={(party) => (
                    <>
                      <span className="truncate">{party.name}</span>
                      <span className="shrink-0 text-xs text-muted-foreground">
                        {party.inn ?? party.phone ?? ""}
                      </span>
                    </>
                  )}
                />
              ) : null}
            </div>

            {lines.length === 0 ? (
              <p
                className={
                  submitted
                    ? "text-sm text-destructive"
                    : "text-sm text-muted-foreground"
                }
                data-testid="crm-stock-doc-empty"
                role={submitted ? "alert" : undefined}
              >
                {dialog.empty}
              </p>
            ) : (
              <div className="overflow-x-auto rounded-xl border border-border/70">
                <table
                  className="w-full min-w-[34rem] border-collapse text-sm"
                  data-testid="crm-stock-doc-lines"
                >
                  <caption className="sr-only">{dialog.linesLabel}</caption>
                  <thead className="bg-muted/40 text-xs text-muted-foreground">
                    <tr>
                      <th
                        className="px-3 py-2 text-left font-medium"
                        scope="col"
                      >
                        {dialog.product}
                      </th>
                      <th className={`${LINE_HEAD} text-right`} scope="col">
                        {dialog.onHand}
                      </th>
                      <th className={LINE_HEAD} scope="col">
                        {qtyLabel(type)}
                      </th>
                      {type === "receipt" || type === "inventory" ? (
                        <th className={LINE_HEAD} scope="col">
                          {costLabel(type)}
                        </th>
                      ) : null}
                      {type === "inventory" ? (
                        <th
                          className="px-3 py-2 text-right font-medium"
                          scope="col"
                        >
                          {dialog.difference}
                        </th>
                      ) : null}
                      {type === "receipt" ? (
                        <th
                          className="px-3 py-2 text-right font-medium"
                          scope="col"
                        >
                          {dialog.amount}
                        </th>
                      ) : null}
                      <th scope="col">
                        <span className="sr-only">{dialog.remove}</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/60">
                    {withBook.map((line, index) => (
                      <LineRow
                        index={index}
                        key={line.product.id}
                        line={line}
                        onChange={(patch) =>
                          setLines((current) =>
                            current.map((item, i) =>
                              i === index ? { ...item, ...patch } : item,
                            ),
                          )
                        }
                        onRemove={() =>
                          setLines((current) =>
                            current.filter((_, i) => i !== index),
                          )
                        }
                        showErrors={submitted}
                        type={type}
                      />
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            {type === "inventory" && lines.length > 0 ? (
              <p className="text-xs text-muted-foreground">
                {dialog.surplusCostHint}
              </p>
            ) : null}
            {tooMany ? (
              <p className="text-sm text-destructive" role="alert">
                {dialog.tooManyLines}
              </p>
            ) : null}

            <CrmField id="crm-stock-doc-note" label={dialog.note}>
              <Textarea
                {...crmControlProps("crm-stock-doc-note", false, undefined)}
                maxLength={500}
                onChange={(event) => setNote(event.target.value)}
                placeholder={
                  type === "write_off"
                    ? dialog.writeOffNotePlaceholder
                    : undefined
                }
                rows={2}
                value={note}
              />
            </CrmField>

            {type === "receipt" ? (
              <dl className="ml-auto grid w-full max-w-xs grid-cols-2 text-sm">
                <dt className="font-medium">{dialog.total}</dt>
                <dd
                  aria-live="polite"
                  className="text-right text-base font-semibold tabular-nums"
                  data-testid="crm-stock-doc-total"
                >
                  {formatRubles(receiptTotal)}
                </dd>
              </dl>
            ) : null}
          </div>
          <CrmErrorAlert error={error} testId="crm-stock-doc-error" />
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-stock-doc-submit"
              disabled={pending || noSecondWarehouse}
              type="submit"
            >
              {pending ? dialog.submitting : dialog.submit}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
