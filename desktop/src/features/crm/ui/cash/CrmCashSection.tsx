import { X } from "lucide-react";
import * as React from "react";
import { toast } from "sonner";

import {
  useCrmCashDeviceQuery,
  useFiscalizeCrmSaleMutation,
} from "@/features/crm/cashHooks";
import {
  useCrmPartiesQuery,
  usePostCrmSaleMutation,
} from "@/features/crm/hooks";
import { ru } from "@/features/crm/i18n/ru";
import {
  evaluateCashPayment,
  newCashPayment,
  type CashPaymentDraft,
} from "@/features/crm/lib/cashPayment";
import { describeCrmError } from "@/features/crm/lib/crmErrors";
import { maxRedeemMinor, spreadBonus } from "@/features/crm/lib/loyalty";
import { clientLabel } from "@/features/crm/lib/journal";
import { useCrmLoyaltyQuery } from "@/features/crm/loyaltyHooks";
import { formatRubles } from "@/features/crm/lib/money";
import { formatClock, fromIsoDate } from "@/features/crm/lib/schedule";
import {
  addProductToLines,
  evaluateLine,
  MAX_SALE_LINES,
  type SaleLineDraft,
  saleTotals,
} from "@/features/crm/lib/saleDraft";
import { CrmCashCart } from "@/features/crm/ui/cash/CrmCashCart";
import {
  CrmCashBonuses,
  evaluateCashBonus,
} from "@/features/crm/ui/cash/CrmCashBonuses";
import { CrmCashPayment } from "@/features/crm/ui/cash/CrmCashPayment";
import { CrmCashQueue } from "@/features/crm/ui/cash/CrmCashQueue";
import { CrmCashScanner } from "@/features/crm/ui/cash/CrmCashScanner";
import { CrmCashShiftCard } from "@/features/crm/ui/cash/CrmCashShiftCard";
import { useVisitCheckout } from "@/features/crm/ui/cash/useVisitCheckout";
import { CrmField, crmControlProps } from "@/features/crm/ui/CrmField";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import { useDebouncedValue } from "@/features/crm/ui/products/useDebouncedValue";
import { CrmSearchPicker } from "@/features/crm/ui/sales/CrmSearchPicker";
import type { CrmMemberSpace } from "@/shared/api/tauriCrm";
import type { CrmAppointment } from "@/shared/api/tauriCrmAppointments";
import { getCrmSale, type CrmParty } from "@/shared/api/tauriCrmSales";
import { Button } from "@/shared/ui/button";
import { NativeSelect } from "@/shared/ui/native-select";
import { SectionHeader } from "@/shared/ui/PageHeader";

const SEARCH_DEBOUNCE_MS = 200;
const PICKER_LIMIT = 8;

function CustomerPicker({
  customer,
  onChange,
}: {
  customer: CrmParty | null;
  onChange: (customer: CrmParty | null) => void;
}) {
  const dialog = ru.sales.dialog;
  const [search, setSearch] = React.useState("");
  const q = useDebouncedValue(search.trim(), SEARCH_DEBOUNCE_MS);
  const customers = useCrmPartiesQuery({
    q,
    role: "customers",
    archived: false,
  });
  const matches =
    q.length > 0 && q === search.trim() && !customers.isPlaceholderData
      ? (customers.data?.pages[0]?.parties ?? []).slice(0, PICKER_LIMIT)
      : [];
  if (customer) {
    return (
      <div className="space-y-1.5">
        <p className="text-xs font-medium text-muted-foreground">
          {dialog.customer}
        </p>
        <div
          className="flex h-9 items-center justify-between gap-2 rounded-md border border-border/70 px-3 text-sm"
          data-testid="crm-cash-customer"
        >
          <span className="truncate">{customer.name}</span>
          <Button
            aria-label={`${dialog.customerNone}: ${customer.name}`}
            className="h-6 w-6"
            onClick={() => onChange(null)}
            size="icon"
            type="button"
            variant="ghost"
          >
            <X aria-hidden="true" className="h-3.5 w-3.5" />
          </Button>
        </div>
      </div>
    );
  }
  return (
    <CrmSearchPicker
      emptyText={ru.parties.noResultsTitle}
      getKey={(party) => party.id}
      id="crm-cash-customer-search"
      items={matches}
      label={dialog.customer}
      loading={customers.isFetching || q !== search.trim()}
      onPick={(party) => {
        onChange(party);
        setSearch("");
      }}
      onQueryChange={setSearch}
      placeholder={dialog.customerSearchPlaceholder}
      query={search}
      renderItem={(party) => (
        <>
          <span className="truncate">{party.name}</span>
          <span className="shrink-0 text-xs text-muted-foreground">
            {party.phone ?? ""}
          </span>
        </>
      )}
    />
  );
}

function VisitBanner({
  error,
  loading,
  onUnlink,
  visit,
}: {
  error: unknown;
  loading: boolean;
  onUnlink: () => void;
  visit: CrmAppointment | null;
}) {
  const v = ru.cash.visit;
  if (error)
    return <CrmErrorAlert error={error} testId="crm-cash-visit-error" />;
  if (loading || !visit) {
    return (
      <p className="text-sm text-muted-foreground" role="status">
        {v.loading}
      </p>
    );
  }
  const date = fromIsoDate(visit.date).toLocaleDateString("ru-RU", {
    day: "numeric",
    month: "long",
  });
  return (
    <div
      className="flex flex-wrap items-center justify-between gap-2 rounded-xl border border-sky-500/40 bg-sky-500/5 px-3 py-2"
      data-testid="crm-cash-visit"
    >
      <div className="min-w-0 text-sm">
        <p className="font-medium">{v.title}</p>
        <p className="truncate text-muted-foreground">
          {visit.saleId
            ? v.alreadyPaid
            : v.detail
                .replace("{client}", clientLabel(visit))
                .replace("{date}", date)
                .replace("{time}", formatClock(visit.start))}
        </p>
      </div>
      <Button
        data-testid="crm-cash-visit-unlink"
        onClick={onUnlink}
        size="sm"
        type="button"
        variant="ghost"
      >
        {v.unlink}
      </Button>
    </div>
  );
}

/**
 * The cash desk: ring up goods, take the payment and print the fiscal
 * receipt on the ATOL register. The sale is posted to the relay before the
 * receipt is printed (ADR-0003); a receipt that cannot print stays in the
 * durable queue below.
 */
export function CrmCashSection({
  onVisitDone,
  space,
  visit,
}: {
  /** Drops the visit from the address once it is paid or unlinked. */
  onVisitDone: () => void;
  space: CrmMemberSpace;
  /** Booked visit to take payment for, from the journal. */
  visit: string | null;
}) {
  const pay = ru.cash.pay;
  const canPost = space.capabilities.has("post_documents");
  const canFiscalize = space.capabilities.has("record_fiscal");
  const fiscalization = space.settings.fiscalization;
  const device = useCrmCashDeviceQuery();
  const post = usePostCrmSaleMutation();
  const fiscalize = useFiscalizeCrmSaleMutation();

  const warehouses = space.warehouses.filter((w) => !w.archived);
  const [warehouseId, setWarehouseId] = React.useState(
    () => (warehouses.find((w) => w.isDefault) ?? warehouses[0])?.id ?? "",
  );
  // One id per receipt being rung up: a retried pay click after a lost
  // response is a relay duplicate, never a second sale.
  const [saleId, setSaleId] = React.useState(() => crypto.randomUUID());
  const [lines, setLines] = React.useState<SaleLineDraft[]>([]);
  const [customer, setCustomer] = React.useState<CrmParty | null>(null);
  const [payment, setPayment] =
    React.useState<CashPaymentDraft>(newCashPayment);
  const [bonusText, setBonusText] = React.useState("");
  const [submitted, setSubmitted] = React.useState(false);
  const [busy, setBusy] = React.useState(false);

  // A visit from the journal fills the cart once; the sale then pays for it.
  const checkout = useVisitCheckout(visit);
  const [appliedVisit, setAppliedVisit] = React.useState<string | null>(null);
  const loaded = checkout.data;
  React.useEffect(() => {
    if (!loaded || loaded.visit.saleId || appliedVisit === loaded.visit.id) {
      return;
    }
    setAppliedVisit(loaded.visit.id);
    setLines(loaded.lines);
    setCustomer(loaded.customer);
    setBonusText("");
    setSaleId(crypto.randomUUID());
  }, [loaded, appliedVisit]);
  React.useEffect(() => {
    if (visit === null) setAppliedVisit(null);
  }, [visit]);
  const payingVisit =
    visit !== null && loaded && appliedVisit === visit && !loaded.visit.saleId
      ? loaded.visit
      : null;

  const totals = saleTotals(lines);
  const evaluatedLines = lines.map(evaluateLine);
  const linesValid =
    lines.length > 0 &&
    lines.length <= MAX_SALE_LINES &&
    evaluatedLines.every((line) => line.ok);
  // Bonuses lower the receipt: they are spread over the lines as discount.
  const loyalty = useCrmLoyaltyQuery(customer?.id ?? null);
  const programme = loyalty.data?.programme;
  const bonusBalance = loyalty.data?.customer?.balanceMinor ?? 0;
  const bonusesOn = customer !== null && programme?.enabled === true;
  const bonus = evaluateCashBonus(
    bonusesOn ? bonusText : "",
    programme ? maxRedeemMinor(programme, totals.totalMinor, bonusBalance) : 0,
  );
  const bonusMinor = bonus.ok ? bonus.bonusMinor : 0;
  const payableMinor = totals.totalMinor - bonusMinor;
  const evaluatedPayment = evaluateCashPayment(payment, payableMinor);

  const shiftOpen =
    device.data?.reachable === true && device.data.shiftState === "opened";
  const printReceipt = fiscalization !== "off" && canFiscalize && shiftOpen;
  const blockedByShift = fiscalization === "required" && !shiftOpen;

  function reset() {
    setLines([]);
    setCustomer(null);
    setBonusText("");
    setPayment(newCashPayment());
    setSubmitted(false);
    setSaleId(crypto.randomUUID());
    post.reset();
  }

  async function handlePay(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (!linesValid || !evaluatedPayment.ok || warehouseId === "") return;
    if (!bonus.ok) return;
    const okLines = evaluatedLines.flatMap((line) => (line.ok ? [line] : []));
    const saleLines = spreadBonus(
      okLines.map((line) => line.line),
      okLines.map((line) => line.amountMinor),
      bonusMinor,
    );
    if (saleLines === null) return;
    if (blockedByShift || busy) return;
    setBusy(true);
    try {
      await post.mutateAsync({
        channelId: space.channelId,
        saleId,
        warehouseId,
        customerId: customer?.id ?? null,
        payment: evaluatedPayment.payment,
        payments: evaluatedPayment.payments,
        cashTenderedMinor: evaluatedPayment.cashTenderedMinor,
        lines: saleLines,
        bonusMinor: bonusMinor > 0 ? bonusMinor : null,
        note: null,
        appointmentId: payingVisit?.id ?? null,
      });
    } catch {
      setBusy(false);
      return; // Shown below through the mutation error.
    }
    const posted = saleId;
    reset();
    if (payingVisit) onVisitDone();
    const number = await getCrmSale(posted)
      .then((sale) => String(sale.number))
      .catch(() => "");
    if (!printReceipt) {
      toast.success(pay.paidNoReceipt.replace("{number}", number));
      setBusy(false);
      return;
    }
    try {
      const job = await fiscalize.mutateAsync({
        channelId: space.channelId,
        saleId: posted,
        operation: "sell",
      });
      if (job === null) {
        toast.success(pay.paidPrinted.replace("{number}", number));
      } else {
        toast.warning(
          pay.paidQueued
            .replace("{number}", number)
            .replace("{error}", job.error ?? ru.cash.queue.states[job.state]),
        );
      }
    } catch (error) {
      toast.warning(
        pay.paidQueued
          .replace("{number}", number)
          .replace("{error}", describeCrmError(error).message),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <section
      aria-labelledby="crm-cash-title"
      className="space-y-6"
      data-testid="crm-cash"
    >
      <SectionHeader
        description={canPost ? undefined : ru.cash.readOnlyHint}
        title={<span id="crm-cash-title">{ru.cash.title}</span>}
      />
      <CrmCashShiftCard canOperate={canFiscalize} channelId={space.channelId} />

      {canPost ? (
        <form
          className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_20rem]"
          noValidate
          onSubmit={(event) => void handlePay(event)}
        >
          <div className="space-y-4">
            <CrmCashScanner
              onPick={(product) =>
                setLines((current) => addProductToLines(current, product))
              }
            />
            {visit !== null ? (
              <VisitBanner
                error={checkout.error}
                loading={checkout.isPending}
                onUnlink={onVisitDone}
                visit={loaded?.visit ?? null}
              />
            ) : null}
            <div className="flex items-center justify-between">
              <h3 className="text-sm font-medium">{ru.cash.cart.title}</h3>
              {lines.length > 0 ? (
                <Button
                  onClick={() => setLines([])}
                  size="sm"
                  type="button"
                  variant="ghost"
                >
                  {ru.cash.cart.clear}
                </Button>
              ) : null}
            </div>
            <CrmCashCart
              lines={lines}
              onChange={(index, patch) =>
                setLines((current) =>
                  current.map((line, i) =>
                    i === index ? { ...line, ...patch } : line,
                  ),
                )
              }
              onRemove={(index) =>
                setLines((current) => current.filter((_, i) => i !== index))
              }
              showErrors={submitted}
            />
            {lines.length > MAX_SALE_LINES ? (
              <p className="text-sm text-destructive" role="alert">
                {ru.sales.dialog.tooManyLines}
              </p>
            ) : null}
          </div>

          <div className="space-y-4 rounded-2xl border border-border/70 p-4">
            <h3 className="text-sm font-medium">{pay.title}</h3>
            {warehouses.length > 1 ? (
              <CrmField
                id="crm-cash-warehouse"
                label={ru.sales.dialog.warehouse}
              >
                <NativeSelect
                  {...crmControlProps("crm-cash-warehouse", false, undefined)}
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
            ) : null}
            <CustomerPicker
              customer={customer}
              onChange={(next) => {
                setCustomer(next);
                setBonusText("");
              }}
            />
            {bonusesOn && programme ? (
              <CrmCashBonuses
                balanceMinor={bonusBalance}
                baseMinor={totals.totalMinor}
                onChange={setBonusText}
                programme={programme}
                showErrors={submitted}
                state={bonus}
                value={bonusText}
              />
            ) : null}
            <CrmCashPayment
              draft={payment}
              evaluated={evaluatedPayment}
              onChange={(patch) =>
                setPayment((current) => ({ ...current, ...patch }))
              }
              showErrors={submitted}
              totalMinor={payableMinor}
            />
            {blockedByShift ? (
              <p
                className="text-sm text-amber-600 dark:text-amber-400"
                data-testid="crm-cash-shift-required"
              >
                {pay.shiftRequired}
              </p>
            ) : fiscalization === "off" ? (
              <p className="text-xs text-muted-foreground">{pay.noReceipt}</p>
            ) : null}
            <CrmErrorAlert error={post.error} testId="crm-cash-pay-error" />
            <Button
              className="h-11 w-full text-base"
              data-testid="crm-cash-pay"
              disabled={busy || blockedByShift}
              type="submit"
            >
              {busy
                ? pay.submitting
                : pay.submit.replace("{total}", formatRubles(payableMinor))}
            </Button>
          </div>
        </form>
      ) : null}

      <CrmCashQueue canOperate={canFiscalize} />
    </section>
  );
}
