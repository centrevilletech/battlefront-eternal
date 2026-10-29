import { MenuScreen, Widget, Button, Tabs, Text } from "../menus/index.js";
import { NETWORK_CONTRACT } from "../runtime/network-contract.js";

// The server browser: MULTIPLAYER on one screen, as Battlefront II keeps
// it, under the page theme (src/menus/themes/page; the Classic theme keeps
// the original's HOST / LAN / ONLINE rows). JOIN lists the servers, the
// Internet's (the master's listing, docs/server-browser.md) or the LAN's,
// to filter, sort and pick one from; CREATE hosts one (host-dialog.js);
// LEADERBOARD is planned. The page draws it (server-browser-view.js); this
// is what it holds and does, with the menus' keys while it has the focus:
// Up and Down pick, Page Up and Page Down by a page, Home and End, Enter
// joins.

export const SOURCES = [["internet", "INTERNET"], ["lan", "LAN"]];
export const ERAS = { cw: "CLONE WARS", gcw: "GALACTIC CIVIL WAR" };
export const MODES = { conquest: "CONQUEST" };
export const PINGS = [["any", "ANY PING"], ["50", "UNDER 50 MS"], ["100", "UNDER 100 MS"], ["150", "UNDER 150 MS"]];
export const FILTERS = { name: "", map: "all", era: "all", ping: "any", hideFull: false, hideEmpty: false };
const REFRESH_EVERY = 30; // s: the list renews itself while it is up
const RETRY_EVERY = 10; // s: after a renewal that failed

const text = (value) => String(value ?? "");
// An error's words for the page, without what Electron puts in front of an
// error from the desktop ("Error invoking remote method 'bfe:network':
// Error: ").
export const errorText = (error) => text(error?.message ?? error).replace(/^Error invoking remote method '[^']*': /, "").replace(/^[A-Za-z]*Error: /, "").trim();
const finite = (value) => (Number.isFinite(value) ? value : Infinity);
// The columns, and the value each sorts on.
export const COLUMNS = [
  { id: "name", label: "GAME NAME", value: (s) => text(s.name).toLowerCase() },
  { id: "players", label: "PLAYERS", value: (s) => Number(s.players) || 0, first: -1 },
  { id: "map", label: "MAP", value: (s) => text(s.battle?.mapName || s.battle?.map) },
  { id: "mode", label: "MODE", value: (s) => text(s.battle?.mode) },
  { id: "era", label: "ERA", value: (s) => text(s.battle?.era) },
  { id: "ping", label: "PING", value: (s) => finite(s.latencyMs) },
  { id: "location", label: "LOCATION", value: (s) => text(s.location) },
];

// Whether this build can play on the server: the same simulation and
// protocol (a server that doesn't say is taken at its word).
export const compatible = (server, contract = NETWORK_CONTRACT) =>
  (server?.simulation ?? contract.simulation) === contract.simulation && (server?.protocol ?? contract.protocol) === contract.protocol;

const compare = (a, b) => (typeof a === "number" && typeof b === "number" ? a - b : String(a).localeCompare(String(b)));

// The servers: where they come from (load(source) resolves { servers },
// or { state, message, consent } for what to show instead), what they are
// filtered by and sorted on, and the one picked. onJoin(server, source).
export class ServerList extends Widget {
  constructor({ id = "servers", load = async () => ({ servers: [] }), onJoin = () => {}, acceptNotice = async () => false, now = () => Date.now(), ...rest } = {}) {
    super("servers", { id, ...rest });
    this.load = load;
    this.onJoin = onJoin;
    this.acceptNotice = acceptNotice;
    this.now = now;
    this.source = "internet";
    this.servers = [];
    this.state = "idle"; // loading, ready, notice (the privacy notice first), error
    this.message = "";
    this.consent = null;
    this.filters = { ...FILTERS };
    this.sort = { key: "ping", dir: 1 };
    this.selectedId = null;
    this.updatedAt = 0;
    this.busy = false; // a load under way
    this.stale = ""; // why the last renewal failed (the list kept), or ""
    this.failedAt = 0;
    this.shown = 10; // rows in view (the page sets it): Page Up and Page Down
    this.version = 0; // bumped on every change, for the page and the cache
    this.cache = null;
    this.loads = 0;
  }

  bump() {
    this.version += 1;
    this.cache = null;
  }

  // The servers shown: filtered, sorted (ties keep the listing's order).
  rows() {
    if (this.cache) return this.cache;
    const f = this.filters;
    const name = f.name.trim().toLowerCase();
    const list = this.servers.filter((s) =>
      (!name || text(s.name).toLowerCase().includes(name) || text(s.battle?.mapName).toLowerCase().includes(name) || text(s.location).toLowerCase().includes(name)) &&
      (f.map === "all" || s.battle?.map === f.map) &&
      (f.era === "all" || s.battle?.era === f.era) &&
      (f.ping === "any" || finite(s.latencyMs) < Number(f.ping)) &&
      (!f.hideFull || (Number(s.players) || 0) < (Number(s.maxPlayers) || Infinity)) &&
      (!f.hideEmpty || (Number(s.players) || 0) > 0));
    const column = COLUMNS.find((c) => c.id === this.sort.key) || COLUMNS.find((c) => c.id === "ping");
    this.cache = list.map((server, i) => [server, i])
      .sort(([a, i], [b, j]) => compare(column.value(a), column.value(b)) * this.sort.dir || i - j)
      .map(([server]) => server);
    return this.cache;
  }

  selected() {
    return this.rows().find((server) => server.id === this.selectedId) || null;
  }

  // The pick stays on its server while there is one; else the first.
  keep() {
    const rows = this.rows();
    if (!rows.some((server) => server.id === this.selectedId)) this.selectedId = rows[0]?.id ?? null;
  }

  select(id, screen = null) {
    if (id === this.selectedId) return false;
    this.selectedId = id;
    this.bump();
    screen?.sound("select");
    return true;
  }

  key(code, item, screen) {
    const rows = this.rows();
    if (!rows.length) return false;
    const at = rows.findIndex((server) => server.id === this.selectedId);
    const page = Math.max(this.shown - 1, 1);
    // Up at the top and Down at the bottom hand the focus on.
    if ((code === "ArrowUp" && at === 0) || (code === "ArrowDown" && at === rows.length - 1)) return false;
    const to = { ArrowUp: at - 1, ArrowDown: at + 1, PageUp: at - page, PageDown: at + page, Home: 0, End: rows.length - 1 }[code];
    if (to === undefined) return false;
    this.select(rows[Math.min(Math.max(to, 0), rows.length - 1)].id, screen);
    return true;
  }

  // Enter, or a double click: join the picked server.
  activate(item, screen) {
    return this.join(screen, true);
  }

  join(screen, sound = false) {
    const server = this.selected();
    if (!server || !compatible(server)) {
      screen?.sound("error");
      return true;
    }
    if (sound) screen?.sound("accept");
    this.onJoin(server, this.source);
    return true;
  }

  // Load the list again. quiet: keep showing the one there (the timed
  // renewal) rather than SEARCHING; a renewal that fails keeps it too, and
  // one waits for another under way.
  async refresh({ quiet = false } = {}) {
    if (quiet && this.busy) return;
    const token = ++this.loads;
    this.busy = true;
    if (!quiet || this.state !== "ready") {
      this.state = "loading";
      this.message = "";
      this.bump();
    }
    let result;
    try {
      result = await this.load(this.source);
    } catch (error) {
      if (token !== this.loads) return;
      this.busy = false;
      const message = errorText(error).toUpperCase() || "THE SERVER LIST COULD NOT BE LOADED";
      if (quiet && this.state === "ready") {
        this.stale = message;
        this.failedAt = this.now();
      } else {
        this.state = "error";
        this.message = message;
        this.servers = [];
      }
      this.bump();
      return;
    }
    if (token !== this.loads) return;
    this.busy = false;
    this.stale = "";
    this.servers = Array.isArray(result?.servers) ? result.servers : [];
    this.state = result?.state || "ready";
    this.message = text(result?.message);
    this.consent = result?.consent ?? null;
    this.updatedAt = this.now();
    this.bump();
    this.keep();
  }

  // The renewal: quietly, once a while, while the list is up; sooner after
  // one that failed.
  tick() {
    if (this.state !== "ready" || this.busy) return;
    const since = this.now() - (this.stale ? this.failedAt : this.updatedAt);
    if (since > (this.stale ? RETRY_EVERY : REFRESH_EVERY) * 1000) this.refresh({ quiet: true });
  }

  setSource(source) {
    if (source === this.source || !SOURCES.some(([id]) => id === source)) return;
    this.source = source;
    this.servers = [];
    this.selectedId = null;
    this.stale = "";
    this.refresh();
  }

  setFilter(key, value) {
    if (!(key in FILTERS) || this.filters[key] === value) return;
    this.filters[key] = value;
    this.bump();
    this.keep();
  }

  // A column's header: sort on it, or the other way when it already is.
  sortBy(key) {
    const column = COLUMNS.find((c) => c.id === key);
    if (!column) return;
    this.sort = this.sort.key === key ? { key, dir: -this.sort.dir } : { key, dir: column.first ?? 1 };
    this.bump();
  }

  // The online privacy notice, then the list.
  async notice() {
    if (await this.acceptNotice(this.consent)) await this.refresh();
  }
}

// The screen: title, the JOIN / CREATE / LEADERBOARD tabs, the list, and
// BACK, REFRESH, DIRECT CONNECT and JOIN along the bottom. host(screen) and
// direct(screen) are CREATE's and DIRECT CONNECT's.
export function serverBrowser({ load, join, host, direct, acceptNotice, strings = {} } = {}) {
  const list = new ServerList({ load, onJoin: join, acceptNotice });
  const tabs = new Tabs({
    id: "sections",
    tabs: [
      { id: "join", label: "JOIN" },
      { id: "create", label: "CREATE" },
      { id: "leaderboard", label: "LEADERBOARD", disabled: true, badge: "PLANNED" },
    ],
    // CREATE opens the host dialog over JOIN, which stays the tab.
    onChange: (index, screen) => {
      tabs.active = 0;
      if (index === 1) host?.(screen);
    },
  });
  let timer = null;
  return new MenuScreen({
    id: "mp",
    focus: "keep",
    list,
    onEnter: ({ forward }) => {
      if (forward || list.state !== "ready") list.refresh();
      clearInterval(timer);
      timer = setInterval(() => list.tick(), 1000);
    },
    onExit: () => clearInterval(timer),
    widgets: [
      new Text({ id: "title", look: "title", text: strings["ifs.main.mp"] || "MULTIPLAYER" }),
      tabs,
      list,
      new Button({ id: "back", place: "back", text: strings["common.back"] || "BACK" }),
      new Button({ id: "refresh", place: "middle", text: "REFRESH", action: () => list.refresh() }),
      new Button({ id: "direct", place: "middle", text: "DIRECT CONNECT", action: (screen) => direct?.(screen) }),
      new Button({ id: "join", place: "accept", text: "JOIN", disabled: () => !list.selected() || !compatible(list.selected()), action: (screen) => list.join(screen) }),
    ],
  });
}
