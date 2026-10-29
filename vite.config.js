import { defineConfig } from "vite";

export const ISOLATION_HEADERS = {
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Embedder-Policy": "require-corp",
};

export default defineConfig(({ mode }) => ({
  // public/ holds the extractor's retail-derived output. The desktop build
  // (--mode desktop) must never copy it into dist/, which gets packaged; the
  // app reads game data from the player's own folder instead.
  publicDir: mode === "desktop" ? false : "public",
  // Desktop packaging must not erase the browser playtest's live dist/.
  build: { outDir: mode === "desktop" ? "dist-desktop" : "dist" },
  worker: { format: "es" },
  server: {
    host: "127.0.0.1",
    port: 18449,
    strictPort: true,
    allowedHosts: true,
    // Cross-origin isolation, so the page can hand the world worker shared
    // memory (src/runtime/worker-host.js). Everything the page loads is
    // same-origin; the desktop app sets the same headers on app://bfe/.
    headers: ISOLATION_HEADERS,
    // Retail data, extracts (which can hold a Wine prefix linking to /),
    // and the audio tools' environment and weights: never watched.
    watch: {
      ignored: ["**/extract/**", "**/data/**", "**/.venv-audio/**", "**/tools/models/**"],
      followSymlinks: false,
    },
  },
  preview: { headers: ISOLATION_HEADERS },
}));
