import * as THREE from "three";
import { classicLayout, SLOTS } from "./layout.js";
import { nineSlice, wrapLines, insideQuad } from "../../interface/shapes.js";
import { TitleIntro } from "../../interface/flashy.js";
import { InterfaceCamera, dressModel, meshesOf, roomTints, LOGO_UNITS, ROOM_SCALE } from "../../interface/models.js";
import { ClassPreview } from "./preview.js";

// The Classic spawn screen, drawn as the 2004 game draws it
// (docs/spawn-screen.md), as a layer of the battle HUD (src/hud.js
// setLayer):
// - under the HUD's quads, the translucent blue grid room (shell_cube);
// - the HUD's reinforcements, from the picked team and moved left, and its
//   map in the spawn layout with a ring around the picked post;
// - five class windows (border_3_pieces, nine-sliced) turned toward the
//   middle, each with an orange title bar (headerbutton*) and its class's
//   weapons; the picked class's text at full strength, the rest at half;
// - the objectives window and the spawn point window at the right;
// - SPAWN at the bottom (opaque_rect, tipped back 45 degrees);
// - over them, the two team logos spinning at the top, the picked team's
//   big and green on the left, the other small and red at a third of full
//   strength, and the picked class's body in the middle;
// - over everything, the mouse pointer (mousecursor).

const WHITE = [1, 1, 1, 1];
const BLACK = [0, 0, 0, 1];
const GREEN = [32 / 255, 223 / 255, 32 / 255];
const RED = [223 / 255, 32 / 255, 32 / 255];
const UNPICKED = 128 / 255; // a class window's text when not picked
const SLOT_DELAY = 0.05; // s between the class titles' intros
// The title: its text a font line tall, 5 units above the window and 18
// in. Its bar, measured on the retail screenshots (wider and lower than
// the Lua's text box): from a font line plus 6.3 units above the window
// down to 0.3 short of it, and from 1.75 units in at the left to 3.9 short
// of the right edge.
const TITLE_GAP = 5;
const TITLE_X = 18;
const BAR_OVER = 6.3;
const BAR_UNDER = 0.3;
const BAR_LEFT = 1.75;
const BAR_RIGHT = 3.9;
const LOGO_SWAP = 0.25; // s
const LOGO_DIM = 0.35;
const SPIN_SWAP = THREE.MathUtils.degToRad(-600); // per second (OmegaY -20)
const SPIN_REST = THREE.MathUtils.degToRad(-9); // (OmegaY -0.3)
const SPIN_START = THREE.MathUtils.degToRad(6); // before the first swap (0.2)
const FLIP_RATE = 15; // rad/s, the reinforcements turning back after a mouse switch
const RING_FADE = 10; // per second, once a post isn't picked
const POINTER = 32; // units
const FALLBACK = { rep: "com_icon_republic", cis: "com_icon_cis", imp: "com_icon_imperial", all: "com_icon_alliance",
  gar: "com_icon_republic", gun: "com_icon_republic", wok: "com_icon_republic", geo: "com_icon_cis" };

const tint = (color, a) => [color[0], color[1], color[2], (color[3] ?? 1) * a];
const lerp = (a, b, t) => a + (b - a) * t;

export class ClassicView {
  // hud: the BattleHud; port: the world port; model: the SpawnModel;
  // common: common.json (the model records); input: for the pointer.
  constructor({ hud, port, model, common = null, input = null }) {
    this.hud = hud;
    this.port = port;
    this.model = model;
    this.common = common;
    this.input = input;
    this.time = 0;
    this.slots = Array.from({ length: SLOTS }, () => new TitleIntro());
    this.objectivesTitle = new TitleIntro();
    this.postTitle = new TitleIntro();
    this.rings = new Map(); // post name -> highlight, 0..1
    this.flip = 0; // the reinforcements' turn, radians
    this.camera = new InterfaceCamera();
    this.models = new THREE.Scene();
    this.roomScene = new THREE.Scene();
    this.preview = new ClassPreview(port);
    this.models.add(this.preview.root);
    this.logos = new Map(); // team -> { name, root, materials }
    this.swap = LOGO_SWAP; // time since the logos began swapping
    this.spin = 0;
    this.spinRate = SPIN_START;
    this.room = null;
    this.roomMaterials = [];
    this.layer = {
      teams: null,
      battlefield: null,
      draw: (hud) => this.draw(hud),
      renderBefore: (renderer) => this.renderRoom(renderer),
      renderAfter: (renderer) => this.renderModels(renderer),
      drawOver: (hud, batch) => this.drawPointer(hud, batch),
    };
  }

  get layout() {
    const H = this.hud.height;
    const fh = this.hud.fonts.lineHeight("gamefont_tiny");
    if (this.cached?.H !== H || this.cached?.fh !== fh) this.cached = classicLayout(H, fh);
    return this.cached;
  }

  text(key, fallback) {
    return this.hud.strings?.[key] || fallback;
  }

  // --- what the screen reacts to ---------------------------------------

  // The screen opened. first: the battle's first time, when the logos swap
  // in as the screen is built.
  open({ first = false } = {}) {
    this.ensureModels();
    this.slots.forEach((intro, i) => intro.start(SLOT_DELAY * i));
    this.objectivesTitle.start(0);
    this.postTitle.start(0);
    if (first) this.swapLogos();
    this.flip = 0;
    this.rings.clear();
    this.preview.show(this.model.className);
  }

  close() {
    this.preview.clear();
  }

  // The team changed: the class titles play again and the logos swap; a
  // mouse switch also flips the reinforcements over.
  teamChanged({ source }) {
    this.slots.forEach((intro, i) => intro.start(SLOT_DELAY * i));
    this.swapLogos();
    if (source === "mouse") this.flip = Math.PI;
  }

  postChanged() {
    this.postTitle.start(0);
  }

  swapLogos() {
    this.swap = 0;
    this.spinRate = SPIN_SWAP;
  }

  update(dt) {
    this.time += dt;
    for (const intro of [...this.slots, this.objectivesTitle, this.postTitle]) intro.update(dt);
    this.swap = Math.min(this.swap + dt, LOGO_SWAP);
    if (this.swap >= LOGO_SWAP && this.spinRate === SPIN_SWAP) this.spinRate = SPIN_REST;
    this.spin += this.spinRate * dt;
    this.flip = Math.max(this.flip - FLIP_RATE * dt, 0);
    // The picked post's ring is lit every frame; the others fade.
    for (const [name, alpha] of this.rings) this.rings.set(name, Math.max(alpha - RING_FADE * dt, 0));
    if (this.model.post) this.rings.set(this.model.post, 1);
    this.preview.show(this.model.className);
    this.preview.update(dt);
    for (const material of this.roomMaterials) material.uniforms.time.value = this.time;
    for (const logo of this.logos.values()) for (const material of logo.materials) material.uniforms.time.value = this.time;
    // What the HUD draws for the layer.
    const team = this.model.team;
    this.layer.teams = { own: team, foe: team === 1 ? 2 : 1, flip: this.flip };
    const posts = this.model.posts();
    this.layer.battlefield = {
      team,
      rings: [...this.rings].filter(([, alpha]) => alpha > 0).map(([name, alpha]) => {
        const post = posts.find((p) => p.name === name);
        return post ? { x: post.position[0], z: post.position[2], size: this.hud.postIconSize, alpha } : null;
      }).filter(Boolean),
    };
  }

  // --- the HUD's quads --------------------------------------------------

  draw(hud) {
    const layout = this.layout;
    const roster = this.model.roster();
    for (let i = 0; i < SLOTS; i++) {
      const entry = roster[i];
      if (!entry) continue;
      const picked = i === this.model.classIndex;
      this.drawClassWindow(hud, layout.slots[i], entry, this.slots[i], picked ? 1 : UNPICKED, layout);
    }
    this.drawObjectives(hud, layout);
    this.drawPostWindow(hud, layout);
    this.drawSpawn(hud, layout);
  }

  // A window's frame: the interface point (x, y) turned `tilt` about its
  // vertical axis (positive turns the right edge away).
  frame(hud, x, y, tilt = 0, axis = "y") {
    const m = hud.anchor(0, 0, false).multiply(new THREE.Matrix4().makeTranslation(x, y, 0));
    if (tilt) m.multiply(axis === "x" ? new THREE.Matrix4().makeRotationX(tilt) : new THREE.Matrix4().makeRotationY(tilt));
    return m;
  }

  texture(name) {
    return this.hud.texture(name);
  }

  // The window background: border_3_pieces nine-sliced.
  drawBackground(hud, m, w, h, name = "border_3_pieces", color = WHITE) {
    const texture = this.texture(name);
    if (!texture) {
      hud.batch.quad(m, -w / 2 + 4, -h / 2 + 4, w / 2 - 4, h / 2 - 4, hud.solid, [0, 0, 1, 1], tint([0, 0.17, 0.38, 1], 0.7 * color[3]));
      return;
    }
    for (const [x0, y0, x1, y1, u0, v0, u1, v1] of nineSlice(w, h)) hud.batch.quad(m, x0, y0, x1, y1, texture, [u0, v0, u1, v1], color);
  }

  // A title over a window: the bar (grown `bar` of its length), the text,
  // and the intro's white block. strength: the title's alpha.
  drawTitle(hud, m, w, h, text, intro, strength) {
    const fh = this.layout.fh;
    const state = intro.frame;
    const top = -h / 2 - TITLE_GAP - fh; // the text's line
    const y0 = -h / 2 - fh - BAR_OVER; // the bar
    const y1 = -h / 2 - BAR_UNDER;
    const x0 = -w / 2 + BAR_LEFT;
    const x1 = w / 2 - BAR_RIGHT;
    const cap = y1 - y0; // the end pieces are square
    const visible = x0 + (x1 - x0) * state.bar;
    const barTint = tint(WHITE, strength);
    if (state.bar > 0) {
      for (const [a, b, name] of [[x0, x0 + cap, "headerbuttonleft"], [x0 + cap, x1 - cap, "headerbuttonmid"], [x1 - cap, x1, "headerbuttonright"]]) {
        if (visible <= a || b <= a) continue;
        const end = Math.min(b, visible);
        const texture = this.texture(name);
        if (texture) hud.batch.quad(m, a, y0, end, y1, texture, [0, 0, (end - a) / (b - a), 1], barTint);
        else hud.batch.quad(m, a, y0, end, y1, hud.solid, [0, 0, 1, 1], tint([1, 0.55, 0.1, 1], strength));
      }
    }
    const tx = -w / 2 + TITLE_X;
    const width = hud.fonts.width("gamefont_tiny", text);
    if (state.block) {
      const bx = tx + state.block.x * Math.max(width - fh, 0);
      hud.batch.quad(m, bx, top, bx + fh, top + fh, this.texture("cursor") || hud.solid, [0, 0, 1, 1], tint(WHITE, strength * state.block.alpha));
    }
    if (state.text > 0.01 && state.alpha > 0) {
      // Grown from the text line's centre.
      const mid = top + fh / 2;
      const grown = m.clone().multiply(new THREE.Matrix4().makeTranslation(0, mid, 0))
        .multiply(new THREE.Matrix4().makeScale(1, state.text, 1)).multiply(new THREE.Matrix4().makeTranslation(0, -mid, 0));
      hud.fonts.draw(hud.batch, grown, tx, top, text, "gamefont_tiny", tint(BLACK, strength * state.alpha), 0);
    }
  }

  // Lines centred on (x, y) under m (align: 0 left, 2 centred), at a
  // scale. The weapon and objective lists end in a line break, so they are
  // centred as one line longer, up to as many lines as their box holds
  // (box: its height; measured on the retail screenshots).
  drawLines(hud, m, lines, scale, color, { align = 2, x = 0, y = 0, box = 0 } = {}) {
    const lh = hud.fonts.lineHeight("gamefont_tiny") * scale;
    const count = box ? Math.max(Math.min(lines.length + 1, Math.floor(box / lh)), lines.length) : lines.length;
    lines.forEach((line, i) => {
      hud.fonts.draw(hud.batch, m, x, y - (count * lh) / 2 + i * lh, line, "gamefont_tiny", color, align, scale);
    });
  }

  drawClassWindow(hud, slot, entry, intro, strength, layout) {
    const m = this.frame(hud, slot.x, slot.y, slot.tilt);
    this.drawBackground(hud, m, slot.w, slot.h);
    this.drawTitle(hud, m, slot.w, slot.h, String(entry.displayName || entry.className).toUpperCase(), intro, strength);
    const weapons = (entry.weapons || []).map((name) => String(name).toUpperCase());
    this.drawLines(hud, m, weapons, layout.infoScale, tint(WHITE, strength), { box: slot.textH });
  }

  drawObjectives(hud, layout) {
    const box = layout.objectives;
    const m = this.frame(hud, box.x, box.y, box.tilt);
    this.drawBackground(hud, m, box.w, box.h);
    this.drawTitle(hud, m, box.w, box.h, this.text("game.mapdisplay.objectives", "OBJECTIVES:"), this.objectivesTitle, 1);
    const objectives = this.port.teams()?.[this.model.team]?.objectives || [];
    const width = box.w - 16;
    const lines = objectives.flatMap((line) => wrapLines(String(line).toUpperCase(), width, (text) => hud.fonts.width("gamefont_tiny", text)));
    this.drawLines(hud, m, lines, 1, WHITE, { align: 0, x: -width / 2, box: box.h - 16 });
  }

  drawPostWindow(hud, layout) {
    const box = layout.post;
    const m = this.frame(hud, box.x, box.y, box.tilt);
    this.drawBackground(hud, m, box.w, box.h);
    const post = this.model.posts().find((p) => p.name === this.model.post);
    const title = post ? String(post.displayName || post.name).toUpperCase() : "";
    if (title) this.drawTitle(hud, m, box.w, box.h, title, this.postTitle, 1);
    const vehicles = post?.vehicles?.length ? post.vehicles.join(", ").toUpperCase() : this.text("common.none", "NONE");
    const lines = wrapLines(vehicles, box.w - 16, (text) => hud.fonts.width("gamefont_tiny", text));
    this.drawLines(hud, m, lines, 1, WHITE);
  }

  drawSpawn(hud, layout) {
    const box = layout.spawn;
    const tipped = this.frame(hud, box.x, box.y, box.tilt, "x");
    // The Lua gives the box alpha 128, but the retail screenshots show its
    // pieces at the texture's own.
    this.drawBackground(hud, tipped, box.w, box.h, "opaque_rect");
    const flat = this.frame(hud, box.x, box.y);
    this.drawLines(hud, flat, [this.text("ifs.SpawnDisplay.Spawn", "SPAWN")], 1, WHITE);
  }

  // The pointer, over everything, while the mouse is the screen's.
  drawPointer(hud, batch) {
    const cursor = this.input?.cursor;
    if (!cursor || this.hidePointer) return;
    const texture = this.texture("mousecursor");
    const m = hud.anchor(0, 0, false);
    if (texture) batch.quad(m, cursor.x, cursor.y, cursor.x + POINTER, cursor.y + POINTER, texture, [0, 0, 1, 1], WHITE);
    else batch.quad(m, cursor.x, cursor.y, cursor.x + 8, cursor.y + 8, hud.solid, [0, 0, 1, 1], WHITE);
  }

  // --- the 3D models ----------------------------------------------------

  records(name) {
    return this.common?.holograms?.[name] || null;
  }

  // The room and both logos, made once.
  ensureModels() {
    const texture = (name) => this.texture(name);
    if (!this.room) {
      const room = this.port.commonModel?.("shell_cube");
      if (room) {
        for (const mesh of meshesOf(room)) roomTints(mesh.geometry);
        this.roomMaterials = dressModel(room, this.records("shell_cube") || [{ texture: "shell_cube", scroll: [0.0625, 0] }], texture,
          { lit: false, tints: true, depth: false });
        this.room = room;
        this.roomScene.add(room);
      }
    }
    const teams = this.port.teams() || {};
    for (const team of [1, 2]) {
      const name = teams[team]?.logo || FALLBACK[teams[team]?.side];
      if (!name || this.logos.get(team)?.name === name) continue;
      const old = this.logos.get(team);
      if (old) this.models.remove(old.root);
      const root = this.port.commonModel?.(name);
      if (!root) continue;
      const materials = dressModel(root, this.records(name) || [{ texture: "com_icon_controlzone", scroll: [0, 0.9921875] }], texture, { lit: true });
      this.logos.set(team, { name, root, materials });
      this.models.add(root);
    }
  }

  // Under the HUD's quads: the room, centred and seen from 5 of its own
  // units in front of its origin.
  renderRoom(renderer) {
    if (!this.room) return;
    const canvas = renderer.domElement;
    this.camera.setView(canvas.clientWidth / Math.max(canvas.clientHeight, 1), this.hud.height);
    this.camera.place(512, this.hud.height / 2, this.room.position);
    this.room.scale.setScalar(ROOM_SCALE);
    renderer.render(this.roomScene, this.camera.camera);
  }

  // Over them: the logos and the body, depth tested among themselves.
  // Under the pause menu (the HUD covered) only the body stays, as the
  // original's pause over this screen shows.
  renderModels(renderer) {
    const layout = this.layout;
    const canvas = renderer.domElement;
    this.camera.setView(canvas.clientWidth / Math.max(canvas.clientHeight, 1), this.hud.height);
    const t = this.swap / LOGO_SWAP;
    const canSwitch = this.model.canSwitchTeams && !this.hud.covered;
    for (const [team, logo] of this.logos) {
      const own = team === this.model.team;
      logo.root.visible = canSwitch;
      if (!canSwitch) continue;
      // The swap runs from the other place to this one, linearly.
      const to = own ? layout.logos.own : layout.logos.other;
      const from = own ? layout.logos.other : layout.logos.own;
      const size = own ? lerp(layout.logos.small, layout.logos.big, t) : lerp(layout.logos.big, layout.logos.small, t);
      const alpha = own ? lerp(LOGO_DIM, 1, t) : lerp(1, LOGO_DIM, t);
      this.camera.place(lerp(from.x, to.x, t), lerp(from.y, to.y, t), logo.root.position);
      logo.root.scale.setScalar(size * LOGO_UNITS);
      logo.root.rotation.y = this.spin;
      for (const material of logo.materials) {
        material.uniforms.color.value.setRGB(...(own ? GREEN : RED));
        material.uniforms.opacity.value = alpha;
      }
    }
    this.preview.place(this.camera, layout.preview);
    renderer.clearDepth();
    renderer.render(this.models, this.camera.camera);
  }

  // --- what a click lands on --------------------------------------------

  // An interface point's target: SPAWN, the logos (switch team), a class
  // window, or one of the team's own posts on the map; null for nothing.
  hit(x, y) {
    const hud = this.hud;
    const layout = this.layout;
    const quad = (m, x0, y0, x1, y1) => [hud.toPlane(m, x0, y0), hud.toPlane(m, x1, y0), hud.toPlane(m, x1, y1), hud.toPlane(m, x0, y1)];
    const spawn = layout.spawn;
    // Match the projected button that drawSpawn renders, including its
    // sides; a small central hotspot silently ignored visible-button clicks.
    if (insideQuad(x, y, quad(this.frame(hud, spawn.x, spawn.y, spawn.tilt, "x"), -spawn.w / 2, -spawn.h / 2, spawn.w / 2, spawn.h / 2))) return { kind: "spawn" };
    const logos = layout.logos.hit;
    if (this.model.canSwitchTeams && Math.abs(x - logos.x) <= logos.halfX && Math.abs(y - logos.y) <= logos.halfY) return { kind: "team" };
    const roster = this.model.roster();
    for (let i = 0; i < SLOTS; i++) {
      if (!roster[i]) continue;
      const slot = layout.slots[i];
      const [x0, y0, x1, y1] = slot.hit;
      if (insideQuad(x, y, quad(this.frame(hud, slot.x, slot.y, slot.tilt), x0, y0, x1, y1))) return { kind: "class", index: i };
    }
    const place = hud.spawnMapPlace();
    const half = hud.postIconSize / 2;
    for (const post of this.model.posts()) {
      if (post.owner !== this.model.team || !post.eligible) continue;
      const [px, py] = hud.mapPoint(post.position[0], post.position[2], place.size);
      if (Math.abs(x - (place.x + px)) <= half && Math.abs(y - (place.y + py)) <= half) return { kind: "post", name: post.name };
    }
    return null;
  }

  dispose() {
    this.preview.clear();
    for (const material of this.roomMaterials) material.dispose();
    for (const logo of this.logos.values()) for (const material of logo.materials) material.dispose();
  }
}
