import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Staff payroll (kinds 42160–42162, `/crm/payroll`): accruals from done
// visits and working days at the current rates, rates editing, payouts and
// their cancellation; owners and admins only.

const OUT = "test-results/screenshots/crm-payroll";
const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const HAIRCUT = "11111111-1111-4111-8111-111111111111";
const BEARD = "22222222-2222-4222-8222-222222222222";
const OLGA = "55555555-5555-4555-8555-555555555555";
const IVAN = "66666666-6666-4666-8666-666666666666";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;
type Space = NonNullable<CrmSeed["space"]>;

function today(): string {
  return new Date().toISOString().slice(0, 10);
}

const service = (
  id: string,
  name: string,
  price: number,
  duration: number,
) => ({
  id,
  name,
  unit: "service_unit" as const,
  price_minor: price,
  vat: "none" as const,
  payment_object: "service" as const,
  duration_min: duration,
});

function seed(role: "owner" | "member"): CrmSeed {
  const day = today();
  const space: Space = {
    channelId: SPACE_CHANNEL,
    role,
    settings: {
      default_vat: "none",
      fiscalization: "optional",
      timezone: "UTC",
    },
    warehouses: [{ id: WAREHOUSE, name: "Салон на Ленина" }],
    products: [
      service(HAIRCUT, "Мужская стрижка", 150_000, 45),
      service(BEARD, "Оформление бороды", 90_000, 30),
    ],
    staff: [
      {
        id: OLGA,
        name: "Ольга",
        position: "Барбер",
        service_ids: [HAIRCUT, BEARD],
        days: [{ date: day, intervals: [{ start: 600, end: 1200 }] }],
      },
      {
        id: IVAN,
        name: "Иван",
        position: "Стажёр",
        service_ids: [HAIRCUT],
        days: [{ date: day, intervals: [] }],
      },
    ],
    appointments: [
      {
        staff_id: OLGA,
        date: day,
        start: "10:00",
        service_ids: [HAIRCUT],
        client_name: "Анна",
        status: "done",
      },
      {
        staff_id: OLGA,
        date: day,
        start: "11:00",
        service_ids: [BEARD],
        client_name: "Пётр",
        status: "done",
      },
      {
        staff_id: OLGA,
        date: day,
        start: "12:00",
        service_ids: [HAIRCUT],
        client_name: "Олег",
        status: "booked",
      },
    ],
    payRates: [{ staff_id: OLGA, percent_bp: 4000, shift_minor: 50_000 }],
  };
  return { space };
}

async function openPayroll(page: Page, value: CrmSeed) {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((data) => window.__BUZZ_E2E_SEED_CRM__?.(data), value);
  await page.getByTestId("open-crm-view").click();
  await page.getByTestId("crm-section-payroll").click();
  await expect(page).toHaveURL(/#\/crm\/payroll$/);
}

const balance = (page: Page, staff: string) =>
  page.getByTestId(`crm-payroll-balance-${staff}`);

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

test("accruals combine percent of done visits and working days", async ({
  page,
}) => {
  await openPayroll(page, seed("owner"));
  const olga = page.getByTestId(`crm-payroll-row-${OLGA}`);
  await expect(olga).toContainText("40 % услуг");
  await expect(olga).toContainText(/500,00\s₽ за смену/);
  // 40 % × (1 500 + 900) ₽ + 1 working day × 500 ₽.
  await expect(balance(page, OLGA)).toHaveText(/1\s460,00\s₽/);
  const ivan = page.getByTestId(`crm-payroll-row-${IVAN}`);
  await expect(ivan).toContainText("ставки не заданы");
  await expect(balance(page, IVAN)).toHaveText(/0,00\s₽/);
});

test("rates are edited in a dialog with validation", async ({ page }) => {
  await openPayroll(page, seed("owner"));
  await page.getByTestId(`crm-payroll-rates-${IVAN}`).click();
  const dialog = page.getByTestId("crm-pay-rates-dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByTestId("crm-pay-rates-percent").fill("120");
  await dialog.getByTestId("crm-pay-rates-submit").click();
  await expect(dialog.getByTestId("crm-pay-rates-percent-error")).toBeVisible();
  await expect(dialog.getByTestId("crm-pay-rates-percent")).toBeFocused();
  await dialog.getByTestId("crm-pay-rates-percent").fill("12,5");
  await dialog.getByTestId("crm-pay-rates-per-visit").fill("100");
  await dialog.getByTestId("crm-pay-rates-submit").click();
  await expect(dialog).toBeHidden();
  const ivan = page.getByTestId(`crm-payroll-row-${IVAN}`);
  await expect(ivan).toContainText("12,5 % услуг");
  await expect(ivan).toContainText(/100,00\s₽ за визит/);
});

test("a payout lowers the balance and its cancellation restores it", async ({
  page,
}) => {
  await openPayroll(page, seed("owner"));
  await page.getByTestId(`crm-payroll-pay-${OLGA}`).click();
  const dialog = page.getByTestId("crm-payout-dialog");
  await expect(dialog.getByTestId("crm-payout-amount")).toHaveValue("1460,00");
  await dialog.getByTestId("crm-payout-amount").fill("0");
  await dialog.getByTestId("crm-payout-submit").click();
  await expect(dialog.getByTestId("crm-payout-amount-error")).toBeVisible();
  await dialog.getByTestId("crm-payout-amount").fill("1000");
  await dialog.getByTestId("crm-payout-note").fill("аванс");
  await dialog.getByTestId("crm-payout-submit").click();
  await expect(dialog).toBeHidden();
  await expect(balance(page, OLGA)).toHaveText(/460,00\s₽/);
  const list = page.getByRole("list", { name: "Выплаты" });
  await expect(list).toContainText("аванс");

  await list.getByRole("button", { name: /Отменить выплату/ }).click();
  await page.getByTestId("crm-payout-void-confirm").click();
  await expect(list).toContainText("отменена");
  await expect(balance(page, OLGA)).toHaveText(/1\s460,00\s₽/);
});

test("members without admin rights do not see payroll", async ({ page }) => {
  await openPayroll(page, seed("member"));
  await expect(page.getByTestId("crm-payroll-no-access")).toBeVisible();
  await expect(page.getByTestId("crm-payroll-table")).toHaveCount(0);
});

test("screenshots: payroll", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 860 });
  await openPayroll(page, seed("owner"));
  await page.getByTestId(`crm-payroll-pay-${OLGA}`).click();
  await page.getByTestId("crm-payout-amount").fill("1000");
  await page.getByTestId("crm-payout-note").fill("аванс");
  await page.getByTestId("crm-payout-submit").click();
  await expect(page.getByTestId("crm-payout-dialog")).toBeHidden();
  await expect(balance(page, OLGA)).toHaveText(/460,00\s₽/);
  await waitForAnimations(page);
  await page.getByTestId("crm-payroll").screenshot({
    path: `${OUT}/01-payroll.png`,
  });
  await page.getByTestId(`crm-payroll-rates-${OLGA}`).click();
  await expect(page.getByTestId("crm-pay-rates-dialog")).toBeVisible();
  await waitForAnimations(page);
  await page.getByTestId("crm-pay-rates-dialog").screenshot({
    path: `${OUT}/02-rates.png`,
  });
});
