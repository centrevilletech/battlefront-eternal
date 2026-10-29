// The scoreboard's state, apart from the world and the screen: both teams'
// rows as the world last gave them, their order, the selection and the
// scroll, and where the board is in its life. The 2004 game's end-of-match
// table (its ifs_teamstats; docs/scoreboard.md) keeps the same.
//
// - closed: nothing shows.
// - peek: Tab's table during play, offline and online alike. This is the
//   port's own: the original honours Tab only in network games, where it
//   lists the human players (NAME, TEAM, PING, KILLS). Here Tab shows the
//   end table's rows instead, bots included, live, and in a network game
//   each player's PING beside them (the view's).
// - end: the end-of-match table, until DONE, Esc, or 30 s without input.
//
// While open it shows one page: the table; DETAILS, the record of the
// row selected (ifs_personalstats); or AWARDS, at the end only
// (ifs_awardstats). BACK or Esc on DETAILS or AWARDS comes back to the
// table, its selection kept.
//
// Pure: the rows come from the port (roster, teams), and nothing here
// touches the page.
//
// Lists are by team: team 1's on the left, team 2's on the right. Each
// lists every Character of its team; the shorter is padded with blank rows
// so the two scroll together, one row index for both. Indexes here are
// 0-based (the Lua's are 1-based).

export const CLOSED = "closed";
export const PEEK = "peek";
export const END = "end";
export const TABLE = "table";
export const DETAILS = "details";
export const AWARDS = "awards";

// Sort modes, as ScriptCB_SetTeamStatsSortMode numbers them.
export const KILLS = 0;
export const DEATHS = 1;
export const POINTS = 2;
export const NAME = 3;
export const IDLE_QUIT = 30; // s without input before the end table quits
const IDLE_STEP = 0.5; // the most a frame takes off the idle clock
// Rows above the player's when the table opens: the Lua's FirstShownIdx is
// max(row - 4, 1), 1-based, with his row 0-based.
const ABOVE_PLAYER = 5;

// Each key and its direction: kills and points most first, deaths fewest
// first, names by UTF-16 code unit from the last (Z before A: the
// original game negates a compare that already runs backwards).
const KEYS = {
  [KILLS]: (a, b) => b.kills - a.kills,
  [DEATHS]: (a, b) => a.deaths - b.deaths,
  [POINTS]: (a, b) => b.points - a.points,
  [NAME]: (a, b) => (a.name < b.name ? 1 : a.name > b.name ? -1 : 0),
};
// The tie-breaks after each mode's own key.
const ORDER = {
  [KILLS]: [KILLS, DEATHS, POINTS, NAME],
  [DEATHS]: [DEATHS, KILLS, POINTS, NAME],
  [POINTS]: [POINTS, KILLS, DEATHS, NAME],
  [NAME]: [NAME, KILLS, DEATHS, POINTS],
};

// The comparison for a mode; reverse turns only its first key around.
// Rows the same in every key keep roster order (the original game's qsort
// leaves them in no particular order).
export function compareRows(mode = KILLS, reverse = false) {
  const keys = ORDER[mode] || ORDER[KILLS];
  return (a, b) => {
    for (let i = 0; i < keys.length; i++) {
      const order = KEYS[keys[i]](a, b);
      if (order) return i === 0 && reverse ? -order : order;
    }
    return (a.slot ?? 0) - (b.slot ?? 0);
  };
}

export class ScoreboardModel {
  // port: the world's scoreboard port (src/scoreboard/port.js).
  constructor({ port } = {}) {
    this.port = port;
    this.state = CLOSED;
    this.mode = KILLS; // kept for the whole session, as the original's is
    this.reverse = false;
    this.lists = { 1: [], 2: [] };
    this.titles = { 1: null, 2: null };
    this.team = 1; // the player's
    this.side = 1; // the list the selection is in
    this.selected = 0;
    this.hover = null; // { side, index } under the mouse
    this.first = 0; // the first row shown
    this.shown = 16; // rows a list shows (the view's layout sets it)
    this.idle = IDLE_QUIT;
    this.page = TABLE;
    this.subject = null; // DETAILS' row: { side, slot, name }
    this.listeners = new Map();
  }

  get isOpen() {
    return this.state !== CLOSED;
  }

  // Rows in the longer list: the lists scroll as one.
  get length() {
    return Math.max(this.lists[1].length, this.lists[2].length);
  }

  // Listeners by event: open ({ state }), close ({ state }), sort ({ mode,
  // reverse }), select ({ side, index }), scroll ({ first }), page ({ page,
  // from, side }), quit.
  on(event, listener) {
    if (!this.listeners.has(event)) this.listeners.set(event, new Set());
    this.listeners.get(event).add(listener);
    return () => this.listeners.get(event)?.delete(listener);
  }

  emit(event, detail) {
    for (const listener of this.listeners.get(event) || []) listener(detail);
  }

  // The board opens as `state` (peek or end): the rows are read, and the
  // selection goes to the player's row on his team's side, scrolled so five
  // rows show above it.
  open(state = PEEK) {
    if (state !== PEEK && state !== END) return false;
    const was = this.state;
    this.state = state;
    this.idle = IDLE_QUIT;
    this.hover = null;
    this.page = TABLE;
    this.subject = null;
    this.refresh();
    const human = this.findHuman();
    this.side = human?.side ?? 1;
    this.selected = human?.index ?? 0;
    this.first = Math.max(this.selected - ABOVE_PLAYER, 0);
    this.autoscroll();
    if (was !== state) this.emit("open", { state, from: was });
    return true;
  }

  close() {
    if (this.state === CLOSED) return;
    const state = this.state;
    this.state = CLOSED;
    this.hover = null;
    this.emit("close", { state });
  }

  // Both teams' rows from the port, each list in the current order.
  refresh() {
    const rows = this.port.roster() || [];
    this.team = this.port.humanTeam?.() ?? 1;
    const compare = compareRows(this.mode, this.reverse);
    for (const team of [1, 2]) this.lists[team] = rows.filter((row) => row.team === team).sort(compare);
    const teams = this.port.teams?.() || {};
    for (const team of [1, 2]) this.titles[team] = teams[team] || null;
    this.clamp();
  }

  findHuman() {
    for (const side of [1, 2]) {
      const index = this.lists[side].findIndex((row) => row.local);
      if (index >= 0) return { side, index };
    }
    return null;
  }

  // The row at an index of a list, or null (a blank or past the end).
  row(side, index) {
    return this.lists[side]?.[index] ?? null;
  }

  // A header clicked: that order, most first; the same header again turns
  // its key around, and a third time back.
  sortBy(mode) {
    if (!(mode in ORDER) || this.page !== TABLE) return false;
    if (mode === this.mode) this.reverse = !this.reverse;
    else {
      this.mode = mode;
      this.reverse = false;
    }
    this.idle = IDLE_QUIT;
    this.refresh();
    this.emit("sort", { mode: this.mode, reverse: this.reverse });
    return true;
  }

  // The selection on a list, held to that list's own rows.
  clamp() {
    const count = this.lists[this.side].length;
    this.selected = Math.min(Math.max(this.selected, 0), Math.max(count - 1, 0));
    this.first = Math.min(Math.max(this.first, 0), Math.max(this.length - this.shown, 0));
  }

  // Keep the selection in view (ListManager's autoscroll).
  autoscroll() {
    this.clamp();
    if (this.first > this.selected) this.first = this.selected;
    if (this.first + this.shown <= this.selected) this.first = Math.max(this.selected + 1 - this.shown, 0);
    this.clamp();
  }

  select(side, index) {
    if (this.page !== TABLE || !this.row(side, index)) return false;
    const changed = side !== this.side || index !== this.selected;
    this.side = side;
    this.selected = index;
    this.idle = IDLE_QUIT;
    this.autoscroll();
    if (changed) this.emit("select", { side, index });
    return changed;
  }

  // Up and down: one row within the list.
  move(step) {
    return this.select(this.side, Math.min(Math.max(this.selected + step, 0), this.lists[this.side].length - 1));
  }

  // A page up or down, as ListManager pages: to the top or bottom of the
  // rows in view first, then a whole page.
  turnPage(step) {
    const top = this.selected - this.first;
    const last = this.lists[this.side].length - 1;
    let index;
    if (step < 0) index = top > 0 ? this.first : Math.max(this.selected - this.shown, 0);
    else index = top < this.shown - 1 ? Math.min(this.first + this.shown - 1, last) : Math.min(this.selected + this.shown, last);
    return this.select(this.side, index);
  }

  // Left or right: the same row on the other team's list, or its last.
  flip() {
    const side = this.side === 1 ? 2 : 1;
    const count = this.lists[side].length;
    if (!count || this.page !== TABLE) return false;
    this.side = side;
    this.selected = Math.min(this.selected, count - 1);
    this.idle = IDLE_QUIT;
    this.autoscroll();
    this.emit("select", { side, index: this.selected });
    return true;
  }

  // An arrow: the view one row up or down, the selection kept in it.
  scroll(step) {
    const first = Math.min(Math.max(this.first + step, 0), Math.max(this.length - this.shown, 0));
    if (first === this.first || this.page !== TABLE) return false;
    this.first = first;
    if (this.selected < first) this.selected = first;
    if (this.selected >= first + this.shown) this.selected = first + this.shown - 1;
    this.selected = Math.min(this.selected, Math.max(this.lists[this.side].length - 1, 0));
    this.idle = IDLE_QUIT;
    this.emit("scroll", { first });
    return true;
  }

  // The rows a list shows ({ index, row } with row null for a blank).
  visible(side) {
    const out = [];
    for (let i = 0; i < this.shown; i++) out.push({ index: this.first + i, row: this.row(side, this.first + i) });
    return out;
  }

  // Up arrow while rows are above the view; down while rows are below it.
  get canScrollUp() {
    return this.first > 0;
  }

  get canScrollDown() {
    return this.first + this.shown < this.length;
  }

  // Any input keeps the end table up.
  touch() {
    this.idle = IDLE_QUIT;
  }

  // DETAILS: the selected row's record.
  showDetails() {
    const row = this.row(this.side, this.selected);
    if (!this.isOpen || this.page !== TABLE || !row) return false;
    this.page = DETAILS;
    this.subject = { side: this.side, slot: row.slot, name: row.name };
    this.idle = IDLE_QUIT;
    this.emit("page", { page: DETAILS, from: TABLE, side: this.side });
    return true;
  }

  // AWARDS, once the battle is over.
  showAwards() {
    if (this.state !== END || this.page !== TABLE) return false;
    this.page = AWARDS;
    this.idle = IDLE_QUIT;
    this.emit("page", { page: AWARDS, from: TABLE, side: null });
    return true;
  }

  // Back from DETAILS or AWARDS to the table.
  back() {
    if (!this.isOpen || this.page === TABLE) return false;
    const from = this.page;
    const side = this.subject?.side ?? null;
    this.page = TABLE;
    this.idle = IDLE_QUIT;
    this.refresh();
    this.emit("page", { page: TABLE, from, side: from === DETAILS ? side : null });
    return true;
  }

  // One frame (dt in seconds): the end table's idle clock, which takes at
  // most half a second a frame; at 0 the table quits. Returns true then.
  update(dt) {
    if (this.state !== END) return false;
    this.idle -= Math.min(Math.max(dt, 0), IDLE_STEP);
    if (this.idle >= 0) return false;
    this.idle = IDLE_QUIT;
    this.emit("quit", { reason: "idle" });
    return true;
  }
}
