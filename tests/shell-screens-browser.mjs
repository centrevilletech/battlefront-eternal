import assert from "node:assert/strict";
import fs from "node:fs/promises";
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");
const browser = await chromium.launch({ args: ["--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const errors = [];
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
page.on("pageerror", (e) => errors.push(e.message));
const base = process.env.SHELL_TEST_URL || "http://127.0.0.1:18449";
const out = process.env.SHELL_SHOTS || "/tmp/bf-shell-resume";
await fs.mkdir(out, { recursive: true });
try {
  await page.goto(`${base}/?screen=main&noreload=1`);
  await page.waitForFunction(() => window.__shell?.stack, null, { timeout: 30000 });
  const screen = (id) => page.waitForFunction((id) => window.__shell.stack.top.id === id, id);
  const settle = () => page.waitForTimeout(2000);
  await settle();
  await page.screenshot({ path: `${out}/main.png` });
  await page.keyboard.press("Enter"); await screen("sp"); await settle();
  await page.keyboard.press("Enter"); await screen("ia"); await settle();
  await page.screenshot({ path: `${out}/instant-action.png` });
  const index = await page.evaluate(() => window.__shell.data.maps.findIndex((map) => map.extracted));
  assert.ok(index >= 0, "an extracted map exists");
  for (let i = 0; i < index; i++) await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  if (await page.evaluate(() => window.__shell.stack.top.id === "ia-era")) {
    await page.screenshot({ path: `${out}/era.png` });
    await page.keyboard.press("Enter");
  }
  await screen("ia"); await settle();
  assert.equal(await page.evaluate(() => window.__shell.stack.top.entries.length), 1);
  await page.screenshot({ path: `${out}/playlist.png` });
  await page.keyboard.press("Escape"); await screen("sp"); await settle();
  await page.keyboard.press("Escape"); await screen("main"); await settle();
  await page.keyboard.press("ArrowDown"); await page.keyboard.press("Enter"); await screen("mp"); await settle();
  await page.keyboard.press("Enter"); await screen("mp-lan"); await settle();
  await page.screenshot({ path: `${out}/lan-stub.png` });
  await page.keyboard.press("Escape"); await screen("mp");
  await page.keyboard.press("Escape"); await screen("main"); await settle();
  await page.keyboard.press("ArrowDown"); await page.keyboard.press("Enter"); await screen("options"); await settle();
  await page.keyboard.press("Enter"); await screen("options-game"); await settle();
  await page.screenshot({ path: `${out}/options.png` });
  assert.deepEqual(errors, []);
  console.log("PASS: main, singleplayer, playlist/era, back, LAN stub, and options with real keys; no page errors");
} finally { await browser.close(); }
