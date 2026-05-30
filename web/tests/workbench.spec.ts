import { test, expect, type Page } from "@playwright/test";

// End-to-end coverage for the bwipp-rs web workbench. These run
// against the production-built Next.js bundle, exercising the
// real WASM payload and the real catalog. They are wired into
// local CI (`scripts/ci-web.sh`) and are intentionally NOT enabled
// in GitHub Actions — see `playwright.config.ts`.
//
// Selectors prefer accessible names (`getByRole`) and stable DOM
// ids (`#mode-select`, `#payload`) so the suite stays decoupled
// from styling and Tailwind class shuffles.

/** Pick a catalog entry by its `<option value="…">` id. */
async function selectMode(page: Page, id: string) {
  await page.locator("#mode-select").selectOption(id);
}

/** Replace the textarea payload contents. */
async function setPayload(page: Page, payload: string) {
  const ta = page.locator("#payload");
  await ta.fill(payload);
  // Pressing Tab guarantees the controlled input commits onChange.
  await ta.press("Tab");
}

/** Wait for the Rust WASM engine to finish loading. The page renders
 *  a "Rust WASM is loading." inline error before the WASM `.wasm` is
 *  fetched + instantiated; once it is, the engine pill shows the
 *  supported-id count and an `<svg>` appears in the preview panel. */
async function waitForRustEngineReady(page: Page) {
  await expect(
    page.locator(".engine-pill.rust"),
    "rust engine pill should display the number of Rust-backed modes",
  ).toContainText(/\d+\s+Rust modes/, { timeout: 30_000 });
}

/** Wait for the preview to contain a fresh `<svg>` matching the
 *  caller's expectations (e.g. has a `viewBox`, has a `<circle>`). */
async function waitForSvg(page: Page) {
  const svgFrame = page.locator(".svg-frame svg").first();
  await expect(svgFrame).toBeVisible({ timeout: 15_000 });
  return svgFrame;
}

test.describe("bwipp-rs web workbench", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await waitForRustEngineReady(page);
  });

  test("page loads with Rust WASM as the default engine", async ({ page }) => {
    // Top-bar heading is the brand mark.
    await expect(page.getByRole("heading", { name: "Barcode Studio" })).toBeVisible();
    // The "Rust WASM (default)" pill button should be the active engine.
    const rustButton = page.getByRole("button", { name: /Rust WASM/ });
    await expect(rustButton).toBeVisible();
    // The default engine is "rust-wasm" — the pill should show Rust modes.
    await expect(page.locator(".engine-pill.rust")).toBeVisible();
  });

  test("Code 128 renders a barcode SVG", async ({ page }) => {
    await selectMode(page, "code128");
    await setPayload(page, "Hello-128");
    const svg = await waitForSvg(page);
    // Code 128 is a linear barcode → produces a column of <rect> bars.
    await expect(svg.locator("rect").first()).toBeVisible();
  });

  test("DotCode renders with circle dots, not rectangles", async ({ page }) => {
    await selectMode(page, "dotcode");
    await setPayload(page, "DOTCODE-TEST");
    const svg = await waitForSvg(page);
    // DotCode is rendered through `Encoded::Dots`, which emits SVG
    // <circle> elements. A rect-based render would be a regression
    // (e.g. fallback to the substrate Matrix renderer).
    const circles = svg.locator("circle");
    await expect(circles.first()).toBeVisible();
    expect(await circles.count()).toBeGreaterThan(20);
  });

  test("Ultracode renders client-side in colour (6-colour palette)", async ({ page }) => {
    await selectMode(page, "ultracode");
    await setPayload(page, "Hello");
    const svg = await waitForSvg(page);
    // Ultracode is the catalog's one colour symbology — it routes through
    // Encoded::ColorMatrix, so the WASM-rendered SVG must carry per-cell
    // palette fills (cyan / magenta / yellow / green), not just black/white.
    // A monochrome render here would be a regression in the colour path.
    const markup = await svg.evaluate((el) => el.outerHTML);
    expect(markup).toMatch(/#00ffff|#ff00ff|#ffff00|#00ff00/i);
  });

  test("QR Code renders through the native Rust encoder", async ({ page }) => {
    await selectMode(page, "qrcode");
    await setPayload(page, "https://example.com/bwipp-rs");
    const svg = await waitForSvg(page);
    // QR is a square matrix → emit a viewBox and at least one rect.
    await expect(svg).toHaveAttribute("viewBox", /.+/);
    await expect(svg.locator("rect").first()).toBeVisible();
    // The Mode meta panel should show the Rust engine pill, not bwip-js.
    await expect(page.locator(".mode-meta").getByText("rust-wasm")).toBeVisible();
  });

  test("MaxiCode renders as a hexagonal grid", async ({ page }) => {
    await selectMode(page, "maxicode");
    await setPayload(page, "Maxi-test-default-mode-4");
    const svg = await waitForSvg(page);
    // MaxiCode hex grid uses <polygon> for each hex module.
    const polygons = svg.locator("polygon");
    expect(await polygons.count()).toBeGreaterThan(100);
  });

  test("invalid EAN-13 payload surfaces inline error", async ({ page }) => {
    await selectMode(page, "ean13");
    // Three letters force the EAN encoder to reject the payload.
    await setPayload(page, "abc");
    // The destructive Alert at .error-box should render.
    const error = page.locator(".error-box");
    await expect(error).toBeVisible({ timeout: 10_000 });
    await expect(error).not.toHaveText("");
  });

  test("SVG download button works for a Code 128 payload", async ({ page }) => {
    await selectMode(page, "code128");
    await setPayload(page, "DOWNLOAD-SVG");
    await waitForSvg(page);
    const downloadPromise = page.waitForEvent("download");
    await page.getByRole("button", { name: /^SVG$/ }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toMatch(/\.svg$/);
  });

  test("PNG download button works for a Code 128 payload", async ({ page }) => {
    await selectMode(page, "code128");
    await setPayload(page, "DOWNLOAD-PNG");
    await waitForSvg(page);
    const downloadPromise = page.waitForEvent("download");
    await page.getByRole("button", { name: /^PNG$/ }).click();
    const download = await downloadPromise;
    expect(download.suggestedFilename()).toMatch(/\.png$/);
  });
});

test.describe("mobile viewport", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    await waitForRustEngineReady(page);
  });

  test("mobile layout remains usable: header, sidebar, preview all reachable", async ({ page }) => {
    // The Pixel 5 device profile gives us a 393x851 viewport.
    await expect(page.getByRole("heading", { name: "Barcode Studio" })).toBeVisible();
    // Sidebar selectors stay reachable.
    await expect(page.locator("#mode-select")).toBeVisible();
    await expect(page.locator("#payload")).toBeVisible();
    // Render a QR code and confirm the SVG fits.
    await selectMode(page, "qrcode");
    await setPayload(page, "mobile-viewport-check");
    const svg = await waitForSvg(page);
    const box = await svg.boundingBox();
    expect(box).not.toBeNull();
    expect(box!.width).toBeGreaterThan(0);
    // Width should never exceed the viewport (393px on Pixel 5).
    expect(box!.width).toBeLessThanOrEqual(393);
  });
});
