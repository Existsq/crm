import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// The appointment journal (kind 42133, `/crm/appointments`, `/crm/slots`):
// a day grid per staff member, booking from free time, overlap and hours
// checks, status changes and cancellation.

const OUT = "test-results/screenshots/crm-journal";
const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const HAIRCUT = "11111111-1111-4111-8111-111111111111";
const BEARD = "22222222-2222-4222-8222-222222222222";
const COLOR = "33333333-3333-4333-8333-333333333333";
const OLGA = "55555555-5555-4555-8555-555555555555";
const IVAN = "66666666-6666-4666-8666-666666666666";
const MARIA = "77777777-7777-4777-8777-777777777777";
const VISIT = "88888888-8888-4888-8888-888888888888";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;
type Space = NonNullable<CrmSeed["space"]>;

function today(): string {
  const now = new Date();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${now.getFullYear()}-${m}-${d}`;
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

function seed(role: "owner" | "guest"): CrmSeed {
  const day = today();
  const space: Space = {
    channelId: SPACE_CHANNEL,
    role,
    settings: { default_vat: "none", fiscalization: "optional" },
    warehouses: [{ id: WAREHOUSE, name: "Салон на Ленина" }],
    products: [
      service(HAIRCUT, "Мужская стрижка", 150_000, 45),
      service(BEARD, "Оформление бороды", 90_000, 30),
      service(COLOR, "Камуфляж седины", 120_000, 90),
    ],
    parties: [{ id: MARIA, name: "Мария Соколова", phone: "+7 912 000-11-22" }],
    staff: [
      {
        id: OLGA,
        name: "Ольга",
        position: "Барбер",
        service_ids: [HAIRCUT, BEARD, COLOR],
        days: [
          {
            date: day,
            intervals: [
              { start: 600, end: 840 },
              { start: 900, end: 1200 },
            ],
          },
        ],
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
        id: VISIT,
        staff_id: OLGA,
        date: day,
        start: "12:00",
        service_ids: [HAIRCUT],
        customer_id: MARIA,
      },
    ],
  };
  return { space };
}

async function openJournal(page: Page, value: CrmSeed) {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((data) => window.__BUZZ_E2E_SEED_CRM__?.(data), value);
  await page.getByTestId("open-crm-view").click();
  await page.getByTestId("crm-section-journal").click();
  await expect(page).toHaveURL(/#\/crm\/journal$/);
  await expect(page.getByTestId("crm-journal-grid")).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

test("the day grid shows staff, hours and booked visits", async ({ page }) => {
  await openJournal(page, seed("owner"));
  const olga = page.getByTestId(`crm-journal-column-${OLGA}`);
  const ivan = page.getByTestId(`crm-journal-column-${IVAN}`);
  await expect(olga).toContainText("Барбер");
  await expect(ivan).toContainText("Выходной");
  const visit = page.getByTestId(`crm-journal-appointment-${VISIT}`);
  await expect(visit).toContainText("12:00–12:45");
  await expect(visit).toContainText("Мария Соколова");
  await expect(visit).toContainText("Мужская стрижка");
  await expect(page.getByTestId("crm-journal-summary")).toHaveText(
    /Записей: 1 · на сумму 1\s?500,00\s?₽/,
  );
  await expect(
    page.getByTestId(`crm-journal-cell-${OLGA}-10:00`),
  ).toHaveAttribute("data-working", "true");
  await expect(
    page.getByTestId(`crm-journal-cell-${OLGA}-14:00`),
  ).not.toHaveAttribute("data-working");

  await page.setViewportSize({ width: 1280, height: 900 });
  await waitForAnimations(page);
  await page
    .getByTestId("crm-journal")
    .screenshot({ path: `${OUT}/01-day-grid.png` });
});

test("booking from a free cell offers free times and checks the form", async ({
  page,
}) => {
  await openJournal(page, seed("owner"));
  await page.getByTestId(`crm-journal-cell-${OLGA}-10:00`).click();
  const dialog = page.getByTestId("crm-appointment-dialog");
  await expect(dialog).toContainText("Новая запись");
  await expect(dialog.getByTestId("crm-appointment-staff")).toHaveValue(OLGA);
  await expect(dialog.getByTestId("crm-appointment-time-hint")).toHaveText(
    "Выберите услуги, чтобы увидеть свободное время.",
  );

  // Nothing filled: the client comes first.
  await dialog.getByTestId("crm-appointment-submit").click();
  await expect(dialog.getByTestId("crm-appointment-client-error")).toHaveText(
    "Выберите клиента из базы или введите имя.",
  );

  await dialog.getByTestId("crm-appointment-client-name").fill("Анна");
  await dialog
    .getByTestId("crm-appointment-client-phone")
    .fill("+7 900 111-22-33");
  await dialog.getByTestId(`crm-appointment-service-${HAIRCUT}`).click();
  await dialog.getByTestId(`crm-appointment-service-${BEARD}`).click();
  await expect(dialog.getByTestId("crm-appointment-total")).toContainText(
    "1 ч 15 мин",
  );
  const time = dialog.getByTestId("crm-appointment-time");
  await expect(time).toHaveValue("600");
  // 75 minutes do not fit before the 12:00 visit after 10:45.
  await expect(time.locator("option", { hasText: "12:45" })).toHaveCount(1);
  const options = await time.locator("option").allTextContents();
  expect(options).toContain("10:45");
  expect(options).not.toContain("11:00");
  expect(options).toContain("12:45");

  await waitForAnimations(page);
  await dialog.screenshot({ path: `${OUT}/02-booking-dialog.png` });
  await dialog.getByTestId("crm-appointment-submit").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId(`crm-journal-column-${OLGA}`)).toContainText(
    "10:00–11:15",
  );
  await expect(page.getByTestId("crm-journal-summary")).toContainText(
    "Записей: 2",
  );
});

test("an overlapping visit is refused with the busy time", async ({ page }) => {
  await openJournal(page, seed("owner"));
  await page.getByTestId(`crm-journal-cell-${OLGA}-11:00`).click();
  const dialog = page.getByTestId("crm-appointment-dialog");
  await dialog.getByTestId("crm-appointment-client-name").fill("Пётр");
  await dialog.getByTestId(`crm-appointment-service-${COLOR}`).click();
  await expect(dialog.getByTestId("crm-appointment-time")).toHaveValue("660");
  await dialog.getByTestId("crm-appointment-submit").click();
  await expect(dialog.getByTestId("crm-appointment-error")).toContainText(
    "У мастера уже есть запись 12:00–12:45",
  );
});

test("booking outside hours is explicit", async ({ page }) => {
  await openJournal(page, seed("owner"));
  await page.getByTestId(`crm-journal-cell-${OLGA}-14:00`).click();
  const dialog = page.getByTestId("crm-appointment-dialog");
  await expect(
    dialog.getByTestId("crm-appointment-outside-hours"),
  ).toBeChecked();
  await expect(dialog.getByTestId("crm-appointment-time")).toHaveValue("14:00");
  await dialog.getByTestId("crm-appointment-customer-search").fill("Мария");
  await dialog
    .getByTestId("crm-appointment-customer-search-results")
    .getByRole("button", { name: /Мария Соколова/ })
    .click();
  await expect(dialog.getByTestId("crm-appointment-customer")).toContainText(
    "Мария Соколова",
  );
  await dialog.getByTestId(`crm-appointment-service-${BEARD}`).click();
  await dialog.getByTestId("crm-appointment-submit").click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId(`crm-journal-column-${OLGA}`)).toContainText(
    "вне графика",
  );
});

test("visits change status and cancel, freeing the time", async ({ page }) => {
  await openJournal(page, seed("owner"));
  const visit = page.getByTestId(`crm-journal-appointment-${VISIT}`);
  await visit.click();
  const dialog = page.getByTestId("crm-appointment-dialog");
  await expect(dialog).toContainText("Запись клиента");
  await expect(dialog.getByTestId("crm-appointment-time")).toHaveValue("720");
  await dialog.getByTestId("crm-appointment-status").selectOption("arrived");
  await dialog.getByTestId("crm-appointment-submit").click();
  await expect(dialog).toBeHidden();
  await expect(visit).toHaveAttribute("data-status", "arrived");
  await expect(visit).toContainText("Пришёл");
  await expect(visit).toBeFocused();

  await visit.click();
  await dialog.getByTestId("crm-appointment-cancel-visit").click();
  await expect(dialog).toBeHidden();
  await expect(visit).toHaveCount(0);
  await expect(page.getByTestId("crm-journal-summary")).toHaveText(
    /Записей: 0 .* · Отменено: 1/,
  );
});

test("days move with the arrows and guests only read", async ({ page }) => {
  await openJournal(page, seed("guest"));
  await expect(page.getByTestId("crm-journal-new")).toHaveCount(0);
  await expect(page.getByTestId(`crm-journal-cell-${OLGA}-10:00`)).toHaveCount(
    0,
  );
  await expect(page.getByTestId("crm-journal")).toContainText(
    "У вашей роли нет права записывать клиентов.",
  );
  await expect(page.getByTestId("crm-journal-today")).toBeDisabled();
  await page.getByTestId("crm-journal-next").click();
  await expect(page.getByTestId("crm-journal-today")).toBeEnabled();
  await expect(
    page.getByTestId(`crm-journal-appointment-${VISIT}`),
  ).toHaveCount(0);
  await expect(page.getByTestId(`crm-journal-column-${OLGA}`)).toContainText(
    "График не задан",
  );
  await page.getByTestId("crm-journal-today").click();
  await expect(
    page.getByTestId(`crm-journal-appointment-${VISIT}`),
  ).toBeVisible();
});

test("a visit is paid at the cash desk and shows in the client's history", async ({
  page,
}) => {
  await openJournal(page, seed("owner"));
  await page.getByTestId(`crm-journal-appointment-${VISIT}`).click();
  const dialog = page.getByTestId("crm-appointment-dialog");
  await dialog.getByTestId("crm-appointment-pay").click();
  await expect(page).toHaveURL(/#\/crm\/cash\?visit=/);
  const banner = page.getByTestId("crm-cash-visit");
  await expect(banner).toContainText("Оплата визита");
  await expect(banner).toContainText("Мария Соколова");
  await expect(page.getByTestId("crm-cash-cart")).toContainText(
    "Мужская стрижка",
  );
  await expect(page.getByTestId("crm-cash-pay")).toContainText("1 500,00");
  await waitForAnimations(page);
  await page
    .getByTestId("crm-cash")
    .screenshot({ path: `${OUT}/03-visit-checkout.png` });
  await page.getByTestId("crm-cash-pay").click();
  await expect(page).toHaveURL(/#\/crm\/cash$/);
  await expect(banner).toHaveCount(0);

  await page.getByTestId("crm-section-journal").click();
  const visit = page.getByTestId(`crm-journal-appointment-${VISIT}`);
  await expect(visit).toHaveAttribute("data-status", "done");
  await expect(visit).toContainText("оплачено");
  await visit.click();
  await expect(dialog.getByTestId("crm-appointment-pay")).toHaveCount(0);
  await expect(dialog.getByTestId("crm-appointment-staff")).toBeDisabled();
  await dialog.getByRole("button", { name: "Отмена" }).click();

  await page.getByTestId("crm-section-parties").click();
  await page.getByTestId(`crm-party-${MARIA}`).getByRole("button").click();
  const visits = page.getByTestId("crm-party-visits");
  await expect(page.getByTestId("crm-party-visit-count")).toHaveText("1");
  await expect(visits).toContainText("Мужская стрижка");
  await expect(visits).toContainText("Визит завершён");
});

test("staff analytics sum visits, load and services", async ({ page }) => {
  const value = seed("owner");
  const space = value.space as Space;
  const day = today();
  space.appointments = [
    ...(space.appointments ?? []),
    {
      staff_id: OLGA,
      date: day,
      start: "10:00",
      service_ids: [HAIRCUT, BEARD],
      client_name: "Пётр",
      status: "done",
    },
    {
      staff_id: OLGA,
      date: day,
      start: "16:00",
      service_ids: [COLOR],
      client_name: "Олег",
      status: "no_show",
    },
  ];
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((data) => window.__BUZZ_E2E_SEED_CRM__?.(data), value);
  await page.getByTestId("open-crm-view").click();
  await page.getByTestId("crm-section-staff-analytics").click();
  await expect(page).toHaveURL(/#\/crm\/staff-analytics$/);

  await expect(page.getByTestId("crm-staff-analytics-visits")).toHaveText("2");
  await expect(page.getByTestId("crm-staff-analytics-revenue")).toHaveText(
    /2\s?400,00\s?₽/,
  );
  // 45 + 30 + 45 booked minutes of 540 worked: 22%.
  await expect(page.getByTestId("crm-staff-analytics-utilisation")).toHaveText(
    "22%",
  );
  await expect(page.getByTestId("crm-staff-analytics-new-clients")).toHaveText(
    "1",
  );
  const olga = page.getByTestId(`crm-staff-analytics-${OLGA}`);
  await expect(olga).toContainText("Ольга");
  await expect(olga).toContainText("22%");
  await expect(page.getByTestId(`crm-staff-analytics-${IVAN}`)).toContainText(
    "нет графика",
  );
  await expect(page.getByTestId("crm-staff-analytics-services")).toContainText(
    "Мужская стрижка",
  );
  await waitForAnimations(page);
  await page
    .getByTestId("crm-staff-analytics")
    .screenshot({ path: `${OUT}/04-staff-analytics.png` });
});
