import * as THREE from "three";
import { teamStatsLayout, statsPageLayout, rowAt, ICON, PITCH, STRING_PAD, DISPLAY_PAD, AWARDS_ROWS } from "./layout.js";
import { StatsPage } from "./pages.js";
import { nineSlice } from "../../interface/shapes.js";
import { TitleIntro } from "../../interface/flashy.js";
import { InterfaceCamera, dressModel, LOGO_UNITS } from "../../interface/models.js";
import { KILLS, DEATHS, POINTS, PEEK, END, TABLE, DETAILS, AWARDS } from "../model.js";

// The Classic scoreboard, drawn as the 2004 game draws its end-of-match
// table (ifs_teamstats; docs/scoreboard.md)
// and its DETAILS and AWARDS (pages.js), as a layer of the battle HUD
// (src/hud.js setLayer). While it is up the HUD's own displays hide, the
// reinforcements and messages included.
//
// - Under everything, opaque_black over the whole screen (black at alpha
//   119): the world shows through, dimmed. Over it the window
//   (border_3_pieces, nine-sliced).
// - Over the window, the two team logos (the command post holograms) at a
//   fifth of full strength, in relation colours, slowly spinning. Each time
//   the table shows they grow from half size and fade in over 0.4 s; the
//   side DETAILS looked at flies back from the window's corner.
// - Over them, each team's title ("REPUBLIC : 147", its reinforcements
//   left), the KILLS, DEATHS and CP POINTS icons and the scroll arrows
//   (fading in over 0.4 s), and the rows: names and numbers in
//   gamefont_medium at 80% width, bots grey, other humans yellow, the player
//   white. Each text plays the flashy intro after a random 0 to 0.15 s.
//   Tab's table in a network game adds each player's PING (the original's
//   network player list's column, ifs.MPLobby.ping_header) one column
//   before KILLS, the names narrowed to make room; bots have none.
// - The selection's frame (listbox_cursor), its strength pulsing, and the
//   row under the pointer lit (listbox_hilight).
// - DETAILS on its tab under the window, DONE (BACK during play) and
//   AWARDS at the bottom corners.
// - Over everything, the mouse pointer (mousecursor).

const rgba = (r, g, b, a = 255) => [r / 255, g / 255, b / 255, a / 255];
const WHITE = rgba(255, 255, 255);
const BOT = rgba(128, 128, 128);
const HUMAN = rgba(223, 223, 32);
const GREEN = [32 / 255, 223 / 255, 32 / 255];
const RED = [223 / 255, 32 / 255, 32 / 255];
const GREY = [192 / 255, 192 / 255, 192 / 255];
const FONT = "gamefont_medium";
const SMALL = "gamefont_small";
const ROW_SCALE = 0.8; // the rows' width (HScale)
const INTRO_DELAY = 0.15; // s, the most a row's text waits to play its intro
const ENTRY = 0.4; // s: the logos' and buttons' animations (gShellAnimTime)
const LOGO_ALPHA = 0.2;
// Spin, 30 degrees a second per OmegaY: the table's left logo 0.3 and right
// 0.25 at rest, 1 while appearing and 20 while flying; DETAILS' 0.3 once
// landed; AWARDS' icons +-20 flying, then a random -0.45 to 0.45 each.
const OMEGA = THREE.MathUtils.degToRad(30);
const REST = { 1: 0.3, 2: 0.25 };
// The selection frame's strength runs 1 to 0.5 and back, 1 a second; only
// on the right-hand list: the Lua fills the right list last, and that
// resets the pulsing list to it, or to none while the selection is on the
// left (whose frame then stays at full strength).
const PULSE_LOW = 0.5;
// Hovered icons and arrows blink: 0.4 to 1 over 0.4 s, back over 0.2 s
// (IFObj_UpdateBlinkyAnim). Hovered buttons pulse 0.3 to 1, 2 a second.
const BLINK = { low: 0.4, up: 0.4, down: 0.2 };
const BUTTON_LOW = 0.3;
const BUTTON_RATE = 2;
const POINTER = 32;
const HEADERS = [["stats_kills", KILLS], ["stats_deaths", DEATHS], ["stats_cpp", POINTS]];
// Tab's table in a network game: the names keep half the row (of the 0.65
// they have) so that PING fits before KILLS.
const PING_NAME_SHARE = 0.5 / 0.65;
const FALLBACK = { rep: "com_icon_republic", cis: "com_icon_cis", imp: "com_icon_imperial", all: "com_icon_alliance" };
const AWARD_ICON = 0.2; // the award icons' size parameter, landed

const tint = (color, a) => [color[0], color[1], color[2], (color[3] ?? 1) * a];
const translate = (x, y) => new THREE.Matrix4().makeTranslation(x, y, 0);
const lerp = (a, b, t) => a + (b - a) * t;

// A hovered element's blink at time t since it was first hovered.
function blink(t) {
  const period = BLINK.up + BLINK.down;
  const u = t % period;
  if (u < BLINK.up) return BLINK.low + (1 - BLINK.low) * (u / BLINK.up);
  return 1 - (1 - BLINK.low) * ((u - BLINK.up) / BLINK.down);
}

// A triangle wave between low and 1, starting at 1, at `rate` a second.
function pulse(t, low, rate) {
  const span = 1 - low;
  const u = (t * rate) % (2 * span);
  return u < span ? 1 - u : low + (u - span);
}

// A model's place, size parameter and strength, moving linearly from one
// state to another over `length` seconds, spinning at `omega` (OmegaY)
// until it lands and `rest` after (the interface's AnimationMgr).
class Flight {
  constructor() {
    this.from = this.to = { x: 0, y: 0, size: 1, alpha: 0 };
    this.time = 0;
    this.length = 0;
    this.omega = 0;
    this.rest = 0;
    this.spin = 0;
  }

  start(from, to, { length = ENTRY, omega = 1, rest = 0 } = {}) {
    this.from = from;
    this.to = to;
    this.time = 0;
    this.length = length;
    this.omega = omega;
    this.rest = rest;
  }

  get flying() {
    return this.time < this.length;
  }

  update(dt) {
    this.spin += (this.flying ? this.omega : this.rest) * OMEGA * dt;
    this.time = Math.min(this.time + dt, this.length);
  }

  get state() {
    const t = this.length > 0 ? this.time / this.length : 1;
    const { from, to } = this;
    return { x: lerp(from.x, to.x, t), y: lerp(from.y, to.y, t), size: lerp(from.size, to.size, t), alpha: lerp(from.alpha, to.alpha, t) };
  }
}

export class ClassicView {
  // hud: the BattleHud; port: the scoreboard port; model: the
  // ScoreboardModel; common: common.json (the logos' records); input: for
  // the pointer.
  constructor({ hud, port, model, common = null, input = null }) {
    this.hud = hud;
    this.port = port;
    this.model = model;
    this.common = common;
    this.input = input;
    this.time = 0;
    this.entered = 0; // the table's time since it last showed
    this.intros = new Map(); // "side:row:column" -> TitleIntro
    this.hovered = null; // the hotspot under the pointer, by key
    this.hoveredAt = 0;
    this.interface = new InterfaceCamera();
    this.models = new THREE.Scene();
    this.logos = new Map(); // team -> { name, root, materials, flight }
    this.icons = []; // AWARDS': { team, name, root, materials, flight }
    this.pages = { [DETAILS]: new StatsPage(this, "details"), [AWARDS]: new StatsPage(this, "awards") };
    this.hidePointer = false;
    this.layer = {
      teams: false,
      draw: (hud) => this.drawUnder(hud),
      renderAfter: (renderer) => this.renderModels(renderer),
      drawOver: (hud, batch) => this.drawOver(hud, batch),
    };
  }

  get layout() {
    const H = this.hud.height;
    if (this.cached?.H !== H) this.cached = teamStatsLayout(H);
    return this.cached;
  }

  text(key, fallback) {
    return this.hud.strings?.[key] || fallback;
  }

  texture(name) {
    return this.hud.texture(name);
  }

  get logoSize() {
    return this.layout.logoSize;
  }

  // --- what the board reacts to ----------------------------------------

  // The board opened, on the table.
  open() {
    this.model.shown = this.layout.shown;
    this.ensureLogos();
    this.time = 0;
    this.enterTable(null);
  }

  close() {}

  // The table shows (again): its rows' intros replay, the header icons and
  // arrows fade in, the logos grow from half size and fade in where they
  // are, the side DETAILS looked at (returning, a side) flying back from
  // the window's corner at full speed.
  enterTable(returning) {
    this.entered = 0;
    this.intros.clear();
    for (const side of [1, 2]) {
      for (let k = 0; k < this.layout.shown; k++) {
        // The name, KILLS, DEATHS, CP POINTS, and a player's PING.
        for (let column = 0; column < 5; column++) {
          const intro = new TitleIntro();
          intro.start(Math.random() * INTRO_DELAY);
          this.intros.set(`${side}:${k}:${column}`, intro);
        }
      }
    }
    const layout = this.layout;
    const full = this.logoSize;
    for (const [team, logo] of this.logos) {
      const at = layout.sides[team].logo;
      const back = team === returning;
      const from = back
        ? { x: at.x + (team === 1 ? -1 : 1) * 0.15 * layout.safe.w, y: at.y - 0.3 * layout.safe.h, size: full / 2, alpha: 0.4 }
        : { x: at.x, y: at.y, size: full / 2, alpha: 0 };
      logo.flight.start(from, { x: at.x, y: at.y, size: full, alpha: LOGO_ALPHA }, { omega: back ? 20 : 1, rest: REST[team] });
    }
  }

  // DETAILS or AWARDS shows, or the table again.
  pageChanged({ page, from, side }) {
    if (page === TABLE) {
      this.enterTable(from === DETAILS ? side : null);
      return;
    }
    if (page === DETAILS) {
      this.pages[DETAILS].open(this.port.personal?.(this.model.subject?.slot) || []);
      // The team's logo flies from its side to the window's corner.
      const logo = this.logos.get(this.model.subject?.side);
      if (logo) {
        const team = this.model.subject.side;
        const at = this.layout.sides[team].logo;
        const corner = statsPageLayout(this.hud.height).corner(team);
        logo.flight.start({ x: at.x, y: at.y, size: this.logoSize, alpha: LOGO_ALPHA }, { ...corner, size: this.logoSize / 2, alpha: 0.4 }, { omega: 20, rest: 0.3 });
      }
      return;
    }
    if (page === AWARDS) {
      const awards = this.port.awards?.() || [];
      this.pages[AWARDS].open(awards.map((award) => ({ key: award.key, label: award.label, value: award.text })));
      this.ensureIcons(awards);
    }
  }

  update(dt) {
    this.time += dt;
    this.entered += dt;
    this.model.shown = this.layout.shown;
    for (const intro of this.intros.values()) intro.update(dt);
    for (const logo of this.logos.values()) logo.flight.update(dt);
    // The award icons' spin settles once the first has landed.
    const landed = this.icons[0] && !this.icons[0].flight.flying;
    for (const icon of this.icons) {
      icon.flight.update(dt);
      if (landed && icon.flight.rest === null) icon.flight.rest = Math.random() * 0.9 - 0.45;
    }
    const page = this.pages[this.model.page];
    if (page) {
      if (this.model.page === DETAILS) page.refresh(this.port.personal?.(this.model.subject?.slot) || page.content);
      page.update(dt);
    }
    // What the pointer is over, for the blinks.
    const cursor = this.input?.cursor;
    const target = cursor ? this.hit(cursor.x, cursor.y) : null;
    const key = target ? `${target.kind}:${target.mode ?? target.step ?? ""}:${target.side ?? ""}` : null;
    if (key !== this.hovered) {
      this.hovered = key;
      this.hoveredAt = this.time;
    }
  }

  hoverAlpha(key, kind = "blink") {
    if (key !== this.hovered) return 1;
    const t = this.time - this.hoveredAt;
    return kind === "button" ? pulse(t, BUTTON_LOW, BUTTON_RATE) : blink(t);
  }

  // --- drawing ----------------------------------------------------------

  frame(hud, x = 0, y = 0) {
    return hud.anchor(0, 0, false).multiply(translate(x, y));
  }

  // A nine-sliced window of `name` centred on (x, y).
  window(batch, m, x, y, w, h, name, color = WHITE) {
    const texture = this.texture(name);
    if (!texture) {
      batch.quad(m, x - w / 2 + 4, y - h / 2 + 4, x + w / 2 - 4, y + h / 2 - 4, this.hud.solid, [0, 0, 1, 1], tint([0, 0.17, 0.38, 1], 0.7 * color[3]));
      return;
    }
    for (const [x0, y0, x1, y1, u0, v0, u1, v1] of nineSlice(w, h)) batch.quad(m, x + x0, y + y0, x + x1, y + y1, texture, [u0, v0, u1, v1], color);
  }

  // Under the logos: the dimmed world and the window.
  drawUnder(hud) {
    const layout = this.layout;
    const m = this.frame(hud);
    const black = this.texture("opaque_black");
    if (black) hud.batch.quad(m, 0, 0, 1024, hud.height, black, [0, 0, 1, 1], WHITE);
    else hud.batch.quad(m, 0, 0, 1024, hud.height, hud.solid, [0, 0, 1, 1], rgba(0, 0, 0, 119));
    const box = layout.window;
    this.window(hud.batch, m, box.x, box.y, box.w, box.h, "border_3_pieces");
  }

  // Over the logos: everything else.
  drawOver(hud, batch) {
    const layout = this.layout;
    const m = this.frame(hud);
    const page = this.pages[this.model.page];
    if (page) {
      page.draw(hud, batch, m);
      this.drawBack(hud, batch, m, layout);
    } else {
      for (const side of [1, 2]) this.drawSide(hud, batch, m, side, layout);
      this.drawButtons(hud, batch, m, layout);
    }
    this.drawPointer(hud, batch, m);
  }

  drawSide(hud, batch, m, side, layout) {
    const model = this.model;
    const list = layout.sides[side];
    const fonts = hud.fonts;
    // The title: the team's name and its reinforcements left.
    const team = model.titles[side];
    if (team) {
      const title = `${String(team.name).toUpperCase()} : ${team.reinforcements}`;
      fonts.draw(batch, m, list.title.x, list.title.y, title, FONT, WHITE);
    }
    // The column icons, which sort, and the arrows, while there is more to
    // scroll to that way: both fade in as the table shows.
    const shown = Math.min(this.entered / ENTRY, 1);
    HEADERS.forEach(([name, mode], j) => {
      const at = list.headers[j];
      const a = this.hoverAlpha(`sort:${mode}:${side}`) * shown;
      const texture = this.texture(name);
      if (texture) batch.quad(m, at.x, at.y, at.x + ICON, at.y + ICON, texture, [0, 0, 1, 1], tint(WHITE, a));
    });
    // PING: one column before KILLS, its label level with the icons.
    const ping = this.pingColumn;
    const pingX = list.columns[0] - (list.columns[1] - list.columns[0]);
    if (ping) {
      const y = list.headers[0].y + ICON / 2 - fonts.lineHeight(FONT) / 2;
      this.drawText(batch, m, pingX, y, this.text("ifs.MPLobby.ping_header", "PING"), WHITE, 1, null, { alpha: shown });
    }
    if (model.canScrollUp) this.drawArrow(batch, m, list.up, -1, side, shown);
    if (model.canScrollDown) this.drawArrow(batch, m, list.down, 1, side, shown);
    // The frames: the row under the pointer, then the selection.
    const hover = model.hover;
    if (hover && hover.side === side && !(hover.side === model.side && hover.index === model.selected)) {
      const k = hover.index - model.first;
      if (k >= 0 && k < layout.shown && model.row(side, hover.index)) this.drawCursor(batch, m, list, layout, k, "listbox_hilight", 1);
    }
    if (model.side === side) {
      const k = model.selected - model.first;
      const a = side === 2 ? pulse(this.time, PULSE_LOW, 1) : 1;
      if (k >= 0 && k < layout.shown && model.row(side, model.selected)) this.drawCursor(batch, m, list, layout, k, "listbox_cursor", a);
    }
    // The rows.
    model.visible(side).forEach(({ row }, k) => {
      if (!row) return;
      const color = row.local ? WHITE : row.human ? HUMAN : BOT;
      const y = layout.top + k * PITCH + layout.textTop;
      const name = this.clip(fonts, String(row.name).toUpperCase(), ping ? list.name.w * PING_NAME_SHARE : list.name.w);
      this.drawText(batch, m, list.name.x, y, name, color, 0, this.intros.get(`${side}:${k}:0`));
      [row.kills, row.deaths, row.points].forEach((value, j) => {
        this.drawText(batch, m, list.columns[j], y, String(value), color, 1, this.intros.get(`${side}:${k}:${j + 1}`));
      });
      if (ping && Number.isFinite(row.ping)) this.drawText(batch, m, pingX, y, String(row.ping), color, 1, this.intros.get(`${side}:${k}:4`));
    });
  }

  // Whether the table shows PING: Tab's, in a network game.
  get pingColumn() {
    return Boolean(this.port.network) && this.model.state === PEEK;
  }

  // small_arrow turned to point up (step -1) or down: its texture points
  // right, and the Lua lays its corners out diagonally, the texture's top
  // edge (its lit side) to the right.
  drawArrow(batch, m, at, step, side, shown = 1) {
    const texture = this.texture("small_arrow");
    const a = this.hoverAlpha(`scroll:${step}:${side}`) * shown;
    const h = ICON / 2;
    const up = [[1, 1], [0, 1], [1, 0], [0, 0]];
    const down = [[0, 1], [1, 1], [0, 0], [1, 0]];
    if (texture) batch.quadCorners(m, at.x - h, at.y - h, at.x + h, at.y + h, texture, step < 0 ? up : down, tint(WHITE, a));
  }

  // A list's frame around row k: the row's width, centred on the list.
  drawCursor(batch, m, list, layout, k, name, a) {
    const y = layout.top + k * PITCH + layout.cursor.dy;
    this.window(batch, m, list.x, y, layout.cursor.w, layout.cursor.h, name, tint(WHITE, a));
  }

  // Text cut to fit `width` by whole characters.
  clip(fonts, text, width) {
    let out = text;
    while (out && fonts.width(FONT, out, ROW_SCALE) > width) out = out.slice(0, -1);
    return out;
  }

  // A text with its line's top at y, playing its intro: the white block
  // sweeping across, then the text growing from its middle and blinking in.
  // align: 0 from x, 1 ending at x, 2 centred on x. scale: its width; the
  // glyphs narrow but the space between them doesn't (the font drawn at the
  // scale, then stretched back to full height), as the retail captures
  // measure the table's 80%.
  drawText(batch, m, x, y, text, color, align, intro, { font = FONT, scale = ROW_SCALE, alpha = 1 } = {}) {
    if (!text) return;
    const fonts = this.hud.fonts;
    const fh = fonts.lineHeight(font);
    const width = fonts.width(font, text, scale);
    const state = intro?.frame ?? { block: null, text: 1, alpha: 1 };
    const left = align === 1 ? x - width : align === 2 ? x - width / 2 : x;
    if (state.block) {
      const bx = left + state.block.x * Math.max(width - fh * scale, 0);
      batch.quad(m, bx, y, bx + fh * scale, y + fh, this.texture("cursor") || this.hud.solid, [0, 0, 1, 1], tint(WHITE, state.block.alpha * alpha));
    }
    if (state.text <= 0.01 || state.alpha <= 0) return;
    const mid = y + fh / 2;
    const at = m.clone().multiply(new THREE.Matrix4().makeTranslation(x, mid, 0))
      .multiply(new THREE.Matrix4().makeScale(1, state.text / scale, 1)).multiply(new THREE.Matrix4().makeTranslation(0, (-fh / 2) * scale, 0));
    fonts.draw(batch, at, 0, 0, text, font, tint(color, state.alpha * alpha), align, scale);
  }

  // The table's buttons: DONE (BACK during play) and AWARDS at the end,
  // DETAILS on its tab.
  drawButtons(hud, batch, m, layout) {
    const fonts = hud.fonts;
    const end = this.model.state === END;
    const small = fonts.lineHeight(SMALL);
    const done = this.buttonBox("done", layout);
    if (!end || this.port.canRestart?.() !== false)
      fonts.draw(batch, m, done.x0, done.top, done.text, FONT, tint(WHITE, this.hoverAlpha(`${done.kind}::`, "button")));
    if (end) {
      const awards = this.buttonBox("awards", layout);
      fonts.draw(batch, m, awards.x0, awards.top, awards.text, FONT, tint(WHITE, this.hoverAlpha("awards::", "button")));
    }
    // DETAILS: its label centred under the window on the back button's
    // tab, which runs from 22 units before the text to 20 after it, the
    // ends square.
    const details = this.buttonBox("details", layout);
    const x0 = details.x0 - 22;
    const x1 = details.x0 + details.width + 20;
    const y0 = details.top;
    const y1 = details.top + small;
    const pieces = [[x0, x0 + small, "backbutton_left"], [x0 + small, x1 - small, "backbutton_mid"], [x1 - small, x1, "backbutton_right"]];
    for (const [a, b, name] of pieces) {
      const texture = this.texture(name);
      if (texture) batch.quad(m, a, y0, b, y1, texture, [0, 0, 1, 1], WHITE);
      else batch.quad(m, a, y0, b, y1, hud.solid, [0, 0, 1, 1], rgba(16, 40, 80, 220));
    }
    fonts.draw(batch, m, details.x0, details.top, details.text, SMALL, tint(WHITE, this.hoverAlpha("details::", "button")));
  }

  // DETAILS' and AWARDS' BACK, where the table's DONE is.
  drawBack(hud, batch, m, layout) {
    const back = this.buttonBox("back", layout);
    hud.fonts.draw(batch, m, back.x0, back.top, back.text, FONT, tint(WHITE, this.hoverAlpha("back::", "button")));
  }

  // A button's label and hotspot: { kind, text, x0, top, width, height }.
  buttonBox(kind, layout = this.layout) {
    const fonts = this.hud.fonts;
    if (kind === "details") {
      const text = this.text("ifs.Stats.details", "DETAILS");
      const width = fonts.width(SMALL, text);
      const h = fonts.lineHeight(SMALL);
      return { kind, text, x0: layout.details.x - (width + DISPLAY_PAD) / 2, top: layout.details.y - h / 2, width, height: h };
    }
    const h = fonts.lineHeight(FONT);
    if (kind === "awards") {
      const text = this.text("ifs.Stats.awards", "AWARDS");
      const width = fonts.width(FONT, text);
      return { kind, text, x0: layout.awards.right - width - STRING_PAD, top: layout.awards.y - h / 2, width, height: h };
    }
    // Bottom left: DONE on the end table; BACK during play, and on DETAILS
    // and AWARDS.
    const done = kind === "done" && this.model.state === END && this.model.page === TABLE;
    const text = done ? this.text("ifs.stats.done", "DONE") : this.text("common.back", "BACK");
    return { kind: done ? "done" : "back", text, x0: layout.done.x, top: layout.done.y - h / 2, width: fonts.width(FONT, text), height: h };
  }

  // The pointer, over everything, while the mouse is the board's.
  drawPointer(hud, batch, m) {
    const cursor = this.input?.cursor;
    if (!cursor || this.hidePointer) return;
    const texture = this.texture("mousecursor");
    if (texture) batch.quad(m, cursor.x, cursor.y, cursor.x + POINTER, cursor.y + POINTER, texture, [0, 0, 1, 1], WHITE);
    else batch.quad(m, cursor.x, cursor.y, cursor.x + 8, cursor.y + 8, hud.solid, [0, 0, 1, 1], WHITE);
  }

  // --- the logos --------------------------------------------------------

  records(name) {
    return this.common?.holograms?.[name] || null;
  }

  // A fresh copy of a logo model, dressed in the interface's material.
  logo(name) {
    const root = this.port.commonModel?.(name);
    if (!root) return null;
    const texture = (part) => this.texture(part);
    const materials = dressModel(root, this.records(name) || [{ texture: "com_icon_controlzone", scroll: [0, 0.9921875] }], texture, { lit: true });
    this.models.add(root);
    return { name, root, materials, flight: new Flight() };
  }

  dropModel(entry) {
    if (!entry) return;
    this.models.remove(entry.root);
    for (const material of entry.materials) material.dispose();
  }

  ensureLogos() {
    const teams = this.port.teams() || {};
    for (const team of [1, 2]) {
      const name = teams[team]?.logo || FALLBACK[teams[team]?.side];
      if (!name || this.logos.get(team)?.name === name) continue;
      this.dropModel(this.logos.get(team));
      const logo = this.logo(name);
      if (logo) this.logos.set(team, logo);
      else this.logos.delete(team);
    }
  }

  // AWARDS' icons: each winner's team's logo, flying from that team's side
  // of the table to its row, spinning at +-600 degrees a second (a random
  // way) until the first lands.
  ensureIcons(awards) {
    for (const icon of this.icons) this.dropModel(icon);
    this.icons = [];
    const teams = this.port.teams() || {};
    const layout = statsPageLayout(this.hud.height, AWARDS_ROWS);
    const table = this.layout;
    awards.forEach((award, k) => {
      const team = award.team;
      const name = teams[team]?.logo || FALLBACK[teams[team]?.side];
      if (!name) return; // no winner, or locals (no logo)
      const icon = this.logo(name);
      if (!icon) return;
      icon.team = team;
      const start = table.sides[team === 2 ? 2 : 1].logo;
      const at = layout.icons[k];
      icon.flight.start({ x: start.x, y: start.y, size: this.logoSize, alpha: 0.2 }, { x: at.x, y: at.y, size: AWARD_ICON, alpha: 0.3 },
        { omega: Math.random() < 0.5 ? -20 : 20, rest: null });
      this.icons.push(icon);
    });
  }

  colour(team) {
    const own = this.model.team;
    const relation = this.port.relation?.(own, team) ?? (team === own ? 1 : -1);
    return relation > 0 ? GREEN : relation < 0 ? RED : GREY;
  }

  // Draw one model at its flight's state.
  place(entry, team) {
    const { x, y, size, alpha } = entry.flight.state;
    this.interface.place(x, y, entry.root.position);
    entry.root.scale.setScalar(Math.max(size, 1e-3) * LOGO_UNITS);
    entry.root.rotation.y = entry.flight.spin;
    entry.root.visible = alpha > 0.001;
    for (const material of entry.materials) {
      material.uniforms.color.value.setRGB(...this.colour(team));
      material.uniforms.opacity.value = alpha;
      material.uniforms.time.value = this.time;
    }
  }

  // The table's logos, DETAILS' one, or AWARDS' icons, over the window;
  // none under the pause menu, which hides the board.
  renderModels(renderer) {
    if (this.hud.covered) return;
    const canvas = renderer.domElement;
    this.interface.setView(canvas.clientWidth / Math.max(canvas.clientHeight, 1), this.hud.height);
    const page = this.model.page;
    for (const [team, logo] of this.logos) {
      logo.root.visible = page === TABLE || (page === DETAILS && team === this.model.subject?.side);
      if (logo.root.visible) this.place(logo, team);
    }
    for (const icon of this.icons) {
      icon.root.visible = page === AWARDS;
      if (icon.root.visible) this.place(icon, icon.team);
    }
    renderer.clearDepth();
    renderer.render(this.models, this.interface.camera);
  }

  // --- what a click lands on --------------------------------------------

  // An interface point's target: a column's icon (sort), an arrow (scroll),
  // a row, or a button; on DETAILS and AWARDS only BACK. Null for nothing.
  hit(x, y) {
    const layout = this.layout;
    const model = this.model;
    const inside = (x0, y0, x1, y1) => x >= x0 && x <= x1 && y >= y0 && y <= y1;
    const button = (kind) => {
      const box = this.buttonBox(kind, layout);
      return inside(box.x0, box.top, box.x0 + box.width + (kind === "details" ? 0 : 5), box.top + box.height) ? { kind: box.kind } : null;
    };
    if (model.page !== TABLE) return button("back");
    for (const side of [1, 2]) {
      const list = layout.sides[side];
      for (let j = 0; j < HEADERS.length; j++) {
        const at = list.headers[j];
        if (inside(at.x, at.y, at.x + ICON, at.y + ICON)) return { kind: "sort", mode: HEADERS[j][1], side };
      }
      const h = ICON / 2;
      if (model.canScrollUp && inside(list.up.x - h, list.up.y - h, list.up.x + h, list.up.y + h)) return { kind: "scroll", step: -1, side };
      if (model.canScrollDown && inside(list.down.x - h, list.down.y - h, list.down.x + h, list.down.y + h)) return { kind: "scroll", step: 1, side };
      const k = rowAt(layout, side, x, y);
      if (k >= 0 && model.row(side, model.first + k)) return { kind: "row", side, index: model.first + k };
    }
    for (const kind of model.state === END ? ["done", "awards", "details"] : ["done", "details"]) {
      if (kind === "done" && model.state === END && this.port.canRestart?.() === false) continue;
      const found = button(kind);
      if (found) return found;
    }
    return null;
  }

  dispose() {
    for (const logo of this.logos.values()) this.dropModel(logo);
    for (const icon of this.icons) this.dropModel(icon);
  }
}
