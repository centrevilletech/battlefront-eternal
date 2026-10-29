// The arc caster's Enhanced lightning (src/arc-lightning.js): which bolts
// it draws and where, who it finds holding and struck, the shock on a struck
// soldier (his body's bones as drawn put back after), the crackle at a
// charging gun, the light it throws, and Video > Bolt Caster choosing it
// over the 2004 ribbons (src/lightning.js).
import test from "node:test";
import assert from "node:assert/strict";
import * as THREE from "three";
import { ArcBatch, ArcLightning, displace } from "../src/arc-lightning.js";
import { Lightning } from "../src/lightning.js";
import { DEFAULTS, PERFORMANCE_PRESETS } from "../src/settings.js";
import { optionEntry } from "../src/menus/options.js";

const BOLT = { Texture: "arccaster_lightning", Width: 0.5, FadeTime: 0.6, SpreadFactor: 0.8, InterpolationSpeed: 5, BranchFactor: 0.15, BranchSpreadFactor: 0.8, BranchLength: 4, MaxBranches: 3, BreakDistance: 1, TextureSize: 3.5, ChildBreakDistance: 1.5, ChildTextureSize: 2, ChildWidth: 0.4, ChildSpreadFactor: 1, NumChildren: 3, Color: [130, 170, 255, 250], ChildColor: [140, 180, 255, 100] };
const ARC = { class: "rep_weap_inf_arccaster", MaxPressedTime: 2.25, ordnance: { LightningEffect: "arccaster_lightning" } };

// A soldier as a page draws him: a skinned body on a few bones, a gun with
// a fire point, and his gun origin as drawn.
function soldier(x, z, { generation = 1, gun = null } = {}) {
  const root = new THREE.Group();
  root.position.set(x, 0, z);
  const names = ["DummyRoot", "bone_pelvis", "bone_b_spine", "bone_ribcage", "bone_neck", "bone_head", "bone_l_upperarm", "bone_l_forearm", "bone_l_hand", "bone_r_upperarm", "bone_r_forearm", "bone_r_hand", "bone_l_foot", "bone_r_foot"];
  const at = { bone_pelvis: [0, 1, 0], bone_b_spine: [0, 0.15, 0], bone_ribcage: [0, 0.25, 0], bone_neck: [0, 0.25, 0], bone_head: [0, 0.1, 0], bone_l_upperarm: [0.2, 0.15, 0], bone_l_forearm: [0.3, 0, 0], bone_l_hand: [0.25, 0, 0], bone_r_upperarm: [-0.2, 0.15, 0], bone_r_forearm: [-0.3, 0, 0], bone_r_hand: [-0.25, 0, 0], bone_l_foot: [0.15, -0.95, 0], bone_r_foot: [-0.15, -0.95, 0] };
  const parent = { bone_pelvis: "DummyRoot", bone_b_spine: "bone_pelvis", bone_ribcage: "bone_b_spine", bone_neck: "bone_ribcage", bone_head: "bone_neck", bone_l_upperarm: "bone_ribcage", bone_l_forearm: "bone_l_upperarm", bone_l_hand: "bone_l_forearm", bone_r_upperarm: "bone_ribcage", bone_r_forearm: "bone_r_upperarm", bone_r_hand: "bone_r_forearm", bone_l_foot: "bone_pelvis", bone_r_foot: "bone_pelvis" };
  const byName = {};
  const bones = names.map((name) => {
    const bone = new THREE.Bone();
    bone.name = name;
    bone.position.fromArray(at[name] || [0, 0, 0]);
    byName[name] = bone;
    return bone;
  });
  for (const bone of bones) if (parent[bone.name]) byName[parent[bone.name]].add(bone);
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute([0, 1, 0, 0.2, 1.5, 0, -0.2, 1.5, 0], 3));
  geometry.setAttribute("normal", new THREE.Float32BufferAttribute([0, 0, 1, 0, 0, 1, 0, 0, 1], 3));
  geometry.setAttribute("skinIndex", new THREE.Uint16BufferAttribute([1, 0, 0, 0, 3, 0, 0, 0, 3, 0, 0, 0], 4));
  geometry.setAttribute("skinWeight", new THREE.Float32BufferAttribute([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], 4));
  geometry.setIndex([0, 1, 2]);
  const body = new THREE.SkinnedMesh(geometry, new THREE.MeshBasicMaterial());
  body.boundingSphere = new THREE.Sphere(new THREE.Vector3(0, 0.9, 0), 2.2);
  root.add(body);
  body.add(bones[0]);
  body.bind(new THREE.Skeleton(bones));
  const weapon = gun ? { def: ARC, charging: false, held: 0, levels: [{ MaxPressedTime: 2.25 }] } : null;
  const unit = {
    generation, removed: false, dead: false, walker: false, seat: null, held: null, soldier: {},
    actor: { root, body, bones, byName },
    combat: {
      dead: false, mounted: false, firstPerson: false,
      arsenal: { weapons: weapon ? [weapon] : [], current: () => weapon },
      drawnGunOrigin: (out) => out.set(root.position.x, 1.5, root.position.z + 0.25),
      firePoint: () => ({ position: new THREE.Vector3(root.position.x, 1.4, root.position.z + 0.8), quaternion: new THREE.Quaternion() }),
    },
  };
  root.updateMatrixWorld(true);
  return unit;
}

function lightPool() {
  const added = [], flashed = [];
  return { added, flashed, add: (position, rgb, radius, scale) => added.push({ position: position.clone(), rgb: [...rgb], radius, scale }), flash: (position, rgb, radius, duration) => flashed.push({ position: position.clone(), radius, duration }) };
}

function camera() {
  const c = new THREE.PerspectiveCamera(60, 16 / 9, 0.5, 200);
  c.position.set(-6, 2, 5);
  c.lookAt(0, 1, 6);
  c.updateMatrixWorld();
  return c;
}

const bolt = (start, end, follow = null, charge = 1) => ({ def: BOLT, start: start.clone(), end: end.clone(), charge, follow, life: BOLT.FadeTime });

test("a jagged channel keeps its ends, has 2^depth segments, and stays within its spread", () => {
  const path = { points: Array.from({ length: 257 }, () => new THREE.Vector3()), count: 0 };
  const a = new THREE.Vector3(0, 1, 0), b = new THREE.Vector3(0, 1, 20);
  displace(path, a, b, 6, new THREE.Vector3(1, 0, 0), new THREE.Vector3(0, 1, 0), { rough: 0.2, spread: 0.6, seed: 7, jag: 11, time: 0.3, writhe: 2.6, coarse: 2 });
  assert.equal(path.count, 65);
  assert.deepEqual(path.points[0].toArray(), [0, 1, 0]);
  assert.deepEqual(path.points[64].toArray(), [0, 1, 20]);
  let most = 0;
  for (let i = 0; i < path.count; i++) most = Math.max(most, Math.hypot(path.points[i].x, path.points[i].y - 1));
  // The levels' pushes add up to at most spread / (1 - 0.64).
  assert.ok(most > 0.05 && most < 0.6 / (1 - 0.64), `pushed ${most} m off the line`);
  for (let i = 1; i < path.count; i++) assert.ok(path.points[i].z >= path.points[i - 1].z - 1e-9, "it never doubles back along the line");
});

test("a held bolt is drawn from its shooter's muzzle as drawn; one ending on a soldier is anchored to his chest and shocks him", () => {
  const shooter = soldier(0, 0, { gun: true });
  const victim = soldier(0, 9);
  const units = { all: () => [shooter, victim] };
  const lights = lightPool();
  const arcs = new ArcLightning(new THREE.Scene(), { lights, units });
  const start = new THREE.Vector3(0, 1.5, 0.25);
  arcs.add(bolt(start, new THREE.Vector3(0, 1, 9), (out) => shooter.combat.drawnGunOrigin(out)));
  const [record] = arcs.bolts;
  assert.equal(record.shooter, shooter, "the soldier whose gun origin the hold follows");
  assert.equal(record.target, victim, "the soldier 1 m below the bolt's end");
  const view = camera();
  arcs.update(1 / 60, view);
  assert.deepEqual(record.from.toArray(), [0, 1.4, 0.8], "from the gun's fire point");
  const chest = victim.actor.byName.bone_ribcage.getWorldPosition(new THREE.Vector3());
  assert.ok(record.to.distanceTo(chest) < 1e-9, "to his ribcage");
  assert.ok(arcs.batch.count > 0, "the leader is drawn");
  for (let i = 0; i < 6; i++) arcs.update(1 / 60, view);
  assert.ok(record.arrived, "landed after its leader crossed");
  assert.ok(arcs.shocks.has(victim), "he is shocked");
  assert.ok(arcs.sparks.length >= 10, "sparks fly where it landed");
  assert.ok(lights.flashed.some((f) => f.position.distanceTo(chest) < 0.5), "a flash of light where it landed");
  assert.ok(lights.added.some((l) => l.position.distanceTo(new THREE.Vector3(0, 1.4, 0.8)) < 1e-6), "light at the gun");
  assert.ok(lights.added.some((l) => l.position.distanceTo(chest) < 0.5 && l.radius >= 5), "light where it struck");
});

test("a chain's next link waits for the one before it, and runs from the soldier it struck", () => {
  const shooter = soldier(0, 0, { gun: true });
  const first = soldier(0, 8), second = soldier(1, 14);
  const arcs = new ArcLightning(new THREE.Scene(), { units: { all: () => [shooter, first, second] } });
  const T1 = new THREE.Vector3(0, 1, 8), T2 = new THREE.Vector3(1, 1, 14);
  arcs.add(bolt(new THREE.Vector3(0, 1.5, 0.25), T1, (out) => shooter.combat.drawnGunOrigin(out)));
  arcs.add(bolt(T1, T2));
  const [a, b] = arcs.bolts;
  assert.equal(b.delay, a.delay + 0.05);
  assert.equal(b.source, first);
  assert.equal(b.target, second);
  assert.equal(b.shooter, null, "a link is held to no one");
  const view = camera();
  arcs.update(0.03, view);
  assert.equal(b.age - b.delay < 0, true, "not yet shown");
  for (let i = 0; i < 12; i++) arcs.update(1 / 60, view);
  const chest = first.actor.byName.bone_ribcage.getWorldPosition(new THREE.Vector3());
  assert.ok(b.from.distanceTo(chest) < 1e-9, "from the first one's chest as drawn");
  assert.ok(arcs.shocks.has(first) && arcs.shocks.has(second));
});

test("a shock twitches the upper body over the pose and puts it back; his glow follows his body's layer", () => {
  const shooter = soldier(0, 0, { gun: true });
  const victim = soldier(0, 6);
  const arcs = new ArcLightning(new THREE.Scene(), { units: { all: () => [shooter, victim] } });
  const head = victim.actor.byName.bone_head;
  const posed = head.quaternion.clone();
  arcs.add(bolt(new THREE.Vector3(0, 1.5, 0.25), new THREE.Vector3(0, 1, 6), (out) => shooter.combat.drawnGunOrigin(out)));
  const view = camera();
  for (let i = 0; i < 8; i++) arcs.update(1 / 60, view);
  assert.ok(arcs.shocks.has(victim));
  assert.ok(!head.quaternion.equals(posed), "jerked off its pose");
  // The animation poses the head anew: that becomes the pose under the twitch.
  const clip = new THREE.Quaternion().setFromEuler(new THREE.Euler(0.3, 0, 0));
  head.quaternion.copy(clip);
  arcs.update(1 / 60, view);
  assert.ok(head.quaternion.angleTo(clip) < 0.35, "the twitch sits on the new pose");
  // His own body on a hidden layer (first person): the glow goes with it.
  victim.actor.body.layers.set(5);
  arcs.update(1 / 60, view);
  const overlay = victim.actor.root.children.find((child) => child.name === "arc-shock");
  assert.ok(overlay, "a glow over his body");
  assert.equal(overlay.layers.mask, victim.actor.body.layers.mask);
  assert.equal(overlay.skeleton, victim.actor.body.skeleton, "skinned to his own bones");
  for (let i = 0; i < 160; i++) arcs.update(1 / 60, view);
  assert.ok(!arcs.shocks.has(victim), "over after its time");
  assert.ok(head.quaternion.equals(clip), "the pose is put back");
  assert.equal(overlay.visible, false);
});

test("a body given a new life is shocked no more", () => {
  const shooter = soldier(0, 0, { gun: true });
  const victim = soldier(0, 6);
  const arcs = new ArcLightning(new THREE.Scene(), { units: { all: () => [shooter, victim] } });
  arcs.add(bolt(new THREE.Vector3(0, 1.5, 0.25), new THREE.Vector3(0, 1, 6), (out) => shooter.combat.drawnGunOrigin(out)));
  const view = camera();
  for (let i = 0; i < 8; i++) arcs.update(1 / 60, view);
  assert.ok(arcs.shocks.has(victim));
  victim.generation += 1;
  arcs.update(1 / 60, view);
  assert.ok(!arcs.shocks.has(victim));
});

test("a lone bolt that met nothing fizzles out in the air: no sparks, no flash, no shock", () => {
  const shooter = soldier(0, 0, { gun: true });
  shooter.combat.arsenal.weapons[0].def = { ...ARC, ordnance: { ...ARC.ordnance, ConeLength: 30 } };
  const lights = lightPool();
  const collision = { cast: () => null };
  const arcs = new ArcLightning(new THREE.Scene(), { lights, collision, units: { all: () => [shooter] } });
  const start = new THREE.Vector3(0, 1.5, 0.25);
  arcs.add(bolt(start, new THREE.Vector3(0, 1.5, 30.25), (out) => shooter.combat.drawnGunOrigin(out)));
  const [record] = arcs.bolts;
  assert.equal(record.air, true);
  const view = camera();
  for (let i = 0; i < 10; i++) arcs.update(1 / 60, view);
  assert.ok(record.arrived);
  assert.equal(arcs.sparks.length, 0);
  assert.equal(lights.flashed.length, 0);
  assert.ok(arcs.batch.count > 0, "still drawn");
  // One that ends short of the cone's length struck a mine or a droid.
  const other = new ArcLightning(new THREE.Scene(), { lights: lightPool(), collision, units: { all: () => [shooter] } });
  other.add(bolt(start, new THREE.Vector3(0, 1, 12), (out) => shooter.combat.drawnGunOrigin(out)));
  assert.equal(other.bolts[0].air, false);
});

test("struck in first person, arcs crawl over his own arms, drawn in the weapon's own scene", () => {
  const shooter = soldier(0, 0, { gun: true });
  const player = soldier(0, 7);
  // His body on a hidden layer and his first-person arms shown.
  player.actor.body.layers.set(5);
  const arms = new THREE.Group();
  const bones = ["r_upperarm", "r_forearm", "r_hand", "l_hand", "hp_weapons"].map((name, i) => {
    const bone = new THREE.Bone();
    bone.name = `bone_${name}`;
    bone.position.set(0.05 * i, -0.05 * i, 0.08 * i);
    return bone;
  });
  bones.reduce((parent, bone) => (parent.add(bone), bone), arms);
  arms.position.set(0, 1.6, 7);
  player.combat.firstPerson = true;
  player.combat.view = { root: arms, bones };
  const viewScene = new THREE.Scene();
  viewScene.add(arms);
  const arcs = new ArcLightning(new THREE.Scene(), { units: { all: () => [shooter, player] }, viewScene });
  arcs.add(bolt(new THREE.Vector3(0, 1.5, 0.25), new THREE.Vector3(0, 1, 7), (out) => shooter.combat.drawnGunOrigin(out)));
  const view = camera();
  let most = 0;
  for (let i = 0; i < 20; i++) {
    arcs.update(1 / 60, view);
    most = Math.max(most, arcs.viewBatch.count);
  }
  assert.ok(arcs.shocks.has(player));
  assert.ok(most > 0, "arcs over his arms");
  assert.ok(viewScene.children.includes(arcs.viewBatch.mesh));
});

test("a droideka's shield stops the bolt on its bubble: it crackles there, and he is not shocked", () => {
  const shooter = soldier(0, 0, { gun: true });
  const droideka = soldier(0, 6);
  droideka.walker = true;
  const hits = [];
  const bubble = { on: true, mesh: { visible: true, position: new THREE.Vector3(0, 1, 6), scale: new THREE.Vector3(1.25, 1.25, 1.25) }, hit: (source, amount) => hits.push({ source: source.clone(), amount }) };
  droideka.bubble = bubble;
  const arcs = new ArcLightning(new THREE.Scene(), { units: { all: () => [shooter, droideka] } });
  arcs.add(bolt(new THREE.Vector3(0, 1.5, 0.25), new THREE.Vector3(0, 1, 6), (out) => shooter.combat.drawnGunOrigin(out)));
  const [record] = arcs.bolts;
  const view = camera();
  for (let i = 0; i < 10; i++) arcs.update(1 / 60, view);
  assert.ok(record.arrived);
  assert.ok(Math.abs(record.to.distanceTo(bubble.mesh.position) - 1.25) < 1e-6, "on the bubble");
  assert.ok(record.to.z < 6, "on the side toward the shooter");
  assert.equal(arcs.shocks.has(droideka), false, "his body is not shocked");
  assert.ok(arcs.shields.has(droideka), "his shield is");
  assert.equal(hits.length, 0, "the world's own hit ripples the bubble, from where the arc started");
  assert.ok(arcs.sparks.length > 0);
  const overlay = droideka.actor.root.children.find((child) => child.name === "arc-shock");
  assert.equal(overlay, undefined, "no glow built over him");
  // A link on from him leaves the bubble toward its next target.
  const next = soldier(1, 12);
  const all = [shooter, droideka, next];
  const chain = new ArcLightning(new THREE.Scene(), { units: { all: () => all } });
  chain.add(bolt(new THREE.Vector3(0, 1.5, 0.25), new THREE.Vector3(0, 1, 6), (out) => shooter.combat.drawnGunOrigin(out)));
  chain.add(bolt(new THREE.Vector3(0, 1, 6), new THREE.Vector3(1, 1, 12)));
  for (let i = 0; i < 16; i++) chain.update(1 / 60, view);
  const link = chain.bolts[1];
  assert.ok(Math.abs(link.from.distanceTo(bubble.mesh.position) - 1.25) < 1e-6, "from the bubble");
  assert.ok(link.from.z > 6, "on the far side");
  // Shield down: he takes the shock.
  bubble.on = false;
  for (let i = 0; i < 80; i++) arcs.update(1 / 60, view);
  arcs.add(bolt(new THREE.Vector3(0, 1.5, 0.25), new THREE.Vector3(0, 1, 6), (out) => shooter.combat.drawnGunOrigin(out)));
  for (let i = 0; i < 10; i++) arcs.update(1 / 60, view);
  assert.ok(arcs.shocks.has(droideka));
});

test("a gun held at a charge crackles at its fire point, brighter as the charge builds", () => {
  const shooter = soldier(0, 0, { gun: true });
  const lights = lightPool();
  const arcs = new ArcLightning(new THREE.Scene(), { lights, units: { all: () => [shooter] }, effects: { lightning: { arccaster_lightning: BOLT } } });
  const view = camera();
  arcs.update(1 / 60, view);
  assert.equal(arcs.batch.count, 0, "nothing while not charging");
  const weapon = shooter.combat.arsenal.weapons[0];
  weapon.charging = true;
  weapon.held = 0.2;
  arcs.update(1 / 60, view);
  const low = lights.added.at(-1);
  assert.ok(arcs.batch.count > 0);
  assert.deepEqual(low.position.toArray(), [0, 1.4, 0.8], "at the fire point");
  weapon.held = 2.25;
  arcs.update(1 / 60, view);
  const high = lights.added.at(-1);
  assert.ok(high.radius > low.radius, "it reaches further charged");
  weapon.charging = false;
  arcs.update(1 / 60, view);
  assert.equal(arcs.charges.size, 0);
});

test("Video > Bolt Caster: Enhanced by default and in the richer presets; Classic draws the 2004 ribbons alone", () => {
  assert.equal(DEFAULTS.arcCasterEffects, "enhanced");
  assert.equal(PERFORMANCE_PRESETS.minimum.values.arcCasterEffects, "classic");
  assert.equal(PERFORMANCE_PRESETS.performance.values.arcCasterEffects, "classic");
  assert.equal(PERFORMANCE_PRESETS.balanced.values.arcCasterEffects, "enhanced");
  assert.equal(PERFORMANCE_PRESETS.quality.values.arcCasterEffects, "enhanced");
  const entry = optionEntry("arcCasterEffects");
  assert.equal(entry.label, "BOLT CASTER");
  assert.equal(entry.apply, "live");

  const values = { arcCasterEffects: "classic" };
  const settings = { get: (key) => values[key] };
  const scene = new THREE.Scene();
  const lightning = new Lightning(scene, () => null, { settings });
  assert.equal(lightning.enhanced, null, "nothing of the Enhanced look is built for Classic");
  const view = camera();
  lightning.spawn(BOLT, new THREE.Vector3(0, 1.5, 0), new THREE.Vector3(0, 1, 10), 1);
  lightning.update(1 / 60, view);
  assert.equal(lightning.mesh.visible, true, "the ribbons");
  values.arcCasterEffects = "enhanced";
  lightning.spawn(BOLT, new THREE.Vector3(0, 1.5, 0), new THREE.Vector3(0, 1, 10), 1);
  lightning.update(1 / 60, view);
  assert.equal(lightning.mesh.visible, false, "no ribbons");
  assert.ok(lightning.enhanced.batch.count > 0, "lightning instead");
  values.arcCasterEffects = "classic";
  lightning.update(1 / 60, view);
  assert.equal(lightning.enhanced.batch.count, 0, "switched back, nothing of it stays");
  assert.equal(lightning.mesh.visible, true);
});

test("a busy fight's capsules all draw: growing the batch lifts three's instance limit, and one range uploads them", () => {
  const batch = new ArcBatch(2);
  // three records the limit at the first draw, from the buffers it binds.
  batch.geometry._maxInstanceCount = 2;
  batch.begin();
  const p = new THREE.Vector3();
  for (let i = 0; i < 5; i++) batch.push(p.set(i, 0, 0), p, null, null, [1, 1, 1], 1, 0.1, 0.2, 1);
  batch.end();
  assert.ok(batch.capacity >= 5);
  assert.equal(batch.geometry._maxInstanceCount, undefined, "every instance is drawn after growing");
  assert.equal(batch.geometry.instanceCount, 5);
  assert.deepEqual(batch.buffer.updateRanges.map((r) => [r.start, r.count]), [[0, 5 * 20]], "one upload of what was drawn");
  assert.deepEqual([batch.data[0], batch.data[20], batch.data[80]], [0, 1, 4], "kept across the growth");
  // The next frame reuses the same range record.
  const range = batch.buffer.updateRanges[0];
  batch.buffer.updateRanges.length = 0;
  batch.begin();
  batch.push(p, p, null, null, [1, 1, 1], 1, 0.1, 0.2, 1);
  batch.end();
  assert.equal(batch.buffer.updateRanges[0], range);
  assert.equal(range.count, 20);
});

test("while warming, an empty batch draws one dark capsule, so the driver builds its pipeline before the first bolt", () => {
  const batch = new ArcBatch(4);
  batch.begin();
  batch.push(new THREE.Vector3(1, 2, 3), new THREE.Vector3(1, 2, 3), null, null, [1, 1, 1], 1, 0.1, 0.2, 1);
  batch.end(true);
  batch.begin();
  batch.end(true);
  assert.equal(batch.geometry.instanceCount, 1, "one instance drawn");
  assert.ok(batch.mesh.visible);
  assert.deepEqual(Array.from(batch.data.subarray(0, 20)), new Array(20).fill(0), "all zero: no brightness, nothing lit");
  assert.deepEqual(batch.buffer.updateRanges.map((r) => [r.start, r.count]), [[0, 20]]);
  batch.buffer.updateRanges.length = 0;
  batch.begin();
  batch.end(false);
  assert.equal(batch.geometry.instanceCount, 0, "warmed: empty draws nothing");
  assert.equal(batch.mesh.visible, false);
  const lightning = new ArcLightning(new THREE.Scene());
  const warm = lightning.warm;
  assert.ok(warm.geometry.attributes.position.array.some((x) => x !== 0), "the shock stand-in is a real triangle");
  assert.equal(warm.material.uniforms.intensity.value, 0, "adding nothing");
});

test("a line is written into the batch as push() lays a segment out, joints and taper included", () => {
  const arcs = new ArcLightning(new THREE.Scene());
  const path = { points: [[0, 1, 0], [0.3, 1.2, 2], [-0.2, 0.9, 4], [0.1, 1.1, 6], [0, 1, 8]].map((v) => new THREE.Vector3(...v)), count: 5 };
  const rgb = [0.4, 0.7, 1];
  arcs.batch.begin();
  arcs.line(path, 3.5, rgb, 0.8, 0.05, 0.2, 0.9, 0.3);
  // push(), segment by segment, as the line was drawn before it was
  // written straight into the batch.
  const expected = new ArcBatch(8);
  expected.begin();
  const normal = (i) => {
    const into = path.points[i].clone().sub(path.points[i - 1]).normalize(), out = path.points[i + 1].clone().sub(path.points[i]).normalize();
    return into.add(out).normalize();
  };
  const n = 4;
  for (let i = 0; i < n; i++) {
    const b = i === n - 1 ? path.points[i].clone().lerp(path.points[i + 1], 0.5) : path.points[i + 1];
    const scale = 1 + (0.3 - 1) * ((i + 0.5) / 4);
    expected.push(path.points[i], b, i > 0 ? normal(i) : null, i + 1 < n ? normal(i + 1) : null, rgb, 0.8, 0.05 * scale, 0.2 * scale, 0.9);
  }
  assert.equal(arcs.batch.count, 4);
  const got = Array.from(arcs.batch.data.subarray(0, 4 * 20)), want = Array.from(expected.data.subarray(0, 4 * 20));
  for (let i = 0; i < want.length; i++) assert.ok(Math.abs(got[i] - want[i]) < 1e-6, `float ${i}: ${got[i]} != ${want[i]}`);
});

test("spent sparks are reused, not remade, and drawn as short streaks", () => {
  const arcs = new ArcLightning(new THREE.Scene());
  const record = { floor: 0, from: new THREE.Vector3(0, 1, 0), to: new THREE.Vector3(0, 1, 5), surface: null, target: null, rgb: [0.5, 0.7, 1] };
  arcs.spray(record, new THREE.Vector3(0, 1, 5), 10, 1);
  const first = new Set(arcs.sparks);
  arcs.batch.begin();
  arcs.drawSparks(1 / 60);
  assert.equal(arcs.batch.count, 10);
  for (let i = 0; i < 90; i++) arcs.drawSparks(1 / 60);
  assert.equal(arcs.sparks.length, 0);
  assert.equal(arcs.sparkPool.length, 10);
  arcs.spray(record, new THREE.Vector3(0, 1, 5), 10, 1);
  assert.ok(arcs.sparks.every((spark) => first.has(spark)), "the same ten");
  assert.ok(arcs.sparks.every((spark) => spark.age === 0 && !spark.bounced && spark.p.distanceTo(record.to) < 1e-9), "each started afresh");
});
