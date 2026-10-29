// The settings a URL key overrides for one visit (src/game.js), and how each
// is written there. RESTART MISSION loads the battle's URL again; one of
// these in it would undo a change the options made since, so it carries
// the setting as it is now.
const OVERRIDES = {
  ai: ["aiMode", (v) => String(v)],
  tier: ["aiDifficulty", (v) => String(v)],
  heroes: ["aiHeroes", (v) => (v ? "1" : "0")],
  size: ["battleSize", (v) => String(v)],
  scale: ["renderScale", (v) => String(v)],
  sens: ["mouseSensitivity", (v) => String(v)],
  invert: ["invertMouse", (v) => (v ? "1" : "0")],
  shadows: ["shadows", (v) => String(v)],
};

// A URL (relative to the page's) with the overrides it has rewritten to
// the settings now; the rest of its query as it was (a bare ?noreload
// stays bare).
export function withSettings(href, settings, base = typeof location === "undefined" ? "http://localhost/" : location.href) {
  const url = new URL(href, base);
  const pairs = url.search.replace(/^\?/, "").split("&").filter(Boolean).map((pair) => {
    const at = pair.indexOf("=");
    const key = decodeURIComponent(at < 0 ? pair : pair.slice(0, at));
    const override = OVERRIDES[key];
    return override ? `${pair.slice(0, at < 0 ? pair.length : at)}=${encodeURIComponent(override[1](settings.get(override[0])))}` : pair;
  });
  url.search = pairs.length ? `?${pairs.join("&")}` : "";
  return url.href;
}
