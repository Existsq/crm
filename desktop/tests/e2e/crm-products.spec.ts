import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Mirrors `.specs/features/crm-catalog/spec.md` (CRMC-04 catalog screen,
// CRMC-05 money handling).

const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const MAIN_WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const MILK = "7d1b6c8e-1a2b-4c3d-8e9f-0a1b2c3d4e5f";
const CHEESE = "8e2c7d9f-2b3c-4d4e-9fa0-1b2c3d4e5f60";
const BREAD = "9f3d8ea0-3c4d-4e5f-a0b1-2c3d4e5f6071";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;
type ProductSeed = NonNullable<
  NonNullable<CrmSeed["space"]>["products"]
>[number];

const CATALOG: ProductSeed[] = [
  {
    id: MILK,
    name: "Молоко 3,2% 1 л",
    sku: "MLK-001",
    barcodes: ["4600051000057"],
    price_minor: 8990,
    vat: "vat10",
    category: "Молочные продукты",
  },
  {
    id: CHEESE,
    name: "Сыр российский",
    sku: "CHS-001",
    unit: "kilogram",
    price_minor: 89900,
    vat: "vat10",
  },
  { id: BREAD, name: "Хлеб бородинский", price_minor: 6500 },
];

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

function seed(
  role: "owner" | "member" | "guest",
  products: ProductSeed[] = CATALOG,
): CrmSeed {
  return {
    space: {
      channelId: SPACE_CHANNEL,
      role,
      settings: { default_vat: "vat22" },
      warehouses: [{ id: MAIN_WAREHOUSE, name: "Основной склад" }],
      products,
    },
  };
}

async function openCatalog(page: Page, value: CrmSeed) {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((data) => window.__BUZZ_E2E_SEED_CRM__?.(data), value);
  await page.getByTestId("open-crm-view").click();
  await expect(page).toHaveURL(/#\/crm\/products$/);
  await expect(page.getByTestId("crm-products")).toBeVisible();
  await expect(page.getByTestId("crm-products-loading")).toHaveCount(0);
}

async function openCreateDialog(page: Page) {
  await page.getByTestId("crm-product-add").click();
  const dialog = page.getByTestId("crm-product-dialog");
  await expect(dialog).toBeVisible();
  await waitForAnimations(page);
  return dialog;
}

function row(page: Page, text: string) {
  return page.getByTestId("crm-products-table").locator("tbody tr", {
    hasText: text,
  });
}

test("an editor creates a product and sees its price in rubles", async ({
  page,
}) => {
  await openCatalog(page, seed("owner", []));
  await expect(page.getByTestId("crm-products-empty")).toContainText(
    "Товаров пока нет",
  );

  const dialog = await openCreateDialog(page);
  await expect(dialog.getByRole("heading")).toHaveText("Новый товар");
  await dialog.getByLabel("Название").fill("Кефир 1%");
  await dialog.getByLabel("Артикул").fill("KFR-1");
  await dialog.getByLabel("Цена за единицу, ₽").fill("1 234,50");
  await expect(dialog.getByLabel("НДС", { exact: true })).toHaveValue("vat22");
  await dialog.getByTestId("crm-product-dialog-submit").click();

  await expect(dialog).toHaveCount(0);
  await expect(page.getByText("Товар добавлен")).toBeVisible();
  const created = row(page, "Кефир 1%");
  await expect(created).toContainText("KFR-1");
  await expect(created).toContainText("шт");
  await expect(created).toContainText("1 234,50 ₽");
  await expect(created).toContainText("НДС 22%");

  const table = page.getByRole("table", { name: "Каталог товаров" });
  await expect(table.getByRole("columnheader", { name: "Цена" })).toBeVisible();
});

test("an invalid price blocks the save with a message", async ({ page }) => {
  await openCatalog(page, seed("owner", []));
  const dialog = await openCreateDialog(page);
  await dialog.getByLabel("Название").fill("Кефир");
  const price = dialog.getByLabel("Цена за единицу, ₽");
  const submit = dialog.getByTestId("crm-product-dialog-submit");

  const cases = [
    ["12,345", "Не больше двух знаков после запятой."],
    ["-5", "Цена не может быть отрицательной."],
    ["двадцать", "Введите цену числом, например 1 234,50."],
  ] as const;
  for (const [text, message] of cases) {
    await price.fill(text);
    await submit.click();
    await expect(dialog.getByText(message)).toBeVisible();
    await expect(price).toHaveAttribute("aria-invalid", "true");
    await expect(price).toBeFocused();
  }

  const writes = await page.evaluate(
    () =>
      window.__BUZZ_E2E_COMMANDS__?.filter(
        (command) => command === "crm_write_product",
      ).length ?? 0,
  );
  expect(writes).toBe(0);
  await expect(dialog).toBeVisible();

  await price.fill("89,9");
  await expect(price).not.toHaveAttribute("aria-invalid", "true");
  await submit.click();
  await expect(dialog).toHaveCount(0);
  await expect(row(page, "Кефир")).toContainText("89,90 ₽");
});

test("an editor edits a product", async ({ page }) => {
  await openCatalog(page, seed("member"));
  await page
    .getByRole("button", { name: "Изменить: Хлеб бородинский" })
    .click();
  const dialog = page.getByTestId("crm-product-dialog");
  await waitForAnimations(page);
  await expect(dialog.getByRole("heading")).toHaveText("Изменить товар");
  await expect(dialog.getByLabel("Название")).toHaveValue("Хлеб бородинский");
  const price = dialog.getByLabel("Цена за единицу, ₽");
  await expect(price).toHaveValue("65,00");

  await price.fill("72,5");
  await dialog.getByLabel("Единица измерения").selectOption("kilogram");
  await dialog.getByLabel("Минимальный остаток, кг").fill("1,5");
  await dialog.getByTestId("crm-product-dialog-submit").click();

  await expect(dialog).toHaveCount(0);
  await expect(page.getByText("Товар сохранён")).toBeVisible();
  const bread = row(page, "Хлеб бородинский");
  await expect(bread).toContainText("72,50 ₽");
  await expect(bread).toContainText("кг");
  // Focus returns to the row's own button, not to the page body.
  await expect(
    page.getByRole("button", { name: "Изменить: Хлеб бородинский" }),
  ).toBeFocused();
});

test("a duplicate SKU keeps the dialog open with the Russian message", async ({
  page,
}) => {
  await openCatalog(page, seed("owner"));
  await page.getByRole("button", { name: "Изменить: Сыр российский" }).click();
  const dialog = page.getByTestId("crm-product-dialog");
  await waitForAnimations(page);
  const sku = dialog.getByLabel("Артикул");
  await sku.fill("MLK-001");
  await dialog.getByTestId("crm-product-dialog-submit").click();

  await expect(dialog.getByTestId("crm-product-dialog-error")).toContainText(
    "Артикул «MLK-001» уже используется другим товаром.",
  );
  await expect(dialog).toBeVisible();
  await expect(sku).toHaveValue("MLK-001");
  // The edit form is taller than a 720px window: its fields scroll, while the
  // relay error and the actions stay on screen.
  await expect(dialog.getByTestId("crm-product-dialog-error")).toBeInViewport();
  await expect(
    dialog.getByTestId("crm-product-dialog-submit"),
  ).toBeInViewport();

  await sku.fill("CHS-002");
  await dialog.getByTestId("crm-product-dialog-submit").click();
  await expect(dialog).toHaveCount(0);
  await expect(row(page, "Сыр российский")).toContainText("CHS-002");
});

test("search is debounced and matches name or SKU prefix", async ({ page }) => {
  await openCatalog(page, seed("owner"));
  const table = page.getByTestId("crm-products-table");
  await expect(table.locator("tbody tr")).toHaveCount(3);

  const search = page.getByRole("searchbox", { name: "Поиск товаров" });
  await search.pressSequentially("молоко", { delay: 20 });
  await expect(table.locator("tbody tr")).toHaveCount(1);
  await expect(row(page, "Молоко 3,2% 1 л")).toBeVisible();
  const queries = await page.evaluate(() =>
    (window.__BUZZ_E2E_COMMAND_PAYLOADS__ ?? [])
      .filter((entry) => entry.command === "crm_list_products")
      .map((entry) => (entry.payload as { query: string | null }).query)
      .filter(Boolean),
  );
  // One request for the settled text, none for the keystrokes before it.
  expect(queries).toEqual(["молоко"]);

  await search.fill("chs");
  await expect(table.locator("tbody tr")).toHaveCount(1);
  await expect(row(page, "Сыр российский")).toBeVisible();

  await search.fill("нет такого");
  await expect(page.getByTestId("crm-products-empty")).toContainText(
    "Ничего не найдено",
  );
  await search.fill("");
  await expect(table.locator("tbody tr")).toHaveCount(3);
});

test("archiving hides a product until archived ones are shown", async ({
  page,
}) => {
  await openCatalog(page, seed("owner"));
  await page
    .getByRole("button", { name: "Изменить: Хлеб бородинский" })
    .click();
  const dialog = page.getByTestId("crm-product-dialog");
  await waitForAnimations(page);
  await dialog.getByRole("checkbox", { name: "В архиве" }).check();
  await dialog.getByTestId("crm-product-dialog-submit").click();
  await expect(dialog).toHaveCount(0);
  await expect(row(page, "Хлеб бородинский")).toHaveCount(0);

  await page.getByRole("switch", { name: "Показать архивные" }).click();
  const archived = row(page, "Хлеб бородинский");
  await expect(archived).toBeVisible();
  await expect(archived).toContainText("В архиве");
});

test("a role without edit_catalog sees the catalog read-only", async ({
  page,
}) => {
  await openCatalog(page, seed("guest"));
  await expect(page.getByTestId("crm-products")).toContainText(
    "У вашей роли нет права изменять каталог.",
  );
  await expect(row(page, "Молоко 3,2% 1 л")).toContainText("89,90 ₽");
  await expect(page.getByTestId("crm-product-add")).toHaveCount(0);
  await expect(page.getByRole("button", { name: /^Изменить/ })).toHaveCount(0);
  await expect(
    page.getByRole("columnheader", { name: "Действия" }),
  ).toHaveCount(0);
});

test("a product can be created with the keyboard alone", async ({ page }) => {
  await openCatalog(page, seed("owner", []));
  const add = page.getByTestId("crm-product-add");
  await page.getByTestId("crm-section-settings").focus();
  await page.keyboard.press("Tab");
  await expect(add).toBeFocused();
  await page.keyboard.press("Enter");

  const dialog = page.getByTestId("crm-product-dialog");
  await expect(dialog.getByLabel("Название")).toBeFocused();
  await page.keyboard.type("Батон");
  // Name → SKU → category → unit → price.
  for (let step = 0; step < 4; step += 1) await page.keyboard.press("Tab");
  await expect(dialog.getByLabel("Цена за единицу, ₽")).toBeFocused();
  await page.keyboard.type("45");
  await page.keyboard.press("Enter");

  await expect(dialog).toHaveCount(0);
  await expect(row(page, "Батон")).toContainText("45,00 ₽");
  await expect(add).toBeFocused();
});

test("long catalogs load the next page on demand", async ({ page }) => {
  const many = Array.from({ length: 55 }, (_, index) => ({
    name: `Товар ${String(index + 1).padStart(2, "0")}`,
    price_minor: 100 * (index + 1),
  }));
  await openCatalog(page, seed("owner", many));
  const rows = page.getByTestId("crm-products-table").locator("tbody tr");
  await expect(rows).toHaveCount(50);
  await page.getByRole("button", { name: "Загрузить ещё" }).click();
  await expect(rows).toHaveCount(55);
  await expect(rows.last()).toContainText("Товар 55");
  await expect(page.getByTestId("crm-products-load-more")).toHaveCount(0);
});

test("an edit that meets a newer revision is a conflict with a way back", async ({
  page,
}) => {
  await openCatalog(page, seed("member"));
  await page
    .getByRole("button", { name: "Изменить: Хлеб бородинский" })
    .click();
  const dialog = page.getByTestId("crm-product-dialog");
  await waitForAnimations(page);
  const price = dialog.getByLabel("Цена за единицу, ₽");
  await price.fill("72,5");

  // Another cashier saves the product; the live CRM event refetches it while
  // this dialog is open with unsaved edits.
  await page.waitForFunction(() => window.__BUZZ_E2E_CRM_LIVE_READY__?.());
  await page.evaluate(
    (productId) =>
      window.__BUZZ_E2E_CRM_EDIT_PRODUCT_ELSEWHERE__?.({
        productId,
        patch: { price_minor: 7000 },
      }),
    BREAD,
  );
  await expect(row(page, "Хлеб бородинский")).toContainText("70,00 ₽");
  await expect(price).toHaveValue("72,5");

  // Saving must not overwrite the other change.
  await dialog.getByTestId("crm-product-dialog-submit").click();
  await expect(dialog.getByTestId("crm-product-dialog-error")).toContainText(
    "Данные изменились",
  );
  await expect(dialog.getByTestId("crm-product-dialog-submit")).toBeDisabled();

  await dialog.getByTestId("crm-product-dialog-reload").click();
  await expect(price).toHaveValue("70,00");
  await expect(dialog.getByTestId("crm-product-dialog-error")).toHaveCount(0);
  await price.fill("75");
  await dialog.getByTestId("crm-product-dialog-submit").click();
  await expect(dialog).toHaveCount(0);
  await expect(row(page, "Хлеб бородинский")).toContainText("75,00 ₽");
});

test("a create retried after a lost response is not duplicated", async ({
  page,
}) => {
  await openCatalog(page, {
    ...seed("owner", []),
    loseNextWriteResponse: true,
  });
  const dialog = await openCreateDialog(page);
  await dialog.getByLabel("Название").fill("Кефир 1%");
  await dialog.getByLabel("Цена за единицу, ₽").fill("99");
  await dialog.getByTestId("crm-product-dialog-submit").click();
  // The relay stored the product but the response was lost.
  await expect(dialog.getByTestId("crm-product-dialog-error")).toBeVisible();
  await dialog.getByTestId("crm-product-dialog-submit").click();
  await expect(dialog.getByTestId("crm-product-dialog-error")).toContainText(
    "Данные изменились",
  );
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(row(page, "Кефир 1%")).toHaveCount(1);
});
