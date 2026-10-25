import { expect, type Page, test } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// Visit reminders (kinds 42150/42151, `/crm/reminders`): rules with live
// preview, the sandbox banner, the delivery log and a customer's Telegram.

const OUT = "test-results/screenshots/crm-reminders";
const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";
const MARIA = "77777777-7777-4777-8777-777777777777";
const PETR = "99999999-9999-4999-8999-999999999999";
const VISIT = "88888888-8888-4888-8888-888888888888";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;
type Space = NonNullable<CrmSeed["space"]>;
type Reminders = NonNullable<Space["reminders"]>;

const minutesAgo = (minutes: number) =>
  new Date(Date.now() - minutes * 60_000).toISOString();
const minutesAhead = (minutes: number) =>
  new Date(Date.now() + minutes * 60_000).toISOString();

function delivery(
  id: string,
  patch: Partial<NonNullable<Reminders["deliveries"]>[number]>,
): NonNullable<Reminders["deliveries"]>[number] {
  return {
    id,
    appointment_id: VISIT,
    rule_key: "booked",
    customer_id: MARIA,
    client_name: "Мария Соколова",
    chat_id: 123456,
    text: "Мария Соколова, вы записаны в Барбершоп на пн, 2 ноября в 10:30.",
    status: "sent",
    reason: null,
    transport: "sandbox",
    attempts: 1,
    last_error: null,
    due_at: minutesAgo(30),
    visit_at: minutesAhead(24 * 60),
    created_at: minutesAgo(30),
    sent_at: minutesAgo(29),
    ...patch,
  };
}

function seed(role: "owner" | "guest", reminders: Reminders = {}): CrmSeed {
  return {
    space: {
      channelId: SPACE_CHANNEL,
      role,
      warehouses: [{ id: WAREHOUSE, name: "Салон на Ленина" }],
      parties: [
        { id: MARIA, name: "Мария Соколова", phone: "+7 912 000-11-22" },
        { id: PETR, name: "Пётр Иванов", phone: "+7 912 000-33-44" },
      ],
      reminders,
    },
  };
}

const LOG: Reminders["deliveries"] = [
  delivery("d0000001-0000-4000-8000-000000000001", {}),
  delivery("d0000001-0000-4000-8000-000000000002", {
    rule_key: "before:1440",
    status: "pending",
    sent_at: null,
    created_at: minutesAgo(5),
    text: "Напоминаем: завтра в 10:30 ждём вас в Барбершоп.",
  }),
  delivery("d0000001-0000-4000-8000-000000000003", {
    rule_key: "before:120",
    customer_id: PETR,
    client_name: "Пётр Иванов",
    chat_id: null,
    status: "skipped",
    reason: "no_telegram",
    transport: null,
    sent_at: null,
    created_at: minutesAgo(60),
  }),
  delivery("d0000001-0000-4000-8000-000000000004", {
    status: "failed",
    reason: "blocked",
    transport: "telegram",
    attempts: 3,
    last_error: "Forbidden: bot was blocked by the user",
    sent_at: null,
    created_at: minutesAgo(120),
  }),
  delivery("d0000001-0000-4000-8000-000000000005", {
    created_at: minutesAgo(9 * 24 * 60),
    text: "Старое сообщение вне окна журнала",
  }),
];

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

test("defaults preview live, reject unknown placeholders and save", async ({
  page,
}) => {
  await openSection(page, seed("owner", { deliveries: LOG }), "reminders");
  await expect(page.getByTestId("crm-reminders-transport")).toContainText(
    "Песочница: сообщения не уходят в Telegram",
  );
  const form = page.getByTestId("crm-reminders-form");
  await expect(form.getByTestId("crm-reminders-enabled")).not.toBeChecked();
  await expect(page.getByTestId("crm-reminder-rule-0")).toContainText(
    "Подтверждение записи",
  );
  await expect(page.getByTestId("crm-reminder-offset-1")).toHaveValue("1440");
  await expect(page.getByTestId("crm-reminder-offset-2")).toHaveValue("120");

  await form.getByTestId("crm-reminders-salon").fill("Барбершоп «Борода»");
  await expect(page.getByTestId("crm-reminder-preview-0")).toHaveText(
    "Анна, вы записаны в Барбершоп «Борода» на пн, 2 ноября в 10:30. Мастер: Олег. Услуги: Стрижка, Укладка.",
  );

  // A chip inserts its placeholder at the caret.
  const template = page.getByTestId("crm-reminder-template-2");
  await template.fill("Ждём вас, ");
  await page.getByTestId("crm-reminder-chip-2-client").click();
  await expect(template).toHaveValue("Ждём вас, {client}");
  await expect(page.getByTestId("crm-reminder-preview-2")).toHaveText(
    "Ждём вас, Анна",
  );

  // Adding fills the fourth slot, then the button is spent.
  await page.getByTestId("crm-reminders-add").click();
  await expect(page.getByTestId("crm-reminder-rule-3")).toBeVisible();
  await expect(page.getByTestId("crm-reminders-add")).toBeDisabled();
  await page.getByTestId("crm-reminder-remove-3").click();

  await template.fill("Привет, {name}");
  await form.getByTestId("crm-reminders-enabled").click();
  await page.getByTestId("crm-reminders-save").click();
  await expect(page.getByTestId("crm-reminder-template-2-error")).toHaveText(
    "Неизвестная подстановка: {name}.",
  );
  await template.fill("{client}, через 2 часа ваш визит в {time}.");
  await page.getByTestId("crm-reminders-save").click();
  await expect(page.getByText("Напоминания сохранены")).toBeVisible();
  await expect(page.getByTestId("crm-reminders-save")).toBeDisabled();

  // The saved rules come back from the relay after a reload.
  await page.getByTestId("crm-section-journal").click();
  await page.getByTestId("crm-section-reminders").click();
  await expect(page.getByTestId("crm-reminders-enabled")).toBeChecked();
  await expect(page.getByTestId("crm-reminders-salon")).toHaveValue(
    "Барбершоп «Борода»",
  );

  // Tall enough that the whole form fits without scrolling under the chrome.
  await page.setViewportSize({ width: 1280, height: 2400 });
  await expect(page.getByText("Напоминания сохранены")).toBeHidden({
    timeout: 10_000,
  });
  await waitForAnimations(page);
  await form.screenshot({ path: `${OUT}/01-settings.png` });
});

test("the log shows a week of messages with their outcome", async ({
  page,
}) => {
  await openSection(page, seed("owner", { deliveries: LOG }), "reminders");
  const log = page.getByTestId("crm-reminder-log");
  await expect(log.locator("tbody tr")).toHaveCount(4);
  await expect(log).not.toContainText("Старое сообщение");
  const rows = log.locator("tbody tr");
  await expect(rows.nth(0)).toContainText("За 24 ч");
  await expect(rows.nth(0)).toContainText("В очереди");
  await expect(rows.nth(1)).toContainText("Подтверждение записи");
  await expect(rows.nth(1)).toContainText("Отправлено");
  await expect(rows.nth(1)).toContainText("песочница");
  await expect(rows.nth(2)).toContainText("Пропущено");
  await expect(rows.nth(2)).toContainText("Клиент не подключил Telegram");
  await expect(rows.nth(3)).toContainText("Клиент заблокировал бота");
  await expect(rows.nth(3)).toContainText("Попыток: 3");

  const toggle = rows.nth(1).getByRole("button", { name: "Показать текст" });
  await toggle.click();
  await expect(
    rows.nth(1).getByRole("button", { name: "Скрыть текст" }),
  ).toHaveAttribute("aria-expanded", "true");

  await page.setViewportSize({ width: 1280, height: 900 });
  await waitForAnimations(page);
  await log.screenshot({ path: `${OUT}/02-log.png` });
});

test("guests see the rules read-only", async ({ page }) => {
  await openSection(
    page,
    seed("guest", {
      transport: "telegram",
      botUsername: "barber_bot",
      settings: {
        enabled: true,
        rules: [{ trigger: "booked", template: "{client}, ждём вас" }],
      },
    }),
    "reminders",
  );
  await expect(page.getByTestId("crm-reminders-transport")).toContainText(
    "@barber_bot",
  );
  await expect(page.getByTestId("crm-reminders-form")).toContainText(
    "Настраивать напоминания могут владелец и администраторы.",
  );
  await expect(page.getByTestId("crm-reminders-enabled")).toBeDisabled();
  await expect(page.getByTestId("crm-reminder-template-0")).toBeDisabled();
  await expect(page.getByTestId("crm-reminders-save")).toHaveCount(0);
  await expect(page.getByTestId("crm-reminders-add")).toHaveCount(0);
  await expect(page.getByTestId("crm-reminder-log-empty")).toBeVisible();
});

test("a customer card links a sandbox test chat and opts out", async ({
  page,
}) => {
  await openSection(page, seed("owner"), "parties");
  await page.getByRole("button", { name: "Открыть: Мария Соколова" }).click();
  const dialog = page.getByTestId("crm-party-dialog");
  const block = dialog.getByTestId("crm-party-telegram");
  await expect(block.getByTestId("crm-party-telegram-status")).toHaveText(
    "Не подключён",
  );
  await expect(block).toContainText("Песочница");

  const chat = block.getByTestId("crm-party-telegram-chat");
  await chat.fill("abc");
  await block.getByTestId("crm-party-telegram-link").click();
  await expect(block.getByTestId("crm-party-telegram-chat-error")).toHaveText(
    "Введите числовой ID чата.",
  );
  await chat.fill("123456");
  // Enter links the chat; it must not submit the customer card.
  await chat.press("Enter");
  await expect(block.getByTestId("crm-party-telegram-status")).toHaveText(
    "Тестовый чат 123456",
  );
  await expect(dialog).toBeVisible();

  await block.getByTestId("crm-party-telegram-optout").click();
  await expect(block).toContainText("Клиент отписался от напоминаний");
  await expect(block.getByTestId("crm-party-telegram-optout")).toBeChecked();
  await block.getByTestId("crm-party-telegram-optout").click();
  await expect(
    block.getByTestId("crm-party-telegram-optout"),
  ).not.toBeChecked();

  await page.setViewportSize({ width: 1280, height: 1100 });
  await waitForAnimations(page);
  await block.screenshot({ path: `${OUT}/03-client-telegram.png` });

  await block.getByTestId("crm-party-telegram-unlink").click();
  await expect(block.getByTestId("crm-party-telegram-status")).toHaveText(
    "Не подключён",
  );
});

test("with a real bot the card offers the invite link", async ({ page }) => {
  await openSection(
    page,
    seed("owner", {
      transport: "telegram",
      botUsername: "barber_bot",
      telegram: [
        {
          customer_id: PETR,
          chat_id: 777,
          username: "petr",
          linked_via: "bot",
        },
      ],
    }),
    "parties",
  );
  await page.getByRole("button", { name: "Открыть: Мария Соколова" }).click();
  let block = page.getByTestId("crm-party-telegram");
  await expect(block.getByTestId("crm-party-telegram-invite")).toHaveValue(
    /^https:\/\/t\.me\/barber_bot\?start=c[0-9a-f]+$/,
  );
  await expect(block.getByTestId("crm-party-telegram-chat")).toHaveCount(0);
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Открыть: Пётр Иванов" }).click();
  block = page.getByTestId("crm-party-telegram");
  await expect(block.getByTestId("crm-party-telegram-status")).toHaveText(
    "Подключён: @petr",
  );
});
