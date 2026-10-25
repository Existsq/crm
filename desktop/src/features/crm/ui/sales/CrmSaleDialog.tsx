import { Trash2, X } from "lucide-react";
import * as React from "react";

import { useCrmPartiesQuery, useCrmProductsQuery } from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import { formatRubles } from "@/features/crm/lib/money";
import {
  addProductToLines,
  CRM_PAYMENT_METHODS,
  evaluateLine,
  MAX_SALE_LINES,
  type SaleLineDraft,
  saleTotals,
} from "@/features/crm/lib/saleDraft";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { useDebouncedValue } from "@/features/crm/ui/products/useDebouncedValue";
import { CrmSearchPicker } from "@/features/crm/ui/sales/CrmSearchPicker";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type {
  CrmParty,
  CrmPaymentMethod,
  CrmSaleInput,
} from "@/shared/api/tauriCrmSales";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { NativeSelect } from "@/shared/ui/native-select";
import { Textarea } from "@/shared/ui/textarea";

const SEARCH_DEBOUNCE_MS = 200;
/** Matches shown under a picker. */
const PICKER_LIMIT = 8;

function LineRow({
  index,
  line,
  onChange,
  onRemove,
  showErrors,
}: {
  index: number;
  line: SaleLineDraft;
  onChange: (patch: Partial<SaleLineDraft>) => void;
  onRemove: () => void;
  showErrors: boolean;
}) {
  const dialog = ru.sales.dialog;
  const evaluated = evaluateLine(line);
  const errors = showErrors && !evaluated.ok ? evaluated.errors : {};
  const id = (field: string) => `crm-sale-line-${index}-${field}`;
  const label = (field: string) => `${field}: ${line.product.name}`;
  return (
    <tr data-testid={`crm-sale-line-${line.product.id}`}>
      <th className="px-3 py-2 text-left font-normal" scope="row">
        <span className="font-medium">{line.product.name}</span>
        <span className="ml-1 text-xs text-muted-foreground">
          {ru.units.short[line.product.unit]}
        </span>
      </th>
      {(["qty", "price", "discount"] as const).map((field) => (
        <td className="px-2 py-1.5 align-top" key={field}>
          <Input
            {...crmControlProps(id(field), false, errors[field])}
            aria-label={label(dialog[field])}
            className="h-8 w-24 text-right tabular-nums"
            inputMode="decimal"
            onChange={(event) => onChange({ [field]: event.target.value })}
            placeholder={field === "discount" ? "0" : undefined}
            value={line[field]}
          />
          {errors[field] ? (
            <p
              className="mt-1 max-w-[8rem] text-xs text-destructive"
              id={`${id(field)}-error`}
            >
              {errors[field]}
            </p>
          ) : null}
        </td>
      ))}
      <td className="whitespace-nowrap px-3 py-2 text-right tabular-nums">
        {evaluated.ok ? formatRubles(evaluated.amountMinor) : "—"}
      </td>
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

/**
 * Post a new sale. The caller owns the mutation and picks `saleId` once per
 * opening, so a retried submit after a lost response is a relay duplicate,
 * never a second sale.
 */
export function CrmSaleDialog({
  error,
  onCloseAutoFocus,
  onOpenChange,
  onSubmit,
  open,
  pending,
  saleId,
  space,
}: {
  error: unknown;
  onCloseAutoFocus: (event: Event) => void;
  onOpenChange: (open: boolean) => void;
  onSubmit: (input: CrmSaleInput) => void;
  open: boolean;
  pending: boolean;
  saleId: string;
  space: CrmMemberSpace;
}) {
  const dialog = ru.sales.dialog;
  const warehouses = space.warehouses.filter(
    (warehouse) => !warehouse.archived,
  );
  const [warehouseId, setWarehouseId] = React.useState(
    () =>
      warehouses.find((warehouse) => warehouse.isDefault)?.id ??
      warehouses[0]?.id ??
      "",
  );
  const [customer, setCustomer] = React.useState<CrmParty | null>(null);
  const [payment, setPayment] = React.useState<CrmPaymentMethod>("cash");
  const [lines, setLines] = React.useState<SaleLineDraft[]>([]);
  const [note, setNote] = React.useState("");
  const [submitted, setSubmitted] = React.useState(false);

  const [productSearch, setProductSearch] = React.useState("");
  const productQ = useDebouncedValue(productSearch.trim(), SEARCH_DEBOUNCE_MS);
  const products = useCrmProductsQuery({ q: productQ, archived: false });
  // Only results for what is typed now: while a new search is debounced or
  // loading, the previous matches would put the wrong product on the sale.
  const productMatches =
    productQ.length > 0 &&
    productQ === productSearch.trim() &&
    !products.isPlaceholderData
      ? (products.data?.pages[0]?.products ?? []).slice(0, PICKER_LIMIT)
      : [];

  const [customerSearch, setCustomerSearch] = React.useState("");
  const customerQ = useDebouncedValue(
    customerSearch.trim(),
    SEARCH_DEBOUNCE_MS,
  );
  const customers = useCrmPartiesQuery({
    q: customerQ,
    role: "customers",
    archived: false,
  });
  const customerMatches =
    customerQ.length > 0 &&
    customerQ === customerSearch.trim() &&
    !customers.isPlaceholderData
      ? (customers.data?.pages[0]?.parties ?? []).slice(0, PICKER_LIMIT)
      : [];

  const totals = saleTotals(lines);
  const evaluated = lines.map(evaluateLine);
  const linesValid = lines.length > 0 && evaluated.every((line) => line.ok);
  const tooMany = lines.length > MAX_SALE_LINES;

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (!linesValid || tooMany || warehouseId === "") return;
    onSubmit({
      channelId: space.channelId,
      saleId,
      warehouseId,
      customerId: customer?.id ?? null,
      payment,
      lines: evaluated.flatMap((line) => (line.ok ? [line.line] : [])),
      note: note.trim() === "" ? null : note.trim(),
    });
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="flex max-h-[calc(100dvh-2rem)] flex-col sm:max-w-3xl"
        data-testid="crm-sale-dialog"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        <form
          className="flex min-h-0 flex-1 flex-col gap-5"
          noValidate
          onSubmit={handleSubmit}
        >
          <DialogHeader>
            <DialogTitle>{dialog.title}</DialogTitle>
          </DialogHeader>
          <div className="-mx-1 min-h-0 flex-1 space-y-4 overflow-y-auto px-1 pb-1">
            <div className="grid gap-4 sm:grid-cols-3">
              <CrmField id="crm-sale-warehouse" label={dialog.warehouse}>
                <NativeSelect
                  {...crmControlProps("crm-sale-warehouse", false, undefined)}
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
              <CrmField id="crm-sale-payment" label={dialog.payment}>
                <NativeSelect
                  {...crmControlProps("crm-sale-payment", false, undefined)}
                  onChange={(value) => setPayment(value as CrmPaymentMethod)}
                  value={payment}
                >
                  {CRM_PAYMENT_METHODS.map((method) => (
                    <option key={method} value={method}>
                      {ru.payments[method]}
                    </option>
                  ))}
                </NativeSelect>
              </CrmField>
              <div className="space-y-1.5">
                <p className="text-xs font-medium text-muted-foreground">
                  {dialog.customer}
                </p>
                <div
                  className="flex h-9 items-center justify-between gap-2 rounded-md border border-border/70 px-3 text-sm"
                  data-testid="crm-sale-customer"
                >
                  <span className="truncate">
                    {customer?.name ?? dialog.customerNone}
                  </span>
                  {customer ? (
                    <Button
                      aria-label={`${dialog.customerNone}: ${customer.name}`}
                      className="h-6 w-6"
                      onClick={() => setCustomer(null)}
                      size="icon"
                      type="button"
                      variant="ghost"
                    >
                      <X aria-hidden="true" className="h-3.5 w-3.5" />
                    </Button>
                  ) : null}
                </div>
              </div>
            </div>

            <div className="grid gap-4 sm:grid-cols-2">
              <CrmSearchPicker
                emptyText={dialog.noProducts}
                getKey={(product) => product.id}
                id="crm-sale-product-search"
                items={productMatches}
                label={dialog.productSearch}
                loading={
                  products.isFetching || productQ !== productSearch.trim()
                }
                onPick={(product) => {
                  setLines((current) => addProductToLines(current, product));
                  setProductSearch("");
                }}
                onQueryChange={setProductSearch}
                placeholder={dialog.productSearchPlaceholder}
                query={productSearch}
                renderItem={(product) => (
                  <>
                    <span className="truncate">{product.name}</span>
                    <span className="shrink-0 tabular-nums text-muted-foreground">
                      {formatRubles(product.priceMinor)}
                    </span>
                  </>
                )}
              />
              <CrmSearchPicker
                emptyText={ru.parties.noResultsTitle}
                getKey={(party) => party.id}
                id="crm-sale-customer-search"
                items={customerMatches}
                label={dialog.customerSearch}
                loading={
                  customers.isFetching || customerQ !== customerSearch.trim()
                }
                onPick={(party) => {
                  setCustomer(party);
                  setCustomerSearch("");
                }}
                onQueryChange={setCustomerSearch}
                placeholder={dialog.customerSearchPlaceholder}
                query={customerSearch}
                renderItem={(party) => (
                  <>
                    <span className="truncate">{party.name}</span>
                    <span className="shrink-0 text-xs text-muted-foreground">
                      {party.phone ?? ""}
                    </span>
                  </>
                )}
              />
            </div>

            {lines.length === 0 ? (
              <p
                className={
                  submitted
                    ? "text-sm text-destructive"
                    : "text-sm text-muted-foreground"
                }
                data-testid="crm-sale-empty"
                role={submitted ? "alert" : undefined}
              >
                {dialog.empty}
              </p>
            ) : (
              <div className="overflow-x-auto rounded-xl border border-border/70">
                <table
                  className="w-full min-w-[36rem] border-collapse text-sm"
                  data-testid="crm-sale-lines"
                >
                  <caption className="sr-only">{dialog.linesLabel}</caption>
                  <thead className="bg-muted/40 text-xs text-muted-foreground">
                    <tr>
                      <th
                        className="px-3 py-2 text-left font-medium"
                        scope="col"
                      >
                        {ru.products.columns.name}
                      </th>
                      <th
                        className="px-2 py-2 text-left font-medium"
                        scope="col"
                      >
                        {dialog.qty}
                      </th>
                      <th
                        className="px-2 py-2 text-left font-medium"
                        scope="col"
                      >
                        {dialog.price}
                      </th>
                      <th
                        className="px-2 py-2 text-left font-medium"
                        scope="col"
                      >
                        {dialog.discount}
                      </th>
                      <th
                        className="px-3 py-2 text-right font-medium"
                        scope="col"
                      >
                        {dialog.amount}
                      </th>
                      <th scope="col">
                        <span className="sr-only">{dialog.remove}</span>
                      </th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/60">
                    {lines.map((line, index) => (
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
                      />
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            {tooMany ? (
              <p className="text-sm text-destructive" role="alert">
                {dialog.tooManyLines}
              </p>
            ) : null}

            <CrmField id="crm-sale-note" label={dialog.note}>
              <Textarea
                {...crmControlProps("crm-sale-note", false, undefined)}
                maxLength={500}
                onChange={(event) => setNote(event.target.value)}
                rows={2}
                value={note}
              />
            </CrmField>

            <dl
              className="ml-auto grid w-full max-w-xs grid-cols-2 gap-y-1 text-sm"
              data-testid="crm-sale-totals"
            >
              <dt className="text-muted-foreground">{dialog.subtotal}</dt>
              <dd className="text-right tabular-nums">
                {formatRubles(totals.grossMinor)}
              </dd>
              <dt className="text-muted-foreground">{dialog.discountTotal}</dt>
              <dd className="text-right tabular-nums">
                {formatRubles(totals.discountMinor)}
              </dd>
              <dt className="font-medium">{dialog.total}</dt>
              <dd
                aria-live="polite"
                className="text-right text-base font-semibold tabular-nums"
                data-testid="crm-sale-total"
              >
                {formatRubles(totals.totalMinor)}
              </dd>
            </dl>
          </div>
          <CrmErrorAlert error={error} testId="crm-sale-dialog-error" />
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-sale-dialog-submit"
              disabled={pending}
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
