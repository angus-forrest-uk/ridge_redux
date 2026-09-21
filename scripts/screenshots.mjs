// README screenshots: drives the running app with Playwright and writes PNGs
// to docs/screenshots/. Run it through `just screenshots`, which installs
// playwright-core and starts nothing: the app must already be running.
//
//   BASE_URL      app to capture (default http://127.0.0.1:8420)
//   CHROMIUM_PATH browser binary; unset uses Playwright's own download
//                 (`npx playwright install chromium`)
import { mkdirSync } from "fs";
import { chromium } from "playwright-core";

const BASE_URL = process.env.BASE_URL ?? "http://127.0.0.1:8420";
const OUT = new URL("../docs/screenshots/", import.meta.url).pathname;
mkdirSync(OUT, { recursive: true });

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || undefined });
const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });

/* Wait until the elevation fetch is done and the canvas has been drawn. */
async function settled() {
  await page.waitForFunction(() => {
    const s = document.getElementById("status");
    return !s.classList.contains("busy") && /ms/.test(s.textContent);
  }, null, { timeout: 120_000 });
  await page.waitForTimeout(500); // let the map finish panning to the new area
  await page.waitForFunction(() => {
    const tiles = [...document.querySelectorAll("#map .leaflet-tile")];
    return tiles.length > 0 && tiles.every((t) => t.classList.contains("leaflet-tile-loaded"));
  }, null, { timeout: 60_000 });
  await page.waitForTimeout(300);
}

async function setSlider(label, value) {
  const row = page.locator(".row", { has: page.locator("label", { hasText: label }) });
  await row.locator('input[type="range"]').evaluate((input, v) => {
    input.value = v;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }, String(value));
}

/* Load the label font, then nudge a slider so the canvas redraws with it. */
async function withFonts() {
  await page.evaluate(() => document.fonts.load('60px "Cinzel"'));
  await setSlider("angle (deg)", await page.locator(".row", { has: page.locator("label", { hasText: "angle (deg)" }) })
    .locator('input[type="range"]').inputValue());
  await page.waitForTimeout(300);
}

// 1. The whole app on its default view.
await page.goto(BASE_URL);
await settled();
await withFonts();
await page.screenshot({ path: `${OUT}app.png` });

// 2. A preset, restyled and rotated: everything here is live in the browser.
await page.selectOption("#preset", { label: "Karwendelgebirge" });
await settled();
await setSlider("angle (deg)", 30);
await settled();
await page.screenshot({ path: `${OUT}rotated.png` });

// 3. The full control sidebar, in a window tall enough that it doesn't scroll.
await page.setViewportSize({ width: 1400, height: 1900 });
await page.waitForTimeout(300);
const controls = await page.locator("#controls").boundingBox();
const contentBottom = await page.locator("#controls .section").last()
  .evaluate((el) => el.getBoundingClientRect().bottom);
await page.screenshot({
  path: `${OUT}controls.png`,
  clip: { ...controls, height: contentBottom - controls.y + 12 },
});

// 4. The map picker, drawing a new area with the select tool.
await page.setViewportSize({ width: 1400, height: 900 });
await page.goto(BASE_URL);
await settled();
await page.click("#map-tool-select");
const map = await page.locator("#map").boundingBox();
const cx = map.x + map.width / 2, cy = map.y + map.height / 2;
await page.mouse.move(cx - 70, cy - 45);
await page.mouse.down();
await page.mouse.move(cx + 40, cy + 30, { steps: 8 });
await page.screenshot({ path: `${OUT}map-select.png`, clip: { ...map, y: map.y - 34, height: map.height + 34 } });
await page.mouse.up();

await browser.close();
console.log(`screenshots written to ${OUT}`);
