import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// PR screenshots for the CRM catalog. Each shot is scoped to its subject
// (the CRM view or the product dialog) so the PNGs differ by state.

const OUT = "test-results/screenshots/crm-products";
const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;
type ProductSeed = NonNullable<
  NonNullable<CrmSeed["space"]>["products"]
>[number];

const CATALOG: ProductSeed[] = [
  {
    name: "Молоко 3,2% 1 л",
    sku: "MLK-001",
    barcodes: ["4600051000057"],
    price_minor: 8990,
    vat: "vat10",
    category: "Молочные продукты",
    min_stock_milli: 10_000,
  },
  {
    name: "Сыр российский",
    sku: "CHS-001",
    unit: "kilogram",
    price_minor: 89900,
    vat: "vat10",
    category: "Молочные продукты",
  },
  { name: "Хлеб бородинский", sku: "BRD-7", price_minor: 6500 },
  {
    name: "Кроссовки беговые, р. 42",
    sku: "SHO-42",
    price_minor: 549000,
    requires_marking: true,
  },
  {
    name: "Доставка по городу",
    unit: "service_unit",
    price_minor: 30000,
    vat: "none",
    payment_object: "service",
  },
  {
    name: "Кефир 2,5% (снят с продажи)",
    sku: "KFR-OLD",
    price_minor: 7450,
    archived: true,
  },
];

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await installMockBridge(page);
});

async function openCatalog(page: Page, role: "owner" | "guest") {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((data) => window.__BUZZ_E2E_SEED_CRM__?.(data), {
    space: {
      channelId: SPACE_CHANNEL,
      role,
      warehouses: [
        { id: "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e", name: "Основной склад" },
      ],
      products: CATALOG,
    },
  } satisfies CrmSeed);
  await page.getByTestId("open-crm-view").click();
  await expect(page.getByTestId("crm-products-table")).toBeVisible();
}

test("owner catalog", async ({ page }) => {
  await openCatalog(page, "owner");
  await waitForAnimations(page);
  await page
    .getByTestId("crm-view")
    .screenshot({ path: `${OUT}/01-catalog.png` });
});

test("new product with an invalid price", async ({ page }) => {
  await openCatalog(page, "owner");
  await page.getByTestId("crm-product-add").click();
  const dialog = page.getByTestId("crm-product-dialog");
  await dialog.getByLabel("Название").fill("Кефир 1% 900 г");
  await dialog.getByLabel("Артикул").fill("KFR-1");
  await dialog.getByLabel("Цена за единицу, ₽").fill("89,999");
  await dialog.getByTestId("crm-product-dialog-submit").click();
  await expect(
    dialog.getByText("Не больше двух знаков после запятой."),
  ).toBeVisible();
  await waitForAnimations(page);
  await dialog.screenshot({ path: `${OUT}/02-invalid-price.png` });
});

test("duplicate SKU conflict", async ({ page }) => {
  await openCatalog(page, "owner");
  await page
    .getByRole("button", { name: "Изменить: Хлеб бородинский" })
    .click();
  const dialog = page.getByTestId("crm-product-dialog");
  await dialog.getByLabel("Артикул").fill("MLK-001");
  await dialog.getByTestId("crm-product-dialog-submit").click();
  await expect(dialog.getByTestId("crm-product-dialog-error")).toBeVisible();
  await waitForAnimations(page);
  await dialog.screenshot({ path: `${OUT}/03-sku-conflict.png` });
});

test("read-only catalog with archived products", async ({ page }) => {
  await openCatalog(page, "guest");
  await page.getByRole("switch", { name: "Показать архивные" }).click();
  await expect(
    page.getByTestId("crm-products-table").getByText("В архиве"),
  ).toBeVisible();
  await waitForAnimations(page);
  await page
    .getByTestId("crm-view")
    .screenshot({ path: `${OUT}/04-read-only-archived.png` });
});
