import { IW } from "../../interface/space.js";
import { KILLS, DEATHS, POINTS, PEEK, END, TABLE, DETAILS, AWARDS } from "../model.js";

// The scoreboard in the front end's page look (src/menus/themes/page;
// docs/scoreboard.md): the board the Classic view draws on the HUD
// (classic/view.js), as elements over the battle in whichever of the three
// looks the player has, through the look's custom properties and the
// theme's titles, tabs and buttons. index.js picks it by the name "page".
//
// - The world keeps running behind it, darkened and blurred a little.
// - At the top, the title (STATS) and the pages' tabs: the table, DETAILS,
//   and AWARDS (the end table's only).
// - The table: each team's name in its relation colour (the colour the
//   Classic board's logos take) with its reinforcements left, over its list
//   under a header bar: NAME, PING (Tab's table in a network game), KILLS,
//   DEATHS and CP POINTS, the column the rows are sorted by marked. Bots are
//   dim, other players in the look's highlight, and the player's own row is
//   lit; the selection is framed and the row under the pointer lit. Both
//   lists scroll together, as the model's one row index has them.
// - DETAILS and AWARDS: the Classic board's rows, each label before the
//   middle and its value after it.
// - Along the bottom, as the Classic board has them: DONE (BACK during play,
//   and on DETAILS and AWARDS), DETAILS and AWARDS.
// - Gone while the pause menu is over it (hidePointer).
//
// The pointer goes through the board's input, as the Classic board's does:
// hit(x, y) reads what is under an interface point from the elements. A
// free mouse moves the input's pointer and clicks through it; a captured one
// (the battle's) moves it as before, drawn here as an arrow. The keys are
// the input's own.

// Relation colours in the look: a friend's team and an enemy's (the server
// browser's signal greens and reds).
const FRIEND = "#5fe08a";
const FOE = "#ff6a5a";
const NEUTRAL = "var(--bfe-text-dim)";
const ENTRY = 0.8; // s: the entrance's longest animation, delays included
// The columns that sort, by the model's modes.
const SORTS = [
  { mode: KILLS, id: "kills", key: "ifs.stats.kills", label: "KILLS" },
  { mode: DEATHS, id: "deaths", key: "ifs.stats.deaths", label: "DEATHS" },
  { mode: POINTS, id: "points", key: null, label: "CP POINTS" },
];
const TABS = [[TABLE, null, "TABLE"], [DETAILS, "ifs.Stats.details", "DETAILS"], [AWARDS, "ifs.Stats.awards", "AWARDS"]];
// The pointer while the mouse is captured.
const ARROW = '<svg viewBox="0 0 22 30" aria-hidden="true"><path d="M2 2v22.6l5.9-5.6 4.2 9.6 4-1.8-4.1-9.4H20z"/></svg>';

function make(tag, className = "", text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

// A button that never takes the keyboard focus, its label kept in lower
// case for the looks that set it in title case (bf2), as the page theme's.
function button(text, className) {
  const node = make("button", className);
  node.type = "button";
  node.tabIndex = -1;
  node.append(make("span", "label", String(text).toLowerCase()));
  node.addEventListener("mousedown", (event) => event.preventDefault());
  return node;
}

function setLabel(node, text) {
  const label = node.querySelector(".label");
  const value = String(text).toLowerCase();
  if (label.textContent !== value) label.textContent = value;
}

function setText(node, text) {
  if (node.textContent !== text) node.textContent = text;
}

// Signal bars for a ping: 4 under 60 ms, 3 under 100, 2 under 160, else 1.
const quality = (ms) => (!Number.isFinite(ms) ? 0 : ms < 60 ? 4 : ms < 100 ? 3 : ms < 160 ? 2 : 1);

// Which way the sorted column's arrow points: up while the fewest come
// first (DEATHS, or KILLS and CP POINTS turned round).
export function sortDirection(mode, reverse = false) {
  return (mode === DEATHS) !== Boolean(reverse) ? "up" : "down";
}

// What a page's tab does from the page shown: the table is BACK, the
// others their own button; nothing on its own page.
export function tabTarget(page, current) {
  if (page === current) return null;
  return page === TABLE ? { kind: "back" } : { kind: page };
}

export class PageView {
  // hud: the BattleHud (its canvas, interface height and strings); port:
  // the scoreboard port; model: the ScoreboardModel; input: the pointer and
  // its clicks. common (the Classic board's logo records) isn't needed.
  constructor({ hud, port, model, input = null }) {
    this.hud = hud;
    this.port = port;
    this.model = model;
    this.input = input;
    // The HUD's layer: the HUD's own displays hide under it, the
    // reinforcements too; the board draws nothing there.
    this.layer = { teams: false };
    this.root = null; // built when the board first opens
    this.state = null; // peek or end while open
    this.covered = false; // the pause menu is over it
    this.pitch = 0; // px from one row to the next
    this.measured = false;
    this.shownFirst = -1; // the model's first row as the lists show it
    this.mouse = null; // a free mouse's last place, in client px
    this.syncPointer = false;
    this.hoverNode = null;
    this.entering = 0;
    this.listeners = [];
    if (typeof window !== "undefined") {
      // A free mouse moves the input's pointer while the board is up, and
      // its place is kept for the next opening; a captured one moves the
      // input's pointer itself (bindDom, on the document, before the
      // window hears the move), and the arrow follows.
      this.listen(window, "mousemove", (event) => this.moved(event), { passive: true });
      this.listen(window, "wheel", (event) => this.wheel(event), { passive: true });
    }
  }

  listen(target, type, handler, options) {
    target.addEventListener(type, handler, options);
    this.listeners.push(() => target.removeEventListener(type, handler, options));
  }

  text(key, fallback) {
    return (key && this.hud.strings?.[key]) || fallback;
  }

  get canvas() {
    return this.hud.renderer?.domElement ?? null;
  }

  get locked() {
    return typeof document !== "undefined" && Boolean(document.pointerLockElement);
  }

  // Whether the table shows PING: Tab's, in a network game.
  get pingColumn() {
    return Boolean(this.port.network) && this.model.state === PEEK;
  }

  // The pause menu over the board takes its place until it closes.
  get hidePointer() {
    return this.covered;
  }

  set hidePointer(on) {
    on = Boolean(on);
    if (on === this.covered) return;
    this.covered = on;
    if (!this.root) return;
    this.root.classList.toggle("covered", on);
    if (on) this.hover(null);
    else this.syncPointer = true;
    this.placeArrow();
  }

  // --- building ---------------------------------------------------------

  build() {
    addStyle();
    const root = (this.root = make("div", "bfe-board"));
    root.setAttribute("role", "dialog");
    root.setAttribute("aria-label", "Scoreboard");
    const screen = make("div", "bfe-screen");
    screen.dataset.screen = "scoreboard";
    screen.dataset.layout = "wide";
    const head = make("header", "bfe-screen-head");
    const title = make("h1", "bfe-text bfe-text-title");
    title.append(make("span", "", this.text("ifs.stats.statstitle", "STATS")));
    head.append(title);
    const body = make("div", "bfe-screen-body");
    const tabs = make("nav", "bfe-tabs bfe-board-tabs");
    this.tabs = {};
    for (const [page, key, label] of TABS) {
      const node = button(this.text(key, label), "bfe-tab");
      node.dataset.hit = "tab";
      node.dataset.page = page;
      this.tabs[page] = node;
      tabs.append(node);
    }
    this.table = this.buildTable();
    this.details = this.buildSheet(DETAILS, this.text("ifs.stats.personalstatstitle", "PERSONAL STATS"));
    this.awards = this.buildSheet(AWARDS, this.text("ifs.stats.awardstatstitle", "PLAYER AWARDS"));
    body.append(tabs, this.table.el, this.details.el, this.awards.el);
    // The bottom row: the Classic board's buttons in its places.
    const foot = make("footer", "bfe-screen-foot");
    const slot = (place, kind, text) => {
      const box = make("div", place);
      const node = button(text, "bfe-btn small");
      node.dataset.hit = kind;
      box.append(node);
      foot.append(box);
      return node;
    };
    this.buttons = {
      back: slot("left", "back", this.text("common.back", "BACK")),
      details: slot("middle", "details", this.text("ifs.Stats.details", "DETAILS")),
      awards: slot("right", "awards", this.text("ifs.Stats.awards", "AWARDS")),
    };
    screen.append(head, body, foot);
    this.arrow = make("div", "bfe-board-pointer");
    this.arrow.innerHTML = ARROW;
    root.append(screen, this.arrow);
    this.listen(root, "mousedown", (event) => this.pressed(event));
    this.listen(root, "contextmenu", (event) => event.preventDefault());
    document.body.append(root);
    // A list's new size: the rows that show whole, measured again.
    this.observer = new ResizeObserver(() => {
      this.measured = false;
    });
    for (const side of [1, 2]) this.observer.observe(this.table.sides[side].list);
  }

  // Both teams side by side: each team's title over its list.
  buildTable() {
    const el = make("section", "bfe-board-table");
    const sides = {};
    for (const side of [1, 2]) {
      const team = make("div", "bfe-board-team");
      team.dataset.side = String(side);
      const head = make("header", "bfe-board-team-head");
      const name = make("h2", "bfe-board-team-name");
      const left = make("span", "bfe-board-team-left");
      const count = make("strong");
      left.append(make("span", "", "REINFORCEMENTS"), count);
      head.append(name, left);
      const panel = make("div", "bfe-board-panel");
      const cols = make("div", "bfe-board-cols");
      cols.append(make("div", "bfe-board-col c-name", this.text("ifs.MPLobby.name_header", "NAME")),
        make("div", "bfe-board-col c-ping", this.text("ifs.MPLobby.ping_header", "PING")));
      const headers = {};
      for (const sort of SORTS) {
        const node = make("div", `bfe-board-col c-${sort.id} sorts`);
        node.dataset.hit = "sort";
        node.dataset.mode = String(sort.mode);
        node.dataset.side = String(side);
        node.append(make("span", "", this.text(sort.key, sort.label)), make("i", "bfe-board-sort"));
        headers[sort.mode] = node;
        cols.append(node);
      }
      const list = make("div", "bfe-board-rows");
      this.listen(list, "scroll", () => this.scrolled(side), { passive: true });
      panel.append(cols, list);
      team.append(head, panel);
      el.append(team);
      sides[side] = { team, name, left, count, headers, list, rows: [], colour: null };
    }
    return { el, sides, ping: null };
  }

  // DETAILS or AWARDS: a panel with the page's title on its bar, and its
  // rows; DETAILS' row's name, team and numbers over them.
  buildSheet(page, title) {
    const el = make("section", `bfe-board-sheet ${page}`);
    el.hidden = true;
    const bar = make("header", "bfe-board-bar");
    bar.append(make("span", "", title));
    const body = make("div", "bfe-board-sheet-body");
    let subject = null;
    if (page === DETAILS) {
      subject = { el: make("div", "bfe-board-subject"), name: make("h3", "bfe-board-subject-name"), chips: make("div", "bfe-board-chips") };
      subject.el.append(subject.name, subject.chips);
      body.append(subject.el);
    }
    const list = make("dl", "bfe-board-stats");
    body.append(list);
    el.append(bar, body);
    return { el, list, subject, key: null };
  }

  // A list's row: name, PING (its bars and number), KILLS, DEATHS and CP
  // POINTS; blank until it has a row of the model's.
  rowNode(side, index) {
    const node = make("div", "bfe-board-row blank");
    node.dataset.side = String(side);
    node.dataset.index = String(index);
    const bars = make("span", "bfe-board-bars");
    for (let i = 0; i < 4; i++) bars.append(make("i"));
    const ms = make("span");
    const ping = make("span", "c-ping");
    ping.append(bars, ms);
    const cells = { name: make("span", "c-name"), kills: make("span", "c-kills"), deaths: make("span", "c-deaths"), points: make("span", "c-points"), bars, ms };
    node.append(cells.name, ping, cells.kills, cells.deaths, cells.points);
    return { node, cells, key: null, filled: false, selected: false };
  }

  // --- what the board reacts to ----------------------------------------

  // The board opened, on the table: the selection brought into view as the
  // model opens it.
  open({ state } = {}) {
    if (typeof document === "undefined") return;
    if (!this.root) this.build();
    this.state = state ?? this.model.state;
    this.root.dataset.state = this.state;
    this.syncPointer = true;
    this.show(TABLE);
    this.renderTable("open");
    this.renderChrome();
    this.root.classList.add("open");
    this.root.classList.toggle("covered", this.covered);
    this.enter("entering");
  }

  close() {
    if (!this.root) return;
    this.state = null;
    this.hover(null);
    clearTimeout(this.entering);
    this.root.classList.remove("open", "locked", "entering", "turning");
  }

  // DETAILS or AWARDS shows, or the table again (its view as the model
  // left it).
  pageChanged({ page }) {
    if (!this.root || !this.state) return;
    this.show(page);
    if (page === TABLE) this.renderTable("return");
    else if (page === DETAILS) {
      this.details.key = null;
      this.renderDetails();
    } else if (page === AWARDS) this.renderAwards();
    this.renderChrome();
    this.hover(null);
    this.enter("turning");
  }

  show(page) {
    this.table.el.hidden = page !== TABLE;
    this.details.el.hidden = page !== DETAILS;
    this.awards.el.hidden = page !== AWARDS;
    this.root.dataset.page = page;
  }

  // The entrance (opening) or a page's (turning): the class its animations
  // hang on, for as long as they run; the rows cascade from the first shown.
  enter(name) {
    const root = this.root;
    root.classList.remove("entering", "turning");
    for (const side of [1, 2]) {
      this.table.sides[side].rows.forEach((row, index) => row.node.style.setProperty("--i", String(Math.max(index - this.model.first, 0))));
    }
    void root.offsetWidth;
    root.classList.add(name);
    clearTimeout(this.entering);
    this.entering = setTimeout(() => root.classList.remove(name), ENTRY * 1000);
  }

  // One frame: the live numbers, the selection, what the pointer is over.
  update() {
    if (!this.root || !this.state) return;
    // A free mouse that hasn't moved since the board opened (or the pause
    // menu closed) is where it was.
    if (this.syncPointer) {
      this.syncPointer = false;
      if (!this.locked && this.mouse) this.pointTo(this.mouse.x, this.mouse.y);
    }
    const page = this.model.page;
    if (page === TABLE) this.renderTable();
    else if (page === DETAILS) this.renderDetails();
    this.renderChrome();
    this.pointed();
  }

  // --- the table --------------------------------------------------------

  // The teams' titles, the sort, the rows and the selection, from the model.
  // place: "open" (the selection brought into view), "return" (the view as
  // the model has it), or none (a frame).
  renderTable(place = null) {
    const model = this.model;
    const table = this.table;
    const ping = this.pingColumn;
    if (ping !== table.ping) {
      table.ping = ping;
      table.el.classList.toggle("pinged", ping);
      this.measured = false;
    }
    const length = model.length;
    const dir = sortDirection(model.mode, model.reverse);
    for (const side of [1, 2]) {
      const part = table.sides[side];
      const team = model.titles[side];
      setText(part.name, team ? String(team.name) : "");
      setText(part.count, team ? String(team.reinforcements) : "");
      part.left.hidden = !team;
      const colour = this.colour(side);
      if (colour !== part.colour) {
        part.colour = colour;
        part.team.style.setProperty("--team", colour);
      }
      for (const sort of SORTS) {
        const node = part.headers[sort.mode];
        node.classList.toggle("sorted", sort.mode === model.mode);
        if (node.dataset.dir !== dir) node.dataset.dir = dir;
      }
      // As many rows as the longer list: the shorter one's last are blank.
      if (part.rows.length !== length) {
        while (part.rows.length < length) {
          const row = this.rowNode(side, part.rows.length);
          part.rows.push(row);
          part.list.append(row.node);
        }
        while (part.rows.length > length) part.rows.pop().node.remove();
        this.measured = false;
      }
      part.rows.forEach((row, index) => this.fill(row, model.row(side, index)));
    }
    if ((place || !this.measured) && this.measure()) {
      if (place === "open") model.autoscroll();
      else model.clamp();
      this.shownFirst = -1;
    }
    if (this.measured && model.first !== this.shownFirst) this.scrollToFirst();
    table.el.classList.toggle("more", model.canScrollDown);
    for (const side of [1, 2]) {
      this.table.sides[side].rows.forEach((row, index) => {
        const selected = row.filled && side === model.side && index === model.selected;
        if (selected === row.selected) return;
        row.selected = selected;
        row.node.classList.toggle("selected", selected);
      });
    }
  }

  // A row's texts and kind (bot, player, the player himself), when they
  // changed; a blank's none.
  fill(row, data) {
    const ping = data && Number.isFinite(data.ping) ? data.ping : null;
    const key = data ? [data.name, data.kills, data.deaths, data.points, ping, data.local ? 1 : 0, data.human ? 1 : 0].join("\u0001") : "";
    if (key === row.key) return;
    row.key = key;
    row.filled = Boolean(data);
    const { node, cells } = row;
    node.classList.toggle("blank", !data);
    node.classList.toggle("me", Boolean(data?.local));
    node.classList.toggle("human", Boolean(data?.human && !data.local));
    if (data) node.dataset.hit = "row";
    else delete node.dataset.hit;
    cells.name.textContent = data ? String(data.name) : "";
    cells.kills.textContent = data ? String(data.kills) : "";
    cells.deaths.textContent = data ? String(data.deaths) : "";
    cells.points.textContent = data ? String(data.points) : "";
    cells.ms.textContent = ping === null ? "" : String(ping);
    cells.bars.className = `bfe-board-bars q${quality(ping)}`;
    cells.bars.hidden = ping === null;
  }

  // How far apart the rows are and how many show whole (the model's
  // shown); the header's columns kept over the rows', the scroll bar's
  // room added.
  // (In fractions of a pixel: the rows' height follows the window's.)
  measure() {
    const part = this.table.sides[1];
    const { rows, list } = part;
    if (!rows.length) return false;
    const box = list.getBoundingClientRect();
    const first = rows[0].node.getBoundingClientRect();
    const pitch = rows[1] ? rows[1].node.getBoundingClientRect().top - first.top : first.height;
    if (!(box.height > 0) || !(first.height > 0) || !(pitch > 0)) return false;
    const top = first.top - box.top + list.scrollTop;
    this.pitch = pitch;
    this.model.shown = Math.max(Math.floor((box.height - top - first.height + 0.5) / pitch) + 1, 1);
    this.table.el.style.setProperty("--gutter", `${list.offsetWidth - list.clientWidth}px`);
    this.measured = true;
    return true;
  }

  // Both lists at the model's first row.
  scrollToFirst() {
    this.shownFirst = this.model.first;
    const top = this.model.first * this.pitch;
    for (const side of [1, 2]) this.table.sides[side].list.scrollTop = top;
  }

  // A list scrolled (the wheel, its bar, or scrollToFirst): the other
  // follows, and the model's first row is the one at the top, the last
  // page's at the bottom. The selection stays where it is.
  scrolled(side) {
    if (!this.state || !this.pitch || this.model.page !== TABLE) return;
    const list = this.table.sides[side].list;
    const other = this.table.sides[side === 1 ? 2 : 1].list;
    if (Math.abs(other.scrollTop - list.scrollTop) >= 1) other.scrollTop = list.scrollTop;
    const model = this.model;
    const last = Math.max(model.length - model.shown, 0);
    const bottom = list.scrollTop >= list.scrollHeight - list.clientHeight - 1;
    const first = bottom ? last : Math.min(Math.max(Math.round(list.scrollTop / this.pitch), 0), last);
    model.first = first;
    this.shownFirst = first;
  }

  // A team's colour as the player's team sees it.
  colour(team) {
    const own = this.model.team;
    const relation = this.port.relation?.(own, team) ?? (team === own ? 1 : -1);
    return relation > 0 ? FRIEND : relation < 0 ? FOE : NEUTRAL;
  }

  // A team's name as a tag in its colour, or null without one.
  tag(team) {
    const name = this.model.titles[team]?.name;
    if (!name) return null;
    const node = make("span", "bfe-board-tag", String(name));
    node.style.setProperty("--team", this.colour(team));
    return node;
  }

  // --- DETAILS and AWARDS -----------------------------------------------

  // DETAILS: the selected row's record, from the world each frame (live
  // during play), redrawn when it changed.
  renderDetails() {
    const model = this.model;
    const subject = model.subject;
    const rows = this.port.personal?.(subject?.slot) || [];
    const row = subject ? model.lists[subject.side]?.find((entry) => entry.slot === subject.slot) ?? null : null;
    const key = JSON.stringify([subject?.side, subject?.name, row?.kills, row?.deaths, row?.points, model.titles[subject?.side]?.name, rows.map((entry) => [entry.key, entry.label, entry.value])]);
    const sheet = this.details;
    if (key === sheet.key) return;
    sheet.key = key;
    setText(sheet.subject.name, String(subject?.name ?? ""));
    const chips = [];
    const team = subject ? this.tag(subject.side) : null;
    if (team) chips.push(team);
    if (row) {
      for (const [label, value] of [[this.text("ifs.stats.kills", "KILLS"), row.kills], [this.text("ifs.stats.deaths", "DEATHS"), row.deaths], ["CP POINTS", row.points]]) {
        const chip = make("span", "bfe-chip");
        chip.append(make("span", "", label), make("b", "", String(value)));
        chips.push(chip);
      }
    }
    sheet.subject.chips.replaceChildren(...chips);
    sheet.list.replaceChildren(...rows.map((entry, i) => this.stat(i, this.text(entry.key, entry.label), entry.value)));
  }

  // AWARDS: each award's winner, "NAME (n)", after his team's tag.
  renderAwards() {
    const awards = this.port.awards?.() || [];
    this.awards.list.replaceChildren(...awards.map((award, i) => this.stat(i, this.text(award.key, award.label), award.text, award.team)));
  }

  // The i-th row of DETAILS or AWARDS: its label, and its value or a dash.
  stat(i, label, value, team = null) {
    const row = make("div", "bfe-board-stat");
    row.style.setProperty("--i", String(i));
    const dd = make("dd");
    const text = String(value ?? "").trim();
    const tag = text && team ? this.tag(team) : null;
    if (tag) dd.append(tag);
    dd.append(make("span", text ? "value" : "value none", text || "-"));
    row.append(make("dt", "", label), dd);
    return row;
  }

  // --- the tabs and buttons ---------------------------------------------

  // The tabs for the pages there are, the one shown lit; DONE on the end
  // table (gone while the next battle isn't ready), BACK elsewhere; DETAILS
  // and AWARDS on the table, AWARDS at the end only.
  renderChrome() {
    const page = this.model.page;
    const end = this.state === END;
    this.tabs[AWARDS].hidden = !end;
    for (const [id, node] of Object.entries(this.tabs)) node.classList.toggle("active", id === page);
    const done = end && page === TABLE;
    const back = this.buttons.back;
    const kind = done ? "done" : "back";
    if (back.dataset.hit !== kind) back.dataset.hit = kind;
    setLabel(back, done ? this.text("ifs.stats.done", "DONE") : this.text("common.back", "BACK"));
    back.hidden = done && this.port.canRestart?.() === false;
    this.buttons.details.hidden = page !== TABLE;
    this.buttons.awards.hidden = !end || page !== TABLE;
  }

  // --- the pointer ------------------------------------------------------

  // Client px to interface units, and back (the HUD's canvas spans them).
  toInterface(x, y) {
    const canvas = this.canvas;
    if (canvas && this.hud.toInterface) {
      const rect = canvas.getBoundingClientRect();
      return this.hud.toInterface(x - rect.left, y - rect.top);
    }
    return { x: (x / Math.max(innerWidth, 1)) * IW, y: (y / Math.max(innerHeight, 1)) * this.hud.height };
  }

  toClient(x, y) {
    const canvas = this.canvas;
    const rect = canvas?.getBoundingClientRect?.() ?? { left: 0, top: 0 };
    const width = canvas?.clientWidth || innerWidth;
    const height = canvas?.clientHeight || innerHeight;
    return { x: rect.left + (x / IW) * width, y: rect.top + (y / Math.max(this.hud.height, 1)) * height };
  }

  pointTo(x, y) {
    if (!this.input) return;
    const point = this.toInterface(x, y);
    this.input.move(point.x, point.y);
  }

  // The element under an interface point that does something, or null.
  nodeAt(x, y) {
    if (!this.root || !this.state || this.covered) return null;
    const at = this.toClient(x, y);
    const node = document.elementFromPoint(at.x, at.y)?.closest?.("[data-hit]") ?? null;
    return node && this.root.contains(node) ? node : null;
  }

  // An interface point's target, as the Classic board's hit() has them: a
  // column (sort), a row, a button; a tab is its page's button.
  hit(x, y) {
    const node = this.nodeAt(x, y);
    if (!node) return null;
    const model = this.model;
    const kind = node.dataset.hit;
    if (kind === "row") {
      const side = Number(node.dataset.side);
      const index = Number(node.dataset.index);
      return model.page === TABLE && model.row(side, index) ? { kind, side, index } : null;
    }
    if (kind === "sort") return model.page === TABLE ? { kind, mode: Number(node.dataset.mode), side: Number(node.dataset.side) } : null;
    if (kind === "tab") return tabTarget(node.dataset.page, model.page);
    return { kind };
  }

  // The element the pointer is over lit: a button as the look lights its
  // focus, the rest as hovered.
  hover(node) {
    if (node === this.hoverNode) return;
    const mark = (target, on) => target?.classList.toggle(target.classList.contains("bfe-btn") ? "active" : "hover", on);
    mark(this.hoverNode, false);
    this.hoverNode = node;
    mark(node, true);
  }

  // A free mouse's press on the board: a click at its place, through the
  // input (a captured mouse's presses reach the input from bindDom). A
  // press on a list's scroll bar is the bar's.
  pressed(event) {
    const target = event.target;
    if (target.classList?.contains("bfe-board-rows") && event.offsetX >= target.clientWidth) return;
    event.preventDefault();
    if (event.button !== 0 || !this.state || this.covered || this.locked || !this.input) return;
    this.pointTo(event.clientX, event.clientY);
    this.input.button(0, true);
  }

  // A move lights what the pointer is over at once, not on the next frame.
  moved(event) {
    if (this.locked) {
      if (this.state) this.pointed();
      return;
    }
    // The browser's move when it lets go of a captured mouse isn't the
    // player's.
    if (event.movementX === 0 && event.movementY === 0) return;
    this.mouse = { x: event.clientX, y: event.clientY };
    if (!this.state) return;
    this.pointTo(event.clientX, event.clientY);
    this.pointed();
  }

  // The arrow and the lit element where the input's pointer is.
  pointed() {
    this.placeArrow();
    const cursor = this.input?.cursor;
    this.hover(cursor && !this.covered ? this.nodeAt(cursor.x, cursor.y) : null);
  }

  // The wheel with the mouse captured scrolls the list under the arrow (a
  // free mouse's scrolls it itself).
  wheel(event) {
    if (!this.state || this.covered || !this.locked || this.model.page !== TABLE || !this.input) return;
    const at = this.toClient(this.input.cursor.x, this.input.cursor.y);
    const list = document.elementFromPoint(at.x, at.y)?.closest?.(".bfe-board-rows");
    if (list && this.root.contains(list)) list.scrollTop += event.deltaMode === 1 ? event.deltaY * (this.pitch || 30) : event.deltaY;
  }

  // The arrow at the input's pointer while the mouse is captured.
  placeArrow() {
    if (!this.root) return;
    const on = Boolean(this.state) && !this.covered && this.locked && Boolean(this.input);
    this.root.classList.toggle("locked", on);
    if (!on) return;
    const at = this.toClient(this.input.cursor.x, this.input.cursor.y);
    this.arrow.style.transform = `translate(${at.x.toFixed(1)}px, ${at.y.toFixed(1)}px)`;
  }

  dispose() {
    for (const off of this.listeners.splice(0)) off();
    clearTimeout(this.entering);
    this.observer?.disconnect();
    this.root?.remove();
    this.root = null;
    this.state = null;
  }
}

let styled = false;
function addStyle() {
  if (styled) return;
  styled = true;
  const style = document.createElement("style");
  style.dataset.bfe = "scoreboard";
  style.textContent = STYLE;
  document.head.append(style);
}

// In the page theme's looks (its custom properties, themes/page/style.js),
// laid out as its screens are, the table after the server browser's
// (src/shell/server-browser-view.js).
const STYLE = `
/* Over the battle, under the pause menu's page (z-index 20): the running
   world through a darkened blur. */
.bfe-board { position: fixed; inset: 0; z-index: 10; overflow: hidden; color: var(--bfe-text); font-family: "BFE Saira", system-ui, sans-serif;
  user-select: none; -webkit-user-select: none; cursor: default;
  background: linear-gradient(180deg, rgba(0, 0, 0, .3) 0%, rgba(0, 0, 0, 0) 22%, rgba(0, 0, 0, 0) 80%, rgba(0, 0, 0, .34) 100%), var(--bfe-overlay);
  -webkit-backdrop-filter: blur(4px); backdrop-filter: blur(4px);
  opacity: 0; visibility: hidden; pointer-events: none; transition: opacity .14s ease, visibility 0s linear .14s; }
.bfe-board.open { opacity: 1; visibility: visible; pointer-events: auto; transition: opacity .14s ease; }
.bfe-board.open.covered { opacity: 0; visibility: hidden; pointer-events: none; transition: opacity .2s ease, visibility 0s linear .2s; }
.bfe-board *, .bfe-board *::before, .bfe-board *::after { box-sizing: border-box; }
.bfe-board button { font: inherit; color: inherit; }
.bfe-board [hidden] { display: none !important; }
.bfe-board * { scrollbar-width: thin; scrollbar-color: var(--bfe-scroll) transparent; }

/* The page: the title, the tabs and the page under them, the buttons along the bottom. */
.bfe-board .bfe-screen[data-screen="scoreboard"] { padding-top: 5vh; }
.bfe-board .bfe-screen[data-screen="scoreboard"] .bfe-screen-head { margin-bottom: 1.2vh; }
.bfe-board .bfe-screen[data-screen="scoreboard"] .bfe-screen-body { align-items: stretch; justify-content: flex-start; gap: 1vh; }
.bfe-board .bfe-tabs { flex: none; }
.bfe-board .bfe-tab.hover:not(.active) { box-shadow: inset 0 -2px 0 var(--bfe-accent); }

/* The table: the teams side by side. */
.bfe-board-table { flex: 1 1 auto; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); grid-template-rows: minmax(0, 1fr); gap: 1.6vw;
  padding-top: .6vh; }
.bfe-board-team { min-width: 0; min-height: 0; display: flex; flex-direction: column; gap: .9vh; }
.bfe-board-team-head { flex: none; display: flex; align-items: center; justify-content: space-between; gap: 1vw; padding: 0 .2vw; }
.bfe-board-team-name { margin: 0; display: flex; align-items: center; gap: .55em; min-width: 0; font-size: clamp(16px, 2.5vh, 26px); font-weight: 800; font-stretch: 112%;
  letter-spacing: .12em; line-height: 1.15; text-transform: uppercase; white-space: nowrap; color: var(--team, var(--bfe-text)); text-shadow: 0 2px 14px rgba(0, 0, 0, .75); }
.bfe-board-team-name::before { content: ""; flex: none; width: .24em; height: .95em; background: currentColor; box-shadow: 0 0 12px currentColor; }
.bfe-board-team-left { display: flex; align-items: baseline; gap: .8em; white-space: nowrap; font-size: clamp(10px, 1.3vh, 13px); font-weight: 700; letter-spacing: .2em;
  color: var(--bfe-text-dim); text-shadow: 0 1px 8px rgba(0, 0, 0, .8); }
.bfe-board-team-left strong { font-size: clamp(16px, 2.5vh, 26px); font-weight: 800; letter-spacing: .04em; color: var(--bfe-text); font-variant-numeric: tabular-nums; }

/* A team's list: the header bar, the rows. The columns are set in lengths
   that are the same for the bar and the rows (--u: the rows' text; --hs:
   the bar's), and the numbers keep the sort arrow's room (--mark) so they
   end where their labels do. */
.bfe-board-panel { --u: clamp(12px, 1.6vh, 16px); --hs: clamp(12px, 1.75vh, 16px); --mark: calc(var(--hs) * 1.2); --pad: 1.1vw; --inset: .3vw;
  --cols: minmax(0, 1fr) calc(var(--u) * 5.4) calc(var(--u) * 6.6) calc(var(--u) * 8.4);
  flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; overflow: hidden;
  background: var(--bfe-panel-bg); border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius); }
.bfe-board-table.pinged .bfe-board-panel { --cols: minmax(0, 1fr) calc(var(--u) * 4.6) calc(var(--u) * 5.4) calc(var(--u) * 6.6) calc(var(--u) * 8.4); }
.bfe-board-cols, .bfe-board-row { display: grid; grid-template-columns: var(--cols); align-items: center; column-gap: 1vw; }
.bfe-board-table:not(.pinged) .c-ping { display: none !important; }
.bfe-board-cols { flex: none; min-height: clamp(30px, 3.8vh, 40px); padding: 0 calc(var(--pad) + var(--inset) + var(--gutter, 0px) + 1px) 0 calc(var(--pad) + var(--inset) + 1px);
  font-size: var(--hs); font-weight: 600; letter-spacing: .14em; text-transform: uppercase; background: var(--bfe-accent-fill); color: var(--bfe-accent-ink); }
html[data-bfe-theme="bf2"] .bfe-board-cols { background: linear-gradient(180deg, #59616b, #2b3036); color: #fff; border-bottom: 2px solid #b31d12; }
html[data-bfe-theme="bf1"] .bfe-board-cols { clip-path: polygon(0 0, calc(100% - 1.6em) 0, 100% 100%, 0 100%); }
.bfe-board-col { position: relative; display: flex; align-items: center; justify-content: flex-end; min-width: 0; white-space: nowrap; }
.bfe-board-col.c-name { justify-content: flex-start; }
.bfe-board-col.sorts, .bfe-board-row > .c-kills, .bfe-board-row > .c-deaths, .bfe-board-row > .c-points { padding-right: var(--mark); }
.bfe-board-col.sorts { cursor: pointer; }
.bfe-board-col.hover > span { text-decoration: underline; text-decoration-thickness: 2px; text-underline-offset: .3em; }
.bfe-board-sort { position: absolute; right: .15em; top: 50%; margin-top: -.225em; width: 0; height: 0; border-left: .35em solid transparent; border-right: .35em solid transparent;
  border-bottom: .45em solid currentColor; opacity: 0; transition: opacity .15s ease, transform .2s ease; }
.bfe-board-col.sorted .bfe-board-sort { opacity: .9; }
.bfe-board-col.hover:not(.sorted) .bfe-board-sort { opacity: .35; }
.bfe-board-col[data-dir="down"] .bfe-board-sort { transform: rotate(180deg); }
.bfe-board-rows { position: relative; flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; gap: 2px; overflow-y: auto; overflow-x: hidden;
  scrollbar-gutter: stable; padding: .4vh var(--inset); }
/* More rows below: the last one in view fades out. */
.bfe-board-table.more .bfe-board-rows { -webkit-mask-image: linear-gradient(180deg, #000 calc(100% - 2.4em), rgba(0, 0, 0, .15) 100%);
  mask-image: linear-gradient(180deg, #000 calc(100% - 2.4em), rgba(0, 0, 0, .15) 100%); }
.bfe-board-row { position: relative; flex: none; height: clamp(26px, 3.4vh, 38px); padding: 0 var(--pad); border: 1px solid transparent;
  font-size: var(--u); font-weight: 700; letter-spacing: .06em; text-transform: uppercase; color: var(--bfe-text-dim); cursor: pointer;
  transition: background .12s ease, border-color .12s ease; }
.bfe-board-row > span { min-width: 0; overflow: hidden; white-space: nowrap; }
.bfe-board-row > .c-name { text-overflow: ellipsis; letter-spacing: .04em; }
.bfe-board-row > :not(.c-name) { text-align: right; font-variant-numeric: tabular-nums; }
.bfe-board-row.blank { cursor: default; }
.bfe-board-row.human { color: var(--bfe-value-focus); }
.bfe-board-row.me { color: var(--bfe-text); background: var(--bfe-row-focus); box-shadow: inset 3px 0 0 var(--bfe-accent); }
.bfe-board-row.hover:not(.me) { background: var(--bfe-list-hover); }
.bfe-board-row.selected { border-color: var(--bfe-accent); }
.bfe-board-row.selected:not(.me) { background: var(--bfe-list-selected); }
.bfe-board-row > .c-ping { display: flex; align-items: center; justify-content: flex-end; gap: .5em; }
.bfe-board-bars { display: inline-flex; align-items: flex-end; gap: 2px; height: .9em; }
.bfe-board-bars i { width: 3px; background: rgba(255, 255, 255, .18); }
.bfe-board-bars i:nth-child(1) { height: 30%; } .bfe-board-bars i:nth-child(2) { height: 55%; } .bfe-board-bars i:nth-child(3) { height: 78%; } .bfe-board-bars i:nth-child(4) { height: 100%; }
.bfe-board-bars.q4 i { background: #5fe08a; } .bfe-board-bars.q3 i:nth-child(-n+3) { background: #b5e05f; }
.bfe-board-bars.q2 i:nth-child(-n+2) { background: #ffc24a; } .bfe-board-bars.q1 i:nth-child(1) { background: #ff6a5a; }

/* DETAILS and AWARDS: a panel with the page's title on its bar, labels
   before the middle and values after it. */
.bfe-board-sheet { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; overflow: hidden; margin-top: .6vh;
  background: var(--bfe-panel-bg); border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius); }
.bfe-board-bar { flex: none; padding: .55vh 1.1vw; font-size: clamp(11px, 1.45vh, 14px); font-weight: 800; letter-spacing: .2em; color: var(--bfe-accent-ink); background: var(--bfe-accent-fill); }
html[data-bfe-theme="bf1"] .bfe-board-bar { clip-path: polygon(0 0, calc(100% - 1.6em) 0, 100% 100%, 0 100%); }
html[data-bfe-theme="bf2"] .bfe-board-bar { background: linear-gradient(180deg, #59616b, #2b3036); color: #fff; border-bottom: 2px solid #b31d12; }
.bfe-board-sheet-body { flex: 1 1 auto; min-height: 0; overflow-y: auto; overflow-x: hidden; display: flex; flex-direction: column; align-items: center; gap: 2.4vh; padding: 3vh 2vw; }
.bfe-board-sheet-body > :first-child { margin-top: auto; }
.bfe-board-sheet-body > :last-child { margin-bottom: auto; }
.bfe-board-subject { flex: none; display: flex; flex-direction: column; align-items: center; gap: 1.4vh; }
.bfe-board-subject-name { margin: 0; font-size: clamp(20px, 3.3vh, 34px); font-weight: 800; font-stretch: 112%; letter-spacing: .1em; line-height: 1.1; text-transform: uppercase; color: var(--bfe-text);
  text-shadow: 0 2px 14px rgba(0, 0, 0, .5); }
.bfe-board-chips { display: flex; flex-wrap: wrap; justify-content: center; align-items: center; gap: .6em; }
.bfe-board .bfe-chip, .bfe-board-subject .bfe-board-tag { display: inline-flex; align-items: center; gap: .6em; height: 2.4em; padding: 0 .9em; font-size: clamp(10px, 1.3vh, 12px);
  font-weight: 800; letter-spacing: .12em; line-height: 1; }
.bfe-board .bfe-chip { color: var(--bfe-accent); border: 1px solid var(--bfe-accent); }
.bfe-board .bfe-chip b { color: var(--bfe-text); font-size: 1.15em; letter-spacing: .04em; font-variant-numeric: tabular-nums; }
.bfe-board-tag { display: inline-block; padding: .2em .65em; font-size: .72em; font-weight: 800; letter-spacing: .14em; line-height: 1.2; vertical-align: middle;
  text-transform: uppercase; color: var(--team); border: 1px solid var(--team); }
.bfe-board-stats { flex: none; width: min(100%, 980px); margin: 0; display: flex; flex-direction: column; }
.bfe-board-stat { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); column-gap: 3vw; align-items: center; min-height: clamp(30px, 4.2vh, 44px);
  border-bottom: 1px solid var(--bfe-row-line); text-transform: uppercase; }
.bfe-board-stat:first-child { border-top: 1px solid var(--bfe-row-line); }
.bfe-board-stat dt { text-align: right; font-size: clamp(12px, 1.65vh, 17px); font-weight: 700; letter-spacing: .16em; color: var(--bfe-text-dim); }
.bfe-board-stat dd { margin: 0; display: flex; align-items: center; gap: .8em; min-width: 0; font-size: clamp(13px, 1.8vh, 18px); font-weight: 700; letter-spacing: .06em; color: var(--bfe-text); }
.bfe-board-stat .value { white-space: nowrap; }
.bfe-board-stat .value.none { color: var(--bfe-text-faint); }
.bfe-board-sheet.awards .bfe-board-stat { min-height: clamp(34px, 5vh, 50px); }

/* The arrow, while the mouse is captured. */
.bfe-board-pointer { position: absolute; left: 0; top: 0; z-index: 5; display: none; width: 22px; height: 30px; margin: -2px 0 0 -2px; pointer-events: none;
  filter: drop-shadow(0 2px 3px rgba(0, 0, 0, .7)); }
.bfe-board.locked .bfe-board-pointer { display: block; }
.bfe-board-pointer svg { display: block; width: 100%; height: 100%; }
.bfe-board-pointer path { fill: #fff; stroke: #0a0c10; stroke-width: 1.5; stroke-linejoin: round; }

/* The entrance and a page's: the panels rise in, the rows cascade. */
.bfe-board.entering .bfe-text-title span { animation: bfe-title-in .45s cubic-bezier(.2, .8, .2, 1) both; }
.bfe-board.entering .bfe-tabs, .bfe-board.entering .bfe-board-team, .bfe-board.entering .bfe-board-sheet, .bfe-board.turning .bfe-board-sheet,
.bfe-board.turning .bfe-board-team { animation: bfe-board-in .3s cubic-bezier(.2, .8, .2, 1) both; }
.bfe-board.entering .bfe-board-team + .bfe-board-team { animation-delay: .04s; }
.bfe-board.entering .bfe-board-row, .bfe-board.turning .bfe-board-row { animation: bfe-row-in .26s cubic-bezier(.2, .8, .2, 1) both; animation-delay: calc(min(var(--i, 0), 16) * 14ms); }
.bfe-board.entering .bfe-board-stat, .bfe-board.turning .bfe-board-stat { animation: bfe-row-in .26s cubic-bezier(.2, .8, .2, 1) both; animation-delay: calc(.06s + min(var(--i, 0), 10) * 22ms); }
.bfe-board.entering .bfe-screen-foot .bfe-btn { animation: bfe-foot-in .3s cubic-bezier(.2, .8, .2, 1) .06s both; }
@keyframes bfe-board-in { from { opacity: 0; transform: translateY(1.2vh); } }
`;
