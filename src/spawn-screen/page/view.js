import * as THREE from "three";
import { IW } from "../../interface/space.js";
import { InterfaceCamera, dressModel } from "../../interface/models.js";
import { ClassPreview } from "../classic/preview.js";
import { SpawnStage } from "./stage.js";
import { ShotDirector } from "./director.js";
import { MAP_UNITS } from "./holomap.js";

// The spawn screen in the front end's page look (src/menus/themes/page;
// docs/spawn-screen.md), laid out as the original's: index.js picks it by
// the name "page".
//
// - The stage (stage.js), a 3D scene on the HUD's canvas: the picked
//   class's body on a hologram projector, the battlefield projected as a
//   hologram beside him, through a camera that sweeps in as the screen
//   opens and swings on a change. In the 2004 look a hologram room hides
//   the battle; in the others the battle shows behind, through the
//   mission's camera shots, drifting, cutting to the one that best sees
//   the post picked (director.js).
// - Across the top, the teams as a choice of two, each with the original's
//   logo turning, its name and reinforcements: the joined one lit, its
//   logo green, YOUR TEAM; the other's red, SWITCH TEAM, and a click on it
//   (or on the swap between them, or Left or Right) switches, unless the
//   server assigns teams, which it then says.
// - Down the left, SELECT SOLDIER: a window a class, its name on a bar,
//   its weapons under it; the picked one lit.
// - In the middle, the body (a drag turns him, and tilts the camera), its
//   class's name, the post picked and its vehicles, and SPAWN under his
//   feet.
// - Down the right, OBJECTIVES, and the hologram, whose posts take a
//   click.
// - Gone while the pause menu is over it (hidePointer).
//
// The pointer is the page's own (systemPointer). Presses on the elements
// go through the screen's input as the Classic view's hotspots do: hit(x,
// y) reads what is under an interface point from the elements, and the
// hologram's posts by a ray from the stage's camera; a press anywhere else
// falls through to the canvas, where the input's own binding takes it.

const RING_FADE = 10; // per second, once a post isn't picked (as the Classic view's)
const HOVER_RING = 0.45; // the ring round a post the pointer is over
const ENTRY = 1.3; // s: the entrance's longest animation, delays included
const POST_REACH = 1.25; // of a post icon's size: how near a press takes it
const FEET = 0.13; // of the figure's height, under his feet (the projector's ring)
const HEAD = 0.04; // over his head
const SIDES = { rep: "REPUBLIC", cis: "CIS", imp: "EMPIRE", all: "REBELS" };
const LOGOS = { rep: "com_icon_republic", cis: "com_icon_cis", imp: "com_icon_imperial", all: "com_icon_alliance",
  gar: "com_icon_republic", gun: "com_icon_republic", wok: "com_icon_republic", geo: "com_icon_cis" };
const GREEN = [32 / 255, 223 / 255, 32 / 255];
const RED = [223 / 255, 32 / 255, 32 / 255];
const SPIN_REST = THREE.MathUtils.degToRad(-14); // per second
const SPIN_SWAP = THREE.MathUtils.degToRad(-720);
const SWAP_TIME = 0.4; // s
// The look's custom properties the stage reads (style below), and what of
// the stage's colours each is.
const LOOK_COLORS = { accent: "--bfe-spawn-light", shade: "--bfe-spawn-shade", pool: "--bfe-spawn-pool", room: "--bfe-spawn-room",
  pulse: "--bfe-spawn-pulse", floor: "--bfe-spawn-floor", hologram: "--bfe-spawn-holo", back: "--bfe-spawn-holo-back",
  beam: "--bfe-spawn-beam", key: "--bfe-spawn-key", rim: "--bfe-spawn-rim" };

function make(tag, className = "", text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

function setText(node, text) {
  if (node.textContent !== text) node.textContent = text;
}

// "1:05" from 65 s, "45" under a minute.
export function clockText(seconds) {
  const s = Math.max(Math.floor(seconds), 0);
  return s >= 60 ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}` : String(s);
}

// A panel title from the original's label ("OBJECTIVES:" without its colon).
export const barText = (text) => String(text ?? "").trim().replace(/:$/, "");

// "rgba(4, 8, 14, .86)", "rgb(4 8 14 / 86%)" or "#04080e" as 0..1 values,
// or null.
export function cssColor(text) {
  const value = String(text ?? "").trim();
  const hex = /^#([0-9a-f]{6})$/i.exec(value);
  if (hex) return [0, 2, 4].map((i) => parseInt(hex[1].slice(i, i + 2), 16) / 255).concat(1);
  const rgb = /^rgba?\(([^)]+)\)$/i.exec(value);
  if (!rgb) return null;
  const parts = rgb[1].split(/[\s,/]+/).filter(Boolean);
  if (parts.length < 3) return null;
  const channel = (part) => (part.endsWith("%") ? parseFloat(part) / 100 : parseFloat(part) / 255);
  const alpha = parts[3] === undefined ? 1 : parts[3].endsWith("%") ? parseFloat(parts[3]) / 100 : parseFloat(parts[3]);
  const color = [...parts.slice(0, 3).map(channel), alpha];
  return color.every(Number.isFinite) ? color : null;
}

export class PageView {
  // hud: the BattleHud (its canvas, interface height, strings, battle and
  // spawn map); port: the world's spawn port; model: the SpawnModel;
  // common: common.json (the hologram records of the logos); input: the
  // screen's input, which presses go through.
  constructor({ hud, port, model, common = null, input = null }) {
    this.hud = hud;
    this.port = port;
    this.model = model;
    this.common = common;
    this.input = input;
    // The page's own pointer, not one drawn on the HUD; the battle behind
    // through its own moving shots (direct()).
    this.systemPointer = true;
    this.directsShots = true;
    this.camera = new InterfaceCamera(); // the logos'
    this.logoScene = new THREE.Scene();
    this.preview = new ClassPreview(port);
    this.stage = new SpawnStage({ hud });
    this.stage.setBody(this.preview.root);
    this.director = new ShotDirector(port.cameraShots?.() || []);
    this.logos = new Map(); // team -> { name, root, materials, extent }
    this.hoverTeam = null; // the team whose place the pointer is over
    this.spin = 0;
    this.swap = SWAP_TIME;
    this.rings = new Map(); // post name -> highlight, 0..1
    this.hovered = null;
    this.cursor = "";
    this.covered = false;
    this.root = null; // built when the screen first opens
    this.shown = false;
    this.measured = false;
    this.place = null; // the logos' places, in interface units
    this.keys = {};
    this.look = null;
    this.entering = 0;
    this.listeners = [];
    this.drag = null; // { id, x, y } while he is being turned
    this.turned = false; // he has been turned once (the hint goes)
    this.mapDrawer = null; // the HUD's own drawSpawnMap while the screen borrows it
    this.lastPost = null;
    // The HUD's layer: its own displays and reinforcements hide under it;
    // its battlefield is the picked team's (the radar's view, and the
    // hologram's), which the stage draws instead of the HUD.
    this.layer = {
      teams: false,
      battlefield: null,
      renderBefore: (renderer) => this.renderBefore(renderer),
      renderAfter: (renderer) => this.renderAfter(renderer),
    };
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

  // The pause menu over the screen takes its place until it closes.
  get hidePointer() {
    return this.covered;
  }

  set hidePointer(on) {
    on = Boolean(on);
    if (on === this.covered) return;
    this.covered = on;
    this.root?.classList.toggle("covered", on);
    if (on) {
      this.setCursor("");
      this.endDrag();
    }
  }

  // --- building ---------------------------------------------------------

  build() {
    addStyle();
    const root = (this.root = make("div", "bfe-spawn"));
    root.setAttribute("role", "dialog");
    root.setAttribute("aria-label", "Spawn");
    const frame = make("div", "bfe-spawn-frame");

    // The teams: a choice of two, in team order (the joined one lit), with
    // a swap between them and the keys that switch.
    const head = make("header", "bfe-spawn-head");
    const teams = make("div", "bfe-spawn-teams");
    teams.setAttribute("role", "radiogroup");
    teams.setAttribute("aria-label", "Team");
    const side = (team) => {
      const el = make("div", "bfe-spawn-side");
      el.dataset.team = String(team);
      el.setAttribute("role", "radio");
      const emblem = make("span", "bfe-spawn-emblem");
      const text = make("span", "bfe-spawn-side-text");
      const name = make("span", "bfe-spawn-side-name");
      const state = make("span", "bfe-spawn-side-state");
      text.append(name, state);
      const count = make("strong", "bfe-spawn-side-count");
      const bar = make("span", "bfe-spawn-side-bar");
      const fill = make("i");
      bar.append(fill);
      // The logo beside its plate, on the canvas (a panel over it would
      // dim it).
      const plate = make("span", "bfe-spawn-side-plate");
      plate.append(text, count, bar);
      el.append(emblem, plate);
      // The logo over a team that may be joined wakes as the pointer comes.
      this.listen(el, "pointerenter", () => { this.hoverTeam = team; });
      this.listen(el, "pointerleave", () => { if (this.hoverTeam === team) this.hoverTeam = null; });
      return { el, emblem, name, state, count, fill };
    };
    const one = side(1);
    const two = side(2);
    this.swapper = make("span", "bfe-spawn-swap", "\u21c4");
    this.swapper.title = "SWITCH TEAM";
    teams.append(one.el, this.swapper, two.el);
    this.teamKeys = make("p", "bfe-spawn-teamkeys");
    this.teamKeys.append(make("kbd", "", "\u2190"), make("kbd", "", "\u2192"), make("span", "", "OR CLICK TO SWITCH TEAM"));
    this.countdown = make("p", "bfe-spawn-countdown");
    head.append(teams, this.teamKeys, this.countdown);
    this.sides = { 1: one, 2: two };

    // SELECT SOLDIER.
    const roster = (this.roster = make("section", "bfe-spawn-roster"));
    this.classList = make("div", "bfe-spawn-classes");
    roster.append(this.label(this.text("game.tooltips.select_soldier", "SELECT SOLDIER")), this.classList);
    this.classNodes = [];

    // The body's place (a drag turns him), his class and SPAWN under him.
    const stage = make("div", "bfe-spawn-stage");
    this.figure = make("div", "bfe-spawn-figure");
    this.figure.dataset.drag = "figure";
    this.sweep = make("div", "bfe-spawn-sweep");
    this.hint = make("div", "bfe-spawn-hint");
    this.hint.append(make("span", "bfe-spawn-hint-arrows", "\u21bb"), make("span", "", "DRAG TO TURN"));
    this.figure.append(this.sweep, this.hint);
    const caption = (this.captionBox = make("div", "bfe-spawn-caption"));
    this.caption = make("h2", "bfe-spawn-class-title");
    // Where he goes: the post picked and the vehicles it gives the team.
    const at = make("p", "bfe-spawn-at");
    this.postName = make("strong", "bfe-spawn-post-name");
    this.vehicles = make("span", "value");
    at.append(make("span", "label", this.text("game.spawndisplay.spawnpointtitle", "COMMAND POST")), this.postName,
      make("span", "label", barText(this.text("game.spawndisplay.vehiclelist", "VEHICLES:"))), this.vehicles);
    caption.append(this.caption, at);
    const go = make("div", "bfe-spawn-go-slot");
    this.go = make("button", "bfe-btn active bfe-spawn-go");
    this.go.type = "button";
    this.go.tabIndex = -1;
    this.go.dataset.hit = "spawn";
    this.go.append(make("span", "label", String(this.text("ifs.SpawnDisplay.Spawn", "SPAWN")).toLowerCase()), make("kbd", "bfe-spawn-key", "ENTER"));
    go.append(this.go);
    stage.append(this.figure, caption, go);

    // OBJECTIVES, and the hologram under them.
    const aside = make("aside", "bfe-spawn-tactical");
    const objectives = make("section", "bfe-spawn-panel bfe-spawn-objectives");
    this.goals = make("ul", "bfe-spawn-goals");
    objectives.append(this.bar(barText(this.text("game.mapdisplay.objectives", "OBJECTIVES:"))), this.goals);
    this.mapFrame = make("div", "bfe-spawn-map");
    aside.append(objectives, this.mapFrame);

    frame.append(head, roster, stage, aside);
    this.flash = make("div", "bfe-spawn-flash");
    root.append(frame, this.flash);
    this.listen(root, "mousedown", (event) => this.pressed(event));
    this.listen(root, "mousemove", (event) => this.moved(event));
    this.listen(root, "contextmenu", (event) => event.preventDefault());
    this.listen(this.figure, "pointerdown", (event) => this.startDrag(event));
    this.listen(window, "pointermove", (event) => this.dragged(event));
    this.listen(window, "pointerup", (event) => this.endDrag(event));
    this.listen(window, "pointercancel", (event) => this.endDrag(event));
    document.body.append(root);
    // The frame or the body's place changed size: the stage frames him
    // again, the hologram and the logos go where they now are.
    this.observer = new ResizeObserver(() => {
      this.measured = false;
    });
    for (const node of [root, this.mapFrame, this.figure, one.emblem, two.emblem]) this.observer.observe(node);
    // The look's font arriving moves the names, and the emblems beside them.
    document.fonts?.ready.then(() => {
      this.measured = false;
    });
  }

  // A section's label: small capitals under an accent rule.
  label(text) {
    const node = make("header", "bfe-spawn-label");
    node.append(make("span", "", text));
    return node;
  }

  // A panel's title bar.
  bar(text) {
    const node = make("header", "bfe-spawn-bar");
    node.append(make("span", "", text));
    return node;
  }

  // --- what the screen reacts to ----------------------------------------

  // The screen opened: the camera sweeps in, the body builds, the panels
  // come in.
  open() {
    this.rings.clear();
    this.hovered = null;
    this.borrowMap(true);
    this.ensureLogos();
    this.swap = 0;
    this.showBody(false);
    this.stage.open();
    this.lastPost = null;
    if (this.director.active) {
      const post = this.model.posts().find((p) => p.name === this.model.post);
      this.director.show(Math.floor(Math.random() * this.director.shots.length));
      if (post) this.director.follow(post.position);
      this.lastPost = this.model.post;
    }
    if (typeof document === "undefined") return;
    if (!this.root) this.build();
    this.keys = {};
    this.shown = true;
    this.render();
    this.root.classList.toggle("covered", this.covered);
    this.root.classList.add("open");
    this.root.classList.remove("spawning");
    this.measure();
    this.enter("entering");
  }

  close() {
    this.shown = false;
    this.preview.clear();
    this.borrowMap(false);
    this.setCursor("");
    this.endDrag();
    clearTimeout(this.entering);
    this.root?.classList.remove("open", "entering", "turning");
  }

  // Another team: its classes play in again, the logos spin round, the
  // camera swings.
  teamChanged() {
    this.ensureLogos();
    this.swap = 0;
    this.stage.changed("team");
    if (this.root && this.shown) this.enter("turning");
  }

  // Another post: a ripple from its pin, its name comes in again; the
  // battle behind cuts to it (direct()).
  postChanged() {
    if (this.model.post) this.stage.map.ping(this.model.post);
    if (!this.root || !this.shown) return;
    this.replay(this.postName, "changed");
  }

  // Play a one-off animation class again.
  replay(node, name) {
    node.classList.remove(name);
    void node.offsetWidth;
    node.classList.add(name);
  }

  // The entrance (opening) or a team's (turning): the class its animations
  // hang on, for as long as they run.
  enter(name) {
    const root = this.root;
    root.classList.remove("entering", "turning");
    void root.offsetWidth;
    root.classList.add(name);
    clearTimeout(this.entering);
    this.entering = setTimeout(() => {
      root.classList.remove(name);
      this.measured = false;
    }, ENTRY * 1000);
  }

  // The picked class's body, if it changed: on the stage, dressed in the
  // look's light, building up from his feet. changed: a pick (not the
  // screen opening), which swings the camera too.
  showBody(changed) {
    const before = this.preview.className;
    this.preview.show(this.model.className);
    if (this.preview.className === before) return false;
    this.stage.setBody(this.preview.root, this.preview.walker ? 0.9 : 1);
    this.stage.dress(this.preview.materials);
    if (changed) {
      this.stage.changed("class");
      if (this.root) this.replay(this.sweep, "run");
    }
    return true;
  }

  // One frame: the picked post's ring, the body, the logos, the battle
  // behind, the stage, the elements from the model.
  update(dt) {
    for (const [name, alpha] of this.rings) this.rings.set(name, Math.max(alpha - RING_FADE * dt, 0));
    if (this.model.post) this.rings.set(this.model.post, 1);
    if (this.shown) this.showBody(true);
    this.preview.update(dt);
    this.swap = Math.min(this.swap + dt, SWAP_TIME);
    this.spin += (this.swap < SWAP_TIME ? SPIN_SWAP : SPIN_REST) * dt;
    const open = this.shown && !this.covered;
    if (this.root && this.shown) {
      this.readLook();
      if (!this.measured) this.measure();
    }
    this.stage.update(dt, open);
    for (const logo of this.logos.values()) for (const material of logo.materials) material.uniforms.time.value = this.stage.time;
    // The pointer: the camera's parallax, the post under it.
    const posts = this.model.posts();
    const cursor = this.input?.cursor;
    if (cursor && this.shown) {
      const ndc = this.toNdc(cursor.x, cursor.y);
      this.stage.look(ndc.x, ndc.y);
    }
    this.hovered = cursor && !this.covered && !this.drag ? this.postAt(cursor.x, cursor.y, posts)?.name ?? null : null;
    const rings = [...this.rings].filter(([, alpha]) => alpha > 0);
    if (this.hovered && !(this.rings.get(this.hovered) > HOVER_RING)) rings.push([this.hovered, HOVER_RING]);
    this.layer.battlefield = {
      team: this.model.team,
      rings: rings.map(([name, alpha]) => {
        const post = posts.find((p) => p.name === name);
        return post ? { x: post.position[0], z: post.position[2], size: this.hud.postIconSize, alpha } : null;
      }).filter(Boolean),
    };
    this.stage.map.pinPosts(this.hud.posts?.posts || [], {
      team: this.model.team,
      picked: this.model.post,
      hovered: this.hovered,
      own: (name) => posts.some((p) => p.name === name && p.owner === this.model.team && p.eligible),
    }, dt);
    this.direct(dt);
    if (!this.root || !this.shown) return;
    this.render();
    // Measured again now and then: text changing beside an element moves it
    // without a resize to tell.
    this.sinceMeasure = (this.sinceMeasure || 0) + dt;
    if (this.sinceMeasure > 1) this.measured = false;
    this.setCursor(this.hovered ? "pointer" : "");
  }

  // The battle behind: the director's shot, and the one that best sees a
  // post newly picked; where the look shows it, a dip to dark on a cut. (In
  // the grid room it is hidden, but heard from there.)
  direct(dt) {
    if (!this.shown || !this.director.active) return;
    if (this.model.post !== this.lastPost) {
      this.lastPost = this.model.post;
      const post = this.model.posts().find((p) => p.name === this.model.post);
      if (post) this.director.follow(post.position);
    }
    const before = this.director.index;
    const view = this.director.update(this.covered ? 0 : dt);
    if (!view) return;
    if (view.cut && before >= 0 && this.stage.strength > 0.5 && this.stage.mode !== "grid") this.stage.changed("cut");
    this.port.setViewShot?.(view, { cut: view.cut });
  }

  // --- the look ---------------------------------------------------------

  // The look's colours, read when the look changes, for the stage.
  readLook() {
    const look = document.documentElement.dataset.bfeTheme ?? "";
    if (look === this.look) return;
    this.look = look;
    const style = getComputedStyle(this.root);
    const colors = {};
    for (const [name, property] of Object.entries(LOOK_COLORS)) colors[name] = cssColor(style.getPropertyValue(property));
    colors.mode = style.getPropertyValue("--bfe-spawn-mode").trim() || "world";
    colors.projector = style.getPropertyValue("--bfe-spawn-projector").trim() || "emitter";
    this.stage.colors(colors);
    // Out of the grid room, the battle behind is the director's again.
    this.lastPost = null;
  }

  // --- the elements -----------------------------------------------------

  render() {
    this.renderTeams();
    this.renderClasses();
    this.renderObjectives();
    this.renderPost();
    this.renderSpawn();
  }

  teamName(team) {
    const side = this.port.teams?.()?.[team]?.side;
    return this.text(side && `common.sides.${side}.name`, SIDES[side] || (side ? String(side).toUpperCase() : `TEAM ${team}`));
  }

  // The two teams in their places: the joined one lit, YOUR TEAM; the
  // other SWITCH TEAM, taking a click (it, or the swap between them) while
  // he may switch, else saying the server assigns teams. Each with its
  // reinforcements and a bar of them; the countdown of a team that holds
  // no posts under them.
  renderTeams() {
    const own = this.model.team;
    const battle = this.hud.battle;
    const side = (team) => {
      const left = battle?.reinforcements?.[team];
      if (!Number.isFinite(left)) return { count: null, fill: 0, bleeding: false };
      const count = Math.max(Math.ceil(left), 0);
      const start = battle.start?.[team] || count || 1;
      return { count, fill: Math.min(Math.max(count / start, 0), 1), bleeding: (battle.bleed?.[team] || 0) > 0 };
    };
    const data = { 1: side(1), 2: side(2) };
    const can = this.model.canSwitchTeams;
    const seconds = battle?.countdown;
    const clock = Number.isFinite(seconds)
      ? `${this.text(battle.losing ? "hud.defeat_in" : "hud.victory_in", battle.losing ? "DEFEAT IN" : "VICTORY IN")} ${clockText(seconds)}`
      : "";
    const key = [own, this.teamName(1), this.teamName(2), ...[1, 2].map((t) => [data[t].count, data[t].fill.toFixed(3), data[t].bleeding].join(",")), can, clock, battle?.losing].join("\u0001");
    if (key === this.keys.teams) return;
    this.keys.teams = key;
    const assigned = String(this.text("ifs.mp.createopts.autoassign_on", "TEAMS: AUTO ASSIGN"));
    for (const team of [1, 2]) {
      const part = this.sides[team];
      const mine = team === own;
      const { count, fill, bleeding } = data[team];
      part.el.classList.toggle("mine", mine);
      part.el.classList.toggle("other", !mine);
      part.el.classList.toggle("switchable", !mine && can);
      part.el.classList.toggle("locked", !mine && !can);
      part.el.setAttribute("aria-checked", String(mine));
      if (!mine && can) part.el.dataset.hit = "team";
      else delete part.el.dataset.hit;
      setText(part.name, this.teamName(team));
      setText(part.state, mine ? "YOUR TEAM" : can ? "SWITCH TEAM" : assigned);
      setText(part.count, count === null ? "" : String(count));
      part.count.hidden = count === null;
      part.count.classList.toggle("bleeding", bleeding);
      part.fill.style.setProperty("--fill", fill.toFixed(3));
    }
    if (can) this.swapper.dataset.hit = "team";
    else delete this.swapper.dataset.hit;
    this.swapper.hidden = !can;
    this.teamKeys.hidden = !can;
    setText(this.countdown, clock);
    this.countdown.hidden = !clock;
    this.countdown.classList.toggle("losing", Boolean(battle?.losing));
  }

  // The team's classes: each a window, its name on a bar, its weapons
  // under it; the picked one lit.
  renderClasses() {
    const roster = this.model.roster();
    const key = `${this.model.team}\u0002${roster.map((entry) => [entry.className, entry.displayName, ...(entry.weapons || [])].join("\u0001")).join("\u0002")}`;
    if (key !== this.keys.roster) {
      this.keys.roster = key;
      this.classNodes = roster.map((entry, index) => {
        const node = make("div", "bfe-spawn-class");
        node.dataset.hit = "class";
        node.dataset.index = String(index);
        node.style.setProperty("--i", String(index));
        const bar = make("header", "bfe-spawn-class-bar");
        bar.append(make("span", "bfe-spawn-class-name", String(entry.displayName || entry.className)));
        const weapons = make("ul", "bfe-spawn-weapons");
        for (const weapon of entry.weapons || []) weapons.append(make("li", "", String(weapon)));
        node.append(bar, weapons);
        return node;
      });
      this.classList.replaceChildren(...this.classNodes);
      this.keys.picked = null;
      this.keys.compact = null;
    }
    const picked = this.model.classIndex;
    if (picked !== this.keys.picked) {
      const first = this.keys.picked === null || this.keys.picked === undefined;
      this.keys.picked = picked;
      this.classNodes.forEach((node, index) => node.classList.toggle("picked", index === picked));
      const entry = roster[picked];
      setText(this.caption, entry ? String(entry.displayName || entry.className) : "");
      if (!first) this.replay(this.captionBox, "changed");
      this.keys.compact = null;
    }
    // Too many to show every window's weapons: the others' fold to a line.
    if (this.keys.compact === null) {
      this.roster.classList.remove("compact");
      this.keys.compact = this.classList.scrollHeight > this.classList.clientHeight + 2;
      this.roster.classList.toggle("compact", this.keys.compact);
    }
  }

  renderObjectives() {
    const objectives = this.port.teams?.()?.[this.model.team]?.objectives || [];
    const key = `${this.model.team}\u0002${objectives.join("\u0001")}`;
    if (key === this.keys.objectives) return;
    this.keys.objectives = key;
    this.goals.replaceChildren(...objectives.map((line) => make("li", "", String(line))));
    this.goals.parentElement.hidden = !objectives.length;
  }

  // The post picked: its name, and the vehicles it gives the team.
  renderPost() {
    const post = this.model.posts().find((p) => p.name === this.model.post);
    const name = post ? String(post.displayName || post.name) : "";
    const vehicles = post?.vehicles?.length ? post.vehicles.join(", ") : this.text("common.none", "NONE");
    const key = `${name}\u0001${vehicles}`;
    if (key === this.keys.post) return;
    this.keys.post = key;
    setText(this.postName, name || this.text("game.spawndisplay.pick_post", "PICK A COMMAND POST"));
    this.postName.classList.toggle("none", !name);
    setText(this.vehicles, vehicles);
  }

  // SPAWN: dim without a class or a post to spawn at, held while the world
  // takes the choice (the camera pushing in, the page flashing).
  renderSpawn() {
    const ready = Boolean(this.model.className && this.model.post);
    const waiting = this.model.state === "committed";
    const key = `${ready}\u0001${waiting}`;
    if (key === this.keys.spawn) return;
    const was = this.keys.spawn;
    this.keys.spawn = key;
    this.go.classList.toggle("disabled", !ready);
    this.go.classList.toggle("waiting", waiting);
    this.go.parentElement.classList.toggle("ready", ready && !waiting);
    this.root.classList.toggle("spawning", waiting);
    if (waiting) {
      this.stage.changed("spawn");
      this.replay(this.flash, "run");
    } else if (was?.endsWith("true")) this.stage.changed("settle");
  }

  // --- where things go --------------------------------------------------

  // Client px to interface units, and back (the HUD's canvas spans them).
  toInterface(x, y) {
    const canvas = this.canvas;
    if (canvas && this.hud.toInterface) {
      const rect = canvas.getBoundingClientRect();
      return this.hud.toInterface(x - rect.left, y - rect.top);
    }
    return { x: (x / Math.max(innerWidth, 1)) * IW, y: (y / Math.max(innerHeight, 1)) * this.hud.height };
  }

  // Interface units to the canvas's NDC (x right, y up, -1..1).
  toNdc(x, y) {
    return { x: (x / IW) * 2 - 1, y: 1 - (y / Math.max(this.hud.height, 1)) * 2 };
  }

  // An element's box in interface units: its centre (x, y), w and h.
  box(node) {
    const r = node.getBoundingClientRect();
    const a = this.toInterface(r.left, r.top);
    const b = this.toInterface(r.right, r.bottom);
    return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, w: b.x - a.x, h: b.y - a.y };
  }

  // The stage framed to the elements: his body in the figure's place, the
  // hologram in the map's; each logo fills its emblem.
  measure() {
    if (!this.root || !this.canvas) return false;
    const room = this.box(this.figure);
    const map = this.box(this.mapFrame);
    if (!(room.w > 0 && room.h > 0)) return false;
    const canvas = this.canvas;
    const aspect = canvas.clientWidth / Math.max(canvas.clientHeight, 1);
    const top = this.toNdc(room.x, room.y - room.h / 2 + room.h * HEAD);
    const bottom = this.toNdc(room.x, room.y + room.h / 2 - room.h * FEET);
    this.stage.frame(aspect, { x: top.x, top: top.y, bottom: bottom.y });
    if (map.w > 0 && map.h > 0) {
      const at = this.toNdc(map.x, map.y);
      this.stage.placeMap({ x: at.x, y: at.y, w: (map.w / IW) * 2, h: (map.h / Math.max(this.hud.height, 1)) * 2 });
    }
    const emblem = (node) => {
      const b = this.box(node);
      return { x: b.x, y: b.y, size: Math.min(b.w, b.h) };
    };
    this.place = { logos: { 1: emblem(this.sides[1].emblem), 2: emblem(this.sides[2].emblem) } };
    this.measured = true;
    this.sinceMeasure = 0;
    return true;
  }

  // While the screen is open the HUD doesn't draw its own spawn map: the
  // stage projects it.
  borrowMap(on) {
    const hud = this.hud;
    if (on && !this.mapDrawer) {
      this.mapDrawer = { own: Object.prototype.hasOwnProperty.call(hud, "drawSpawnMap") ? hud.drawSpawnMap : null };
      hud.drawSpawnMap = () => {};
    } else if (!on && this.mapDrawer) {
      if (this.mapDrawer.own) hud.drawSpawnMap = this.mapDrawer.own;
      else delete hud.drawSpawnMap;
      this.mapDrawer = null;
    }
  }

  // The layout in interface units, as the Classic view's has it: the class
  // windows (slots), SPAWN, the panels.
  get layout() {
    if (!this.root) return null;
    if (!this.measured) this.measure();
    return {
      H: this.hud.height,
      slots: this.classNodes.map((node) => this.box(node)),
      spawn: this.box(this.go),
      team: this.box(this.sides[this.model.team === 1 ? 2 : 1].el),
      objectives: this.box(this.goals.parentElement),
      post: this.box(this.postName.parentElement),
      map: this.box(this.mapFrame),
      preview: this.box(this.figure),
    };
  }

  // --- the 3D models ----------------------------------------------------

  // Both teams' logos, made once per logo (the original's com_icon models),
  // each turning about its own middle.
  ensureLogos() {
    const teams = this.port.teams?.() || {};
    for (const team of [1, 2]) {
      const name = teams[team]?.logo || LOGOS[teams[team]?.side];
      if (!name || this.logos.get(team)?.name === name) continue;
      const old = this.logos.get(team);
      if (old) this.logoScene.remove(old.root);
      const root = this.port.commonModel?.(name);
      if (!root) continue;
      const records = this.common?.holograms?.[name] || [{ texture: "com_icon_controlzone", scroll: [0, 0.9921875] }];
      const materials = dressModel(root, records, (texture) => this.hud.texture?.(texture), { lit: true });
      root.updateMatrixWorld(true);
      const bounds = new THREE.Box3().setFromObject(root);
      const size = bounds.getSize(new THREE.Vector3());
      const pivot = new THREE.Group();
      root.position.sub(bounds.getCenter(new THREE.Vector3()));
      pivot.add(root);
      this.logos.set(team, { name, root: pivot, materials, extent: Math.max(size.x, size.y, 1e-3) });
      this.logoScene.add(pivot);
    }
  }

  // --- the HUD's passes -------------------------------------------------

  // Under the HUD's quads: the hologram's map, and the room or the
  // vignette. Not under the pause menu.
  renderBefore(renderer) {
    if (!this.shown || this.hud.covered) return;
    this.stage.before(renderer, this.layer.battlefield);
  }

  // Over everything: the stage, then the logos. Under the pause menu only
  // the body stays, as the Classic view's does.
  renderAfter(renderer) {
    if (!this.shown) return;
    const covered = this.hud.covered;
    this.stage.projector.visible = !covered;
    this.stage.map.root.visible = !covered;
    this.stage.floor.mesh.visible = !covered && this.stage.mode !== "grid";
    this.stage.after(renderer);
    const place = this.place;
    if (covered || !place?.logos) return;
    const canvas = renderer.domElement;
    this.camera.setView(canvas.clientWidth / Math.max(canvas.clientHeight, 1), this.hud.height);
    const t = this.swap / SWAP_TIME;
    for (const [team, logo] of this.logos) {
      const own = team === this.model.team;
      const at = place.logos[team];
      logo.root.visible = Boolean(at);
      if (!at) continue;
      // The other team's wakes under the pointer while it may be joined.
      const woken = !own && this.hoverTeam === team && this.model.canSwitchTeams;
      this.camera.place(at.x, at.y, logo.root.position);
      // It fills its emblem's box (interface units, which are 768 / H on
      // the camera's plane), the other team's smaller, growing in as the
      // logos swap.
      const size = (at.size * (768 / Math.max(this.hud.height, 1))) / logo.extent;
      logo.root.scale.setScalar(size * (own ? 1 : woken ? 0.92 : 0.8) * (0.8 + 0.2 * Math.min(t, 1)));
      logo.root.rotation.y = this.spin * (own ? 1 : woken ? -2.5 : -0.6);
      for (const material of logo.materials) {
        material.uniforms.color.value.setRGB(...(own ? GREEN : RED));
        material.uniforms.opacity.value = own || woken ? 1 : 0.55;
      }
    }
    renderer.clearDepth();
    renderer.render(this.logoScene, this.camera.camera);
  }

  // --- the pointer ------------------------------------------------------

  // The element under an interface point, if it is one of the screen's.
  nodeAt(x, y) {
    if (!this.root || !this.shown || this.covered || typeof document === "undefined") return null;
    const canvas = this.canvas;
    const rect = canvas?.getBoundingClientRect?.() ?? { left: 0, top: 0 };
    const width = canvas?.clientWidth || innerWidth;
    const height = canvas?.clientHeight || innerHeight;
    const node = document.elementFromPoint(rect.left + (x / IW) * width, rect.top + (y / Math.max(this.hud.height, 1)) * height);
    return node && this.root.contains(node) ? node : null;
  }

  // One of the picked team's own posts that may be spawned at, on the
  // hologram under an interface point (its pin, or near it on the disc), or
  // null.
  postAt(x, y, posts = this.model.posts()) {
    const ndc = this.toNdc(x, y);
    const hit = this.stage.pick(ndc.x, ndc.y);
    if (!hit) return null;
    const mine = (post) => post.owner === this.model.team && post.eligible;
    if (hit.pin) return posts.find((post) => post.name === hit.pin && mine(post)) ?? null;
    const reach = ((this.hud.postIconSize || 15) / 2) * POST_REACH;
    let best = null;
    let nearest = Infinity;
    for (const post of posts) {
      if (!mine(post)) continue;
      const point = this.hud.mapPoint?.(post.position[0], post.position[2], MAP_UNITS);
      if (!point) continue;
      const d = Math.hypot(hit.x - point[0], hit.y - point[1]);
      if (d <= reach && d < nearest) {
        nearest = d;
        best = post;
      }
    }
    return best;
  }

  // An interface point's target, as the Classic view's hit() has them:
  // SPAWN, the other team (switch), a class, or one of the team's own
  // posts on the hologram; null for nothing.
  hit(x, y) {
    const node = this.nodeAt(x, y);
    if (node) {
      const target = node.closest("[data-hit]");
      const kind = target?.dataset.hit;
      if (kind === "class") return { kind, index: Number(target.dataset.index) };
      if (kind === "team") return this.model.canSwitchTeams ? { kind } : null;
      if (kind === "spawn") return { kind };
      if (node.closest("[data-drag]")) return null;
    }
    const post = this.postAt(x, y);
    return post ? { kind: "post", name: post.name } : null;
  }

  // A press on one of the screen's elements: the input's button at its
  // place (the right one spawns, as anywhere on the screen). A left press
  // on him is a drag (startDrag).
  pressed(event) {
    event.preventDefault();
    if (!this.shown || this.covered || !this.input) return;
    if (event.button === 0 && event.target?.closest?.("[data-drag]")) return;
    const point = this.toInterface(event.clientX, event.clientY);
    this.input.move(point.x, point.y);
    this.input.button(event.button, true);
  }

  moved(event) {
    // The browser's move when it lets go of a captured mouse isn't the
    // player's.
    if (!this.input || (event.movementX === 0 && event.movementY === 0)) return;
    const point = this.toInterface(event.clientX, event.clientY);
    this.input.move(point.x, point.y);
  }

  // Turning him: a left drag on his place.
  startDrag(event) {
    if (event.button !== 0 || !this.shown || this.covered) return;
    this.drag = { id: event.pointerId, x: event.clientX, y: event.clientY, at: event.timeStamp };
    this.figure.setPointerCapture?.(event.pointerId);
    this.root.classList.add("dragging");
    if (!this.turned) {
      this.turned = true;
      this.hint.classList.add("gone");
    }
  }

  dragged(event) {
    const drag = this.drag;
    if (!drag || event.pointerId !== drag.id) return;
    // A pixel at 1080 lines turns him as much on any screen.
    const k = 1080 / Math.max(innerHeight, 1);
    this.stage.drag((event.clientX - drag.x) * k, (event.clientY - drag.y) * k, (event.timeStamp - drag.at) / 1000);
    drag.x = event.clientX;
    drag.y = event.clientY;
    drag.at = event.timeStamp;
  }

  endDrag(event) {
    const drag = this.drag;
    if (!drag || (event && event.pointerId !== drag.id)) return;
    this.figure?.releasePointerCapture?.(drag.id);
    this.drag = null;
    this.root?.classList.remove("dragging");
    // Held still before letting go: no fling.
    this.stage.release(!event || event.timeStamp - drag.at > 90);
  }

  // The canvas's pointer: a hand over a post that takes a click.
  setCursor(cursor) {
    if (cursor === this.cursor) return;
    this.cursor = cursor;
    if (this.canvas) this.canvas.style.cursor = cursor;
  }

  dispose() {
    this.close();
    for (const off of this.listeners.splice(0)) off();
    this.observer?.disconnect();
    this.root?.remove();
    this.root = null;
    this.stage.dispose();
    for (const logo of this.logos.values()) for (const material of logo.materials) material.dispose();
  }
}

let styled = false;
function addStyle() {
  if (styled) return;
  styled = true;
  const style = document.createElement("style");
  style.dataset.bfe = "spawn-screen";
  style.textContent = STYLE;
  document.head.append(style);
}

// In the page theme's looks (its custom properties, themes/page/style.js),
// in the frame its screens line up in.
const STYLE = `
/* Over the battle, under the pause menu's page (z-index 20). Only what takes a press
   takes the pointer; elsewhere it reaches the canvas (the hologram's posts). */
.bfe-spawn { position: fixed; inset: 0; z-index: 10; overflow: hidden; pointer-events: none; color: var(--bfe-text);
  font-family: "BFE Saira", system-ui, sans-serif; user-select: none; -webkit-user-select: none;
  --own: #4ff07a; --foe: #ff5a4a;
  opacity: 0; visibility: hidden; transition: opacity .2s ease, visibility 0s linear .2s; }
.bfe-spawn.open { opacity: 1; visibility: visible; transition: opacity .2s ease; }
.bfe-spawn.open.covered { opacity: 0; visibility: hidden; transition: opacity .2s ease, visibility 0s linear .2s; }
/* Shut or covered, its endless loops (the pulses, the sheen, the blinks) stop, to start
   afresh when it opens: hidden, they would still cost the page style work every frame of
   the battle. The one-shot animations (the flash, the sweep, the entrance) are left be. */
.bfe-spawn:is(:not(.open), .covered) :is(.bfe-spawn-side-state, .bfe-spawn-side-count, .bfe-spawn-countdown, .bfe-spawn-hint, .bfe-spawn-go-slot, .bfe-spawn-go .label),
.bfe-spawn:is(:not(.open), .covered) .bfe-spawn-class.picked::after { animation: none !important; }
.bfe-spawn *, .bfe-spawn *::before, .bfe-spawn *::after { box-sizing: border-box; }
.bfe-spawn button { font: inherit; color: inherit; }
.bfe-spawn [hidden] { display: none !important; }
.bfe-spawn [data-hit] { pointer-events: auto; cursor: pointer; }
.bfe-spawn [data-drag] { pointer-events: auto; cursor: grab; touch-action: none; }
.bfe-spawn.dragging, .bfe-spawn.dragging [data-drag] { cursor: grabbing; }

/* The stage in each look (view.js reads these for stage.js): grid (the hologram room)
   or the world behind; the map's projector; the light of the projector under him, the
   vignette's edge and pool, the room's tint and running light, the floor, the hologram,
   the dark under it, its beam, his key light and rim. */
html[data-bfe-theme="eternal"] .bfe-spawn { --bfe-spawn-mode: world; --bfe-spawn-projector: emitter;
  --bfe-spawn-light: rgb(255, 160, 50); --bfe-spawn-shade: rgb(2, 3, 6); --bfe-spawn-pool: rgb(30, 18, 8);
  --bfe-spawn-floor: rgb(255, 150, 50); --bfe-spawn-holo: rgb(255, 178, 90); --bfe-spawn-holo-back: rgba(12, 8, 4, .62); --bfe-spawn-beam: rgb(255, 150, 60);
  --bfe-spawn-key: rgb(255, 246, 228); --bfe-spawn-rim: rgba(255, 150, 40, .6); }
html[data-bfe-theme="bf1"] .bfe-spawn { --bfe-spawn-mode: grid; --bfe-spawn-projector: emitter;
  --bfe-spawn-light: rgb(90, 180, 255); --bfe-spawn-room: rgb(255, 255, 255); --bfe-spawn-pulse: rgb(130, 205, 255);
  --bfe-spawn-holo: rgb(205, 232, 255); --bfe-spawn-holo-back: rgba(3, 12, 36, .7); --bfe-spawn-beam: rgb(140, 200, 255);
  --bfe-spawn-key: rgb(236, 246, 255); --bfe-spawn-rim: rgba(110, 190, 255, .7); }
html[data-bfe-theme="bf2"] .bfe-spawn { --bfe-spawn-mode: world; --bfe-spawn-projector: cone;
  --bfe-spawn-light: rgb(110, 170, 255); --bfe-spawn-shade: rgb(4, 8, 18); --bfe-spawn-pool: rgb(16, 28, 54);
  --bfe-spawn-floor: rgb(110, 160, 255); --bfe-spawn-holo: rgb(130, 185, 255); --bfe-spawn-holo-back: rgba(6, 14, 34, .55); --bfe-spawn-beam: rgb(140, 190, 255);
  --bfe-spawn-key: rgb(250, 252, 255); --bfe-spawn-rim: rgba(200, 220, 255, .55); }

/* The frame: the teams across the top; SELECT SOLDIER, him, the post and its map. */
.bfe-spawn-frame { position: absolute; top: 0; bottom: 0; left: var(--bfe-edge); width: var(--bfe-page-frame);
  display: grid; grid-template-columns: minmax(250px, 23%) minmax(0, 1fr) minmax(300px, 33%); grid-template-rows: auto minmax(0, 1fr);
  column-gap: 1.6vw; padding: 2.4vh 0 2.8vh; }
.bfe-spawn-head { grid-column: 1 / -1; display: flex; flex-direction: column; align-items: center; gap: .6vh; }
.bfe-spawn-roster { grid-column: 1; display: flex; flex-direction: column; gap: 1.1vh; min-height: 0; padding-top: 1vh; }
.bfe-spawn-stage { grid-column: 2; position: relative; display: flex; flex-direction: column; align-items: center; min-height: 0; }
.bfe-spawn-tactical { grid-column: 3; display: flex; flex-direction: column; gap: 1.2vh; min-height: 0; padding-top: 1vh; }

/* Labels: small capitals over an accent rule that runs to the right. */
.bfe-spawn-label { display: flex; align-items: center; gap: .8em; font-size: clamp(11px, 1.45vh, 14px); font-weight: 800; letter-spacing: .28em; color: var(--bfe-text-dim); }
.bfe-spawn-label::after { content: ""; flex: 1; height: 1px; background: linear-gradient(90deg, var(--bfe-accent), transparent); opacity: .7; }

/* The teams: a choice of two in team order, each its logo (drawn in the emblem's box),
   name, state and reinforcements, a bar of them along its foot; the joined one lit in
   its team's green, the other a button while he may switch (or locked); a swap between. */
.bfe-spawn-teams { display: flex; align-items: center; justify-content: center; gap: .9vw; }
.bfe-spawn-side { --side: var(--own); display: flex; align-items: center; gap: .7vw; text-shadow: 0 2px 10px rgba(0, 0, 0, .8); transition: opacity .2s ease, transform .2s ease; }
.bfe-spawn-side[data-team="2"] { flex-direction: row-reverse; text-align: right; }
.bfe-spawn-side-plate { position: relative; display: flex; align-items: center; gap: .9vw; width: min(24vw, 340px); padding: .8vh 1.2vw 1vh;
  overflow: hidden; background: var(--bfe-panel-bg); border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius);
  -webkit-backdrop-filter: blur(8px); backdrop-filter: blur(8px); transition: border-color .2s ease, box-shadow .2s ease; }
.bfe-spawn-side[data-team="2"] .bfe-spawn-side-plate { flex-direction: row-reverse; }
.bfe-spawn-side.mine .bfe-spawn-side-plate { border-color: color-mix(in srgb, var(--own) 75%, transparent);
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--own) 35%, transparent), 0 0 28px color-mix(in srgb, var(--own) 22%, transparent), 0 10px 30px rgba(0, 0, 0, .45); }
.bfe-spawn-side.other { --side: var(--foe); opacity: .8; }
.bfe-spawn-side.switchable:hover { opacity: 1; transform: translateY(-2px); }
.bfe-spawn-side.switchable:hover .bfe-spawn-side-plate { border-color: var(--bfe-accent);
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--bfe-accent) 50%, transparent), 0 0 26px color-mix(in srgb, var(--bfe-accent) 32%, transparent), 0 10px 30px rgba(0, 0, 0, .45); }
.bfe-spawn-side.locked { opacity: .5; }
.bfe-spawn-emblem { width: clamp(46px, 7.4vh, 80px); aspect-ratio: 1; flex: none; border-radius: 50%;
  background: radial-gradient(circle, color-mix(in srgb, var(--side) 18%, transparent) 0%, transparent 70%); }
.bfe-spawn-side-text { display: flex; flex-direction: column; align-items: flex-start; gap: .4vh; flex: 1 1 auto; min-width: 0; }
.bfe-spawn-side[data-team="2"] .bfe-spawn-side-text { align-items: flex-end; }
.bfe-spawn-side-name { max-width: 100%; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-size: clamp(16px, 2.6vh, 28px); font-weight: 900; font-stretch: 112%;
  letter-spacing: .12em; line-height: 1; color: var(--side); }
.bfe-spawn-side-state { display: inline-flex; align-items: center; gap: .5em; font-size: clamp(9px, 1.2vh, 12px); font-weight: 800; letter-spacing: .22em; color: var(--bfe-text-dim); }
.bfe-spawn-side.mine .bfe-spawn-side-state { color: var(--own); }
.bfe-spawn-side.mine .bfe-spawn-side-state::before { content: ""; width: .5em; height: .5em; border-radius: 50%; background: var(--own); box-shadow: 0 0 8px var(--own); }
.bfe-spawn-side.switchable .bfe-spawn-side-state { padding: .25em .8em; color: var(--bfe-accent-ink); background: var(--bfe-accent-fill); border-radius: var(--bfe-dialog-button-radius);
  text-shadow: none; animation: bfe-spawn-call 2.4s ease-in-out infinite; }
.bfe-spawn-side.switchable:hover .bfe-spawn-side-state { animation: none; filter: brightness(1.15); }
@keyframes bfe-spawn-call { 0%, 100% { box-shadow: 0 0 0 0 color-mix(in srgb, var(--bfe-accent) 0%, transparent); } 50% { box-shadow: 0 0 12px 1px color-mix(in srgb, var(--bfe-accent) 60%, transparent); } }
.bfe-spawn-side-count { flex: none; font-size: clamp(18px, 2.8vh, 30px); font-weight: 800; color: #fff; font-variant-numeric: tabular-nums; }
.bfe-spawn-side-count.bleeding { color: var(--foe); animation: bfe-spawn-blink 1s steps(2, start) infinite; }
.bfe-spawn-side-bar { position: absolute; left: 0; right: 0; bottom: 0; height: 3px; overflow: hidden; background: rgba(255, 255, 255, .08); }
.bfe-spawn-side-bar i { position: absolute; top: 0; bottom: 0; left: 0; width: calc(var(--fill, 1) * 100%); background: var(--side); box-shadow: 0 0 10px var(--side); transition: width .4s ease; }
.bfe-spawn-side[data-team="2"] .bfe-spawn-side-bar i { left: auto; right: 0; }
.bfe-spawn-swap { align-self: center; flex: none; display: grid; place-items: center; width: clamp(32px, 4.6vh, 46px); aspect-ratio: 1; border-radius: 50%;
  font-size: clamp(16px, 2.5vh, 26px); line-height: 1; color: var(--bfe-accent); background: rgba(0, 0, 0, .5);
  border: 1px solid color-mix(in srgb, var(--bfe-accent) 55%, transparent); box-shadow: 0 0 16px color-mix(in srgb, var(--bfe-accent) 25%, transparent);
  transition: transform .35s cubic-bezier(.2, .8, .2, 1), background .2s ease; }
.bfe-spawn-swap:hover { transform: rotate(180deg) scale(1.08); background: color-mix(in srgb, var(--bfe-accent) 28%, rgba(0, 0, 0, .55)); }
.bfe-spawn-teamkeys { margin: .2vh 0 0; display: flex; align-items: center; justify-content: center; gap: .45em; font-size: clamp(9px, 1.1vh, 11px); font-weight: 800;
  letter-spacing: .24em; color: var(--bfe-text-faint); text-shadow: 0 2px 6px rgba(0, 0, 0, .9); }
.bfe-spawn-teamkeys span { margin-left: .4em; }
.bfe-spawn-teamkeys kbd { min-width: 1.9em; padding: .1em .4em; font: 800 1.1em/1.2 system-ui, sans-serif; letter-spacing: 0; text-align: center;
  color: var(--bfe-text-dim); border: 1px solid currentColor; border-radius: 3px; }
.bfe-spawn.turning .bfe-spawn-side.mine .bfe-spawn-side-plate { animation: bfe-spawn-joined .6s cubic-bezier(.2, .8, .2, 1) both; }
@keyframes bfe-spawn-joined { 0% { transform: scale(.94); filter: brightness(1.8); } 100% { transform: none; filter: none; } }
.bfe-spawn-countdown { margin: 0; font-size: clamp(12px, 1.7vh, 16px); font-weight: 800; letter-spacing: .26em; color: var(--own); }
.bfe-spawn-countdown.losing { color: var(--foe); animation: bfe-spawn-blink 1s steps(2, start) infinite; }
@keyframes bfe-spawn-blink { 50% { opacity: .35; } }

/* SELECT SOLDIER: a window a class, its name on a bar, its weapons under it. Too many to
   fit: the others fold to a line (compact). */
.bfe-spawn-classes { display: flex; flex-direction: column; gap: .9vh; min-height: 0; flex: 1 1 auto; overflow: hidden; }
.bfe-spawn-class { --bar: var(--bfe-spawn-bar, rgba(255, 255, 255, .08)); --bar-ink: var(--bfe-spawn-bar-ink, var(--bfe-text));
  position: relative; flex: none; overflow: hidden; background: var(--bfe-panel-bg); border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius);
  -webkit-backdrop-filter: blur(8px); backdrop-filter: blur(8px); opacity: .82;
  transition: border-color .18s ease, transform .18s ease, box-shadow .18s ease, opacity .18s ease; }
.bfe-spawn-class:hover { opacity: 1; transform: translateX(4px); }
.bfe-spawn-class-bar { display: flex; align-items: center; justify-content: space-between; padding: .55vh 1vw; background: var(--bar); color: var(--bar-ink);
  transition: background .18s ease, color .18s ease; }
.bfe-spawn-class-name { font-size: clamp(13px, 1.9vh, 19px); font-weight: 800; letter-spacing: .12em; text-transform: uppercase; }
.bfe-spawn-weapons { margin: 0; padding: .7vh 1vw .8vh; list-style: none; display: grid; grid-template-columns: 1fr 1fr; gap: .25em 1em;
  font-size: clamp(10px, 1.3vh, 12px); font-weight: 700; letter-spacing: .08em; text-transform: uppercase; color: var(--bfe-text-faint); }
.bfe-spawn-weapons li { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
.bfe-spawn-roster.compact .bfe-spawn-class:not(.picked) .bfe-spawn-weapons { display: flex; flex-wrap: nowrap; gap: 0 .9em; overflow: hidden; padding-block: .45vh .55vh;
  -webkit-mask-image: linear-gradient(90deg, #000 82%, transparent); mask-image: linear-gradient(90deg, #000 82%, transparent); }
.bfe-spawn-roster.compact .bfe-spawn-class:not(.picked) .bfe-spawn-weapons li { overflow: visible; }
.bfe-spawn-class.picked { opacity: 1; transform: translateX(10px); border-color: var(--bfe-accent); --bar: var(--bfe-spawn-bar-picked, var(--bfe-accent-fill)); --bar-ink: var(--bfe-spawn-bar-picked-ink, var(--bfe-accent-ink));
  box-shadow: 0 0 0 1px color-mix(in srgb, var(--bfe-accent) 40%, transparent), 0 10px 34px rgba(0, 0, 0, .5), -6px 0 26px color-mix(in srgb, var(--bfe-accent) 30%, transparent); }
.bfe-spawn-class.picked .bfe-spawn-weapons { color: var(--bfe-text); }
.bfe-spawn-class.picked .bfe-spawn-weapons li::before { content: ""; display: inline-block; width: .45em; height: .45em; margin-right: .5em; vertical-align: .1em; background: var(--bfe-accent); transform: rotate(45deg); }
.bfe-spawn-class.picked::after { content: ""; position: absolute; inset: 0; pointer-events: none; transform: translateX(-120%);
  background: linear-gradient(105deg, transparent 35%, rgba(255, 255, 255, .16) 50%, transparent 65%); animation: bfe-spawn-sheen 3.2s ease-in-out 1s infinite; }
@keyframes bfe-spawn-sheen { 0%, 60% { transform: translateX(-120%); } 100% { transform: translateX(120%); } }

/* His place (a drag turns him), his class and SPAWN under his feet. */
.bfe-spawn-figure { position: relative; flex: 1 1 auto; width: min(100%, 46vh); min-height: 0; }
.bfe-spawn-sweep { position: absolute; left: 12%; right: 12%; top: 0; height: 18%; opacity: 0; pointer-events: none; mix-blend-mode: screen;
  background: linear-gradient(180deg, transparent, color-mix(in srgb, var(--bfe-spawn-light) 45%, transparent) 70%, rgba(255, 255, 255, .6) 92%, transparent); filter: blur(3px); }
.bfe-spawn-sweep.run { animation: bfe-spawn-sweep .65s cubic-bezier(.3, .1, .3, 1) both; }
@keyframes bfe-spawn-sweep { 0% { opacity: 0; top: 92%; } 15% { opacity: .9; } 85% { opacity: .8; } 100% { opacity: 0; top: -10%; } }
.bfe-spawn-hint { position: absolute; right: 0; top: 3%; display: flex; align-items: center; gap: .6em; white-space: nowrap; padding: .35em .8em;
  font-size: clamp(9px, 1.15vh, 11px); font-weight: 800; letter-spacing: .26em; color: var(--bfe-text-dim); background: rgba(0, 0, 0, .42);
  border: 1px solid color-mix(in srgb, var(--bfe-accent) 35%, transparent); border-radius: 999px; transition: opacity .6s ease; animation: bfe-spawn-hint 3s ease-in-out 2s infinite; }
.bfe-spawn-hint-arrows { font-size: 1.5em; line-height: 1; letter-spacing: 0; color: var(--bfe-accent); }
.bfe-spawn-hint.gone { opacity: 0; animation: none; }
@keyframes bfe-spawn-hint { 0%, 100% { opacity: .55; } 50% { opacity: 1; } }
.bfe-spawn-caption { position: relative; isolation: isolate; flex: none; display: flex; flex-direction: column; align-items: center; margin-top: .4vh; text-align: center; }
.bfe-spawn-caption::before { content: ""; position: absolute; inset: -1.2vh -4vw -.6vh; z-index: -1; pointer-events: none;
  background: radial-gradient(ellipse 50% 50% at 50% 50%, rgba(0, 0, 0, .5), rgba(0, 0, 0, .25) 55%, transparent 75%); }
.bfe-spawn-class-title { margin: 0; font-size: clamp(22px, 4.2vh, 46px); font-weight: 900; font-stretch: 116%; letter-spacing: .1em; line-height: 1; color: #fff;
  text-shadow: 0 0 24px color-mix(in srgb, var(--bfe-spawn-light) 55%, transparent), 0 3px 12px rgba(0, 0, 0, .9); }
.bfe-spawn-class-title::after { content: ""; display: block; height: 3px; margin: .3em auto 0; width: 60%;
  background: linear-gradient(90deg, transparent, var(--bfe-accent), transparent); }
.bfe-spawn-at { display: flex; align-items: baseline; justify-content: center; flex-wrap: wrap; gap: .3em .7em; margin: .7vh 0 0;
  font-size: clamp(10px, 1.35vh, 13px); font-weight: 700; letter-spacing: .16em; color: var(--bfe-text-faint); text-shadow: 0 2px 8px rgba(0, 0, 0, .9); }
.bfe-spawn-at .label + .label, .bfe-spawn-at strong + .label { margin-left: .9em; }
.bfe-spawn-at .value { color: var(--bfe-text); }
.bfe-spawn-caption.changed { animation: bfe-spawn-caption .5s cubic-bezier(.2, .8, .2, 1) both; }
@keyframes bfe-spawn-caption { from { opacity: 0; transform: translateY(1.2vh); letter-spacing: .3em; } }

/* SPAWN: under his feet, breathing while it can go. */
.bfe-spawn-go-slot { flex: none; width: min(100%, 360px); margin-top: 1.4vh; transition: filter .3s ease; }
.bfe-spawn-go-slot.ready { animation: bfe-spawn-breathe 2.2s ease-in-out infinite; }
@keyframes bfe-spawn-breathe { 0%, 100% { filter: drop-shadow(0 0 4px color-mix(in srgb, var(--bfe-accent) 40%, transparent)); }
  50% { filter: drop-shadow(0 0 18px color-mix(in srgb, var(--bfe-accent) 85%, transparent)); } }
.bfe-spawn .bfe-spawn-go { width: 100%; height: clamp(50px, 7vh, 74px); gap: 1em; font-size: clamp(20px, 3vh, 32px); letter-spacing: .32em; }
.bfe-spawn .bfe-spawn-go .label { font-weight: 900; }
.bfe-spawn-key { padding: .15em .5em; font: 700 clamp(9px, 1.2vh, 12px) "BFE Saira", system-ui, sans-serif; letter-spacing: .1em; border: 1px solid currentColor; border-radius: 3px; opacity: .65; }
.bfe-spawn .bfe-spawn-go.disabled { opacity: .45; cursor: default; }
.bfe-spawn .bfe-spawn-go.waiting .label { animation: bfe-spawn-blink .8s steps(2, start) infinite; }

/* OBJECTIVES and the post: windows with a title bar; under them the hologram's place. */
.bfe-spawn-panel { position: relative; flex: none; overflow: hidden; background: var(--bfe-panel-bg); border: var(--bfe-panel-border); border-radius: var(--bfe-panel-radius);
  -webkit-backdrop-filter: blur(8px); backdrop-filter: blur(8px); }
.bfe-spawn-bar { padding: .55vh 1vw; background: var(--bfe-spawn-bar, rgba(255, 255, 255, .08)); color: var(--bfe-spawn-bar-ink, var(--bfe-text-dim));
  font-size: clamp(11px, 1.5vh, 14px); font-weight: 800; letter-spacing: .24em; }
.bfe-spawn-goals { margin: 0; padding: .9vh 1vw 1vh; list-style: none; display: flex; flex-direction: column; gap: .5vh; }
.bfe-spawn-goals li { position: relative; padding-left: 1.2em; font-size: clamp(11px, 1.55vh, 15px); font-weight: 700; letter-spacing: .08em; text-transform: uppercase; color: var(--bfe-text); }
.bfe-spawn-goals li::before { content: ""; position: absolute; left: 0; top: .45em; width: .5em; height: .5em; background: var(--bfe-accent); transform: rotate(45deg); box-shadow: 0 0 8px var(--bfe-accent); }
.bfe-spawn-post-name { font-size: clamp(15px, 2.2vh, 24px); font-weight: 900; font-stretch: 112%; letter-spacing: .1em; color: #fff;
  text-shadow: 0 0 16px color-mix(in srgb, var(--bfe-accent) 45%, transparent), 0 2px 8px rgba(0, 0, 0, .9); }
.bfe-spawn-post-name.none { font-size: clamp(12px, 1.6vh, 15px); color: var(--bfe-text-faint); text-shadow: none; }
.bfe-spawn-post-name.changed { animation: bfe-spawn-caption .45s cubic-bezier(.2, .8, .2, 1) both; }
.bfe-spawn-map { position: relative; flex: 1 1 auto; min-height: 0; }

/* SPAWN pressed: a flash as the camera drops in. */
.bfe-spawn-flash { position: absolute; inset: 0; pointer-events: none; opacity: 0;
  background: radial-gradient(ellipse at 50% 55%, rgba(255, 255, 255, .9), color-mix(in srgb, var(--bfe-spawn-light) 55%, transparent) 40%, transparent 75%); }
.bfe-spawn-flash.run { animation: bfe-spawn-flash .9s ease-out both; }
@keyframes bfe-spawn-flash { 0% { opacity: 0; } 25% { opacity: .85; } 100% { opacity: 0; } }
.bfe-spawn.spawning .bfe-spawn-roster, .bfe-spawn.spawning .bfe-spawn-tactical { opacity: 0; transition: opacity .35s ease; }
.bfe-spawn.spawning .bfe-spawn-roster { transform: translateX(-3vw); transition: opacity .35s ease, transform .35s ease; }
.bfe-spawn.spawning .bfe-spawn-tactical { transform: translateX(3vw); transition: opacity .35s ease, transform .35s ease; }

/* Coming in: the teams drop, the windows slide from the left one by one, the post and
   objectives from the right, his name rises; a team's classes play in again. */
.bfe-spawn.entering .bfe-spawn-head { animation: bfe-spawn-drop .6s cubic-bezier(.2, .8, .2, 1) .15s both; }
.bfe-spawn.entering .bfe-spawn-roster .bfe-spawn-label, .bfe-spawn.entering .bfe-spawn-tactical > * { animation: bfe-spawn-side .6s cubic-bezier(.2, .8, .2, 1) .3s both; }
.bfe-spawn.entering .bfe-spawn-tactical > * { animation-name: bfe-spawn-side-right; }
.bfe-spawn.entering .bfe-spawn-tactical > :nth-child(2) { animation-delay: .38s; }
.bfe-spawn.entering .bfe-spawn-class, .bfe-spawn.turning .bfe-spawn-class { animation: bfe-spawn-class .5s cubic-bezier(.2, .8, .2, 1) both; animation-delay: calc(.35s + var(--i, 0) * 60ms); }
.bfe-spawn.turning .bfe-spawn-class { animation-delay: calc(var(--i, 0) * 60ms); }
.bfe-spawn.entering .bfe-spawn-caption, .bfe-spawn.entering .bfe-spawn-go-slot { animation: bfe-spawn-caption .7s cubic-bezier(.2, .8, .2, 1) .6s both; }
@keyframes bfe-spawn-drop { from { opacity: 0; transform: translateY(-3vh); } }
@keyframes bfe-spawn-side { from { opacity: 0; transform: translateX(-4vw); } }
@keyframes bfe-spawn-side-right { from { opacity: 0; transform: translateX(4vw); } }
@keyframes bfe-spawn-class { from { opacity: 0; transform: translateX(-3vw); } }

/* The looks' own windows. The 2004 game's: orange title bars on blue glass. The 2005
   game's: pale steel capsules on navy, a red edge on the pick. */
html[data-bfe-theme="eternal"] .bfe-spawn { --bfe-spawn-bar: rgba(255, 255, 255, .06); --bfe-spawn-bar-ink: var(--bfe-text); }
html[data-bfe-theme="eternal"] .bfe-spawn-class:not(.picked) .bfe-spawn-class-bar { box-shadow: inset 3px 0 0 color-mix(in srgb, var(--bfe-accent) 55%, transparent); }
html[data-bfe-theme="bf1"] .bfe-spawn { --bfe-spawn-bar: linear-gradient(180deg, rgba(255, 196, 92, .78), rgba(220, 110, 10, .7)); --bfe-spawn-bar-ink: #fff3dc;
  --bfe-spawn-bar-picked: linear-gradient(180deg, #ffd27a 0%, #ffa42a 45%, #f57500 100%); --bfe-spawn-bar-picked-ink: #13204a; }
html[data-bfe-theme="bf1"] .bfe-spawn-class-bar, html[data-bfe-theme="bf1"] .bfe-spawn-bar { text-shadow: 0 1px 2px rgba(60, 20, 0, .6); }
html[data-bfe-theme="bf1"] .bfe-spawn-class.picked .bfe-spawn-class-bar { text-shadow: none; }
html[data-bfe-theme="bf1"] .bfe-spawn-class.picked { border-color: rgba(190, 230, 255, .9); box-shadow: 0 0 0 1px rgba(150, 210, 255, .5), 0 0 28px rgba(90, 170, 255, .45), 0 10px 34px rgba(0, 0, 0, .5); }
html[data-bfe-theme="bf2"] .bfe-spawn { --bfe-spawn-bar: linear-gradient(180deg, #c9d4e2 0%, #9aa9bd 50%, #8496ad 52%, #b3c0d0 100%); --bfe-spawn-bar-ink: #0d1626;
  --bfe-spawn-bar-picked: linear-gradient(180deg, #f0675a 0%, #cf2a1d 48%, #a5180e 52%, #d8483a 100%); --bfe-spawn-bar-picked-ink: #fff; }
html[data-bfe-theme="bf2"] .bfe-spawn-class-bar, html[data-bfe-theme="bf2"] .bfe-spawn-bar { margin: .5vh .5vw 0; border-radius: 999px; padding-block: .35vh; justify-content: center; }
html[data-bfe-theme="bf2"] .bfe-spawn-class { border-radius: 12px; }
html[data-bfe-theme="bf2"] .bfe-spawn-class.picked { border-color: #cf2a1d; }
html[data-bfe-theme="bf2"] .bfe-spawn-class-title, html[data-bfe-theme="bf2"] .bfe-spawn-post-name { text-shadow: -1px -1px 0 #111, 1px -1px 0 #111, -1px 1px 0 #111, 1px 1px 0 #111, 0 3px 10px rgba(0, 0, 0, .8); }
`;
