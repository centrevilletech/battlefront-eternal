// The front end's data, as tools/zero_shell.py writes it
// (docs/shell-assets.md): public/common/shell/shell.json with the movies,
// the maps, the music and the loading screen, plus common.json's strings,
// fonts and interface textures (the loading screen's disk, lights and lines
// are textures named there). Any part may be missing (nothing extracted
// yet, or an older extraction); the front end then runs on what there is,
// and the reader here fills in the movie config's fades (shell.lvl mcfg)
// where shell.json leaves them out.

export const SHELL_ROOT = "/common/shell";

// The movie config's three base fades (FadeIn, FadeOut in seconds): intros
// and one-shots, screen loops, and the maps' flyovers.
export const FADES = {
  intro: [0, 0.2],
  loop: [0.1, 0.5],
  fly: [1, 1],
};
// Movies whose own entries change a base fade: the main menu's intro cuts
// straight into its loop, and the briefing's and Galactic Conquest's intros
// fade both ways over a second.
const OWN_FADES = {
  main: { intro: [0, 0], loop: [0, 0.5] },
  briefing: { intro: [1, 1] },
  meta: { intro: [1, 1] },
};

// The port's screen ids for the movies, and the original's screen names
// that may key them in shell.json's "movies" (the first found is taken).
// Every entry is also kept under its own name (ifs_meta_load...).
export const MOVIE_SCREENS = {
  start: ["ifs_start", "ifs_login", "start_screen"],
  main: ["ifs_main", "ifs_main_intro"],
  sp: ["ifs_sp", "ifs_sp_intro", "atat_screen"],
  options: ["ifs_opt_contmain", "ifs_opt_top", "ifs_opt_top_intro", "coreship_screen"],
  briefing: ["ifs_sp_briefing", "ifs_sp_briefing_intro", "generic2_screen"],
  meta: ["ifs_meta_top", "ifs_meta_new_load", "ifs_meta", "ifs_meta_intro", "generic3_screen"],
  galaxy: ["ifs_meta_main", "ifs_meta_screen", "gc_screen"],
};

const json = (url) => fetch(url).then((r) => (r.ok ? r.json() : null)).catch(() => null);

// Everything the front end and the loading screen read: { movies, maps,
// music, whooshes, sounds, load, strings, fonts, missing }.
export async function loadShellData({ fetchJson = json } = {}) {
  const [shell, common, raster] = await Promise.all([
    fetchJson(`${SHELL_ROOT}/shell.json`),
    fetchJson("/common/common.json"),
    fetchJson("/common/raster-hud/manifest.json"),
  ]);
  return readShellData(shell, common, raster);
}

// A path in shell.json: relative to public/common/shell/, or from the site's
// root when it starts with "/".
export function resolveUrl(path, root = SHELL_ROOT) {
  if (typeof path !== "string" || !path) return null;
  if (path.startsWith("/") || /^[a-z][a-z0-9+.-]*:/i.test(path)) return path;
  return `${root}/${path.replace(/^\.\//, "")}`;
}

const number = (value, fallback) => (Number.isFinite(Number(value)) && value !== null && value !== "" ? Number(value) : fallback);

// One movie: { url, fadeIn, fadeOut }, from a path or { file|url, fadeIn,
// fadeOut }, with the given fades where it has none.
export function readClip(value, [fadeIn, fadeOut], own = {}) {
  const path = typeof value === "string" ? value : value?.file ?? value?.url ?? value?.movie;
  const url = resolveUrl(path);
  if (!url) return null;
  return {
    url,
    fadeIn: number(value?.fadeIn, number(own.fadeIn, fadeIn)),
    fadeOut: number(value?.fadeOut, number(own.fadeOut, fadeOut)),
  };
}

// shell.json's "movies": { intro, loop } each a clip or null, by the port's
// screen ids and by each entry's own name. An entry's own fadeIn/fadeOut
// are its loop's; introFadeIn/introFadeOut its intro's.
export function readMovies(movies = {}) {
  const out = {};
  const byName = new Map(Object.entries(movies || {}).map(([key, value]) => [key.toLowerCase(), value]));
  const read = (entry, own = {}) => {
    const intro = readClip(entry.intro, own.intro || FADES.intro, { fadeIn: entry.introFadeIn, fadeOut: entry.introFadeOut });
    const loop = readClip(entry.loop ?? entry.background ?? (typeof entry === "string" ? entry : null), own.loop || FADES.loop, entry);
    return intro || loop ? { intro, loop } : null;
  };
  for (const [name, entry] of byName) {
    const movie = entry && read(entry);
    if (movie) out[name] = movie;
  }
  for (const [id, aliases] of Object.entries(MOVIE_SCREENS)) {
    const entry = [id, ...aliases].map((name) => byName.get(name)).find(Boolean);
    const movie = entry && read(entry, OWN_FADES[id]);
    if (movie) out[id] = movie;
  }
  return out;
}

// A target rectangle [x, y, w, h], from an array or { x, y, w, h }.
function rect(value) {
  const list = Array.isArray(value) ? value : value ? [value.x, value.y, value.w ?? value.width, value.h ?? value.height] : [];
  const out = list.slice(0, 4).map(Number);
  return out.length === 4 && out.every(Number.isFinite) ? out : null;
}

// A map's loading screen: { planet, planetKey, stages: [{ url, target }] },
// the zoom from the galaxy to the planet, its surface and the level.
export function readMapLoad(load, strings = {}) {
  if (!load) return null;
  const list = Array.isArray(load) ? load : load.stages ?? load.zoom ?? load.images ?? [];
  const stages = list
    .map((stage) => ({ url: resolveUrl(typeof stage === "string" ? stage : stage?.image ?? stage?.texture ?? stage?.file ?? stage?.url), target: rect(stage?.target ?? stage?.rect) }))
    .filter((stage) => stage.url);
  // The planet's name: a string key (planets.geonosis.name) or the text.
  const isKey = (value) => typeof value === "string" && value.startsWith("planets.");
  const key = load.planetKey ?? load.nameKey ?? (isKey(load.planet) ? load.planet : null);
  const text = [load.planet, load.name].find((value) => typeof value === "string" && !isKey(value));
  return { planet: (key && strings[key]) || text || null, planetKey: key, stages };
}

// shell.json's "maps": [{ id, name, extracted, eras, fly, load }].
export function readMaps(maps = [], strings = {}) {
  return (Array.isArray(maps) ? maps : Object.entries(maps || {}).map(([id, value]) => ({ id, ...value })))
    .filter((map) => map && typeof map.id === "string")
    .map((map) => ({
      id: map.id.toLowerCase(),
      name: map.name ?? (map.nameKey ? strings[map.nameKey] : null) ?? map.id.toUpperCase(),
      nameKey: map.nameKey ?? null,
      // null: not known (shell.json didn't say).
      extracted: typeof map.extracted === "boolean" ? map.extracted : null,
      eras: map.eras && typeof map.eras === "object" ? map.eras : {},
      fly: readClip(map.fly, FADES.fly),
      load: readMapLoad(map.load, strings),
    }));
}

// The loading screen's own art and timing: { backdrop, disk, leds, edges,
// text, track, zoom, hold } (load.lvl's "load" config: TargetTime's pair,
// which LoadDisplay reads as 3.5 s of tracking on each picture and a 1 s
// zoom after it; a third number, the port's, is how long the level holds
// after the last zoom, else not at all, as the original). Art is a path,
// or the name of a common.json texture (texture(name) finds its file).
export function readLoad(load = {}, texture = () => null) {
  const art = (value) => (typeof value === "string" && !/[/.]/.test(value) ? texture(value) : resolveUrl(value));
  const pair = load?.targetTime ?? load?.timing;
  const timing = Array.isArray(pair) ? pair : [pair?.track ?? pair?.stage, pair?.zoom];
  const edges = load?.crosshair ?? load?.edges ?? {};
  return {
    backdrop: art(load?.backdrop),
    disk: art(load?.disk ?? load?.rotatingDisk),
    leds: (Array.isArray(load?.leds) ? load.leds : []).map(art).filter(Boolean),
    edges: { horz: art(edges.horz ?? edges.horizontal), vert: art(edges.vert ?? edges.vertical), corner: art(edges.corner) },
    // LOADING's string key.
    text: typeof load?.text === "string" ? load.text : "common.loading",
    track: number(timing[0], 3.5),
    zoom: number(timing[1], 1),
    hold: number(timing[2] ?? pair?.hold, 0),
  };
}

// A common.json texture's file by name: its 4x upscale where
// tools/upscale_hud.py made one, else the original.
export function textureFiles(common, raster) {
  const textures = common?.textures || {};
  return (name) => {
    const path = textures[name];
    if (!path) return null;
    const upscaled = raster?.textures?.[path]?.file;
    return upscaled ? `/common/raster-hud/${upscaled}` : `/common/${path}`;
  };
}

// The game's bitmap fonts for a 2D canvas: the glyph tables, and each
// page's file (its 4x upscale where tools/upscale_hud.py made one).
export function readFonts(common, raster) {
  const fonts = common?.fonts || {};
  const file = textureFiles(common, raster);
  const pages = {};
  for (const font of Object.values(fonts)) {
    for (const page of font.pages || []) {
      const url = file(page);
      if (url) pages[page] = url;
    }
  }
  return { fonts, pages };
}

export function readShellData(shell, common = null, raster = null) {
  const strings = { ...(common?.strings || {}), ...(shell?.strings || {}) };
  const sounds = {};
  for (const [name, value] of Object.entries(shell?.sounds || {})) {
    const url = resolveUrl(typeof value === "string" ? value : value?.file ?? value?.url);
    if (url) sounds[name] = { url, gain: number(value?.gain, 1), pitch: number(value?.pitch, 1) };
  }
  return {
    missing: !shell,
    texture: textureFiles(common, raster),
    movies: readMovies(shell?.movies),
    maps: readMaps(shell?.maps, strings),
    // properties: the music names a track plays under (shell_soundtrack
    // and its own); without them, every track is the soundtrack's.
    music: (shell?.music || [])
      .map((track, i) => ({
        name: track?.name ?? `track${i + 1}`,
        url: resolveUrl(typeof track === "string" ? track : track?.file ?? track?.url),
        properties: Array.isArray(track?.properties) ? track.properties : null,
        soundtrack: track?.soundtrack !== false,
      }))
      .filter((track) => track.url),
    whooshes: (shell?.whooshes || []).map((path) => resolveUrl(typeof path === "string" ? path : path?.file ?? path?.url)).filter(Boolean),
    sounds,
    load: readLoad(shell?.load, textureFiles(common, raster)),
    strings,
    ...readFonts(common, raster),
  };
}
