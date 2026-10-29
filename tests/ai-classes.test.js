import test from "node:test";
import assert from "node:assert/strict";
import { Vector3 } from "three";
import { Arsenal } from "../src/weapons.js";
import { Units as Registry } from "../src/registry.js";
import { SnipeHelper, MineHelper, RepairServices, selectSnipe, snipeSuitable, snipeCorridorClear, minePlacement, mineWeapon, remainingAmmo } from "../src/ai/classic/classes.js";
import { HintSet } from "../src/ai/classic/nav/hints.js";
import { ClassicNav } from "../src/ai/classic/nav/index.js";
import { refOf } from "../src/ai/classic/controller.js";
import { FireControl, consumeFire, arsenalInput } from "../src/ai/classic/weapons-ai.js";
import { PlayerCombat } from "../src/combat.js";
import { LiveObjects } from "../src/props.js";

const v = (x = 0, y = 0, z = 0) => new Vector3(x, y, z);
function unit(x = 0, team = 1, extra = {}) {
  return { position: v(x), velocity: v(), sphere: { center: v(x), radius: 0 }, alive: true, generation: 1,
    team, kind: 0, classDef: { MaxHealth: 100, HealthType: "person" }, health: 50, maxHealth: 100, ...extra };
}
function setup(loadout = []) {
  const units = new Registry();
  const owner = unit();
  owner.combat = { arsenal: new Arsenal(loadout, { player: false }), health: 100, maxHealth: 100 };
  units.add(owner);
  const nav = ClassicNav.empty();
  const world = { units, nav, time: 1, battle: { reinforcements: { 1: 100, 2: 100 } }, items: [], missionControls: {} };
  const draws = [];
  const randomA = { int: (lo, hi) => { draws.push([lo, hi]); return lo; }, range: (lo, hi) => { draws.push([lo, hi]); return lo; } };
  const ai = { world, nav, randomA, combat: {}, warnOnce() {} };
  const helper = { arrived: false, status: 0, hasPath: true, goto(p) { this.destination = p.clone(); this.arrived = false; }, stop() { this.stopped = true; }, jet(p) { this.impulse = p.clone(); } };
  const controller = { ai, world, unit: owner, perception: { fire: { channel: 0, burst: {} } }, rangeScale: 1,
    lookAt(p) { this.look = p.clone(); }, aimAt(ref) { this.aim = ref; }, setStance(s) { this.stance = s; }, navControl: { jetting: false } };
  const agent = { ai, world, unit: owner, helper, controller, canEngage: () => true, boardPolicy: () => true };
  controller.agent = agent;
  return { ai, agent, world, owner, units, nav, draws, controller, helper };
}
function hint(x, extra = {}) { return { bit: 1, position: v(x), forward: v(0, 0, 1), mode: 1, primaryStance: 1, radius: 4, registerRadius: 1, reservedAt: -10, ...extra }; }
function install(s, hints) { s.nav.hints = new HintSet(hints); }
function enemy(s, p) { const target = unit(p.x, 2); target.position.copy(p); target.sphere.center.copy(p); s.units.add(target); return target; }
const mine = { class: "minegun", base: "dispenser", channel: 1, RoundsPerClip: 1, ammo: 4, MaxItems: 4, item: { class: "mine", base: "mine" } };
const pack = { class: "pack", base: "dispenser", channel: 1, RoundsPerClip: 1, ammo: 5, ai: { maxRange: 10, extremeRange: 100, implicitRange: false }, item: { base: "powerupitem", PersonHealth: 10, PersonAmmo: 1 } };

test("snipe modes, ticket boundary, and configured forward sphere", () => {
  const s = setup(); const h = hint(30, { mode: 0 }); install(s, [h]); enemy(s, v(30, 0, 100));
  assert.equal(selectSnipe(s.agent, new SnipeHelper(s.agent, false)), false);
  assert.equal(selectSnipe(s.agent, new SnipeHelper(s.agent, true)), true);
  s.world.missionControls = { attackingTeam: 1, snipe: { attacker: 40, defender: 140 } };
  h.mode = 1;
  assert.equal(snipeSuitable(s.agent, h), false);
  h.mode = 0; assert.equal(snipeSuitable(s.agent, h), true);
  s.world.battle.reinforcements[1] = 25; assert.equal(snipeSuitable(s.agent, h), false);
  s.world.battle.reinforcements[1] = 26; assert.equal(snipeSuitable(s.agent, h), true);
});

test("snipe corridor is a strict cylinder within a midpoint sphere", () => {
  const s = setup(); const h = hint(40); const e = enemy(s, v(20, 12, 0));
  assert.equal(snipeCorridorClear(s.agent, h), true);
  e.position.y = 11.999; e.sphere.center.copy(e.position);
  assert.equal(snipeCorridorClear(s.agent, h), false);
  s.world.network = true; s.world.secondaryWorldActive = true;
  assert.equal(snipeCorridorClear(s.agent, h), true);
});

test("snipe reservations do not spend the eleven suitability attempts", () => {
  const s = setup(); const list = Array.from({ length: 13 }, (_, i) => hint(i + 10));
  install(s, list); enemy(s, v(22, 0, 130));
  // First eleven fail the narrow authored range; the twelfth could succeed.
  list.forEach((h, i) => { h.forward = i < 11 ? v(0, 0, -1) : v(0, 0, 1); });
  const helper = new SnipeHelper(s.agent);
  assert.equal(selectSnipe(s.agent, helper), false);
  list[0].reservedAt = s.world.time;
  assert.equal(selectSnipe(s.agent, helper), true);
});

test("occupied snipe helper applies posture, cone fallback, range and 15 second revalidation", () => {
  const s = setup(); const h = hint(0); install(s, [h]); enemy(s, v(0, 0, 80));
  const helper = new SnipeHelper(s.agent); helper.start(h); s.helper.arrived = true; helper.tick(0.1);
  assert.equal(helper.state, 3); assert.equal(s.controller.rangeScale, 2); assert.equal(helper.revalidateAt, 16);
  s.ai.combat.currentTarget = () => ({ position: v(0, 0, -10) }); helper.tick(0.1);
  assert.equal(helper.busy, true); assert.deepEqual(s.controller.look, v(0, 0, 10));
  s.world.battle.reinforcements[1] = 25; s.world.time = 16; helper.tick(0.1); assert.equal(helper.done, false);
  s.world.time = 16.001; helper.tick(0.1); assert.equal(helper.done, true);
});

test("mine availability honors negative ammo, unlimited sentinels and deployed cap", () => {
  const s = setup([mine]); const w = s.owner.combat.arsenal.weapons[0];
  assert.equal(remainingAmmo(w), 4); w.reserve = -2; assert.equal(remainingAmmo(w), -1); assert.equal(mineWeapon(s.agent), w);
  w.reserve = 3.4028234663852886e38; assert.equal(remainingAmmo(w), 2147483647);
  s.world.items = Array.from({ length: 4 }, () => ({ def: mine.item, owner: s.owner })); assert.equal(mineWeapon(s.agent), null);
});

test("mine lattice uses strict radius, allied spacing and inclusive 2.25 boundary", () => {
  const s = setup([mine]); const h = hint(0, { bit: 16, radius: 3 });
  assert.deepEqual(minePlacement(s.agent, h), v(0, 0, 0));
  s.world.items = [{ mine: true, team: 2, position: v(), radius: 0 }]; assert.deepEqual(minePlacement(s.agent, h), v());
  s.world.items[0].team = 1; assert.equal(minePlacement(s.agent, h), null);
  s.world.items[0].position.x = 2.25; assert.deepEqual(minePlacement(s.agent, h), v());
});

test("mine helper switches first, fires once, waits strictly over three seconds", () => {
  const s = setup([mine]); const h = hint(0, { bit: 16 }); const helper = new MineHelper(s.agent);
  helper.start(h, v()); s.helper.arrived = true; helper.tick(0.1);
  assert.equal(helper.state, 3); assert.equal(s.controller.fireA, undefined);
  helper.tick(0.1); assert.equal(s.controller.fireA, true); s.controller.fireA = false;
  s.world.time = 4; helper.tick(0.1); assert.equal(helper.state, 3); assert.equal(s.controller.fireA, false);
  s.world.time = 4.01; helper.tick(0.1); assert.equal(helper.state, 2);
});

test("failed mine arrival applies thirty-second cooldown", () => {
  const s = setup([mine]); const h = hint(0); const helper = new MineHelper(s.agent);
  helper.start(h, v()); s.helper.arrived = true; s.helper.status = 1; helper.tick(0.1);
  assert.equal(helper.done, true); assert.equal(h.reservedAt, 31);
});

test("mine selection caps at three gathered hints and vetoes an enemy mine", () => {
  const s = setup([mine]); const list = Array.from({ length: 4 }, (_, i) => hint(i * 10, { bit: 16, radius: i === 3 ? 4 : 0 }));
  install(s, list); const helper = new MineHelper(s.agent);
  assert.equal(helper.select(), false);
  s.world.time = 12; list[0].radius = 4;
  s.world.items = [{ mine: true, team: 2, position: v(), radius: 0 }];
  assert.equal(helper.select(), false);
  s.world.time = 23; s.world.items[0].team = 1; assert.equal(helper.select(), true);
});

test("pilot job selection prioritizes moving humans with literal cos(15 radians)", () => {
  const s = setup([pack]); const service = new RepairServices(s.agent.ai);
  const aiSoldier = unit(1); const building = unit(2, 1, { kind: 9 }); const human = unit(20, 1, { human: true, velocity: v(1), forward: () => v(0, 0, 1) });
  for (const u of [aiSoldier, building, human]) s.units.add(u);
  assert.equal(service.findRepairJob(s.agent).target.unit, human);
  human.position.x = 30; human.sphere.center.x = 30;
  assert.equal(service.findRepairJob(s.agent).target.unit, building);
  s.world.battle.reinforcements[1] = 25; assert.equal(service.findRepairJob(s.agent), null);
});

test("repair service retains the health-based eight-second guard for ammo jobs", () => {
  const s = setup([pack]); const service = new RepairServices(s.agent.ai); const target = unit(1, 1, { human: true });
  target.combat = { health: 100, maxHealth: 100, arsenal: new Arsenal([{ channel: 0, RoundsPerClip: 10, ammo: 4 }]) };
  target.combat.arsenal.weapons[0].reserve = 0;
  s.agent.job = { target: refOf(target), kind: "ammo" }; service.startRepairService(s.agent);
  service.repairService(s.agent); service.repairService(s.agent);
  assert.equal(s.controller.fireA, true);
  s.world.time = 9; assert.ok(service.repairService(s.agent));
  s.world.time = 9.001; assert.equal(service.repairService(s.agent), null);
});

test("repair approach runs over twenty seconds only with an unstuck path", () => {
  const s = setup([pack]); const service = new RepairServices(s.agent.ai); const target = unit(40);
  s.agent.job = { target: refOf(target), kind: "health" }; service.startRepairService(s.agent);
  s.world.time = 21; s.helper.hasPath = false; assert.ok(service.repairService(s.agent));
  s.world.time = 21.01; assert.equal(service.repairService(s.agent), null);
  s.helper.hasPath = true; assert.ok(service.repairService(s.agent));
  s.helper.status = 1; assert.equal(service.repairService(s.agent), null);
});

test("a dropped powerup ends matching service without granting its resource", () => {
  const s = setup([pack]); const services = new RepairServices(s.ai); const target = unit(1);
  s.agent.job = { target: refOf(target), kind: "health" }; services.startRepairService(s.agent);
  s.world.items = [{ position: v(2), def: { base: "powerupitem", PersonHealth: 10 }, team: 2 }];
  assert.equal(services.repairService(s.agent), null); assert.equal(target.health, 50);
});

test("service weakrefs admit dead build/rebuild targets but reject another generation", () => {
  const repair = { base: "repair", channel: 0, RoundsPerClip: -1, ammo: 1, BuildingRebuild: 20 };
  const s = setup([repair]); const services = new RepairServices(s.agent.ai);
  const target = unit(2, 0, { alive: false, healthType: 5, kind: 9 });
  s.world.serviceCandidates = () => [target]; s.agent.job = services.findRepairJob(s.agent); assert.ok(s.agent.job);
  services.startRepairService(s.agent); assert.ok(services.repairService(s.agent));
  target.generation++; assert.equal(services.repairService(s.agent), null);
});

test("jet query keeps AABB corners and consumes a random permutation with horizontal ties", () => {
  const s = setup(); const a = hint(39, { bit: 8, position: v(39, 39, 39) }); const b = hint(39, { bit: 8, position: v(39, -39, 39) });
  install(s, [a, b]); assert.equal(s.nav.jetHints.find(s.controller), a);
  assert.deepEqual(s.draws, [[0, 1], [0, 0]]);
  a.reservedAt = s.world.time; assert.equal(s.nav.jetHints.find(s.controller), b);
  s.world.time += 4; assert.equal(s.nav.jetHints.find(s.controller), a);
});

test("jet helper preserves authored impulse magnitude and one-second completion boundary", () => {
  const s = setup(); const h = hint(2, { bit: 8, forward: v(0, 2, 3) });
  s.nav.jetHints.enter(s.controller, h); assert.deepEqual(s.helper.destination, h.position);
  s.helper.arrived = true; assert.equal(s.nav.jetHints.tick(s.controller), false);
  assert.deepEqual(s.helper.destination, v(0, 160, 240)); assert.deepEqual(s.helper.impulse, h.forward);
  s.world.time = 2; assert.equal(s.nav.jetHints.tick(s.controller), false);
  s.world.time = 2.01; s.controller.navControl.jetting = true; assert.equal(s.nav.jetHints.tick(s.controller), false);
  s.controller.navControl.jetting = false; assert.equal(s.nav.jetHints.tick(s.controller), true);
});

test("mine job drives signed secondary input and consumes real dispenser ammunition", () => {
  const def = { ...mine, ShotDelay: 0.5, ReloadTime: 0.75, TriggerSingle: 1, SalvoCount: 1,
    ShotsPerSalvo: 1, SalvoDelay: 0, InitialSalvoDelay: 0, HeatRecoverRate: 0, HeatPerShot: 0 };
  const s = setup([def]); s.controller.perception.fire = new FireControl(); s.owner.intent = { buttons: [0, 0] };
  const helper = new MineHelper(s.agent); helper.start(hint(0, { bit: 16 }), v()); s.helper.arrived = true;
  let previous = [0, 0]; let shots = 0;
  for (let i = 0; i < 12; i++) {
    s.controller.fireA = false;
    helper.tick(0.05); consumeFire(s.controller, s.owner, s.world);
    const input = arsenalInput(previous, s.owner.intent); previous = s.owner.intent.buttons.slice();
    const fired = s.owner.combat.arsenal.update(0.05, input, { blocked: false, posture: "stand", stick: 0, zoomed: false });
    shots += fired.length;
    assert.equal(input.primary.held, false);
    s.world.time += 0.05;
  }
  assert.equal(shots, 1); assert.equal(s.controller.perception.fire.channel, 1);
  const w = s.owner.combat.arsenal.weapons[0]; assert.ok(w.clip + w.reserve < 3.01);
});

test("pilot input fires a real fusion cutter and shared gameplay applies its repair quantity", () => {
  const def = { base: "repair", class: "cutter", channel: 1, RoundsPerClip: -1, ammo: 1,
    BuildingHealth: 60, ShotDelay: 0.5, HeatPerShot: 0.06, HeatRecoverRate: 0.16, HeatThreshold: 0.7,
    TargetEnemy: 0, TargetNeutral: 1, TargetFriendly: 1, ai: { maxRange: 3, extremeRange: 100, implicitRange: false } };
  const s = setup([def]); s.owner.team = 2; s.controller.perception.fire = new FireControl(); s.owner.intent = { buttons: [0, 0] };
  const building = { position: v(1), health: 100, maxHealth: 300, healthType: "building", alive: true, team: 2,
    setHealth(value) { this.health = Math.min(value, this.maxHealth); } };
  const target = unit(1, 2, { kind: 9, healthType: 4, building });
  Object.defineProperties(target, { health: { get: () => building.health }, maxHealth: { get: () => building.maxHealth } });
  const live = Object.create(LiveObjects.prototype); live.playerTeam = 1;
  const combat = s.owner.combat; combat.live = live; combat.team = 2; combat.repairTarget = building;
  const services = new RepairServices(s.ai); s.agent.job = { target: refOf(target), kind: "health" }; services.startRepairService(s.agent);
  let previous = [0, 0]; let repairs = 0;
  for (let i = 0; i < 4; i++) {
    s.controller.fireA = false; services.repairService(s.agent);
    consumeFire(s.controller, s.owner, s.world);
    const input = arsenalInput(previous, s.owner.intent); previous = s.owner.intent.buttons.slice();
    for (const shot of combat.arsenal.update(0.05, input, { blocked: false, repairTarget: building })) {
      assert.equal(shot.repair, true); PlayerCombat.prototype.repairTick.call(combat, shot.weapon); repairs++;
    }
    s.world.time += 0.05;
  }
  assert.equal(repairs, 1); assert.equal(building.health, 160);
  assert.equal(combat.arsenal.weapons[0].channel, 1);
});
