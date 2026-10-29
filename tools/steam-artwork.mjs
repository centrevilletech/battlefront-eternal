// Draws the Steam library pictures in electron/assets/steam (the capsule,
// the portrait, the hero banner, the logo over it and the icon) from the
// HTML below, in the 2004 front end's look the website uses: the blue
// hologram room with its grid floor, the holo planets, BATTLEFRONT over
// the orange ETERNAL bar. Original drawing only: no retail art, no
// franchise logos. Saira is the website's self-hosted font (SIL OFL).
//
//   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs \
//   SAIRA_FONT=../battlefront-eternal-website/static/fonts/saira-latin.woff2 \
//   [CHROMIUM=/path/to/chrome] node tools/steam-artwork.mjs
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const out = path.join(root, "electron", "assets", "steam");
const font = readFileSync(process.env.SAIRA_FONT || path.join(root, "..", "battlefront-eternal-website", "static", "fonts", "saira-latin.woff2")).toString("base64");
const emblem = readFileSync(path.join(root, "electron", "assets", "icon.svg"), "utf8");
const { chromium } = await import(process.env.PLAYWRIGHT_MODULE || "playwright");

const css = `
@font-face { font-family: "Saira"; src: url(data:font/woff2;base64,${font}) format("woff2"); font-weight: 100 900; font-stretch: 50% 125%; }
* { box-sizing: border-box; margin: 0; }
html, body { width: 100%; height: 100%; background: transparent; overflow: hidden; }
.room { position: absolute; inset: 0; overflow: hidden;
  background: radial-gradient(ellipse 60% 55% at var(--cx, 50%) var(--cy, 42%), #74b8ff 0%, #4290f2 25%, #2366d8 50%, #113d9e 74%, #071f5e 100%); }
.room::after { content: ""; position: absolute; inset: 0;
  background: repeating-linear-gradient(180deg, rgba(6, 24, 84, .18) 0 1px, transparent 1px 3px),
    radial-gradient(ellipse 80% 75% at 50% 45%, transparent 52%, rgba(3, 14, 50, .62) 100%); }
.floor { position: absolute; left: -40%; right: -40%; bottom: -4%; height: var(--floor, 52%); perspective: 520px; perspective-origin: 50% -40%;
  mask-image: linear-gradient(180deg, transparent 0%, #000 34%, #000 78%, transparent 100%); }
.grid { position: absolute; inset: -84px 0 0; transform: rotateX(62deg); transform-origin: 50% 0;
  background-image: linear-gradient(rgba(210, 236, 255, .5) 1.5px, transparent 1.5px), linear-gradient(90deg, rgba(210, 236, 255, .5) 1.5px, transparent 1.5px);
  background-size: var(--cell, 84px) var(--cell, 84px); }
.planet { position: absolute; border-radius: 50%; overflow: hidden; width: var(--size); height: var(--size);
  background: radial-gradient(circle at 34% 30%, var(--hi) 0%, var(--mid) 40%, var(--lo) 82%);
  box-shadow: 0 0 calc(var(--size) * .35) calc(var(--size) * .05) var(--glow), inset -5px -7px 12px rgba(4, 20, 70, .35); opacity: .92; }
.planet::after { content: ""; position: absolute; inset: 0; border-radius: 50%;
  background: radial-gradient(circle at 32% 28%, rgba(255, 255, 255, .55), transparent 38%), radial-gradient(circle at 74% 78%, rgba(3, 16, 64, .5), transparent 62%); }
.white { --hi: #fff; --mid: #dcefff; --lo: #7aa6da; --glow: rgba(205, 232, 255, .75); }
.amber { --hi: #ffe6b8; --mid: #f2a25a; --lo: #a7552a; --glow: rgba(255, 186, 112, .6); }
.teal { --hi: #e8fffb; --mid: #8fdcd2; --lo: #2f7f8c; --glow: rgba(160, 240, 230, .55); }
.ring { position: absolute; border-radius: 50%; border: calc(var(--size) * .012) solid rgba(210, 236, 255, .55); width: var(--size); height: var(--size);
  box-shadow: 0 0 18px rgba(200, 230, 255, .45); }
.ring.dashed { border-style: dashed; border-color: rgba(210, 236, 255, .35); }
.logo { position: absolute; display: grid; justify-items: end; text-transform: uppercase; font-family: "Saira", sans-serif; line-height: .84; }
.main { position: relative; display: block; font-size: var(--title); font-weight: 900; font-stretch: 125%; letter-spacing: -.01em; color: #102a68;
  text-shadow: 0 1px 0 #0f2862, 0 2px 0 #0e255b, 0 3px 0 #0c2254, 0 4px 0 #0b1f4d, 0 5px 0 #0a1c46, 0 6px 0 #09193f, 0 7px 0 #081738, 0 8px 0 #071431, 0 16px 26px rgba(2, 10, 44, .5);
  transform: perspective(1100px) rotateX(16deg); }
.main::after { content: attr(data-text); position: absolute; inset: 0; color: transparent; text-shadow: none;
  background: linear-gradient(180deg, #4d77d2 0%, #24489c 30%, #142f72 56%, #0d2459 100%); -webkit-background-clip: text; background-clip: text; }
.light .main { color: #cfe4ff; text-shadow: 0 1px 0 #6f97d8, 0 2px 0 #5d86cc, 0 3px 0 #4c75bf, 0 4px 0 #3c64b0, 0 5px 0 #2f55a0, 0 6px 0 #24478f, 0 7px 0 #1a3a7d, 0 8px 0 #122f6b, 0 16px 26px rgba(2, 10, 44, .6); }
.light .main::after { background: linear-gradient(180deg, #ffffff 0%, #e2f0ff 38%, #9fc6f5 70%, #6f9ee3 100%); -webkit-background-clip: text; background-clip: text; }
.sub { margin-top: .45em; margin-right: .1em; padding: .18em 2.4em .18em .9em; font-size: var(--sub); font-weight: 800; font-stretch: 112%; letter-spacing: .5em; line-height: 1.2; color: #13204a;
  background: linear-gradient(180deg, #ffd27a 0%, #ffa42a 42%, #f57500 100%); clip-path: polygon(0 0, calc(100% - 1.5em) 0, 100% 100%, 0 100%); }
.emblem { position: absolute; }
.emblem svg { width: 100%; height: 100%; display: block; }
`;

const planets = (list) => list.map(([kind, size, x, y]) => `<div class="planet ${kind}" style="--size:${size}px;left:${x}px;top:${y}px"></div>`).join("");
const rings = (list) => list.map(([size, x, y, dashed]) => `<div class="ring${dashed ? " dashed" : ""}" style="--size:${size}px;left:${x}px;top:${y}px"></div>`).join("");
// The logo, its title sized to `width` (fit() below) with the bar a
// quarter of its size, its box's top left at x, y (centre: x is the middle).
const logo = ({ width, x, y, light = true, centre = false }) =>
  `<div class="logo${light ? " light" : ""}" data-fit="${width}" style="left:${x}px;top:${y}px;${centre ? "transform:translateX(-50%)" : ""}"><span class="main" data-text="Battlefront">Battlefront</span><span class="sub">Eternal</span></div>`;
const fit = () => {
  for (const logo of document.querySelectorAll("[data-fit]")) {
    const width = Number(logo.dataset.fit), main = logo.querySelector(".main");
    let size = 200;
    logo.style.setProperty("--title", `${size}px`);
    size *= width / main.getBoundingClientRect().width;
    logo.style.setProperty("--title", `${size}px`);
    logo.style.setProperty("--sub", `${size / 4}px`);
  }
};
const room = (vars, inner) => `<div class="room" style="${vars}"><div class="floor"><div class="grid"></div></div>${inner}</div>`;

const PICTURES = [
  // The library's horizontal capsule (Steam's grid view).
  { file: "capsule.jpg", width: 920, height: 430, html: room("--cx:66%;--cy:34%;--floor:52%;--cell:64px",
    rings([[300, 610, 24], [230, 645, 59, true]]) + planets([["white", 70, 805, 22], ["amber", 38, 742, 118], ["teal", 46, 850, 132]]) +
    logo({ width: 560, x: 44, y: 128 })) },
  // The portrait capsule (the library's shelves).
  { file: "portrait.jpg", width: 600, height: 900, html: room("--cx:50%;--cy:30%;--floor:46%;--cell:72px",
    rings([[360, 120, 90], [280, 160, 130, true]]) + planets([["white", 96, 418, 70], ["amber", 52, 100, 120], ["teal", 60, 470, 250]]) +
    logo({ width: 520, x: 300, y: 520, centre: true })) },
  // The hero banner behind the logo (no text; Steam draws the logo over it).
  { file: "hero.jpg", width: 1920, height: 620, html: room("--cx:72%;--cy:30%;--floor:62%;--cell:96px",
    rings([[520, 1100, -20], [400, 1160, 40, true], [180, 1580, 200]]) + planets([["white", 120, 1500, 60], ["amber", 64, 1330, 230], ["teal", 80, 1700, 300], ["white", 40, 980, 120]])) },
  // The logo Steam lays over the hero, on transparency.
  { file: "logo.png", width: 1280, height: 330, transparent: true, html: logo({ width: 1200, x: 640, y: 50, centre: true }) },
  // The icon: the app's own emblem.
  { file: "icon.png", width: 256, height: 256, transparent: true, html: `<div class="emblem" style="inset:0">${emblem}</div>` },
];

// CHROMIUM: a Chromium to use instead of Playwright's own download.
const browser = await chromium.launch(process.env.CHROMIUM ? { executablePath: process.env.CHROMIUM } : {});
try {
  for (const picture of PICTURES) {
    const page = await browser.newPage({ viewport: { width: picture.width, height: picture.height }, deviceScaleFactor: 1 });
    await page.setContent(`<!doctype html><html><head><style>${css}</style></head><body>${picture.html}</body></html>`);
    await page.evaluate(() => document.fonts.ready);
    await page.evaluate(fit);
    const jpeg = picture.file.endsWith(".jpg");
    await page.screenshot({ path: path.join(out, picture.file), omitBackground: !!picture.transparent, ...(jpeg ? { type: "jpeg", quality: 90 } : {}) });
    await page.close();
    console.log(`${picture.file} ${picture.width}x${picture.height}`);
  }
} finally {
  await browser.close();
}
