import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Cash desk: shifts, payment with change, fiscal receipts and the queue
// (kinds 42104, 42120, 42121 over the mock ATOL register).

const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const MAIN_WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const KEFIR = "7d1b6c8e-1a2b-4c3d-8e9f-0a1b2c3d4e5f";
const CHEESE = "8e2c7d9f-2b3c-4d4e-9fa0-1b2c3d4e5f60";
const KEFIR_BARCODE = "4600000000017";

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
        {
          id: KEFIR,
          name: "Кефир 1%",
          price_minor: 8990,
          barcodes: [KEFIR_BARCODE],
        },
        { id: CHEESE, name: "Сыр", unit: "kilogram", price_minor: 59990 },
      ],
      ...extra,
    },
  };
}

async function openCash(page: Page, value: CrmSeed) {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((data) => window.__BUZZ_E2E_SEED_CRM__?.(data), value);
  await page.getByTestId("open-crm-view").click();
  await page.getByTestId("crm-section-cash").click();
  await expect(page).toHaveURL(/#\/crm\/cash$/);
}

async function scan(page: Page, code: string) {
  const input = page.getByTestId("crm-cash-scan");
  await input.fill(code);
  await input.press("Enter");
}

test("a cashier opens a shift, scans goods, takes cash and prints the receipt", async ({
  page,
}) => {
  await openCash(page, seed("member"));
  await expect(page.getByTestId("crm-cash-device-status")).toHaveText(
    "Смена закрыта",
  );
  // Fiscalization is required by default: no receipt, no sale.
  await expect(page.getByTestId("crm-cash-pay")).toBeDisabled();
  await page.getByTestId("crm-cash-open-shift").click();
  await expect(page.getByTestId("crm-cash-device-status")).toHaveText(
    "Смена №1 открыта",
  );

  // A scanner types the barcode and Enter faster than the search debounce.
  await scan(page, KEFIR_BARCODE);
  await scan(page, KEFIR_BARCODE);
  await expect(page.getByTestId(`crm-cash-line-${KEFIR}`)).toContainText(
    "179,80 ₽",
  );
  await page.getByTestId("crm-cash-scan").fill("сыр");
  await page
    .getByTestId("crm-cash-scan-option")
    .filter({ hasText: "Сыр" })
    .click();
  await page.getByTestId("crm-cash-line-1-qty").fill("0,5");
  // 2 × 89,90 + 0,5 × 599,90 = 179,80 + 299,95.
  await expect(page.getByTestId("crm-cash-total")).toHaveText("479,75 ₽");

  await page.getByTestId("crm-cash-tendered").fill("400");
  await page.getByTestId("crm-cash-pay").click();
  await expect(page.getByTestId("crm-cash-tendered-error")).toBeVisible();
  await page.getByTestId("crm-cash-tendered").fill("500");
  await expect(page.getByTestId("crm-cash-change")).toHaveText("20,25 ₽");
  await page.getByTestId("crm-cash-pay").click();

  await expect(page.getByText("Продажа №1 оплачена, чек пробит")).toBeVisible();
  await expect(page.getByTestId("crm-cash-cart-empty")).toBeVisible();
  await expect(page.getByTestId("crm-cash-queue-empty")).toBeVisible();
  await expect(page.getByTestId("crm-cash-last-shift")).toContainText(
    "чеков 1 на 479,75 ₽",
  );

  await page.getByTestId("crm-section-sales").click();
  await page
    .getByTestId("crm-sale-1")
    .getByRole("button", { name: /Подробнее/ })
    .click();
  const details = page.getByTestId("crm-sale-details");
  await waitForAnimations(page);
  await expect(details.getByTestId("crm-sale-fiscal-sell")).toContainText(
    "Чек прихода: ФД №2",
  );
  await expect(details.getByTestId("crm-sale-fiscal")).toContainText(
    "Получено 500,00 ₽, сдача 20,25 ₽",
  );
});

test("a mixed payment splits cash and card exactly", async ({ page }) => {
  await openCash(page, seed("member", { cash: { shiftOpen: true } }));
  await scan(page, KEFIR_BARCODE);
  await page.getByTestId("crm-cash-mode-mixed").check({ force: true });
  await page.getByTestId("crm-cash-pay").click();
  await expect(page.getByTestId("crm-cash-cash-part-error")).toBeVisible();
  await page.getByTestId("crm-cash-cash-part").fill("50");
  await expect(page.getByTestId("crm-cash-card-part")).toHaveText("39,90 ₽");
  await page.getByTestId("crm-cash-pay").click();
  await expect(page.getByText("Продажа №1 оплачена, чек пробит")).toBeVisible();

  await page.getByTestId("crm-section-sales").click();
  await page
    .getByTestId("crm-sale-1")
    .getByRole("button", { name: /Подробнее/ })
    .click();
  await expect(page.getByTestId("crm-sale-split")).toHaveText(
    "Наличные: 50,00 ₽ · Карта: 39,90 ₽",
  );
});

test("a receipt the register refuses stays in the queue until it is retried", async ({
  page,
}) => {
  await openCash(
    page,
    seed("owner", { settings: { fiscalization: "optional" } }),
  );
  // Optional fiscalization: the sale goes through with the shift closed,
  // and the receipt is printed later from the sale card.
  await scan(page, KEFIR_BARCODE);
  await page.getByTestId("crm-cash-pay").click();
  await expect(page.getByText("Продажа №1 оформлена")).toBeVisible();

  await page.getByTestId("crm-section-sales").click();
  await page
    .getByTestId("crm-sale-1")
    .getByRole("button", { name: /Подробнее/ })
    .click();
  const details = page.getByTestId("crm-sale-details");
  await waitForAnimations(page);
  await details.getByTestId("crm-sale-fiscalize").click();
  await expect(page.getByText(/Смена не открыта/).first()).toBeVisible();
  await page.keyboard.press("Escape");

  await page.getByTestId("crm-section-cash").click();
  const job = page.getByTestId("crm-cash-job-1-sell");
  await expect(job).toContainText("Ошибка");
  await page.getByTestId("crm-cash-open-shift").click();
  await job.getByRole("button", { name: "Повторить" }).click();
  await expect(page.getByTestId("crm-cash-queue-empty")).toBeVisible();

  // Voiding the printed sale offers the return receipt.
  await page.getByTestId("crm-section-sales").click();
  await page
    .getByTestId("crm-sale-1")
    .getByRole("button", { name: /Подробнее/ })
    .click();
  await waitForAnimations(page);
  await expect(details.getByTestId("crm-sale-fiscal-sell")).toBeVisible();
  await details.getByTestId("crm-sale-void").click();
  await details.getByTestId("crm-sale-void-reason").fill("возврат товара");
  await details.getByTestId("crm-sale-void-confirm").click();
  await expect(details.getByTestId("crm-sale-fiscalize")).toHaveText(
    "Пробить чек возврата",
  );
  await details.getByTestId("crm-sale-fiscalize").click();
  await expect(details.getByTestId("crm-sale-fiscal-return")).toContainText(
    "Чек возврата: ФД №",
  );
});

test("guests see the register state but cannot sell or operate it", async ({
  page,
}) => {
  await openCash(page, seed("guest"));
  await expect(page.getByTestId("crm-cash-device-status")).toBeVisible();
  await expect(page.getByTestId("crm-cash-pay")).toHaveCount(0);
  await expect(page.getByTestId("crm-cash-open-shift")).toHaveCount(0);
});
