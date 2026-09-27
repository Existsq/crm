import * as React from "react";

import { ru } from "@/features/crm/i18n/ru";
import {
  emptyProductDraft,
  PRODUCT_DRAFT_FIELDS,
  type ProductDraft,
  productDraftFrom,
  sameProductDraft,
  validateProductDraft,
} from "@/features/crm/lib/catalog";
import { CrmErrorAlert } from "@/features/crm/ui/CrmErrorAlert";
import {
  CrmProductFormFields,
  productFieldId,
} from "@/features/crm/ui/products/CrmProductFormFields";
import type { CrmVatRate } from "@/shared/api/tauriCrm";
import type {
  CrmProduct,
  CrmProductFields,
} from "@/shared/api/tauriCrmProducts";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";

/**
 * Create or edit a product; the caller owns the mutation. Mount it with a
 * fresh `key` per opening so the draft starts from `product`.
 */
export function CrmProductDialog({
  defaultVat,
  error,
  onCloseAutoFocus,
  onOpenChange,
  onSubmit,
  open,
  pending,
  product,
}: {
  defaultVat: CrmVatRate;
  error: unknown;
  /** Where focus goes on close (the dialog has no Radix trigger). */
  onCloseAutoFocus: (event: Event) => void;
  onOpenChange: (open: boolean) => void;
  onSubmit: (fields: CrmProductFields) => void;
  open: boolean;
  pending: boolean;
  /** Live product being edited, `null` to create one. */
  product: CrmProduct | null;
}) {
  const initialDraft = () =>
    product ? productDraftFrom(product) : emptyProductDraft(defaultVat);
  const [baseline, setBaseline] = React.useState<ProductDraft>(initialDraft);
  const [draft, setDraft] = React.useState<ProductDraft>(initialDraft);
  const [baseRevision, setBaseRevision] = React.useState(
    product?.revision ?? null,
  );
  // Errors appear on the first save attempt, then track the draft live.
  // Not on blur: a message appearing on blur shifts the Save button between
  // mousedown and click, swallowing the click.
  const [submitted, setSubmitted] = React.useState(false);

  // Adopt a product changed elsewhere unless the user has unsaved edits
  // (same baseline/draft rule as the settings form).
  if (product && product.revision !== baseRevision) {
    const next = productDraftFrom(product);
    setBaseRevision(product.revision);
    setBaseline(next);
    if (sameProductDraft(draft, baseline)) setDraft(next);
  }

  const validation = validateProductDraft(draft);
  const visibleErrors = submitted && !validation.ok ? validation.errors : {};

  function handleSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSubmitted(true);
    if (!validation.ok) {
      const first = PRODUCT_DRAFT_FIELDS.find(
        (field) => validation.errors[field],
      );
      if (first) {
        event.currentTarget
          .querySelector<HTMLElement>(`#${productFieldId(first)}`)
          ?.focus();
      }
      return;
    }
    onSubmit(validation.fields);
  }

  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="flex max-h-[calc(100dvh-2rem)] flex-col"
        data-testid="crm-product-dialog"
        onCloseAutoFocus={onCloseAutoFocus}
      >
        {/* Fields scroll; the title, relay error and actions stay in view. */}
        <form
          className="flex min-h-0 flex-1 flex-col gap-5"
          noValidate
          onSubmit={handleSubmit}
        >
          <DialogHeader>
            <DialogTitle>
              {product ? ru.products.editTitle : ru.products.createTitle}
            </DialogTitle>
          </DialogHeader>
          <div className="-mx-1 min-h-0 flex-1 overflow-y-auto px-1 pb-1">
            <CrmProductFormFields
              draft={draft}
              errors={visibleErrors}
              mode={product ? "edit" : "create"}
              onChange={(patch) =>
                setDraft((current) => ({ ...current, ...patch }))
              }
            />
          </div>
          <CrmErrorAlert error={error} testId="crm-product-dialog-error" />
          <DialogFooter>
            <Button
              onClick={() => onOpenChange(false)}
              type="button"
              variant="ghost"
            >
              {ru.common.cancel}
            </Button>
            <Button
              data-testid="crm-product-dialog-submit"
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
