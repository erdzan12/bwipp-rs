import { chromium, type FullConfig } from "@playwright/test";

/**
 * Identity gate for the e2e suite.
 *
 * Before any test runs, navigate to the configured baseURL and assert the
 * served page is genuinely the bwipp-rs "Barcode Studio" workbench. This
 * fails FAST and LOUD if the suite is pointed (via PLAYWRIGHT_BASE_URL or a
 * stale server) at a foreign app — instead of every test mysteriously timing
 * out against the wrong page.
 */
export default async function globalSetup(config: FullConfig) {
  const baseURL =
    config.projects[0]?.use?.baseURL ??
    process.env.PLAYWRIGHT_BASE_URL ??
    "http://127.0.0.1:3137";

  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.goto(baseURL, { waitUntil: "domcontentloaded", timeout: 30_000 });
    const heading = page.getByRole("heading", { name: "Barcode Studio" });
    try {
      await heading.waitFor({ state: "attached", timeout: 15_000 });
    } catch {
      const title = await page.title().catch(() => "<unknown>");
      throw new Error(
        `e2e identity check FAILED: the server at ${baseURL} is not the ` +
          `bwipp-rs "Barcode Studio" workbench (page title: "${title}"). ` +
          `Refusing to run the suite against a foreign app. Ensure nothing ` +
          `else owns the port, or set PLAYWRIGHT_BASE_URL correctly.`,
      );
    }
  } finally {
    await browser.close();
  }
}
