import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Customers (kind 42103), sales (42104/42105) and analytics screens.

const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const MAIN_WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const KEFIR = "7d1b6c8e-1a2b-4c3d-8e9f-0a1b2c3d4e5f";
const CHEESE = "8e2c7d9f-2b3c-4d4e-9fa0-1b2c3d4e5f60";
const MARIA = "1a2b3c4d-5e6f-4a7b-8c9d-0e1f2a3b4c5d";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;
type Space = NonNullable<CrmSeed["space"]>;

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

function seed(
  role: "owner" | "member" | "guest",
  extra: Partial<Space> = {},
): CrmSeed {
  return {
    space: {
      channelId: SPACE_CHANNEL,
      role,
      warehouses: [{ id: MAIN_WAREHOUSE, name: "Основной склад" }],
      products: [
        { id: KEFIR, name: "Кефир 1%", price_minor: 8990 },
        { id: CHEESE, name: "Сыр", unit: "kilogram", price_minor: 59990 },
      ],
      parties: [
        {
          id: MARIA,
          name: "Мария Иванова",
          phone: "+7 912 000-11-22",
          inn: "500100732259",
        },
        { name: "Молокозавод", type: "supplier", inn: "7707083893" },
      ],
      ...extra,
    },
  };
}

async function openSection(page: Page, value: CrmSeed, section: string) {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((data) => window.__BUZZ_E2E_SEED_CRM__?.(data), value);
  await page.getByTestId("open-crm-view").click();
  await page.getByTestId(`crm-section-${section}`).click();
  await expect(page).toHaveURL(new RegExp(`#/crm/${section}$`));
}

test("the customer base lists, filters, searches and adds customers", async ({
  page,
}) => {
  await openSection(page, seed("member"), "parties");
  const table = page.getByTestId("crm-parties-table");
  await expect(table.locator("tbody tr")).toHaveCount(2);

  await page.getByTestId("crm-parties-role-customers").click();
  await expect(table.locator("tbody tr")).toHaveCount(1);
  await expect(table).toContainText("Мария Иванова");

  await page.getByTestId("crm-parties-role-all").click();
  await page.getByTestId("crm-parties-search").fill("000-11");
  await expect(table.locator("tbody tr")).toHaveCount(1);
  await page.getByTestId("crm-parties-search").fill("");

  await page.getByTestId("crm-party-add").click();
  const dialog = page.getByTestId("crm-party-dialog");
  await waitForAnimations(page);
  await dialog.getByTestId("crm-party-name").fill("Иван Петров");
  await dialog.getByTestId("crm-party-inn").fill("500100732258");
  await dialog.getByTestId("crm-party-dialog-submit").click();
  await expect(dialog.getByTestId("crm-party-inn-error")).toBeVisible();
  await dialog.getByTestId("crm-party-inn").fill("500100732259");
  await dialog.getByTestId("crm-party-dialog-submit").click();
  await expect(page.getByTestId("crm-party-dialog-error")).toContainText(
    "ИНН 500100732259 уже указан у другого клиента.",
  );
  await dialog.getByTestId("crm-party-inn").fill("");
  await dialog.getByTestId("crm-party-dialog-submit").click();
  await expect(dialog).toBeHidden();
  await expect(table).toContainText("Иван Петров");
});

test("a seller posts a sale with a discount and sees it in the journal and analytics", async ({
  page,
}) => {
  await openSection(page, seed("member"), "sales");
  await expect(page.getByTestId("crm-sales-empty")).toBeVisible();

  await page.getByTestId("crm-sale-add").click();
  const dialog = page.getByTestId("crm-sale-dialog");
  await waitForAnimations(page);
  await dialog.getByTestId("crm-sale-dialog-submit").click();
  await expect(dialog.getByTestId("crm-sale-empty")).toHaveAttribute(
    "role",
    "alert",
  );

  await dialog.getByTestId("crm-sale-product-search").fill("кеф");
  await dialog
    .getByTestId("crm-sale-product-search-option")
    .filter({ hasText: "Кефир 1%" })
    .click();
  await dialog.getByTestId("crm-sale-product-search").fill("сыр");
  await dialog
    .getByTestId("crm-sale-product-search-option")
    .filter({ hasText: "Сыр" })
    .click();
  await dialog.getByTestId("crm-sale-line-0-qty").fill("2");
  await dialog.getByTestId("crm-sale-line-0-discount").fill("9,80");
  await dialog.getByTestId("crm-sale-line-1-qty").fill("1,25");
  await dialog.getByTestId("crm-sale-customer-search").fill("мария");
  await dialog
    .getByTestId("crm-sale-customer-search-option")
    .filter({ hasText: "Мария Иванова" })
    .click();
  await expect(dialog.getByTestId("crm-sale-customer")).toContainText(
    "Мария Иванова",
  );
  // 2 × 89,90 − 9,80 + 1,25 × 599,90 = 170,00 + 749,88.
  await expect(dialog.getByTestId("crm-sale-total")).toHaveText("919,88 ₽");
  await dialog.getByTestId("crm-sale-dialog-submit").click();
  await expect(dialog).toBeHidden();

  const row = page.getByTestId("crm-sale-1");
  await expect(row).toContainText("Мария Иванова");
  await expect(row).toContainText("919,88 ₽");

  await page.getByTestId("crm-section-analytics").click();
  await expect(page.getByTestId("crm-analytics-revenue")).toHaveText(
    "919,88 ₽",
  );
  await expect(page.getByTestId("crm-analytics-sales")).toHaveText("1");
  await expect(page.getByTestId("crm-analytics-top-products")).toContainText(
    "Сыр",
  );
  await expect(page.getByTestId("crm-analytics-top-customers")).toContainText(
    "Мария Иванова",
  );

  await page.getByTestId("crm-section-parties").click();
  await expect(page.getByTestId(`crm-party-${MARIA}`)).toContainText(
    "919,88 ₽",
  );
});

test("an owner voids a sale and it leaves the analytics", async ({ page }) => {
  await openSection(
    page,
    seed("owner", {
      sales: [
        {
          customerId: MARIA,
          lines: [{ productId: KEFIR, qtyMilli: 3000, priceMinor: 8990 }],
        },
      ],
    }),
    "sales",
  );
  await page
    .getByTestId("crm-sale-1")
    .getByRole("button", { name: /Подробнее/ })
    .click();
  const details = page.getByTestId("crm-sale-details");
  await waitForAnimations(page);
  await expect(details.getByTestId("crm-sale-details-total")).toHaveText(
    "269,70 ₽",
  );
  await details.getByTestId("crm-sale-void").click();
  await details.getByTestId("crm-sale-void-confirm").click();
  await expect(details.getByTestId("crm-sale-void-reason-error")).toBeVisible();
  await details.getByTestId("crm-sale-void-reason").fill("ошибка кассира");
  await details.getByTestId("crm-sale-void-confirm").click();
  await expect(details.getByTestId("crm-sale-voided")).toContainText(
    "ошибка кассира",
  );
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("crm-sale-1")).toContainText("Отменена");

  await page.getByTestId("crm-section-analytics").click();
  await expect(page.getByTestId("crm-analytics-empty")).toBeVisible();
});

test("guests read sales and customers but cannot post or edit", async ({
  page,
}) => {
  await openSection(page, seed("guest"), "sales");
  await expect(page.getByTestId("crm-sale-add")).toHaveCount(0);
  await page.getByTestId("crm-section-parties").click();
  await expect(page.getByTestId("crm-party-add")).toHaveCount(0);
  await page.getByTestId(`crm-party-${MARIA}`).getByRole("button").click();
  await expect(page.getByTestId("crm-party-dialog-submit")).toHaveCount(0);
  await expect(page.getByTestId("crm-party-name")).toBeDisabled();
});
