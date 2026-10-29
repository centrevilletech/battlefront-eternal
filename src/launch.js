// Which page a URL is, and the URLs the front end moves between
// (docs/shell.md). Pure: query strings in, URLs out. Only the defaults read
// the page's own location.
//
// A battle is its URL: the page and the world worker read the same one. The
// Instant Action playlist rides along in it, so a reload, a test or the
// worker all see the same battle:
//
//   /?map=geo1&era=cw&rotation=geo1:cw,hot1:gcw&round=0&order=random
//
// - map and era: this battle (era is cw or gcw; src/mission.js picks the
//   map's battle of that era);
// - rotation: the playlist, map:era entries in the order they play this lap;
// - round: this battle's entry in it, from 0;
// - order=random: shuffle the playlist again each time it wraps, as the
//   original's mission queue does in RANDOM ORDER (absent: PLAY IN ORDER).
//
// Connection keys are dropped when returning to menus or launching Instant
// Action. Other keys are dev flags (noreload, ai, seed, shadows...) and are kept
// in every URL built here. side, unit and spawn pick the player's unit on
// one map, and mission one battle of a map by its mission's name (a
// historical version only that way), so they don't carry over to another;
// shell and screen belong to the front end and don't carry into a battle.

// Any of these makes the page a battle (?shell=1 or ?shell=0 overrides).
export const ROUTE_KEYS = ["map", "side", "unit", "spawn", "spawnscreen"];
// The playlist's own keys, in the order they're written.
export const PLAYLIST_KEYS = ["map", "era", "rotation", "round", "order"];
// The player's pick on one map.
const PICK_KEYS = ["side", "unit", "spawn", "mission"];
// The front end's own.
const SHELL_KEYS = ["shell", "screen"];
// A connection belongs to one multiplayer session, never a menu launch.
const NETWORK_KEYS = ["network", "server", "address", "port", "relay", "journal", "journalTransients", "journalRelative", "fresh", "entity", "stateLifeMs", "stateLife"];

// The original's mission queue holds 64 entries
// (ScriptCB_GetMaxMissionQueue).
export const MAX_PLAYLIST = 64;
export const ERAS = ["cw", "gcw"];

// A query as URLSearchParams: from a URL, a query string (with or without
// its "?"), URLSearchParams, or a URL object.
function paramsOf(from) {
  if (from instanceof URLSearchParams) return new URLSearchParams(from);
  return new URLSearchParams(parts(from).search);
}

// { path, search } of a URL ("/?map=geo1", "http://...", a URL or Location)
// or a bare query string ("map=geo1", "?map=geo1").
function parts(from) {
  if (from && typeof from === "object" && "search" in from) return { path: from.pathname || "/", search: from.search };
  const text = String(from ?? "");
  if (text.startsWith("/") || /^[a-z][a-z0-9+.-]*:\/\//i.test(text)) {
    const url = new URL(text, "http://localhost/");
    return { path: url.pathname, search: url.search };
  }
  return { path: "/", search: text && !text.startsWith("?") ? `?${text}` : text };
}

// The page's own URL, where there is a page.
function here() {
  return typeof location === "undefined" ? "/" : location.href;
}

// Whether a URL is a battle (else the front end shows).
export function isBattle(from = here()) {
  const params = paramsOf(from);
  const forced = params.get("shell");
  if (forced === "1") return false;
  if (forced === "0") return true;
  return ROUTE_KEYS.some((key) => params.has(key));
}

// "geo1:cw,hot1:gcw" -> [{ map: "geo1", era: "cw" }, { map: "hot1", era: "gcw" }].
// An entry may leave its era out ("geo1"); malformed entries are dropped,
// and the list stops at MAX_PLAYLIST.
export function parseRotation(text) {
  const entries = [];
  for (const part of String(text ?? "").split(",")) {
    const [map, era, extra] = part.trim().toLowerCase().split(":");
    if (extra !== undefined || !/^[a-z0-9_]+$/.test(map || "") || (era && !/^[a-z0-9_]+$/.test(era))) continue;
    entries.push({ map, era: era || null });
    if (entries.length >= MAX_PLAYLIST) break;
  }
  return entries;
}

export function formatRotation(entries) {
  return entries.map(({ map, era }) => (era ? `${map}:${era}` : map)).join(",");
}

// The playlist a URL carries: { map, era, entries, round, random }, or null
// when it has no rotation.
export function readPlaylist(from = here()) {
  const params = paramsOf(from);
  const entries = parseRotation(params.get("rotation"));
  if (!entries.length) return null;
  const round = Math.trunc(Number(params.get("round")) || 0);
  return {
    map: params.get("map") || entries[0].map,
    era: params.get("era") || null,
    entries,
    round: ((round % entries.length) + entries.length) % entries.length,
    random: params.get("order") === "random",
  };
}

// The original's shuffle (its mission queue's): 7 x N swaps of two entries
// picked at random. A new list; random() gives [0, 1).
export function shuffle(entries, random = Math.random) {
  const list = entries.slice();
  const n = list.length;
  for (let i = 0; i < 7 * n; i++) {
    const a = Math.min(Math.floor(random() * n), n - 1);
    const b = Math.min(Math.floor(random() * n), n - 1);
    [list[a], list[b]] = [list[b], list[a]];
  }
  return list;
}

// A value as it's written in a URL: escaped, but with the playlist's , and :
// left readable.
function encode(value) {
  return encodeURIComponent(value).replace(/%2C/gi, ",").replace(/%3A/gi, ":");
}

// path?query from an ordered list of [key, value]; a key with an empty value
// is written bare (?noreload).
function build(path, pairs) {
  const query = pairs.map(([key, value]) => (value === "" ? encode(key) : `${encode(key)}=${encode(value)}`)).join("&");
  return query ? `${path}?${query}` : path;
}

// The dev flags of a URL: every key but those dropped.
function flags(params, drop) {
  return [...params].filter(([key]) => !drop.includes(key));
}

// A battle's URL: playlist entry `round` (from 0) of `rotation` (entries or
// "map:era,..." text), keeping the dev flags of `from`. map and era default
// to that entry's; with no rotation it is a single battle.
export function battleUrl({ map = null, era = null, rotation = null, round = 0, order = null } = {}, from = here()) {
  const entries = Array.isArray(rotation) ? rotation.slice(0, MAX_PLAYLIST) : parseRotation(rotation);
  const index = entries.length ? ((Math.trunc(Number(round) || 0) % entries.length) + entries.length) % entries.length : 0;
  const entry = entries[index] || null;
  map = map || entry?.map;
  era = era || (map === entry?.map ? entry?.era : null) || null;
  if (!map) throw new Error("battleUrl needs a map or a rotation");
  const pairs = [["map", map]];
  if (era) pairs.push(["era", era]);
  if (entries.length) {
    pairs.push(["rotation", formatRotation(entries)], ["round", String(index)]);
    if (order === "random" || order === true) pairs.push(["order", "random"]);
  }
  return build(parts(from).path, [...pairs, ...flags(paramsOf(from), [...PLAYLIST_KEYS, ...PICK_KEYS, ...SHELL_KEYS])]);
}

// LAUNCH: the playlist's first battle, shuffled first in random order (as
// ScriptCB_SetMissionNames shuffles before it picks the first).
export function launchUrl(entries, { random = false, rand = Math.random } = {}, from = here()) {
  const list = random ? shuffle(entries.slice(0, MAX_PLAYLIST), rand) : entries.slice(0, MAX_PLAYLIST);
  if (!list.length) throw new Error("launchUrl needs at least one entry");
  const local = build(parts(from).path, flags(paramsOf(from), NETWORK_KEYS));
  return battleUrl({ rotation: list, round: 0, order: random ? "random" : null }, local);
}

// The URL as it stands: the same battle again.
export function restartUrl(from = here()) {
  const { path, search } = parts(from);
  return `${path}${search}`;
}

// After a battle (DONE, or the end table's timeout): the playlist's next
// entry, wrapping to the first after the last, and shuffled again on the
// wrap in random order. With no playlist, the same battle again.
export function nextUrl(from = here(), { random = Math.random } = {}) {
  const playlist = readPlaylist(from);
  if (!playlist) return restartUrl(from);
  let { entries, round } = playlist;
  round += 1;
  if (round >= entries.length) {
    round = 0;
    if (playlist.random) entries = shuffle(entries, random);
  }
  return battleUrl({ rotation: entries, round, order: playlist.random ? "random" : null }, from);
}

// A front-end screen ("main": the main menu), keeping the dev flags. A
// leftover route key (spawnscreen is a dev flag too) would make it a
// battle, so shell=1 then forces the front end.
export function shellUrl(screen = null, from = here()) {
  const params = paramsOf(from);
  const pairs = flags(params, [...PLAYLIST_KEYS, ...PICK_KEYS, ...SHELL_KEYS, ...NETWORK_KEYS]);
  const head = [];
  if (pairs.some(([key]) => ROUTE_KEYS.includes(key))) head.push(["shell", "1"]);
  if (screen) head.push(["screen", screen]);
  return build(parts(from).path, [...head, ...pairs]);
}

// The pause menu's QUIT: the front end's main menu.
export function mainMenuUrl(from = here()) {
  return shellUrl("main", from);
}
