import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Stock levels, stock documents (kinds 42106/42107) and tech cards (42108).

const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const MAIN = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const SALON = "1c8f2a8b-7b64-4d56-8e9f-2a3b4c5d6e7f";
const SHAMPOO = "7d1b6c8e-1a2b-4c3d-8e9f-0a1b2c3d4e5f";
const DYE = "8e2c7d9f-2b3c-4d4e-9fa0-1b2c3d4e5f60";
const HAIRCUT = "9f3d8eaf-3c4d-4e5f-8a1b-2c3d4e5f6071";
const OXIDE = "a04e9fb0-4d5e-4f60-9b2c-3d4e5f607182";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

function seed(role: "owner" | "member" | "guest"): CrmSeed {
  return {
    space: {
      channelId: SPACE_CHANNEL,
      role,
      warehouses: [
        { id: MAIN, name: "Основной склад" },
        { id: SALON, name: "Зал" },
      ],
      products: [
        {
          id: SHAMPOO,
          name: "Шампунь",
          price_minor: 50000,
          min_stock_milli: 5000,
        },
        { id: DYE, name: "Краска", price_minor: 90000 },
        {
          id: OXIDE,
          name: "Оксид 6%",
          unit: "milliliter",
          price_minor: 100,
        },
        {
          id: HAIRCUT,
          name: "Стрижка",
          price_minor: 150000,
          payment_object: "service",
          unit: "service_unit",
          duration_min: 60,
        },
      ],
      parties: [{ name: "Косметика Опт", type: "supplier" }],
      stock: [
        { productId: SHAMPOO, qtyMilli: 10_000, costMinor: 25000 },
        { productId: DYE, qtyMilli: 3_000, costMinor: 40000 },
        { productId: OXIDE, qtyMilli: 1_000_000, costMinor: 50 },
      ],
    },
  };
}

async function openStock(page: Page, value: CrmSeed) {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((data) => window.__BUZZ_E2E_SEED_CRM__?.(data), value);
  await page.getByTestId("open-crm-view").click();
  await page.getByTestId("crm-section-stock").click();
  await expect(page).toHaveURL(/#\/crm\/stock$/);
}

async function newDoc(page: Page, type: string) {
  await page.getByTestId("crm-stock-new-doc").click();
  await page.getByTestId(`crm-stock-new-${type}`).click();
  const dialog = page.getByTestId("crm-stock-doc-dialog");
  await waitForAnimations(page);
  return dialog;
}

async function addLine(dialog: ReturnType<Page["getByTestId"]>, q: string) {
  await dialog.getByTestId("crm-stock-doc-product-search").fill(q);
  await dialog
    .getByTestId("crm-stock-doc-product-search-option")
    .first()
    .click();
}

const qtyOf = (page: Page, productId: string) =>
  page.getByTestId(`crm-stock-item-${productId}`).getByTestId("crm-stock-qty");

test("a receipt from a supplier raises stock and its void takes it back", async ({
  page,
}) => {
  await openStock(page, seed("owner"));
  await expect(qtyOf(page, SHAMPOO)).toHaveText("10 шт");
  await expect(page.getByTestId("crm-stock-low-count")).toHaveText("0");

  const dialog = await newDoc(page, "receipt");
  await dialog.getByTestId("crm-stock-doc-submit").click();
  await expect(dialog.getByTestId("crm-stock-doc-empty")).toHaveAttribute(
    "role",
    "alert",
  );
  await dialog.getByTestId("crm-stock-doc-supplier-search").fill("косм");
  await dialog
    .getByTestId("crm-stock-doc-supplier-search-option")
    .filter({ hasText: "Косметика Опт" })
    .click();
  await expect(dialog.getByTestId("crm-stock-doc-supplier")).toContainText(
    "Косметика Опт",
  );
  await addLine(dialog, "шамп");
  // The cost starts at the average cost of the stock on hand.
  await expect(dialog.getByTestId("crm-stock-line-0-cost")).toHaveValue(
    "250,00",
  );
  await dialog.getByTestId("crm-stock-line-0-qty").fill("5");
  await dialog.getByTestId("crm-stock-line-0-cost").fill("abc");
  await dialog.getByTestId("crm-stock-doc-submit").click();
  await expect(dialog.getByTestId("crm-stock-line-0-cost")).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  await dialog.getByTestId("crm-stock-line-0-cost").fill("300");
  await expect(dialog.getByTestId("crm-stock-doc-total")).toContainText("1");
  await dialog.getByTestId("crm-stock-doc-reference").fill("ТН-17");
  await dialog.getByTestId("crm-stock-doc-submit").click();
  await expect(dialog).toBeHidden();
  await expect(qtyOf(page, SHAMPOO)).toHaveText("15 шт");

  await page.getByTestId("crm-stock-tab-docs").click();
  const row = page.getByTestId("crm-stock-doc-1");
  await expect(row).toContainText("Поступление");
  await expect(row).toContainText("Косметика Опт");
  await row.getByRole("button").click();
  const details = page.getByTestId("crm-stock-doc-details");
  await expect(details).toContainText("Поступление №1");
  await expect(details).toContainText("ТН-17");
  await details.getByTestId("crm-stock-void").click();
  await details.getByTestId("crm-stock-void-confirm").click();
  await expect(
    details.getByTestId("crm-stock-void-reason-error"),
  ).toBeVisible();
  await details.getByTestId("crm-stock-void-reason").fill("Ошибка в накладной");
  await details.getByTestId("crm-stock-void-confirm").click();
  await expect(details.getByTestId("crm-stock-doc-voided")).toContainText(
    "Ошибка в накладной",
  );
  await page.keyboard.press("Escape");
  await expect(row).toContainText("Отменён");

  await page.getByTestId("crm-stock-tab-levels").click();
  await expect(qtyOf(page, SHAMPOO)).toHaveText("10 шт");
});

test("write-offs need stock; inventory and transfer move it", async ({
  page,
}) => {
  await openStock(page, seed("member"));

  let dialog = await newDoc(page, "write_off");
  await addLine(dialog, "краск");
  await dialog.getByTestId("crm-stock-line-0-qty").fill("20");
  await dialog.getByTestId("crm-stock-doc-submit").click();
  await expect(dialog.getByTestId("crm-stock-doc-error")).toContainText(
    "Недостаточно товара «Краска» на складе.",
  );
  await dialog.getByTestId("crm-stock-line-0-qty").fill("2");
  await dialog.getByTestId("crm-stock-doc-submit").click();
  await expect(dialog).toBeHidden();
  await expect(qtyOf(page, DYE)).toHaveText("1 шт");

  dialog = await newDoc(page, "inventory");
  await addLine(dialog, "шамп");
  // An inventory line starts at the book quantity.
  await expect(dialog.getByTestId("crm-stock-line-0-qty")).toHaveValue("10");
  await dialog.getByTestId("crm-stock-line-0-qty").fill("4");
  await expect(dialog.getByTestId("crm-stock-line-difference")).toHaveText(
    "−6 шт",
  );
  await dialog.getByTestId("crm-stock-doc-submit").click();
  await expect(dialog).toBeHidden();
  await expect(qtyOf(page, SHAMPOO)).toHaveText("4 шт");
  // At or below the minimum of 5.
  await expect(page.getByTestId("crm-stock-low-count")).toHaveText("1");

  dialog = await newDoc(page, "transfer");
  await expect(dialog.getByTestId("crm-stock-doc-to-warehouse")).toHaveValue(
    SALON,
  );
  await addLine(dialog, "краск");
  await dialog.getByTestId("crm-stock-doc-submit").click();
  await expect(dialog).toBeHidden();
  await page.getByTestId("crm-stock-warehouse").selectOption(MAIN);
  await expect(qtyOf(page, DYE)).toHaveText("0 шт");
  await page.getByTestId("crm-stock-warehouse").selectOption(SALON);
  await expect(qtyOf(page, DYE)).toHaveText("1 шт");

  await page.getByTestId("crm-stock-warehouse").selectOption("");
  await page
    .getByTestId(`crm-stock-item-${SHAMPOO}`)
    .getByRole("button")
    .click();
  const moves = page.getByTestId("crm-stock-moves-table");
  await expect(page.getByTestId("crm-stock-moves-product")).toContainText(
    "Шампунь",
  );
  await expect(moves.locator("tbody tr")).toHaveCount(1);
  await expect(moves).toContainText("Инвентаризация №2");
  await page.getByTestId("crm-stock-moves-product").getByRole("button").click();
  // Write-off, inventory, transfer out of the main warehouse and into the hall.
  await expect(moves.locator("tbody tr")).toHaveCount(4);
});

test("a paid service writes off the consumables of its tech card", async ({
  page,
}) => {
  await openStock(page, seed("owner"));
  await page.getByTestId("crm-stock-tab-techCards").click();
  await page.getByTestId(`crm-tech-card-edit-${HAIRCUT}`).click();
  const dialog = page.getByTestId("crm-tech-card-dialog");
  await waitForAnimations(page);
  await dialog.getByTestId("crm-tech-card-product-search").fill("шамп");
  await dialog.getByTestId("crm-tech-card-product-search-option").click();
  // Pieces are whole: half a bottle is not a valid consumable.
  await dialog.getByTestId("crm-tech-card-item-0").fill("0,5");
  await dialog.getByTestId("crm-tech-card-submit").click();
  await expect(dialog.getByTestId("crm-tech-card-item-0")).toHaveAttribute(
    "aria-invalid",
    "true",
  );
  await dialog.getByRole("button", { name: /Шампунь/ }).click();
  await dialog.getByTestId("crm-tech-card-product-search").fill("оксид");
  await dialog.getByTestId("crm-tech-card-product-search-option").click();
  await dialog.getByTestId("crm-tech-card-item-0").fill("30");
  await dialog.getByTestId("crm-tech-card-submit").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId(`crm-tech-card-${HAIRCUT}`)).toContainText(
    "Оксид 6% · 30 мл",
  );

  await page.getByTestId("crm-section-sales").click();
  await page.getByTestId("crm-sale-add").click();
  const sale = page.getByTestId("crm-sale-dialog");
  await waitForAnimations(page);
  await sale.getByTestId("crm-sale-product-search").fill("стриж");
  await sale.getByTestId("crm-sale-product-search-option").click();
  await sale.getByTestId("crm-sale-product-search").fill("шамп");
  await sale.getByTestId("crm-sale-product-search-option").click();
  await sale.getByTestId("crm-sale-dialog-submit").click();
  await expect(sale).toBeHidden();

  await page.getByTestId("crm-section-stock").click();
  // One shampoo sold and 30 ml of oxide used for the haircut.
  await expect(qtyOf(page, SHAMPOO)).toHaveText("9 шт");
  await expect(qtyOf(page, OXIDE)).toHaveText("970 мл");
  await page.getByTestId("crm-stock-tab-moves").click();
  const moves = page.getByTestId("crm-stock-moves-table");
  await expect(moves).toContainText("Расход по техкарте №1");
  await expect(moves).toContainText("Продажа №1");
});

test("a guest sees stock read-only", async ({ page }) => {
  await openStock(page, seed("guest"));
  await expect(page.getByTestId("crm-stock")).toContainText(
    "У вашей роли нет права проводить складские документы.",
  );
  await expect(page.getByTestId("crm-stock-new-doc")).toHaveCount(0);
  await expect(qtyOf(page, DYE)).toHaveText("3 шт");
  await page.getByTestId("crm-stock-tab-techCards").click();
  await expect(page.getByTestId(`crm-tech-card-${HAIRCUT}`)).toContainText(
    "Расходники не заданы",
  );
  await expect(page.getByTestId(`crm-tech-card-edit-${HAIRCUT}`)).toHaveCount(
    0,
  );
});
