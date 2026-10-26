import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Loyalty (kinds 42170, 42171, `/crm/loyalty`): the programme in settings,
// paying part of a receipt with bonuses at the cash desk, the cashback it
// earns, and a customer's balance with manual adjustment.

const OUT = "test-results/screenshots/crm-loyalty";
const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const HAIRCUT = "11111111-1111-4111-8111-111111111111";
const HAIRCUT_BARCODE = "2000000000015";
const ANNA = "77777777-7777-4777-8777-777777777777";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;
type Space = NonNullable<CrmSeed["space"]>;

function seed(role: "owner" | "member", extra: Partial<Space> = {}): CrmSeed {
  return {
    space: {
      channelId: SPACE_CHANNEL,
      role,
      settings: { default_vat: "none", fiscalization: "off" },
      warehouses: [{ id: WAREHOUSE, name: "Салон на Ленина" }],
      products: [
        {
          id: HAIRCUT,
          name: "Мужская стрижка",
          unit: "service_unit",
          price_minor: 150_000,
          vat: "none",
          payment_object: "service",
          barcodes: [HAIRCUT_BARCODE],
        },
      ],
      parties: [{ id: ANNA, type: "customer", name: "Анна Смирнова" }],
      loyalty: { enabled: true, cashback_bp: 500, max_redeem_bp: 3000 },
      bonuses: [{ customer_id: ANNA, balance_minor: 50_000 }],
      ...extra,
    },
  };
}

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

async function open(page: Page, value: CrmSeed, section: string) {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((data) => window.__BUZZ_E2E_SEED_CRM__?.(data), value);
  await page.getByTestId("open-crm-view").click();
  await page.getByTestId(`crm-section-${section}`).click();
}

async function ringUpForAnna(page: Page) {
  const scanner = page.getByTestId("crm-cash-scan");
  await scanner.fill(HAIRCUT_BARCODE);
  await scanner.press("Enter");
  await page.getByTestId("crm-cash-customer-search").fill("Анна");
  await page.getByTestId("crm-cash-customer-search-option").first().click();
  await expect(page.getByTestId("crm-cash-customer")).toContainText(
    "Анна Смирнова",
  );
}

test("a customer pays part of the receipt with bonuses and earns cashback", async ({
  page,
}) => {
  await open(page, seed("member"), "cash");
  await ringUpForAnna(page);

  const bonuses = page.getByTestId("crm-cash-bonuses");
  // 30 % of 1 500 ₽ is 450 ₽, below the 500 ₽ balance.
  await expect(bonuses).toContainText("Доступно 450,00 ₽ из 500,00 ₽");
  await expect(page.getByTestId("crm-cash-bonus-earns")).toContainText(
    "Начислится 75,00 ₽ бонусов",
  );

  await page.getByTestId("crm-cash-bonus").fill("460");
  await page.getByTestId("crm-cash-pay").click();
  await expect(page.getByTestId("crm-cash-bonus-error")).toContainText(
    "Не больше 450,00 ₽",
  );

  await page.getByTestId("crm-cash-bonus-max").click();
  await expect(page.getByTestId("crm-cash-bonus")).toHaveValue("450,00");
  await expect(page.getByTestId("crm-cash-pay")).toContainText("1 050,00 ₽");
  await expect(page.getByTestId("crm-cash-bonus-earns")).toContainText(
    "Начислится 52,50 ₽ бонусов",
  );
  await page.getByTestId("crm-cash-pay").click();
  await expect(page.getByText("Продажа №1 оформлена")).toBeVisible();

  // The sale carries the bonus as discount: 1 050 ₽ paid.
  await page.getByTestId("crm-section-sales").click();
  await expect(page.getByTestId("crm-sale-1")).toContainText("1 050,00 ₽");
});

test("the customer card shows the balance and an owner adjusts it", async ({
  page,
}) => {
  await open(page, seed("owner"), "cash");
  await ringUpForAnna(page);
  await page.getByTestId("crm-cash-bonus").fill("100");
  await page.getByTestId("crm-cash-pay").click();
  await expect(page.getByText("Продажа №1 оформлена")).toBeVisible();

  await page.getByTestId("crm-section-parties").click();
  await page
    .getByTestId(`crm-party-${ANNA}`)
    .getByRole("button", { name: /Анна Смирнова/ })
    .click();
  const card = page.getByTestId("crm-party-bonuses");
  // 500 − 100 + 5 % of 1 400 = 470 ₽.
  await expect(page.getByTestId("crm-party-bonus-balance")).toHaveText(
    "Баланс: 470,00 ₽",
  );
  await expect(card).toContainText("Начислено с покупки");
  await expect(card).toContainText("Оплачено бонусами");

  await card.getByTestId("crm-party-bonus-adjust").click();
  const dialog = page.getByTestId("crm-bonus-adjust-dialog");
  await dialog.getByTestId("crm-bonus-adjust-amount").fill("−1000");
  await dialog.getByTestId("crm-bonus-adjust-reason").fill("Ошибка");
  await dialog.getByTestId("crm-bonus-adjust-submit").click();
  await expect(dialog.getByTestId("crm-bonus-adjust-error")).toContainText(
    "470",
  );
  await dialog.getByTestId("crm-bonus-adjust-amount").fill("200");
  await dialog.getByTestId("crm-bonus-adjust-reason").fill("День рождения");
  await dialog.getByTestId("crm-bonus-adjust-submit").click();
  await expect(dialog).toHaveCount(0);
  // The party dialog stays open and refreshes.
  await expect(page.getByTestId("crm-party-dialog")).toBeVisible();
  await expect(page.getByTestId("crm-party-bonus-balance")).toHaveText(
    "Баланс: 670,00 ₽",
  );
  await expect(card).toContainText("Вручную · День рождения");
});

test("an owner changes the programme; members only read it", async ({
  page,
}) => {
  await open(page, seed("owner"), "settings");
  const card = page.getByTestId("crm-loyalty-card");
  await expect(card.getByTestId("crm-loyalty-cashback")).toHaveValue("5");
  await card.getByTestId("crm-loyalty-cashback").fill("150");
  await card.getByTestId("crm-loyalty-save").click();
  await expect(card.getByTestId("crm-loyalty-cashback-error")).toBeVisible();
  await card.getByTestId("crm-loyalty-cashback").fill("7,5");
  await card.getByTestId("crm-loyalty-save").click();
  await expect(page.getByText("Программа лояльности сохранена")).toBeVisible();
  await expect(card.getByTestId("crm-loyalty-save")).toBeDisabled();

  // Off: the cash desk offers no bonuses and promises no cashback.
  await card.getByTestId("crm-loyalty-enabled").click();
  await card.getByTestId("crm-loyalty-save").click();
  await expect(card.getByTestId("crm-loyalty-enabled")).not.toBeChecked();
  await page.getByTestId("crm-section-cash").click();
  await ringUpForAnna(page);
  await expect(page.getByTestId("crm-cash-bonuses")).toHaveCount(0);
});

test("members see the programme read-only", async ({ page }) => {
  await open(page, seed("member"), "settings");
  const card = page.getByTestId("crm-loyalty-card");
  await expect(card.getByTestId("crm-loyalty-cashback")).toBeDisabled();
  await expect(card.getByTestId("crm-loyalty-save")).toHaveCount(0);
  await expect(card).toContainText("владелец или администратор");
});

test("screenshots: loyalty", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await open(page, seed("owner"), "cash");
  await ringUpForAnna(page);
  await page.getByTestId("crm-cash-bonus-max").click();
  await waitForAnimations(page);
  await page.getByTestId("crm-cash").screenshot({ path: `${OUT}/cash.png` });

  await page.getByTestId("crm-section-settings").click();
  await expect(page.getByTestId("crm-loyalty-cashback")).toHaveValue("5");
  await waitForAnimations(page);
  await page
    .getByTestId("crm-loyalty-card")
    .screenshot({ path: `${OUT}/settings.png` });
});
