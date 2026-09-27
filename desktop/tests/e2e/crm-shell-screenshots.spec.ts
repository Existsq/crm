import { expect, test, type Page } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

// PR screenshots for the CRM desktop shell. Each shot is scoped to the CRM
// view so the PNGs differ by state, not by surrounding chrome.

const OUT = "test-results/screenshots/crm-shell";
const SPACE_CHANNEL = "5b0f5c8e-3a9d-4d59-9f2b-0d3c7a1e2f40";

type CrmSeed = NonNullable<
  Parameters<NonNullable<Window["__BUZZ_E2E_SEED_CRM__"]>>[0]
>;

test.beforeEach(async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await installMockBridge(page);
});

async function shoot(page: Page, seed: CrmSeed, name: string) {
  await page.goto("/");
  await page.waitForFunction(() => Boolean(window.__BUZZ_E2E_SEED_CRM__));
  await page.evaluate((value) => window.__BUZZ_E2E_SEED_CRM__?.(value), seed);
  await page.getByTestId("open-crm-view").click();
  const view = page.getByTestId("crm-view");
  await expect(view).toBeVisible();
  // Members land on the catalog; these shots document the settings section.
  if (seed.space?.role) {
    await page.getByTestId("crm-section-settings").click();
    await expect(page.getByTestId("crm-settings-form")).toBeVisible();
  }
  await expect(page.getByTestId("crm-gate-loading")).toHaveCount(0);
  await waitForAnimations(page);
  await view.screenshot({ path: `${OUT}/${name}.png` });
}

test("create-space form", async ({ page }) => {
  await shoot(page, { canCreate: true, space: null }, "01-create-space");
});

test("owner settings and warehouses", async ({ page }) => {
  await shoot(
    page,
    {
      space: {
        channelId: SPACE_CHANNEL,
        role: "owner",
        warehouses: [
          {
            id: "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e",
            name: "Основной склад",
          },
          {
            id: "1c8f2a8b-7b64-4d56-8e9f-2a3b4c5d6e7f",
            name: "Магазин на Ленина",
          },
        ],
      },
    },
    "02-owner-settings",
  );
});

test("member read-only settings", async ({ page }) => {
  await shoot(
    page,
    {
      space: {
        channelId: SPACE_CHANNEL,
        role: "member",
        warehouses: [
          {
            id: "0b7e1f7a-6a53-4c45-9d8e-1f2a3b4c5d6e",
            name: "Основной склад",
          },
        ],
      },
    },
    "03-member-read-only",
  );
});

test("no access", async ({ page }) => {
  await shoot(
    page,
    { space: { channelId: SPACE_CHANNEL, role: null } },
    "04-no-access",
  );
});
