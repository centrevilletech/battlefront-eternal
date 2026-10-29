// The front end in a browser, for working on the menus:
// every screen clicks through as on the desktop, with the live server list,
// and the page reloads as its code changes. Nothing joins a game.
//
//   npx vite --config tools/gui-preview/vite.config.mjs
//
// Behind an HTTPS reverse proxy, set GUI_PREVIEW_HOST to the host the page
// is reached at (its live reload connects back through it) and
// GUI_PREVIEW_HTTPS_PORT to the proxy's port (default 443).
import { readFile } from "node:fs/promises";
import { defineConfig, mergeConfig } from "vite";
import base from "../../vite.config.js";
import { withLatency } from "../../electron/network.js";

const HOST = process.env.GUI_PREVIEW_HOST || "";
const HTTPS_PORT = Number(process.env.GUI_PREVIEW_HTTPS_PORT || 443);
const MASTER = "https://online.battlefronteternal.com";
const root = new URL("../../", import.meta.url);

// The desktop's bridge (electron/preload.cjs), stood in for in the page.
const BRIDGE = `
(() => {
  if (window.bfeDesktop) return;
  const json = (url) => fetch(url).then((response) => { if (!response.ok) throw new Error("GUI preview: " + response.status); return response.json(); });
  const off = () => Promise.reject(new Error("Joining is off in the GUI preview"));
  window.bfeDesktop = {
    preview: true,
    platform: "linux",
    versions: { electron: "gui-preview", chrome: navigator.userAgent },
    about: () => json("/__gui/about"),
    contentDir: async () => "/gui-preview/game-data",
    chooseContentDir: async () => "/gui-preview/game-data",
    contentTransfer: async () => false,
    presence: { update() {} },
    host: { request: async (action) => (action === "status" ? { active: false } : off()) },
    integrations: {
      request: async (action) => action === "state"
        ? { discord: { detail: "full", state: "disconnected" }, steam: { available: true, added: true, declined: false, startedBySteam: false, lost: false, launch: false, overlay: { supported: false, on: false, switches: false, active: false } } }
        : true,
    },
    network: {
      request: async (action, value) => {
        if (action === "notice") return { accepted: true, version: "2026-10-11", operator: "Josh Lambert", master: "${MASTER}" };
        if (action === "accept") return true;
        if (action === "servers") return json("/__gui/servers");
        if (action === "discover") return [];
        if (action === "identity") return { publicKey: "gui-preview", id: "0".repeat(64) };
        if (action === "logo") return json("/__gui/logo?url=" + encodeURIComponent(value)).then((result) => result.src);
        return off();
      },
      onSignal: () => () => {},
      openSession() {},
    },
    prepare: { state: async () => ({ mode: "ready" }), action: async () => false, onState: () => () => {} },
  };
})();`;

// Made-up servers beside the real listing, to judge the browser's layout
// with more than two rows. Their names say so; nothing joins in the preview.
const sample = (n, name, map, mapName, era, players, maxPlayers, latencyMs, location, extra = {}) => ({
  id: n.toString(16).padStart(64, "0"), name: `${name} (sample)`, protocol: 3, simulation: "bfe-sim-25", players, maxPlayers, relayPolicy: "auto", endpoints: [],
  location, description: "A made-up server in the GUI preview, to judge the server browser's layout. Not a real server.", sponsor: null,
  battle: { map, mapName, side: era === "cw" ? "rep" : "imp", era, mode: "conquest" }, bots: true, autoAssignTeams: false, uptime: 3600 * n + 420,
  rotation: [`swbf1/${map}:${era === "cw" ? "rep" : "imp"}:conquest`, "swbf1/nab2:rep:conquest", "swbf1/hot1:imp:conquest"], latencyMs, ...extra,
});
const SAMPLES = [
  sample(1, "Kashyyyk Rumble 24/7", "kas2", "KASHYYYK: DOCKS", "cw", 41, 64, 38, "Texas, US"),
  sample(2, "Old Republic Veterans", "geo1", "GEONOSIS: SPIRE", "cw", 17, 32, 112, "Frankfurt, DE", { bots: false }),
  sample(3, "Hoth Snowball Fight", "hot1", "HOTH: ECHO BASE", "gcw", 64, 64, 71, "Ontario, CA", { autoAssignTeams: true }),
  sample(4, "Midnight Mos Eisley", "tat2", "TATOOINE: MOS EISLEY", "gcw", 3, 64, 189, "Sydney, AU"),
  sample(5, "Endor Ewok Patrol", "end1", "ENDOR: BUNKER", "gcw", 0, 48, 96, "London, GB"),
  sample(6, "Naboo Plains Classic", "nab1", "NABOO: PLAINS", "cw", 9, 64, 147, "S\u00e3o Paulo, BR", { simulation: "bfe-sim-24" }),
  sample(7, "Rhen Var Citadel Night", "rhn2", "RHEN VAR: CITADEL", "gcw", 22, 64, 54, "Virginia, US"),
];

function bridge() {
  return {
    name: "gui-preview-bridge",
    transformIndexHtml: (html) => html.replace("<head>", `<head>\n    <script>${BRIDGE}</script>`),
    configureServer(server) {
      server.middlewares.use(async (request, response, next) => {
        const send = (status, value) => { response.statusCode = status; response.setHeader("content-type", "application/json"); response.end(JSON.stringify(value)); };
        try {
          if (request.url === "/__gui/servers") {
            // The master's listing with the ping from this machine, as the
            // desktop's bridge gives it (electron/network.js), and samples
            // to judge the browser's layout with (marked as such).
            const listing = await (await fetch(`${MASTER}/v1/servers`, { headers: { "x-bfe-notice": "2026-10-11" }, signal: AbortSignal.timeout(10000) })).json();
            const servers = await withLatency(Array.isArray(listing.servers) ? listing.servers : []);
            return send(200, { servers: [...servers, ...SAMPLES] });
          }
          if (request.url.startsWith("/__gui/logo?")) {
            // A sponsor's logo as a data: address, as the desktop would hand
            // it over: https, an image, 256 KB at most.
            const url = new URL(new URL(request.url, "http://x").searchParams.get("url"));
            if (url.protocol !== "https:") return send(200, { src: null });
            const response = await fetch(url, { signal: AbortSignal.timeout(5000) });
            const type = (response.headers.get("content-type") || "").split(";")[0];
            const body = Buffer.from(await response.arrayBuffer());
            const ok = response.ok && /^image\/(png|jpeg|webp|gif|svg\+xml)$/.test(type) && body.length <= 262144;
            return send(200, { src: ok ? `data:${type};base64,${body.toString("base64")}` : null });
          }
          if (request.url === "/__gui/about") {
            const wanted = JSON.parse(await readFile(new URL("tools/content-version.json", root), "utf8"));
            const state = JSON.parse(await readFile(new URL("public/.prepare.json", root), "utf8").catch(() => "null"));
            return send(200, { version: process.env.GUI_PREVIEW_VERSION || "0.1.0-preview.287", wanted, prepared: state && { maps: state.mapImportVersion, sides: state.muzzleVersion, common: state.commonVersion ?? null, current: true } });
          }
        } catch (error) { return send(502, { error: error.message }); }
        next();
      });
    },
  };
}

export default defineConfig((env) => mergeConfig(base(env), {
  plugins: [bridge()],
  server: { port: 18479, ...(HOST ? { hmr: { protocol: "wss", host: HOST, clientPort: HTTPS_PORT } } : {}) },
}));
