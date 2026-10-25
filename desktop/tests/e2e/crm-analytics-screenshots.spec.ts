import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// PR screenshots for the CRM sidebar sections and the analytics dashboard,
// over a month of generated sales.

const OUT = "test-results/screenshots/crm-analytics";
const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;
type Space = NonNullable<CrmSeed["space"]>;

const PRODUCTS = [
  ["Кефир 1%", 8990],
  ["Молоко 3,2%", 10490],
  ["Хлеб бородинский", 6590],
  ["Сыр российский", 59990],
  ["Кофе зерновой", 89900],
  ["Яблоки", 14990],
  ["Шоколад горький", 12990],
  ["Вода 1,5 л", 4990],
] as const;

const CUSTOMERS = [
  "Анна Смирнова",
  "ООО «Ромашка»",
  "Игорь Петров",
  "Мария Козлова",
  "Кафе «Уют»",
];

/** Deterministic pseudo-random sales over the last 60 days. */
function seed(): CrmSeed {
  let state = 7;
  const next = () => {
    state = (state * 1_103_515_245 + 12_345) % 2_147_483_648;
    return state / 2_147_483_648;
  };
  const products = PRODUCTS.map(([name, price], index) => ({
    id: `00000000-0000-4000-8000-0000000000${String(index + 10)}`,
    name,
    price_minor: price,
  }));
  const parties = CUSTOMERS.map((name, index) => ({
    id: `00000000-0000-4000-9000-0000000000${String(index + 10)}`,
    name,
  }));
  const sales: NonNullable<Space["sales"]> = [];
  const now = Date.now();
  for (let day = 0; day < 60; day += 1) {
    const count = 2 + Math.floor(next() * 6) + (day % 7 < 2 ? 3 : 0);
    for (let i = 0; i < count; i += 1) {
      const hour = 9 + Math.floor(next() * 12);
      const at = new Date(now - day * 86_400_000);
      at.setHours(hour, Math.floor(next() * 60), 0, 0);
      if (at.getTime() > now) at.setTime(now - 60_000);
      const lineCount = 1 + Math.floor(next() * 3);
      const lines = Array.from({ length: lineCount }, () => {
        const product = products[Math.floor(next() * products.length)];
        return {
          productId: product.id,
          qtyMilli: 1000 * (1 + Math.floor(next() * 3)),
          priceMinor: product.price_minor,
        };
      });
      const roll = next();
      sales.push({
        soldAt: at.toISOString(),
        payment: roll < 0.45 ? "cash" : roll < 0.9 ? "card" : "transfer",
        customerId:
          next() < 0.4 ? parties[Math.floor(next() * parties.length)].id : null,
        lines,
      });
    }
  }
  return {
    space: {
      channelId: SPACE_CHANNEL,
      role: "owner",
      warehouses: [{ id: WAREHOUSE, name: "Основной склад" }],
      products,
      parties,
      sales,
    },
  };
}

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 1700 });
  await installMockBridge(page);
});

async function openAnalytics(page: Page) {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((value) => window.__BUZZ_E2E_SEED_CRM__?.(value), seed());
  await page.getByTestId("open-crm-view").click();
  await page.getByTestId("crm-section-analytics").click();
  await expect(page.getByTestId("crm-analytics-revenue")).toBeVisible();
  await waitForAnimations(page);
}

test("analytics dashboard", async ({ page }) => {
  await openAnalytics(page);
  const chart = page.getByTestId("crm-analytics-by-day-chart");
  await chart.hover();
  await expect(
    page.getByTestId("crm-analytics-by-day-chart-tooltip"),
  ).toBeVisible();
  await page
    .getByTestId("crm-view")
    .screenshot({ path: `${OUT}/01-analytics.png` });
});

test("analytics in the dark theme", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await openAnalytics(page);
  await page
    .getByTestId("crm-view")
    .screenshot({ path: `${OUT}/02-analytics-dark.png` });
});

test("sidebar with the CRM sections", async ({ page }) => {
  await openAnalytics(page);
  await page.mouse.move(900, 600);
  await page.screenshot({
    path: `${OUT}/03-sidebar.png`,
    clip: { x: 0, y: 0, width: 300, height: 600 },
  });
});
