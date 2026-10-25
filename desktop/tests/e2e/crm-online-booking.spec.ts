import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Online booking (kind 42135, `GET /crm/online-booking`): the settings
// screen with its public link, and the "online" mark on journal visits
// booked from the public page (kind 42134 → appointment source "online").

const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const HAIRCUT = "11111111-1111-4111-8111-111111111111";
const BEARD = "22222222-2222-4222-8222-222222222222";
const OLGA = "55555555-5555-4555-8555-555555555555";
const IVAN = "66666666-6666-4666-8666-666666666666";
const ONLINE_VISIT = "99999999-9999-4999-8999-999999999999";
const STAFF_VISIT = "88888888-8888-4888-8888-888888888888";
const PUBLIC_URL = "http://localhost:3000/book";

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

const service = (id: string, name: string, duration: number) => ({
  id,
  name,
  unit: "service_unit" as const,
  price_minor: 150_000,
  vat: "none" as const,
  payment_object: "service" as const,
  duration_min: duration,
});

function seed(role: "owner" | "member", extra: Partial<Space> = {}): CrmSeed {
  const day = today();
  return {
    space: {
      channelId: SPACE_CHANNEL,
      role,
      settings: { default_vat: "none" },
      warehouses: [{ id: WAREHOUSE, name: "Салон на Ленина" }],
      products: [
        service(HAIRCUT, "Мужская стрижка", 45),
        service(BEARD, "Оформление бороды", 30),
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
          days: [{ date: day, intervals: [{ start: 600, end: 1200 }] }],
        },
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

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

test("an owner enables online booking and gets the public link", async ({
  page,
}) => {
  await openSection(page, seed("owner"), "online-booking");
  const form = page.getByTestId("crm-online-booking-form");
  const link = page.getByTestId("crm-online-booking-link");
  await expect(form).toBeVisible();
  await expect(link.getByTestId("crm-online-booking-url")).toHaveText(
    PUBLIC_URL,
  );
  await expect(
    link.getByTestId("crm-online-booking-disabled-hint"),
  ).toContainText("ещё не сохранены");

  // A blank title is caught before anything is sent.
  await form.getByTestId("crm-online-booking-save").click();
  await expect(
    form.getByTestId("crm-online-booking-title-error"),
  ).toBeVisible();
  await expect(form.getByTestId("crm-online-booking-title")).toBeFocused();

  const enabled = form.getByTestId("crm-online-booking-enabled");
  await expect(enabled).toHaveAttribute("aria-checked", "false");
  await enabled.click();
  await expect(enabled).toHaveAttribute("aria-checked", "true");
  await form.getByTestId("crm-online-booking-title").fill("Барбершоп «Борода»");
  await expect(form.getByTestId("crm-online-booking-title-error")).toHaveCount(
    0,
  );
  await form.getByTestId("crm-online-booking-address").fill("ул. Ленина, 1");
  await form.getByTestId("crm-online-booking-minNotice").selectOption("120");
  await form.getByTestId("crm-online-booking-step").selectOption("30");
  await form.getByTestId(`crm-online-booking-staff-${OLGA}`).click();
  await expect(
    form.getByTestId(`crm-online-booking-services-${HAIRCUT}`),
  ).toBeVisible();

  await form.getByTestId("crm-online-booking-save").click();
  await expect(
    page.getByText("Настройки онлайн-записи сохранены"),
  ).toBeVisible();
  await expect(form.getByTestId("crm-online-booking-save")).toBeDisabled();
  await expect(
    link.getByTestId("crm-online-booking-disabled-hint"),
  ).toHaveCount(0);

  // The saved row comes back from the relay mock after leaving the section.
  await page.getByTestId("crm-section-journal").click();
  await page.getByTestId("crm-section-online-booking").click();
  await expect(form.getByTestId("crm-online-booking-title")).toHaveValue(
    "Барбершоп «Борода»",
  );
  await expect(form.getByTestId("crm-online-booking-step")).toHaveValue("30");
  await expect(
    form.getByTestId(`crm-online-booking-staff-${OLGA}`),
  ).toHaveAttribute("aria-checked", "true");
  await expect(
    form.getByTestId(`crm-online-booking-staff-${IVAN}`),
  ).toHaveAttribute("aria-checked", "false");

  // Turning it off again is a CAS update and brings the disabled hint back.
  await form.getByTestId("crm-online-booking-enabled").click();
  await form.getByTestId("crm-online-booking-save").click();
  await expect(
    link.getByTestId("crm-online-booking-disabled-hint"),
  ).toContainText("выключена");

  await link.getByTestId("crm-online-booking-open").click();
  await expect
    .poll(() =>
      page.evaluate(() =>
        (
          window as unknown as {
            __TAURI_INTERNALS__: { invoke: (cmd: string) => Promise<unknown> };
          }
        ).__TAURI_INTERNALS__.invoke("get_e2e_opened_external_urls"),
      ),
    )
    .toEqual([PUBLIC_URL]);
});

test("members without space rights see online booking read-only", async ({
  page,
}) => {
  await openSection(
    page,
    seed("member", {
      onlineBooking: {
        settings: {
          enabled: true,
          title: "Борода",
          horizon_days: 14,
          min_notice_min: 60,
          step_min: 15,
          staff_ids: [],
          service_ids: [],
        },
      },
    }),
    "online-booking",
  );
  const form = page.getByTestId("crm-online-booking-form");
  await expect(form.getByTestId("crm-online-booking-title")).toHaveValue(
    "Борода",
  );
  await expect(form.getByTestId("crm-online-booking-title")).toBeDisabled();
  await expect(form.getByTestId("crm-online-booking-enabled")).toBeDisabled();
  await expect(form.getByTestId("crm-online-booking-save")).toHaveCount(0);
  await expect(page.getByTestId("crm-online-booking-url")).toHaveText(
    PUBLIC_URL,
  );
});

test("visits booked online carry an accessible online badge", async ({
  page,
}) => {
  const day = today();
  await openSection(
    page,
    seed("owner", {
      appointments: [
        {
          id: ONLINE_VISIT,
          staff_id: OLGA,
          date: day,
          start: "12:00",
          service_ids: [HAIRCUT],
          client_name: "Пётр",
          client_phone: "+7 900 000-00-00",
          source: "online",
        },
        {
          id: STAFF_VISIT,
          staff_id: IVAN,
          date: day,
          start: "12:00",
          service_ids: [HAIRCUT],
          client_name: "Анна",
        },
      ],
    }),
    "journal",
  );
  const online = page.getByTestId(`crm-journal-appointment-${ONLINE_VISIT}`);
  await expect(
    online.getByTestId(`crm-journal-appointment-${ONLINE_VISIT}-online`),
  ).toBeVisible();
  await expect(online).toHaveAccessibleName(/Клиент записался онлайн/);
  const staffVisit = page.getByTestId(`crm-journal-appointment-${STAFF_VISIT}`);
  await expect(staffVisit).toBeVisible();
  await expect(
    staffVisit.getByTestId(`crm-journal-appointment-${STAFF_VISIT}-online`),
  ).toHaveCount(0);
  await expect(staffVisit).not.toHaveAccessibleName(/онлайн/);

  await online.click();
  const dialog = page.getByTestId("crm-appointment-dialog");
  await waitForAnimations(page);
  await expect(dialog.getByTestId("crm-appointment-online")).toBeVisible();
  await expect(dialog.getByTestId("crm-appointment-online")).toContainText(
    "Клиент записался онлайн",
  );
});
