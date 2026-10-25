import type * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import {
  CRM_PAYMENT_OBJECTS,
  CRM_UNITS,
  isBookable,
  type ProductDraft,
  type ProductDraftErrors,
  productVatOptions,
} from "@/features/crm/lib/catalog";
import type { CrmVatRate } from "@/shared/api/tauriCrm";
import type {
  CrmPaymentObject,
  CrmProductKind,
  CrmUnit,
} from "@/shared/api/tauriCrmProducts";
import { cn } from "@/shared/lib/cn";
import { Checkbox } from "@/shared/ui/checkbox";
import { Input } from "@/shared/ui/input";
import { FieldLabel, NativeSelect } from "@/shared/ui/native-select";
import { Textarea } from "@/shared/ui/textarea";

/** DOM id of a product form control (also used to focus invalid fields). */
export function productFieldId(field: string): string {
  return `crm-product-${field}`;
}

type FieldsProps = {
  draft: ProductDraft;
  errors: ProductDraftErrors;
  kind: CrmProductKind;
  mode: "create" | "edit";
  onChange: (patch: Partial<ProductDraft>) => void;
};

/** Accessibility wiring for a control with an optional hint and error. */
function controlProps(field: string, hint: boolean, error: string | undefined) {
  const id = productFieldId(field);
  const described = [hint && `${id}-hint`, error && `${id}-error`].filter(
    Boolean,
  );
  return {
    id,
    "data-testid": id,
    "aria-invalid": error ? true : undefined,
    "aria-describedby": described.length > 0 ? described.join(" ") : undefined,
  } as const;
}

function Field({
  children,
  className,
  error,
  field,
  hint,
  label,
}: {
  children: React.ReactNode;
  className?: string;
  error?: string;
  field: string;
  hint?: string;
  label: string;
}) {
  const id = productFieldId(field);
  return (
    <div className={cn("space-y-1.5", className)}>
      <FieldLabel htmlFor={id}>{label}</FieldLabel>
      {children}
      {hint ? (
        <p className="text-xs text-muted-foreground" id={`${id}-hint`}>
          {hint}
        </p>
      ) : null}
      {error ? (
        <p className="text-xs text-destructive" id={`${id}-error`}>
          {error}
        </p>
      ) : null}
    </div>
  );
}

function FlagField({
  checked,
  field,
  hint,
  label,
  onChange,
}: {
  checked: boolean;
  field: string;
  hint: string;
  label: string;
  onChange: (checked: boolean) => void;
}) {
  const id = productFieldId(field);
  return (
    <div className="flex items-start gap-2.5">
      <Checkbox
        aria-describedby={`${id}-hint`}
        checked={checked}
        className="mt-0.5"
        data-testid={id}
        id={id}
        onCheckedChange={(value) => onChange(value === true)}
      />
      <div className="space-y-0.5">
        <label className="text-sm" htmlFor={id}>
          {label}
        </label>
        <p className="text-xs text-muted-foreground" id={`${id}-hint`}>
          {hint}
        </p>
      </div>
    </div>
  );
}

/** Payment objects a service may have; goods offer them all. */
const SERVICE_PAYMENT_OBJECTS = CRM_PAYMENT_OBJECTS.filter(isBookable);

export function CrmProductFormFields({
  draft,
  errors,
  kind,
  mode,
  onChange,
}: FieldsProps) {
  const f = ru.products.fields;
  const services = kind === "services";
  const durationField = isBookable(draft.paymentObject) ? (
    <Field
      error={errors.duration}
      field="duration"
      hint={ru.services.fields.durationHint}
      label={ru.services.fields.duration}
    >
      <Input
        {...controlProps("duration", true, errors.duration)}
        inputMode="numeric"
        max={720}
        min={5}
        onChange={(event) => onChange({ duration: event.target.value })}
        step={5}
        type="number"
        value={draft.duration}
      />
    </Field>
  ) : null;
  const archivedField =
    mode === "edit" ? (
      <FlagField
        checked={draft.archived}
        field="archived"
        hint={services ? ru.services.fields.archivedHint : f.archivedHint}
        label={f.archived}
        onChange={(archived) => onChange({ archived })}
      />
    ) : null;
  const paymentObjectField = (
    <Field field="paymentObject" label={f.paymentObject}>
      <NativeSelect
        {...controlProps("paymentObject", false, undefined)}
        onChange={(value) =>
          onChange({ paymentObject: value as CrmPaymentObject })
        }
        value={draft.paymentObject}
      >
        {(services ? SERVICE_PAYMENT_OBJECTS : CRM_PAYMENT_OBJECTS).map(
          (object) => (
            <option key={object} value={object}>
              {ru.paymentObjects[object]}
            </option>
          ),
        )}
      </NativeSelect>
    </Field>
  );
  const nameField = (
    <Field
      className="sm:col-span-2"
      error={errors.name}
      field="name"
      label={f.name}
    >
      <Input
        {...controlProps("name", false, errors.name)}
        autoFocus
        maxLength={128}
        onChange={(event) => onChange({ name: event.target.value })}
        value={draft.name}
      />
    </Field>
  );
  const categoryField = (
    <Field field="category" label={f.category}>
      <Input
        {...controlProps("category", false, undefined)}
        maxLength={64}
        onChange={(event) => onChange({ category: event.target.value })}
        value={draft.category}
      />
    </Field>
  );
  const priceField = (
    <Field
      error={errors.price}
      field="price"
      hint={f.priceHint}
      label={services ? ru.services.fields.price : f.price}
    >
      <Input
        {...controlProps("price", true, errors.price)}
        inputMode="decimal"
        onChange={(event) => onChange({ price: event.target.value })}
        value={draft.price}
      />
    </Field>
  );
  const vatField = (
    <Field error={errors.vat} field="vat" label={f.vat}>
      <NativeSelect
        {...controlProps("vat", false, errors.vat)}
        onChange={(vat) => onChange({ vat: vat as CrmVatRate })}
        value={draft.vat}
      >
        {productVatOptions(draft.vat).map((rate) => (
          <option key={rate} value={rate}>
            {ru.vat[rate]}
          </option>
        ))}
      </NativeSelect>
    </Field>
  );

  if (services) {
    return (
      <div className="grid gap-4 sm:grid-cols-2">
        {nameField}
        {durationField}
        {categoryField}
        {priceField}
        {vatField}
        {paymentObjectField}
        {archivedField ? (
          <div className="sm:col-span-2">{archivedField}</div>
        ) : null}
      </div>
    );
  }

  return (
    <div className="grid gap-4 sm:grid-cols-2">
      {nameField}

      <Field error={errors.sku} field="sku" hint={f.skuHint} label={f.sku}>
        <Input
          {...controlProps("sku", true, errors.sku)}
          maxLength={64}
          onChange={(event) => onChange({ sku: event.target.value })}
          value={draft.sku}
        />
      </Field>

      {categoryField}

      <Field field="unit" label={f.unit}>
        <NativeSelect
          {...controlProps("unit", false, undefined)}
          onChange={(unit) => onChange({ unit: unit as CrmUnit })}
          value={draft.unit}
        >
          {CRM_UNITS.map((unit) => (
            <option key={unit} value={unit}>
              {ru.units.full[unit]}
            </option>
          ))}
        </NativeSelect>
      </Field>

      {priceField}

      {vatField}

      {paymentObjectField}

      {durationField}

      <Field
        error={errors.minStock}
        field="minStock"
        hint={f.minStockHint}
        label={`${f.minStock}, ${ru.units.short[draft.unit]}`}
      >
        <Input
          {...controlProps("minStock", true, errors.minStock)}
          inputMode="decimal"
          onChange={(event) => onChange({ minStock: event.target.value })}
          value={draft.minStock}
        />
      </Field>

      <Field
        className="sm:col-span-2"
        error={errors.barcodes}
        field="barcodes"
        hint={f.barcodesHint}
        label={f.barcodes}
      >
        <Textarea
          {...controlProps("barcodes", true, errors.barcodes)}
          className="min-h-16 font-mono"
          onChange={(event) => onChange({ barcodes: event.target.value })}
          rows={2}
          value={draft.barcodes}
        />
      </Field>

      <div className="space-y-3 sm:col-span-2">
        <FlagField
          checked={draft.requiresMarking}
          field="requiresMarking"
          hint={f.requiresMarkingHint}
          label={f.requiresMarking}
          onChange={(requiresMarking) => onChange({ requiresMarking })}
        />
        {archivedField}
      </div>
    </div>
  );
}
