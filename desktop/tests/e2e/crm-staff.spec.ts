import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Services, staff and working schedules (kinds 42102 with durations, 42131,
// 42132): the first YCLIENTS-style phase of the CRM.

const OUT = "test-results/screenshots/crm-staff";
const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const HAIRCUT = "11111111-1111-4111-8111-111111111111";
const BEARD = "22222222-2222-4222-8222-222222222222";
const COLOR = "33333333-3333-4333-8333-333333333333";
const SHAMPOO = "44444444-4444-4444-8444-444444444444";
const OLGA = "55555555-5555-4555-8555-555555555555";
const IVAN = "66666666-6666-4666-8666-666666666666";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;
type Space = NonNullable<CrmSeed["space"]>;

function iso(date: Date): string {
  const m = String(date.getMonth() + 1).padStart(2, "0");
  const d = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${m}-${d}`;
}

/** This week's dates, Monday first, in the browser's local time. */
function thisWeek(): string[] {
  const now = new Date();
  const monday = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  monday.setDate(monday.getDate() - ((monday.getDay() + 6) % 7));
  return Array.from({ length: 7 }, (_, i) => {
    const day = new Date(monday);
    day.setDate(monday.getDate() + i);
    return iso(day);
  });
}

const PRODUCTS: Space["products"] = [
  {
    id: HAIRCUT,
    name: "Мужская стрижка",
    unit: "service_unit",
    price_minor: 150_000,
    vat: "none",
    payment_object: "service",
    category: "Стрижки",
    duration_min: 45,
  },
  {
    id: BEARD,
    name: "Оформление бороды",
    unit: "service_unit",
    price_minor: 90_000,
    vat: "none",
    payment_object: "service",
    duration_min: 30,
  },
  {
    id: COLOR,
    name: "Камуфляж седины",
    unit: "service_unit",
    price_minor: 120_000,
    vat: "none",
    payment_object: "service",
    duration_min: 90,
  },
  { id: SHAMPOO, name: "Шампунь для бороды", price_minor: 89_000 },
];

function seed(role: "owner" | "guest", extra: Partial<Space> = {}): CrmSeed {
  return {
    space: {
      channelId: SPACE_CHANNEL,
      role,
      settings: { default_vat: "none" },
      warehouses: [{ id: WAREHOUSE, name: "Салон на Ленина" }],
      products: PRODUCTS,
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

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

test("services list durations, apart from goods, and validate them", async ({
  page,
}) => {
  await openSection(page, seed("owner"), "services");
  const table = page.getByTestId("crm-services-table");
  await expect(table.getByRole("row")).toHaveCount(4);
  await expect(page.getByTestId(`crm-product-${HAIRCUT}`)).toContainText(
    "45 мин",
  );
  await expect(page.getByTestId(`crm-product-${COLOR}`)).toContainText(
    "1 ч 30 мин",
  );
  await expect(table).not.toContainText("Шампунь");

  await page.getByTestId("crm-service-add").click();
  const dialog = page.getByTestId("crm-product-dialog");
  await expect(dialog).toContainText("Новая услуга");
  await expect(dialog.getByTestId("crm-product-sku")).toHaveCount(0);
  await expect(dialog.getByTestId("crm-product-duration")).toHaveValue("60");
  await dialog.getByTestId("crm-product-name").fill("Детская стрижка");
  await dialog.getByTestId("crm-product-duration").fill("32");
  await dialog.getByTestId("crm-product-price").fill("1000");
  await dialog.getByTestId("crm-product-dialog-submit").click();
  await expect(dialog).toContainText("Длительность: от 5 до 720 минут");
  await expect(dialog.getByTestId("crm-product-duration")).toBeFocused();
  await dialog.getByTestId("crm-product-duration").fill("40");
  await dialog.getByTestId("crm-product-dialog-submit").click();
  await expect(dialog).toBeHidden();
  await expect(
    table.getByRole("row", { name: /Детская стрижка/ }),
  ).toContainText("40 мин");

  // Goods stay in their own section.
  await page.getByTestId("crm-section-products").click();
  await expect(page.getByTestId("crm-products-table")).toContainText("Шампунь");
  await expect(page.getByTestId("crm-products-table")).not.toContainText(
    "стрижка",
  );
});

test("staff are added with their services, edited and archived", async ({
  page,
}) => {
  await openSection(page, seed("owner"), "staff");
  await expect(page.getByTestId("crm-staff-empty")).toBeVisible();

  await page.getByTestId("crm-staff-add").click();
  const dialog = page.getByTestId("crm-staff-dialog");
  await waitForAnimations(page);
  await dialog.getByTestId("crm-staff-dialog-submit").click();
  await expect(dialog.getByTestId("crm-staff-name-error")).toBeVisible();
  await dialog.getByTestId("crm-staff-name").fill("Ольга Смирнова");
  await dialog.getByTestId("crm-staff-position").fill("Барбер");
  await dialog.getByLabel("Мужская стрижка").check();
  await dialog.getByLabel("Оформление бороды").check();
  await expect(dialog.getByTestId("crm-staff-services")).not.toContainText(
    "Шампунь",
  );
  await dialog.getByTestId("crm-staff-dialog-submit").click();
  await expect(dialog).toBeHidden();

  const olga = page.getByRole("row", { name: /Ольга Смирнова/ });
  await expect(olga).toContainText("Барбер");
  await expect(olga).toContainText("Мужская стрижка, Оформление бороды");

  // Editing returns focus to the row button; archiving hides the row.
  const edit = page.getByRole("button", { name: "Изменить: Ольга Смирнова" });
  await edit.click();
  await waitForAnimations(page);
  await dialog.getByLabel("Оформление бороды").uncheck();
  await dialog.getByTestId("crm-staff-archived").check();
  await dialog.getByTestId("crm-staff-dialog-submit").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("crm-staff-empty")).toBeVisible();
  await page.getByTestId("crm-staff-show-archived").click();
  await expect(olga).toContainText("В архиве");
  await expect(olga).not.toContainText("Оформление бороды");
});

test("a template fills the week and single days are edited", async ({
  page,
}) => {
  const week = thisWeek();
  await openSection(
    page,
    seed("owner", {
      staff: [
        {
          id: OLGA,
          name: "Ольга Смирнова",
          position: "Барбер",
          service_ids: [HAIRCUT, BEARD],
        },
        { id: IVAN, name: "Иван Петров", service_ids: [HAIRCUT] },
      ],
    }),
    "schedule",
  );
  const cell = (staff: string, day: number) =>
    page.getByTestId(`crm-schedule-cell-${staff}-${week[day]}`);
  await expect(cell(OLGA, 0)).toContainText("Не задан");

  await page.getByTestId("crm-schedule-fill").click();
  const template = page.getByTestId("crm-schedule-template-dialog");
  await waitForAnimations(page);
  await template.getByTestId("crm-schedule-template-staff").selectOption(OLGA);
  await template.getByTestId("crm-schedule-template-weeks").fill("1");
  await template.getByTestId("crm-schedule-template-break-start").fill("14:00");
  await expect(
    template.getByTestId("crm-schedule-template-summary"),
  ).toHaveCount(0);
  await template.getByTestId("crm-schedule-template-submit").click();
  await expect(template).toContainText("Укажите и начало, и конец перерыва.");
  await template.getByTestId("crm-schedule-template-break-end").fill("15:00");
  await expect(
    template.getByTestId("crm-schedule-template-summary"),
  ).toHaveText("Будет задано дней: 7, из них рабочих: 5.");
  await template.getByTestId("crm-schedule-template-submit").click();
  await expect(template).toBeHidden();

  await expect(cell(OLGA, 0)).toContainText("10:00–14:00");
  await expect(cell(OLGA, 0)).toContainText("15:00–20:00");
  await expect(cell(OLGA, 5)).toContainText("Выходной");
  await expect(cell(IVAN, 0)).toContainText("Не задан");
  await expect(page.getByRole("row", { name: /Ольга Смирнова/ })).toContainText(
    "45 ч за неделю",
  );

  // Ivan works a short Saturday.
  await cell(IVAN, 5).click();
  const day = page.getByTestId("crm-schedule-day-dialog");
  await waitForAnimations(page);
  await day.getByTestId("crm-schedule-interval-0-start").fill("12:00");
  await day.getByTestId("crm-schedule-interval-0-end").fill("11:00");
  await day.getByTestId("crm-schedule-day-submit").click();
  await expect(day.getByTestId("crm-schedule-interval-error")).toHaveText(
    "Конец должен быть позже начала.",
  );
  await day.getByTestId("crm-schedule-interval-0-end").fill("18:00");
  await day.getByTestId("crm-schedule-day-submit").click();
  await expect(day).toBeHidden();
  await expect(cell(IVAN, 5)).toBeFocused();
  await expect(cell(IVAN, 5)).toContainText("12:00–18:00");

  // Olga takes Wednesday off.
  await cell(OLGA, 2).click();
  await waitForAnimations(page);
  await day.getByTestId("crm-schedule-toggle-day-off").click();
  await day.getByTestId("crm-schedule-day-submit").click();
  await expect(cell(OLGA, 2)).toContainText("Выходной");

  // The next week is not scheduled yet.
  await page.getByTestId("crm-schedule-next").click();
  await expect(page.getByTestId("crm-schedule-table")).not.toContainText(
    "10:00",
  );
  await page.getByRole("button", { name: "Сегодня" }).click();
  await expect(cell(OLGA, 0)).toContainText("10:00–14:00");
});

test("guests see services, staff and the schedule read-only", async ({
  page,
}) => {
  const week = thisWeek();
  await openSection(
    page,
    seed("guest", {
      staff: [
        {
          id: OLGA,
          name: "Ольга Смирнова",
          days: [{ date: week[0], intervals: [{ start: 600, end: 1200 }] }],
        },
      ],
    }),
    "schedule",
  );
  await expect(page.getByTestId("crm-schedule-fill")).toHaveCount(0);
  const monday = page.getByTestId(`crm-schedule-cell-${OLGA}-${week[0]}`);
  await expect(monday).toContainText("10:00–20:00");
  await expect(monday).not.toHaveRole("button");
  await page.getByTestId("crm-section-staff").click();
  await expect(page.getByTestId("crm-staff-add")).toHaveCount(0);
  await expect(
    page.getByRole("button", { name: /Изменить: Ольга/ }),
  ).toHaveCount(0);
  await page.getByTestId("crm-section-services").click();
  await expect(page.getByTestId("crm-service-add")).toHaveCount(0);
});

test("screenshots: services, staff and schedule", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  const week = thisWeek();
  const nineToSix = (from: number, to: number) =>
    week.slice(from, to).map((date) => ({
      date,
      intervals: [
        { start: 600, end: 840 },
        { start: 900, end: 1200 },
      ],
    }));
  await openSection(
    page,
    seed("owner", {
      staff: [
        {
          id: OLGA,
          name: "Ольга Смирнова",
          position: "Старший барбер",
          phone: "+7 912 000-11-22",
          service_ids: [HAIRCUT, BEARD, COLOR],
          days: [
            ...nineToSix(0, 5),
            { date: week[5], intervals: [] },
            { date: week[6], intervals: [] },
          ],
        },
        {
          id: IVAN,
          name: "Иван Петров",
          position: "Барбер",
          service_ids: [HAIRCUT, BEARD],
          days: [
            ...nineToSix(2, 7),
            { date: week[0], intervals: [] },
            { date: week[1], intervals: [] },
          ],
        },
      ],
    }),
    "services",
  );
  await expect(page.getByTestId("crm-services-table")).toBeVisible();
  await page
    .getByTestId("crm-view")
    .screenshot({ path: `${OUT}/01-services.png` });

  await page.getByTestId("crm-section-staff").click();
  await expect(page.getByTestId("crm-staff-table")).toBeVisible();
  await page
    .getByTestId("crm-view")
    .screenshot({ path: `${OUT}/02-staff.png` });

  await page.getByRole("button", { name: "Изменить: Ольга Смирнова" }).click();
  await waitForAnimations(page);
  await page
    .getByTestId("crm-staff-dialog")
    .screenshot({ path: `${OUT}/03-staff-dialog.png` });
  await page.keyboard.press("Escape");

  await page.getByTestId("crm-section-schedule").click();
  await expect(page.getByTestId("crm-schedule-table")).toBeVisible();
  await page
    .getByTestId("crm-view")
    .screenshot({ path: `${OUT}/04-schedule.png` });

  await page.getByTestId("crm-schedule-fill").click();
  await waitForAnimations(page);
  await page
    .getByTestId("crm-schedule-template-dialog")
    .screenshot({ path: `${OUT}/05-template.png` });
  await page.keyboard.press("Escape");

  await page.getByTestId("crm-section-schedule").hover();
  await page.screenshot({
    path: `${OUT}/06-sidebar.png`,
    clip: { x: 0, y: 0, width: 300, height: 700 },
  });
});
