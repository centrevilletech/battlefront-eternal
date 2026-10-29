import test from "node:test";
import assert from "node:assert/strict";
import { createSpawnScreen } from "../src/spawn-screen/index.js";
import { ClassicView } from "../src/spawn-screen/classic/view.js";
import * as THREE from "three";
import { PageView, clockText, barText, cssColor } from "../src/spawn-screen/page/view.js";
import { MAP_UNITS } from "../src/spawn-screen/page/holomap.js";
import { IW } from "../src/interface/space.js";
import { FakeSpawnPort, geonosisData } from "./spawn-screen-fake-port.js";

// The page view's logic without a page (its elements need a browser:
// tests/spawn-screen-browser.mjs and the GUI preview show those).

const FRAME = 1 / 60;
const HUD_PLACE = { x: 809, y: 384, size: 420 };

// Just enough of the battle HUD: its spawn map's place and drawing
// (methods of its class, as the BattleHud's) and the map's mapping, 1 unit
// a metre.
class HudStub {
  constructor() {
    Object.assign(this, { renderer: null, height: 768, strings: {}, battle: { over: false }, postIconSize: 15, layer: null, drawn: 0,
      fonts: { lineHeight: () => 18, width: () => 0 } });
  }
  setLayer(layer) { this.layer = layer; }
  spawnMapPlace() { return { ...HUD_PLACE }; }
  drawSpawnMap() { this.drawn += 1; }
  mapPoint(x, z) { return [x, z]; }
}

// The stage framed as a 16:9 page would: him in the middle, the hologram
// on the right; the camera at rest.
function frame(view) {
  const stage = view.stage;
  assert.ok(stage.frame(16 / 9, { x: -0.1, top: 0.75, bottom: -0.7 }));
  assert.ok(stage.placeMap({ x: 0.62, y: -0.25, w: 0.6, h: 1.1 }));
  for (const spring of [stage.rig.yaw, stage.rig.pitch, stage.rig.crane]) spring.set(0);
  stage.rig.dolly.set(1);
  stage.rig.update(0);
  // Lit (full size), as it is a moment after opening.
  stage.map.strength = 1;
  stage.map.update(0, true);
}

// The interface point where a world point (x, z) shows on the hologram.
function onMap(view, x, z) {
  const stage = view.stage;
  const [mx, my] = view.hud.mapPoint(x, z, MAP_UNITS);
  stage.map.root.updateMatrixWorld(true);
  const world = stage.map.disc.localToWorld(new THREE.Vector3((mx / MAP_UNITS) * 2, (-my / MAP_UNITS) * 2, 0));
  const ndc = world.project(stage.camera);
  return { x: ((ndc.x + 1) / 2) * IW, y: ((1 - ndc.y) / 2) * view.hud.height };
}

function setup(options = {}) {
  const port = new FakeSpawnPort({ ...geonosisData(), ...options });
  const hud = new HudStub();
  const screen = createSpawnScreen({ port, hud, view: "page", clock: "manual", random: () => 0 });
  return { port, hud, screen };
}

test("\"page\" names the page view; none keeps the Classic one", () => {
  const { screen } = setup();
  assert.ok(screen.view instanceof PageView);
  const port = new FakeSpawnPort(geonosisData());
  assert.ok(createSpawnScreen({ port, hud: new HudStub(), clock: "manual" }).view instanceof ClassicView);
});

test("its layer hides the HUD's reinforcements and shows the picked team's battlefield, the picked post ringed", () => {
  const { hud, screen } = setup();
  screen.update(FRAME);
  assert.equal(hud.layer, screen.view.layer);
  assert.equal(hud.layer.teams, false);
  assert.equal(hud.layer.battlefield.team, 1);
  assert.deepEqual(hud.layer.battlefield.rings.map(({ x, z, alpha }) => [x, z, alpha]), [[30, 40, 1]], "cp10");
  screen.model.switchTeam("mouse");
  screen.update(FRAME);
  assert.equal(hud.layer.battlefield.team, 2);
  const rings = hud.layer.battlefield.rings;
  assert.equal(rings.at(-1).alpha, 1, "the new team's post lit");
  assert.ok(rings.some((ring) => ring.x === 30 && ring.alpha < 1), "the old one fading");
});

test("while open the HUD doesn't draw its own spawn map (the stage projects it), and does again after", () => {
  const { port, hud, screen } = setup();
  hud.drawSpawnMap();
  assert.equal(hud.drawn, 0, "borrowed while the screen is open");
  screen.spawn();
  port.step();
  assert.equal(screen.isOpen, false);
  assert.equal(Object.hasOwn(hud, "drawSpawnMap"), false);
  hud.drawSpawnMap();
  assert.equal(hud.drawn, 1);
  port.await();
  assert.equal(screen.isOpen, true);
  hud.drawSpawnMap();
  assert.equal(hud.drawn, 1, "borrowed again on reopening");
  screen.dispose();
  assert.equal(Object.hasOwn(hud, "drawSpawnMap"), false);
});

test("a click on the hologram picks one of the team's own posts, through the input; others take nothing", () => {
  const { screen } = setup();
  const view = screen.view;
  frame(view);
  screen.update(FRAME);
  frame(view);
  assert.equal(screen.model.post, "cp10");
  // cp2, FORWARD CENTER, the team's, at (50, 120).
  const at = onMap(view, 50, 120);
  assert.deepEqual(view.hit(at.x, at.y), { kind: "post", name: "cp2" });
  screen.input.move(at.x + 2, at.y - 2);
  screen.input.button(0, true);
  screen.update(FRAME);
  assert.equal(screen.model.post, "cp2");
  // cp7, TECHNO UNION 1, the enemy's, at (20, -120), on the disc too.
  frame(view);
  const enemy = onMap(view, 20, -120);
  assert.equal(view.hit(enemy.x, enemy.y), null);
  assert.equal(view.hit(80, 700), null, "off the hologram");
});

test("the post under the pointer is ringed faintly and its pin raised; off the hologram nothing is", () => {
  const { hud, screen } = setup();
  const view = screen.view;
  frame(view);
  const at = onMap(view, 50, 120);
  screen.input.move(at.x, at.y);
  screen.update(FRAME);
  const ring = hud.layer.battlefield.rings.find((r) => r.x === 50 && r.z === 120);
  assert.ok(ring && ring.alpha > 0 && ring.alpha < 1);
  assert.equal(view.hovered, "cp2");
  screen.input.move(80, 700);
  screen.update(FRAME);
  assert.equal(view.hovered, null);
});

test("the pause menu over it hides it (hidePointer)", () => {
  const { port, hud } = setup();
  const menu = { isOpen: false, onClose() {}, open() { this.isOpen = true; }, enableRespawn() {} };
  const screen = createSpawnScreen({ port, hud, menu, view: "page", clock: "manual" });
  screen.update(FRAME);
  assert.equal(screen.view.hidePointer, false);
  menu.isOpen = true;
  screen.update(FRAME);
  assert.equal(screen.view.hidePointer, true);
  menu.isOpen = false;
  screen.update(FRAME);
  assert.equal(screen.view.hidePointer, false);
});

test("the page's pointer stays over the canvas; the Classic view hides it for its own", (t) => {
  const listeners = { addEventListener() {}, removeEventListener() {} };
  globalThis.window = listeners;
  t.after(() => delete globalThis.window);
  const canvas = () => ({ ...listeners, style: { cursor: "" }, getBoundingClientRect: () => ({ left: 0, top: 0 }) });
  const page = canvas();
  createSpawnScreen({ port: new FakeSpawnPort(geonosisData()), hud: new HudStub(), element: page, view: "page", clock: "manual" });
  assert.equal(page.style.cursor, "");
  const classic = canvas();
  createSpawnScreen({ port: new FakeSpawnPort(geonosisData()), hud: new HudStub(), element: classic, clock: "manual" });
  assert.equal(classic.style.cursor, "none");
});

test("its texts: the countdown's clock, a label as a panel title, the look's colours", () => {
  assert.equal(clockText(65), "1:05");
  assert.equal(clockText(45.7), "45");
  assert.equal(clockText(-2), "0");
  assert.equal(barText("OBJECTIVES:"), "OBJECTIVES");
  assert.equal(barText("COMMAND POST"), "COMMAND POST");
  const near = (a, b) => a.every((v, i) => Math.abs(v - b[i]) < 1e-6);
  assert.ok(near(cssColor(" rgba(4, 8, 14, .86)"), [4 / 255, 8 / 255, 14 / 255, 0.86]));
  assert.ok(near(cssColor("rgb(255 0 0 / 50%)"), [1, 0, 0, 0.5]));
  assert.ok(near(cssColor("#0c0f15"), [12 / 255, 15 / 255, 21 / 255, 1]));
  assert.equal(cssColor(""), null);
  assert.equal(cssColor("linear-gradient(#000, #fff)"), null);
});
