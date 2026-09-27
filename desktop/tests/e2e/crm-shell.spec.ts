import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";
import { FEATURE_OVERRIDES_STORAGE_KEY } from "../helpers/features";

// Mirrors `.specs/features/crm-desktop-shell/spec.md` (CRMD-01..04).

const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";
const MAIN_WAREHOUSE = "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;

test.beforeEach(async ({ page }) => {
  await installMockBridge(page);
});

async function openCrm(
  page: Page,
  seed?: CrmSeed,
  section: "products" | "settings" = "products",
) {
  await page.goto("/");
  if (seed) {
    await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
    await page.evaluate((value) => window.__BUZZ_E2E_SEED_CRM__?.(value), seed);
  }
  await page.getByTestId("open-crm-view").click();
  // `/crm` opens the first section, the catalog.
  await expect(page).toHaveURL(/#\/crm\/products$/);
  await expect(page.getByTestId("crm-view")).toBeVisible();
  if (section === "settings") await openSettings(page);
}

async function openSettings(page: Page) {
  await page.getByTestId("crm-section-settings").click();
  await expect(page).toHaveURL(/#\/crm\/settings$/);
}

function memberSeed(role: "owner" | "member"): CrmSeed {
  return {
    space: {
      channelId: SPACE_CHANNEL,
      role,
      warehouses: [{ id: MAIN_WAREHOUSE, name: "Основной склад" }],
    },
  };
}

test("sidebar entry opens the CRM and marks itself active", async ({
  page,
}) => {
  await openCrm(page, memberSeed("owner"));
  await expect(page.getByTestId("open-crm-view")).toHaveAttribute(
    "data-active",
    "true",
  );
  const nav = page.getByRole("navigation", { name: "Разделы CRM" });
  await expect(nav.getByRole("button", { name: "Товары" })).toHaveAttribute(
    "aria-current",
    "page",
  );
  await expect(
    nav.getByRole("button", { name: "Настройки" }),
  ).not.toHaveAttribute("aria-current", "page");
});

test("unknown CRM sections redirect to the first section", async ({ page }) => {
  await page.goto("/#/crm/does-not-exist");
  await expect(page).toHaveURL(/#\/crm\/products$/);
});

test("the CRM entry is hidden when the preview feature is off", async ({
  page,
}) => {
  await page.addInitScript((key) => {
    const overrides = JSON.parse(
      window.localStorage.getItem(key) ?? "{}",
    ) as Record<string, boolean>;
    overrides.crm = false;
    window.localStorage.setItem(key, JSON.stringify(overrides));
  }, FEATURE_OVERRIDES_STORAGE_KEY);
  await page.goto("/");
  await expect(page.getByTestId("open-workflows-view")).toBeVisible();
  await expect(page.getByTestId("open-crm-view")).toHaveCount(0);
});

test("a community admin creates the CRM space in a new private channel", async ({
  page,
}) => {
  await openCrm(page, { canCreate: true, space: null });
  const form = page.getByTestId("crm-create-space");
  await expect(form).toBeVisible();
  await expect(
    form.getByRole("radio", { name: "Создать новый закрытый канал «crm»" }),
  ).toBeChecked();

  await form.getByTestId("crm-warehouse-name").fill("Склад на Ленина");
  await form.getByTestId("crm-create-submit").click();

  await expect(page.getByTestId("crm-products-empty")).toContainText(
    "Товаров пока нет",
  );
  await openSettings(page);
  const settings = page.getByTestId("crm-settings-form");
  await expect(settings).toBeVisible();
  const warehouses = page.getByTestId("crm-warehouses");
  await expect(warehouses).toContainText("Склад на Ленина");
  await expect(warehouses).toContainText("По умолчанию");
});

test("members without admin rights see the ask-admin state", async ({
  page,
}) => {
  await openCrm(page, { canCreate: false, space: null });
  const state = page.getByTestId("crm-gate-no-space");
  await expect(state).toContainText("CRM ещё не создана");
  await expect(page.getByTestId("crm-create-space")).toHaveCount(0);
});

test("non-members of the space channel see the no-access state", async ({
  page,
}) => {
  await openCrm(page, {
    space: { channelId: SPACE_CHANNEL, role: null },
  });
  await expect(page.getByTestId("crm-gate-no-access")).toContainText(
    "Нет доступа к CRM",
  );
  await expect(page.getByTestId("crm-settings-form")).toHaveCount(0);
  await expect(page.getByTestId("crm-products")).toHaveCount(0);
});

test("owners save settings and manage warehouses", async ({ page }) => {
  await openCrm(page, memberSeed("owner"), "settings");

  const form = page.getByTestId("crm-settings-form");
  const save = form.getByTestId("crm-settings-save");
  await expect(save).toBeDisabled();
  await form.getByTestId("crm-settings-fiscalization").selectOption("optional");
  await expect(save).toBeEnabled();
  await save.click();
  await expect(save).toBeDisabled();
  await expect(form.getByTestId("crm-settings-fiscalization")).toHaveValue(
    "optional",
  );

  const warehouses = page.getByTestId("crm-warehouses");
  await warehouses.getByTestId("crm-warehouse-add").click();
  const dialog = page.getByTestId("crm-warehouse-dialog");
  await waitForAnimations(page);
  await dialog.getByTestId("crm-warehouse-dialog-name").fill("Магазин");
  await dialog.getByTestId("crm-warehouse-dialog-submit").click();
  await expect(dialog).toHaveCount(0);
  await expect(warehouses.getByText("Магазин", { exact: true })).toBeVisible();

  await warehouses
    .getByRole("button", { name: "Действия со складом: Магазин" })
    .click();
  await page.getByRole("menuitem", { name: "Сделать основным" }).click();
  const shopRow = warehouses.locator("li", { hasText: "Магазин" }).filter({
    hasNotText: "Основной склад",
  });
  await expect(shopRow).toContainText("По умолчанию");
  await expect(
    warehouses.locator("li", { hasText: "Основной склад" }),
  ).not.toContainText("По умолчанию");
});

test("members see settings read-only and cannot manage warehouses", async ({
  page,
}) => {
  await openCrm(page, memberSeed("member"), "settings");
  const form = page.getByTestId("crm-settings-form");
  await expect(form).toContainText(
    "Изменять настройки могут только владелец и администраторы канала CRM.",
  );
  await expect(form.getByTestId("crm-settings-fiscalization")).toBeDisabled();
  await expect(form.getByTestId("crm-settings-save")).toHaveCount(0);
  await expect(page.getByTestId("crm-warehouse-add")).toHaveCount(0);
});

test("a stale settings save shows the conflict message and keeps the draft", async ({
  page,
}) => {
  await openCrm(page, memberSeed("owner"), "settings");
  const form = page.getByTestId("crm-settings-form");
  await form.getByTestId("crm-settings-fiscalization").selectOption("off");
  // Another client changes the space: the mock revision moves on.
  await page.evaluate(
    ({ channelId, warehouse }) =>
      window.__BUZZ_E2E_SEED_CRM__?.({
        space: {
          channelId,
          role: "owner",
          warehouses: [{ id: warehouse, name: "Основной склад" }],
        },
      }),
    { channelId: SPACE_CHANNEL, warehouse: MAIN_WAREHOUSE },
  );
  await form.getByTestId("crm-settings-save").click();
  await expect(form.getByTestId("crm-settings-error")).toContainText(
    "Данные изменились",
  );
  await expect(form.getByTestId("crm-settings-fiscalization")).toHaveValue(
    "off",
  );
});

test("a settings change arriving during an edit is a conflict with a way back", async ({
  page,
}) => {
  await openCrm(page, memberSeed("owner"), "settings");
  const form = page.getByTestId("crm-settings-form");
  await form.getByTestId("crm-settings-fiscalization").selectOption("off");
  // Another client saves different settings and this client refetches them.
  await page.evaluate(
    ({ channelId, warehouse }) =>
      window.__BUZZ_E2E_SEED_CRM__?.({
        space: {
          channelId,
          role: "owner",
          settings: { fiscalization: "optional" },
          warehouses: [{ id: warehouse, name: "Основной склад" }],
        },
      }),
    { channelId: SPACE_CHANNEL, warehouse: MAIN_WAREHOUSE },
  );
  await page.getByRole("button", { name: "Обновить данные CRM" }).click();
  await expect(form.getByTestId("crm-settings-fiscalization")).toHaveValue(
    "off",
  );

  // Saving must not overwrite the newer settings.
  await form.getByTestId("crm-settings-save").click();
  await expect(form.getByTestId("crm-settings-error")).toContainText(
    "Данные изменились",
  );
  await expect(form.getByTestId("crm-settings-save")).toBeDisabled();

  await form.getByTestId("crm-settings-reload").click();
  await expect(form.getByTestId("crm-settings-fiscalization")).toHaveValue(
    "optional",
  );
  await expect(form.getByTestId("crm-settings-error")).toHaveCount(0);
  await expect(form.getByTestId("crm-settings-reload")).toHaveCount(0);
});

test("a warehouse create retried after a lost response is not duplicated", async ({
  page,
}) => {
  await openCrm(
    page,
    { ...memberSeed("owner"), loseNextWriteResponse: true },
    "settings",
  );
  const warehouses = page.getByTestId("crm-warehouses");
  await warehouses.getByTestId("crm-warehouse-add").click();
  const dialog = page.getByTestId("crm-warehouse-dialog");
  await waitForAnimations(page);
  await dialog.getByTestId("crm-warehouse-dialog-name").fill("Магазин");
  await dialog.getByTestId("crm-warehouse-dialog-submit").click();
  // The relay applied the create but the response was lost.
  await expect(dialog.getByRole("alert")).toBeVisible();
  await dialog.getByTestId("crm-warehouse-dialog-submit").click();
  await expect(dialog.getByRole("alert")).toContainText("Данные изменились");
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(warehouses.getByText("Магазин", { exact: true })).toHaveCount(1);
});
