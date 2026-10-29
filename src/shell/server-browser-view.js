import { MapPreview } from "./backdrop.js";
import { COLUMNS, SOURCES, ERAS, MODES, PINGS, compatible } from "./server-browser.js";

// The server browser on the page (the page theme's view of the "servers"
// widget, server-browser.js), after the 2004 game's session list and the
// 2005 game's multiplayer screen: the source (INTERNET or LAN) and filters
// across the top, the servers under their column headers (a header sorts
// on its column), how many there are, and the picked server's GAME INFO:
// its map's flyover, what it plays and how, where it is, and who donates
// its hosting.
//
// A server's text is its operator's: it is only ever set as text. logo(url)
// resolves a sponsor's logo as an address the page may show (the desktop's
// renderer loads no remote images), or null: then the sponsor's name shows
// in its place.

export function serversView({ maps = [], logo = null } = {}) {
  return (widget, view) => new ServersView(widget, view, { maps, logo });
}

const logos = new Map(); // a sponsor logo's address -> Promise of what to show

function make(tag, className = "", text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

// A control of the bar: its keys are its own, not the menus' (Esc only
// lets go of it).
function own(control) {
  control.addEventListener("keydown", (event) => {
    event.stopPropagation();
    if (event.key === "Escape" || (event.key === "Enter" && control.tagName === "INPUT")) control.blur();
  });
  return control;
}

function choice(options, onChange, className = "") {
  const box = make("label", `bfe-field ${className}`);
  const control = own(make("select"));
  for (const [value, label] of options) control.add(new Option(label, value));
  control.addEventListener("change", () => {
    onChange(control.value);
    control.blur();
  });
  box.append(control);
  return { box, control };
}

function toggle(label, onChange) {
  const box = make("label", "bfe-toggle");
  const control = own(make("input"));
  control.type = "checkbox";
  control.addEventListener("change", () => onChange(control.checked));
  box.append(control, make("span", "bfe-toggle-switch"), make("span", "", label));
  return { box, control };
}

// "Alabama, US" -> "US".
export const countryOf = (location) => /,\s*([A-Z]{2})$/.exec(String(location || "").trim())?.[1] || "";
// Its flag: the regional indicator letters, which the system draws as a
// flag where it has the emoji (not on Windows: there the letters show).
export const flagOf = (location) => {
  const code = countryOf(location);
  return code ? String.fromCodePoint(...[...code].map((c) => 0x1f1e6 + c.charCodeAt(0) - 65)) : "";
};
// Whether the system draws flags: a flag drawn on a canvas has colour.
let flags = null;
function drawsFlags() {
  if (flags !== null) return flags;
  try {
    const ctx = document.createElement("canvas").getContext("2d", { willReadFrequently: true });
    ctx.canvas.width = ctx.canvas.height = 32;
    ctx.font = "24px sans-serif";
    ctx.textBaseline = "top";
    ctx.fillText(flagOf("x, US"), 0, 0);
    const data = ctx.getImageData(0, 0, 32, 32).data;
    flags = false;
    for (let i = 0; i < data.length; i += 4) if (data[i + 3] && (Math.abs(data[i] - data[i + 1]) > 40 || Math.abs(data[i + 1] - data[i + 2]) > 40)) { flags = true; break; }
  } catch { flags = false; }
  return flags;
}
// A place's mark: its flag, else its country's letters.
function country(location) {
  const code = countryOf(location);
  if (!code) return null;
  return drawsFlags() ? make("span", "bfe-flag", flagOf(location)) : make("span", "bfe-country", code);
}

export function uptimeText(seconds) {
  if (!Number.isFinite(seconds)) return "";
  const days = Math.floor(seconds / 86400), hours = Math.floor((seconds % 86400) / 3600), minutes = Math.floor((seconds % 3600) / 60);
  return days ? `${days}D ${hours}H` : hours ? `${hours}H ${minutes}M` : `${Math.max(minutes, 1)}M`;
}

// Signal bars for a ping: 4 under 60 ms, 3 under 100, 2 under 160, else 1.
const quality = (ms) => (!Number.isFinite(ms) ? 0 : ms < 60 ? 4 : ms < 100 ? 3 : ms < 160 ? 2 : 1);

class ServersView {
  constructor(widget, view, { maps, logo }) {
    addStyle();
    this.widget = widget;
    this.view = view;
    this.maps = maps;
    this.logo = logo;
    this.version = -1;
    this.listSignature = null;
    this.infoSignature = null;
    this.nodes = new Map();
    this.selectedNode = null;
    this.clock = 0;
    const el = (this.el = make("div", "bfe-browser"));

    // The bar: source and filters.
    const bar = make("div", "bfe-browser-bar");
    this.source = choice(SOURCES, (value) => widget.setSource(value), "bfe-browser-source");
    this.search = own(make("input", "bfe-input bfe-browser-search"));
    this.search.type = "search";
    this.search.placeholder = "SEARCH SERVERS";
    this.search.title = "A server's name, its map or its place";
    this.search.spellcheck = false;
    this.search.addEventListener("input", () => widget.setFilter("name", this.search.value));
    const known = maps.filter((map) => map.extracted !== false);
    this.map = choice([["all", "ALL MAPS"], ...known.map((map) => [map.id, map.name])], (value) => widget.setFilter("map", value));
    this.era = choice([["all", "ALL ERAS"], ...Object.entries(ERAS)], (value) => widget.setFilter("era", value));
    this.ping = choice(PINGS, (value) => widget.setFilter("ping", value));
    this.full = toggle("HIDE FULL", (on) => widget.setFilter("hideFull", on));
    this.empty = toggle("HIDE EMPTY", (on) => widget.setFilter("hideEmpty", on));
    bar.append(this.source.box, this.search, this.map.box, this.era.box, this.ping.box, this.full.box, this.empty.box);

    // The list under its headers.
    const table = make("div", "bfe-browser-table");
    const head = make("div", "bfe-browser-head");
    this.headers = COLUMNS.map((column) => {
      const node = make("button", `bfe-browser-col c-${column.id}`);
      node.type = "button";
      node.tabIndex = -1;
      node.append(make("span", "", column.label), make("i", "bfe-sort"));
      node.addEventListener("mousedown", (event) => event.preventDefault());
      node.addEventListener("click", () => {
        if (!view.live) return;
        widget.sortBy(column.id);
        view.screen.sound("select");
      });
      return [column.id, node];
    });
    head.append(...this.headers.map(([, node]) => node));
    this.rows = make("div", "bfe-browser-rows");
    this.rows.addEventListener("pointerenter", () => view.pointer({ widget, item: 0 }));
    table.append(head, this.rows);

    this.status = make("div", "bfe-browser-status");
    this.count = make("span", "count");
    this.updated = make("span", "updated");
    this.status.append(this.count, this.updated);

    this.info = this.buildInfo();
    el.append(bar, table, this.status, this.info.el);

    // How many rows are in view: Page Up and Page Down's step.
    this.observer = new ResizeObserver(() => {
      const row = this.rows.querySelector(".bfe-browser-row");
      if (row) widget.shown = Math.max(Math.floor(this.rows.clientHeight / Math.max(row.offsetHeight, 1)), 1);
    });
    this.observer.observe(this.rows);
  }

  buildInfo() {
    const el = make("section", "bfe-browser-info");
    const head = make("header", "bfe-browser-info-head");
    head.append(make("span", "", "GAME INFO"));
    const body = make("div", "bfe-browser-info-body");
    const preview = new MapPreview();
    const details = make("div", "bfe-browser-details");
    const name = make("h3", "bfe-browser-info-name");
    const description = make("p", "bfe-browser-info-description");
    const chips = make("div", "bfe-chips");
    const facts = make("dl", "bfe-facts");
    details.append(name, description, chips, facts);
    // Who donates the hosting: their logo (or, without one, their name) and
    // their site.
    const sponsor = make("aside", "bfe-sponsor");
    const sponsorLabel = make("span", "bfe-sponsor-label", "Hosting Generously Donated By");
    const logoBox = make("div", "bfe-sponsor-logo");
    const sponsorName = make("strong", "bfe-sponsor-name");
    const sponsorLink = make("button", "bfe-sponsor-link");
    sponsorLink.type = "button";
    sponsorLink.tabIndex = -1;
    sponsorLink.addEventListener("mousedown", (event) => event.preventDefault());
    sponsor.append(sponsorLabel, logoBox, sponsorName, sponsorLink);
    const empty = make("p", "bfe-browser-info-empty", "PICK A SERVER TO SEE ITS GAME INFO");
    body.append(preview.el, details, sponsor, empty);
    el.append(head, body);
    return { el, body, preview, name, description, chips, facts, sponsor, logoBox, sponsorName, sponsorLink, empty };
  }

  update(live) {
    const w = this.widget;
    this.el.classList.toggle("focused", live && this.view.screen.isFocused(w, 0));
    if (w.version !== this.version) {
      this.version = w.version;
      this.render();
    }
    // The "updated ... ago" line, once a second.
    const now = Date.now();
    if (now - this.clock > 1000) {
      this.clock = now;
      this.when();
    }
  }

  render() {
    const w = this.widget;
    this.source.control.value = w.source;
    this.map.control.value = w.filters.map;
    this.era.control.value = w.filters.era;
    this.ping.control.value = w.filters.ping;
    this.full.control.checked = w.filters.hideFull;
    this.empty.control.checked = w.filters.hideEmpty;
    if (document.activeElement !== this.search) this.search.value = w.filters.name;
    for (const [id, node] of this.headers) {
      node.classList.toggle("sorted", w.sort.key === id);
      node.dataset.dir = w.sort.dir > 0 ? "up" : "down";
    }
    const rows = w.rows();
    this.list(rows);
    const players = rows.reduce((sum, server) => sum + (Number(server.players) || 0), 0);
    this.count.textContent = w.state === "ready" ? `${rows.length} ${rows.length === 1 ? "SERVER" : "SERVERS"}  ·  ${players} ${players === 1 ? "PLAYER" : "PLAYERS"}${rows.length < w.servers.length ? `  ·  ${w.servers.length - rows.length} FILTERED OUT` : ""}` : "";
    this.when();
    this.describe(w.selected());
  }

  when() {
    const w = this.widget;
    const seconds = Math.max(0, Math.round((Date.now() - w.updatedAt) / 1000));
    this.updated.textContent = w.state === "loading" ? "SEARCHING…" : w.state !== "ready" ? "" : w.stale ? "CAN'T UPDATE, RETRYING…"
      : seconds < 5 ? "UPDATED JUST NOW" : `UPDATED ${seconds < 60 ? `${seconds} S` : `${Math.floor(seconds / 60)} MIN`} AGO`;
    this.updated.classList.toggle("stale", Boolean(w.stale));
    this.updated.title = w.stale || "";
  }

  // The rows: rebuilt when what they show changed; the pick marked.
  list(rows) {
    const w = this.widget;
    const signature = `${w.state}\u0002${w.message}\u0002` + rows.map((s) => [s.id, s.name, s.players, s.maxPlayers, s.battle?.map, s.battle?.mode, s.battle?.era, s.latencyMs, s.location, compatible(s)].join("\u0001")).join("\u0002");
    if (signature !== this.listSignature) {
      this.listSignature = signature;
      this.nodes = new Map();
      this.selectedNode = null;
      const fresh = !this.rows.childElementCount;
      if (w.state === "ready" && rows.length) {
        this.rows.replaceChildren(...rows.map((server, i) => {
          const node = this.row(server);
          if (fresh) node.style.setProperty("--i", String(i));
          this.nodes.set(server.id, node);
          return node;
        }));
        this.rows.classList.toggle("fresh", fresh);
      } else this.rows.replaceChildren(this.placeholder());
    }
    const selected = this.nodes.get(w.selectedId) || null;
    for (const [id, node] of this.nodes) node.classList.toggle("selected", id === w.selectedId);
    if (selected !== this.selectedNode) {
      this.selectedNode = selected;
      if (selected && performance.now() - this.view.theme.pointerAt > 150) {
        const r = selected.getBoundingClientRect(), b = this.rows.getBoundingClientRect();
        if (r.top < b.top) this.rows.scrollTop -= b.top - r.top;
        else if (r.bottom > b.bottom) this.rows.scrollTop += r.bottom - b.bottom;
      }
    }
  }

  row(server) {
    const w = this.widget;
    const node = make("div", "bfe-browser-row");
    node.classList.toggle("incompatible", !compatible(server));
    const name = make("span", "c-name");
    name.append(make("span", "bfe-browser-name", server.name || "UNNAMED SERVER"));
    if (!compatible(server)) name.append(make("span", "bfe-tag warn", "OTHER VERSION"));
    const players = make("span", "c-players");
    const fill = Math.min((Number(server.players) || 0) / Math.max(Number(server.maxPlayers) || 1, 1), 1);
    const meter = make("i", "bfe-meter");
    meter.style.setProperty("--fill", fill.toFixed(3));
    players.append(make("span", "", `${server.players ?? 0}/${server.maxPlayers ?? "?"}`), meter);
    const ping = make("span", "c-ping");
    const bars = make("span", `bfe-bars q${quality(server.latencyMs)}`);
    for (let i = 0; i < 4; i++) bars.append(make("i"));
    ping.append(bars, make("span", "", Number.isFinite(server.latencyMs) ? String(server.latencyMs) : "-"));
    const location = make("span", "c-location");
    const mark = country(server.location);
    if (mark) location.append(mark);
    location.append(make("span", "", server.location || "-"));
    node.append(name, players,
      make("span", "c-map", server.battle?.mapName || server.battle?.map || "-"),
      make("span", "c-mode", MODES[server.battle?.mode] || String(server.battle?.mode || "-").toUpperCase()),
      make("span", "c-era", ERAS[server.battle?.era] || "-"),
      ping, location);
    node.addEventListener("mousedown", (event) => event.preventDefault());
    node.addEventListener("click", () => {
      if (!this.view.live) return;
      this.view.pointer({ widget: w, item: 0 });
      w.select(server.id, this.view.screen);
    });
    node.addEventListener("dblclick", () => {
      if (this.view.live) this.view.screen.accept({ widget: w, item: 0 });
    });
    return node;
  }

  // What the list shows instead of servers: searching, none, the notice
  // to accept, an error.
  placeholder() {
    const w = this.widget;
    const box = make("div", `bfe-browser-empty ${w.state}`);
    const act = (label, action) => {
      const node = make("button", "bfe-btn small", undefined);
      node.type = "button";
      node.tabIndex = -1;
      node.append(make("span", "label", label.toLowerCase()));
      node.addEventListener("mousedown", (event) => event.preventDefault());
      node.addEventListener("click", () => {
        if (!this.view.live) return;
        this.view.screen.sound("accept");
        action();
      });
      return node;
    };
    if (w.state === "loading") {
      box.append(make("span", "bfe-scan"), make("p", "", w.source === "lan" ? "LOOKING FOR SERVERS ON YOUR NETWORK…" : "SEARCHING FOR SERVERS…"));
    } else if (w.state === "notice") {
      box.append(make("p", "", "ONLINE PLAY NEEDS YOUR OK FIRST"), make("p", "hint", "Read how the online service handles your connection, then accept to see the servers."), act("Review notice", () => w.notice()));
    } else if (w.state === "error" || w.state === "desktop") {
      box.append(make("p", "", w.message || "THE SERVER LIST COULD NOT BE LOADED"));
      if (w.state === "error") box.append(act("Try again", () => w.refresh()));
    } else if (w.state === "ready") {
      box.append(make("p", "", w.servers.length ? "NO SERVERS MATCH THESE FILTERS" : w.source === "lan" ? "NO SERVERS ON YOUR NETWORK" : "NO SERVERS ONLINE RIGHT NOW"),
        make("p", "hint", w.servers.length ? "Clear a filter to see more." : "Host one with CREATE, or reach one by its address with DIRECT CONNECT."));
    }
    return box;
  }

  // GAME INFO for the picked server.
  describe(server) {
    const info = this.info;
    const signature = server ? JSON.stringify([server.id, server.name, server.description, server.players, server.maxPlayers, server.battle, server.latencyMs, server.location, server.uptime, server.sponsor, server.bots, server.autoAssignTeams, server.rotation?.length]) : "";
    if (signature === this.infoSignature) return;
    const changed = server?.id !== this.infoId;
    this.infoSignature = signature;
    this.infoId = server?.id;
    info.body.classList.toggle("none", !server);
    if (changed) {
      info.body.classList.remove("swap");
      void info.body.offsetWidth;
      if (server) info.body.classList.add("swap");
    }
    const map = server ? this.maps.find((entry) => entry.id === server.battle?.map) || null : null;
    info.preview.show(map);
    if (!server) return;
    info.preview.caption.textContent = server.battle?.mapName || map?.name || "";
    info.name.textContent = server.name || "UNNAMED SERVER";
    info.description.textContent = server.description || "";
    info.description.hidden = !server.description;
    const chips = [
      MODES[server.battle?.mode] || server.battle?.mode,
      ERAS[server.battle?.era],
      server.bots === false ? "NO BOTS" : server.bots === true ? "BOTS FILL TEAMS" : null,
      server.autoAssignTeams ? "TEAMS ASSIGNED" : server.autoAssignTeams === false ? "PICK YOUR TEAM" : null,
    ].filter(Boolean);
    info.chips.replaceChildren(...chips.map((chip) => make("span", "bfe-chip", String(chip).toUpperCase())));
    const rotation = Array.isArray(server.rotation) ? server.rotation : [];
    const next = rotation[1] ? /^swbf1\/([^:]+)/.exec(rotation[1])?.[1] : null;
    const nextName = next ? this.maps.find((entry) => entry.id === next)?.name || next.toUpperCase() : null;
    const facts = [
      ["PLAYERS", `${server.players ?? 0} OF ${server.maxPlayers ?? "?"}`],
      ["PING", Number.isFinite(server.latencyMs) ? `${server.latencyMs} MS` : "NO ANSWER"],
      ["LOCATION", server.location || "UNKNOWN"],
      ["NEXT", nextName || (rotation.length === 1 ? "SAME BATTLE" : "-")],
      ["ROTATION", rotation.length ? `${rotation.length} ${rotation.length === 1 ? "BATTLE" : "BATTLES"}` : "-"],
      ["UP FOR", uptimeText(server.uptime) || "-"],
      ["VERSION", compatible(server) ? `SIM ${String(server.simulation || "").replace(/\D+/g, "") || "?"} \u2713` : "UPDATE TO JOIN"],
      ["LINK", server.relayPolicy === "always" ? "RELAYED" : server.relayPolicy === "never" ? "DIRECT" : server.address ? "LAN" : "DIRECT OR RELAY"],
    ];
    info.facts.replaceChildren(...facts.flatMap(([label, value]) => {
      const dd = make("dd", "", value);
      if (label === "LOCATION") {
        const mark = country(server.location);
        if (mark) dd.prepend(mark);
      }
      return [make("dt", "", label), dd];
    }));
    // Who donates the hosting: the logo (through the desktop), else the
    // name; the site under it.
    const sponsor = server.sponsor;
    info.sponsor.hidden = !sponsor?.name;
    if (sponsor?.name) {
      info.sponsorName.textContent = sponsor.name;
      const site = sponsor.url ? new URL(sponsor.url) : null;
      info.sponsorLink.hidden = !site;
      if (site) {
        info.sponsorLink.textContent = `${site.hostname.replace(/^www\./, "")}${site.pathname.replace(/\/$/, "")} ↗`;
        info.sponsorLink.title = site.href;
        info.sponsorLink.onclick = () => window.open(site.href, "_blank", "noopener");
      }
      info.logoBox.replaceChildren();
      info.logoBox.classList.remove("shown");
      const url = sponsor.logo;
      const logo = Boolean(url && this.logo);
      // The name waits for the logo, and shows if there is none.
      info.logoBox.hidden = !logo;
      info.sponsorName.hidden = logo;
      if (logo) {
        if (!logos.has(url)) logos.set(url, Promise.resolve(this.logo(url)).catch(() => null));
        logos.get(url).then((src) => {
          if (this.infoId !== server.id) return;
          if (!src) {
            info.logoBox.hidden = true;
            info.sponsorName.hidden = false;
            return;
          }
          const image = make("img");
          image.alt = sponsor.name;
          image.src = src;
          info.logoBox.replaceChildren(image);
          info.logoBox.classList.add("shown");
        });
      }
    }
  }

  dispose() {
    this.observer.disconnect();
    this.info.preview.dispose();
  }
}

let styled = false;
function addStyle() {
  if (styled) return;
  styled = true;
  const style = document.createElement("style");
  style.dataset.bfe = "server-browser";
  style.textContent = STYLE;
  document.head.append(style);
}

// In the page theme's looks (its custom properties, themes/page/style.js).
const STYLE = `
.bfe-screen[data-screen="mp"] { padding-top: 5vh; }
.bfe-screen[data-screen="mp"] .bfe-screen-head { margin-bottom: 1.2vh; }
.bfe-screen[data-screen="mp"] .bfe-screen-body { align-items: stretch; justify-content: flex-start; gap: 1vh; }
.bfe-screen[data-screen="mp"] .bfe-tabs { flex: none; }
.bfe-browser { flex: 1 1 auto; min-height: 0; display: grid; grid-template-rows: auto minmax(0, 1fr) auto auto; gap: 1vh; }
.bfe-browser button { font: inherit; color: inherit; }

/* The bar: source and filters. */
.bfe-browser-bar { display: flex; align-items: center; gap: .8vw; flex-wrap: wrap; padding: .9vh .9vw; background: var(--bfe-panel-bg); border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius); }
.bfe-browser-source select { min-width: 10.5em; background: var(--bfe-accent-fill); color: var(--bfe-accent-ink); border-color: transparent; letter-spacing: .16em; }
.bfe-browser-source::after { border-color: var(--bfe-accent-ink); }
.bfe-browser-search { flex: 1 1 14em; min-width: 10em; padding-right: .9em; cursor: text; }
.bfe-browser-search::placeholder { color: var(--bfe-text-faint); }
.bfe-toggle { display: inline-flex; align-items: center; gap: .5em; font-size: clamp(11px, 1.4vh, 13px); font-weight: 700; letter-spacing: .12em; color: var(--bfe-text-dim); cursor: pointer; }
.bfe-toggle input { position: absolute; opacity: 0; pointer-events: none; }
.bfe-toggle-switch { position: relative; width: 2.4em; height: 1.3em; border-radius: 1em; background: rgba(255, 255, 255, .14); transition: background .18s ease; }
.bfe-toggle-switch::after { content: ""; position: absolute; top: .15em; left: .15em; width: 1em; height: 1em; border-radius: 50%; background: var(--bfe-text); transition: transform .18s cubic-bezier(.2, .8, .2, 1.3); }
.bfe-toggle input:checked + .bfe-toggle-switch { background: var(--bfe-accent); }
.bfe-toggle input:checked + .bfe-toggle-switch::after { transform: translateX(1.1em); }

/* The list: the columns, the header bar, the rows. */
.bfe-browser-table { display: flex; flex-direction: column; min-height: 0; background: var(--bfe-panel-bg); border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius);
  -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px); overflow: hidden; }
.bfe-browser-head, .bfe-browser-row { display: grid; grid-template-columns: minmax(0, 3.1fr) minmax(0, .95fr) minmax(0, 1.9fr) minmax(0, 1fr) minmax(0, 1.55fr) minmax(0, .85fr) minmax(0, 1.35fr); align-items: center; column-gap: 1vw; padding: 0 1.1vw; }
.bfe-browser-head { flex: none; min-height: clamp(30px, 3.8vh, 40px); background: var(--bfe-accent-fill); color: var(--bfe-accent-ink); }
.bfe-browser-col { appearance: none; display: flex; align-items: center; gap: .5em; padding: 0; border: 0; background: none; color: inherit; font-size: clamp(11px, 1.45vh, 14px); font-weight: 800; letter-spacing: .14em; text-align: left; cursor: pointer; white-space: nowrap; }
.bfe-browser-col .bfe-sort { width: 0; height: 0; border-left: .35em solid transparent; border-right: .35em solid transparent; border-bottom: .45em solid currentColor; opacity: 0; transition: opacity .15s ease, transform .2s ease; }
.bfe-browser-col.sorted .bfe-sort { opacity: .9; }
.bfe-browser-col.sorted[data-dir="down"] .bfe-sort { transform: rotate(180deg); }
html[data-bfe-theme="bf2"] .bfe-browser-head { background: linear-gradient(180deg, #59616b, #2b3036); color: #fff; border-bottom: 2px solid #b31d12; }
html[data-bfe-theme="bf1"] .bfe-browser-head { clip-path: polygon(0 0, calc(100% - 1.6em) 0, 100% 100%, 0 100%); }
.bfe-browser-rows { position: relative; flex: 1 1 auto; min-height: 0; overflow-y: auto; overflow-x: hidden; padding: .4vh .3vw; }
.bfe-browser-row { position: relative; min-height: clamp(28px, 3.7vh, 40px); margin: 2px 0; border: 1px solid transparent; font-size: clamp(12px, 1.6vh, 16px); font-weight: 700; letter-spacing: .06em;
  text-transform: uppercase; color: var(--bfe-text); cursor: pointer; transition: background .12s ease, border-color .12s ease; }
.bfe-browser-rows.fresh .bfe-browser-row { animation: bfe-row-in .3s cubic-bezier(.2, .8, .2, 1) both; animation-delay: calc(min(var(--i, 0), 14) * 28ms); }
.bfe-browser-row:hover { background: var(--bfe-list-hover); }
.bfe-browser-row.selected { background: var(--bfe-list-selected); border-color: var(--bfe-accent); }
.bfe-browser.focused .bfe-browser-row.selected { background: var(--bfe-row-focus); box-shadow: inset 3px 0 0 var(--bfe-accent); }
.bfe-browser-row.incompatible { color: var(--bfe-text-faint); }
.bfe-browser-row > span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bfe-browser-row > .c-name { display: flex; align-items: center; gap: .6em; }
.bfe-browser-name { overflow: hidden; text-overflow: ellipsis; text-transform: none; letter-spacing: .02em; font-weight: 700; }
.bfe-tag { flex: none; font-size: .68em; padding: .15em .5em; letter-spacing: .12em; border: 1px solid currentColor; }
.bfe-tag.warn { color: #ff8a7a; }
.bfe-browser-row > .c-players { display: flex; flex-direction: column; gap: .25em; }
.bfe-meter { display: block; width: 4.2em; height: 3px; background: rgba(255, 255, 255, .14); }
.bfe-meter::after { content: ""; display: block; height: 100%; width: calc(var(--fill) * 100%); background: var(--bfe-accent); }
.bfe-browser-row > .c-ping { display: flex; align-items: center; gap: .55em; }
.bfe-bars { display: inline-flex; align-items: flex-end; gap: 2px; height: .95em; }
.bfe-bars i { width: 3px; background: rgba(255, 255, 255, .18); }
.bfe-bars i:nth-child(1) { height: 30%; } .bfe-bars i:nth-child(2) { height: 55%; } .bfe-bars i:nth-child(3) { height: 78%; } .bfe-bars i:nth-child(4) { height: 100%; }
.bfe-bars.q4 i { background: #5fe08a; } .bfe-bars.q3 i:nth-child(-n+3) { background: #b5e05f; }
.bfe-bars.q2 i:nth-child(-n+2) { background: #ffc24a; } .bfe-bars.q1 i:nth-child(1) { background: #ff6a5a; }
.bfe-browser-row > .c-location { display: flex; align-items: center; gap: .5em; }
.bfe-flag { font-size: 1.15em; line-height: 1; }
.bfe-country { flex: none; display: inline-grid; place-items: center; min-width: 1.9em; padding: .05em .25em; font-size: .7em; font-weight: 800; letter-spacing: .06em;
  color: var(--bfe-accent-ink); background: var(--bfe-accent-fill); border-radius: 2px; }
.bfe-facts dd .bfe-flag, .bfe-facts dd .bfe-country { margin-right: .5em; }
.bfe-browser-empty { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 1.2vh; height: 100%; min-height: 14vh; text-align: center; font-weight: 800; letter-spacing: .16em; color: var(--bfe-text-dim); }
.bfe-browser-empty p { margin: 0; }
.bfe-browser-empty .hint { font-weight: 600; letter-spacing: .04em; color: var(--bfe-text-faint); text-transform: none; }
.bfe-scan { position: relative; width: min(320px, 40%); height: 3px; overflow: hidden; background: rgba(255, 255, 255, .1); }
.bfe-scan::after { content: ""; position: absolute; inset: 0; width: 35%; background: var(--bfe-accent-fill); animation: bfe-scan 1.1s cubic-bezier(.45, 0, .55, 1) infinite; }
@keyframes bfe-scan { from { transform: translateX(-100%); } to { transform: translateX(290%); } }
.bfe-browser-status { display: flex; justify-content: space-between; gap: 2vw; padding: 0 .4vw; font-size: clamp(11px, 1.4vh, 13px); font-weight: 700; letter-spacing: .16em; color: var(--bfe-text-dim); }
.bfe-browser-status .updated.stale { color: var(--bfe-note); }

/* GAME INFO: the picked server. */
.bfe-browser-info { display: flex; flex-direction: column; height: clamp(160px, 26vh, 300px); background: var(--bfe-panel-bg); border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius);
  -webkit-backdrop-filter: blur(10px); backdrop-filter: blur(10px); overflow: hidden; }
.bfe-browser-info-head { flex: none; padding: .55vh 1.1vw; font-size: clamp(11px, 1.45vh, 14px); font-weight: 800; letter-spacing: .2em; color: var(--bfe-accent-ink); background: var(--bfe-accent-fill); }
html[data-bfe-theme="bf1"] .bfe-browser-info-head { clip-path: polygon(0 0, calc(100% - 1.6em) 0, 100% 100%, 0 100%); }
html[data-bfe-theme="bf2"] .bfe-browser-info-head { background: linear-gradient(180deg, #59616b, #2b3036); color: #fff; border-bottom: 2px solid #b31d12; }
.bfe-browser-info-body { position: relative; flex: 1 1 auto; min-height: 0; display: grid; grid-template-columns: auto minmax(0, 1fr) minmax(0, 15em); gap: 1.6vw; padding: 1.4vh 1.2vw; }
.bfe-browser-info-body.swap > :not(.bfe-browser-info-empty) { animation: bfe-info-in .38s cubic-bezier(.2, .8, .2, 1) both; }
.bfe-browser-info-body.swap > .bfe-browser-details { animation-delay: .05s; }
.bfe-browser-info-body.swap > .bfe-sponsor { animation-delay: .1s; }
@keyframes bfe-info-in { from { opacity: 0; transform: translateY(1vh); } }
.bfe-browser-info-body.none > :not(.bfe-browser-info-empty) { visibility: hidden; }
.bfe-browser-info-empty { position: absolute; inset: 0; display: none; place-items: center; margin: 0; font-weight: 800; letter-spacing: .16em; color: var(--bfe-text-faint); }
.bfe-browser-info-body.none .bfe-browser-info-empty { display: grid; }
.bfe-browser-info .bfe-preview { width: auto; height: 100%; aspect-ratio: auto; display: flex; flex-direction: column; gap: .6vh; }
.bfe-browser-info .bfe-preview-frame { flex: 1 1 auto; height: 0; width: auto; aspect-ratio: 3 / 2; }
.bfe-browser-info .bfe-preview figcaption { font-size: clamp(11px, 1.45vh, 14px); }
.bfe-browser-details { min-width: 0; display: flex; flex-direction: column; gap: .7vh; overflow: hidden; }
.bfe-browser-details > * { flex-shrink: 0; }
.bfe-browser-info-name { margin: 0; padding-bottom: .08em; font-size: clamp(15px, 2.2vh, 22px); line-height: 1.25; font-weight: 800; letter-spacing: .02em; color: var(--bfe-text); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.bfe-browser-info-description { margin: 0; font-size: clamp(12px, 1.55vh, 15px); font-weight: 500; line-height: 1.35; color: var(--bfe-text-dim); display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
@media (max-height: 800px) { .bfe-browser-info-description { -webkit-line-clamp: 1; } }
.bfe-chips { display: flex; flex-wrap: wrap; gap: .5em; }
.bfe-chip { padding: .25em .7em; font-size: clamp(10px, 1.3vh, 12px); font-weight: 800; letter-spacing: .12em; color: var(--bfe-accent); border: 1px solid var(--bfe-accent); }
.bfe-facts { display: grid; grid-template-columns: repeat(3, max-content minmax(0, 1fr)); gap: .45vh 1vw; margin: 0; font-size: clamp(11px, 1.45vh, 14px); line-height: 1.3; letter-spacing: .08em; }
.bfe-facts dt { color: var(--bfe-text-faint); font-weight: 700; }
.bfe-facts dd { margin: 0; color: var(--bfe-text); font-weight: 700; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.bfe-sponsor { display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 1vh; padding-left: 1.2vw; border-left: 1px solid var(--bfe-row-line); text-align: center; }
.bfe-sponsor-logo { display: grid; place-items: center; width: 100%; height: 48%; min-height: 40px; }
.bfe-sponsor-logo img { max-width: 100%; max-height: 100%; object-fit: contain; opacity: 0; transition: opacity .4s ease; filter: drop-shadow(0 2px 8px rgba(0, 0, 0, .5)); }
.bfe-sponsor-logo.shown img { opacity: 1; }
.bfe-sponsor-label { font-size: clamp(10px, 1.2vh, 11px); font-weight: 700; line-height: 1.5; letter-spacing: .18em; text-transform: uppercase; text-wrap: balance; color: var(--bfe-text-faint); }
.bfe-sponsor-name { font-size: clamp(13px, 1.8vh, 17px); letter-spacing: .06em; color: var(--bfe-text); }
.bfe-sponsor-link { appearance: none; max-width: 100%; padding: 0; border: 0; background: none; font-size: clamp(11px, 1.4vh, 14px); font-weight: 700; line-height: 1.35;
  letter-spacing: .03em; text-transform: none; overflow-wrap: anywhere; color: var(--bfe-accent); cursor: pointer; }
.bfe-sponsor .bfe-sponsor-link:hover { color: var(--bfe-accent); text-decoration: underline; text-underline-offset: .2em; }
`;
