import { defineConfig } from "@playwright/test";

// CI may point PLAYWRIGHT_CHROMIUM_EXECUTABLE at a provisioned browser;
// otherwise Playwright uses its managed Chromium installation.
const executablePath = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE;

export default defineConfig({
  testDir: "./e2e",
  outputDir: "./e2e/test-results",
  timeout: 60_000,
  expect: { timeout: 15_000 },
  fullyParallel: false,
  forbidOnly: Boolean(process.env.CI),
  reporter: process.env.CI
    ? [
        ["list"],
        ["html", { outputFolder: "e2e/playwright-report", open: "never" }],
      ]
    : "list",
  use: {
    baseURL: process.env.ABSTRACTCODE_E2E_URL || "http://127.0.0.1:18782",
    headless: true,
    viewport: { width: 1440, height: 960 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "retain-on-failure",
    ...(executablePath ? { launchOptions: { executablePath } } : {}),
  },
});
