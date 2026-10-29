import { statsPageLayout, DETAILS_ROWS, AWARDS_ROWS } from "./layout.js";
import { TitleIntro } from "../../interface/flashy.js";

// DETAILS and AWARDS as the 2004 game draws them (ifs_personalstats and
// ifs_awardstats; docs/scoreboard.md), over the table's dimming and window:
//
// - DETAILS: "PERSONAL STATS" and the row's name centred at the top, then
//   HEAD SHOTS, FAVORITE CLASS, FAVORITE WEAPON, FAVORITE VEHICLE, LONGEST
//   LIVING STREAK, NARROW ESCAPES, VEHICLES DESTROYED, NEMESIS and BAIT in
//   gamefont_small, each label right-aligned before the middle and its
//   value left-aligned after it, below three blank rows. The team's logo
//   flies to the window's top corner on its side.
// - AWARDS: "PLAYER AWARDS", then TANK BUSTER, KILLING SPREE, CAMPER,
//   SURVIVALIST, TRAITOR, PUBLIC ENEMY, BANTHA FODDER and DEAD EYE in
//   gamefont_medium, below two blank rows, each winner as "NAME (n)"; a
//   small logo of the winner's team between label and value.
// Both titles fade in over 0.4 s; every text plays the flashy intro, the
// rows after a random 0 to 0.25 s. BACK (or Esc) goes back to the table.

const WHITE = [1, 1, 1, 1];
const MEDIUM = "gamefont_medium";
const TITLE_FADE = 0.4; // s
const ROW_DELAY = 0.25; // s, the most a row's text waits for its intro

const tint = (color, a) => [color[0], color[1], color[2], (color[3] ?? 1) * a];

export class StatsPage {
  // view: the ClassicView (its HUD, fonts and text drawing); kind:
  // "details" or "awards".
  constructor(view, kind) {
    this.view = view;
    this.kind = kind;
    this.rows = kind === "details" ? DETAILS_ROWS : AWARDS_ROWS;
    this.time = 0;
    this.intros = new Map();
    this.content = [];
  }

  get layout() {
    const H = this.view.hud.height;
    if (this.cached?.H !== H) this.cached = statsPageLayout(H, this.rows);
    return this.cached;
  }

  // The page opened: its texts play their intros.
  open(content) {
    this.time = 0;
    this.content = content;
    this.intros.clear();
    const intro = (key, delay) => {
      const t = new TitleIntro();
      t.start(delay);
      this.intros.set(key, t);
    };
    intro("title", 0);
    intro("name", 0);
    content.forEach((_, i) => {
      intro(`label:${i}`, Math.random() * ROW_DELAY);
      intro(`value:${i}`, Math.random() * ROW_DELAY);
    });
  }

  // The rows' text, from the world each frame.
  refresh(content) {
    this.content = content;
  }

  update(dt) {
    this.time += dt;
    for (const intro of this.intros.values()) intro.update(dt);
  }

  draw(hud, batch, m) {
    const view = this.view;
    const layout = this.layout;
    const titleKey = this.kind === "details" ? ["ifs.stats.personalstatstitle", "PERSONAL STATS"] : ["ifs.stats.awardstatstitle", "PLAYER AWARDS"];
    const fade = Math.min(this.time / TITLE_FADE, 1);
    const title = layout.title;
    view.drawText(batch, m, title.x, title.y, view.text(...titleKey), WHITE, 2, this.intros.get("title"), { font: MEDIUM, scale: 1, alpha: fade });
    if (this.kind === "details") {
      const name = String(view.model.subject?.name ?? "").toUpperCase();
      view.drawText(batch, m, layout.name.x, layout.name.y, name, WHITE, 2, this.intros.get("name"), { font: MEDIUM, scale: 1 });
    }
    const font = this.rows.font;
    this.content.forEach((row, i) => {
      const k = this.rows.blank + i;
      if (k >= layout.shown) return;
      const y = layout.rowTop + k * layout.pitch - 10;
      view.drawText(batch, m, layout.labelRight, y, view.text(row.key, row.label), WHITE, 1, this.intros.get(`label:${i}`), { font, scale: 1 });
      if (row.value) view.drawText(batch, m, layout.valueLeft, y, String(row.value).toUpperCase(), WHITE, 0, this.intros.get(`value:${i}`), { font, scale: 1 });
    });
  }
}
