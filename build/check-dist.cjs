// electron-builder beforePack hook: package only a desktop-mode Vite build.
// Retail-derived game data (the extractor's maps, sides, common and audio)
// must never ship; `vite build` without --mode desktop copies public/ in.
const { existsSync } = require("node:fs");
const path = require("node:path");

const CONTENT_ROOTS = ["maps", "sides", "common", "audio"];

exports.default = async function checkDist(context) {
  const dist = path.join(context.packager.projectDir, "dist-desktop");
  if (!existsSync(path.join(dist, "index.html"))) {
    throw new Error("dist-desktop/index.html is missing: run `npm run build:desktop` before electron-builder.");
  }
  const found = CONTENT_ROOTS.filter((name) => existsSync(path.join(dist, name)));
  if (found.length) {
    throw new Error(`dist-desktop/ contains game data (${found.join(", ")}). Rebuild with \`npm run build:desktop\`; retail data is never packaged.`);
  }
};
