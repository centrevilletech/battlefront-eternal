import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { fixture } from './flight-fixture.js';
import { VehicleDriver } from '../src/vehicles.js';
import { EntitySeats } from '../src/runtime/entity-seats.js';
import { BattleHud, flightPrompt, seatName, minimapRange } from '../src/hud.js';

// The player at a craft's stick, as the page holds him (tests/flight-lifecycle.test.js).
function mounted() {
  const f = fixture(); f.v.driver = null;
  const soldier = { position: new THREE.Vector3(), previous: new THREE.Vector3(), settled: new THREE.Vector3(), velocity: new THREE.Vector3(), aimYaw: 0, aimPitch: 0, spawn(x, y, z, yaw) { this.position.set(x, y, z); this.velocity.set(0, 0, 0); this.aimYaw = yaw; } };
  const actor = { root: new THREE.Group() }, animator = { gunner() {}, reset() {} }, combat = { dead: false, mounted: false, team: 1 };
  const unit = { id: 7, generation: 1, alive: true, soldier, actor, animator, combat }; combat.owner = unit;
  const driver = new VehicleDriver({ vehicles: f.system, soldier, combat, actor, animator, camera: f.system.camera, collision: f.c, hud: { message() {} }, presentation: { reset() {} } });
  assert.ok(driver.enter(f.v));
  return { ...f, driver, unit, soldier, combat };
}

// A BattleHud with no renderer: its quads and text, by texture name (each
// quad's tint in tints).
function hudDrawing(vehicle, reticleWeapon) {
  const quads = [], texts = [], tints = [], textures = new Map();
  const hud = Object.create(BattleHud.prototype);
  Object.defineProperty(hud, 'height', { value: 768 });
  Object.assign(hud, {
    vehicle, reticle: reticleWeapon ? { x: .5, y: .4, weapon: reticleWeapon } : null, lockMarker: null, targetRelation: null,
    aimCamera: new THREE.PerspectiveCamera(48, 16 / 9), strings: {}, solid: { name: 'solid' },
    texture: (name) => { if (!name) return null; if (!textures.has(name)) textures.set(name, { name, image: { width: 64, height: 64 } }); return textures.get(name); },
    anchor: (x, y) => new THREE.Matrix4().makeTranslation(x * 1024, y * 768, 0),
    batch: { quad: (...args) => { quads.push(args[5]?.name ?? null); tints.push(args[7]); } },
    fonts: { draw: (...args) => texts.push(String(args[4])), width: () => 0, lineHeight: () => 12, capsMiddle: () => 6 },
    time: 0,
  });
  return { hud, quads, texts, tints };
}

test('a seat switch on the page leaves his body seated, as the world has it', () => {
  // His old seat empties before the new one is taken (the seat keys go in
  // seat order), which unseats the body on the way.
  const f = mounted();
  const seats = [{ lease: null, weapons: [] }, { lease: null, weapons: [] }];
  const v = { seats, alive: true }, game = { vehicles: { vehicles: [v] }, units: { byId: () => f.unit }, driver: {}, playerUnit: f.unit };
  f.unit.seat = null;
  const reader = new EntitySeats(game);
  reader.setLease(null,v,[7,1,'pilot',1],value=>{seats[0].lease=value;},0);
  assert.equal(f.unit.seat.index, 0);
  assert.ok(f.combat.mounted); assert.equal(f.unit.targetable, false);
  reader.setLease(null,v,null,value=>{seats[0].lease=value;},0);
  reader.setLease(null,v,[7,1,'gunner',1],value=>{seats[1].lease=value;},1);
  assert.equal(f.unit.seat, seats[1].lease);
  assert.equal(seats[0].lease, null);
  assert.ok(f.combat.mounted, 'still mounted: his own weapon stays out of his hands');
  assert.equal(f.unit.targetable, false, 'still no target of his own while he sits');
  assert.equal(game.driver.lease, seats[1].lease);
  // Off the hull altogether, he is on his feet.
  reader.setLease(null,v,null,value=>{seats[1].lease=value;},1);
  assert.equal(f.unit.seat, null); assert.equal(f.combat.mounted, false); assert.equal(f.unit.targetable, true);
});

test('a pilot\'s reticle follows the nose as drawn; any other seat its view', () => {
  const f = mounted();
  f.v.flightFrameQ = new THREE.Quaternion().setFromAxisAngle(new THREE.Vector3(0, 1, 0), .5);
  f.v.quaternion.identity();
  const nose = f.driver.reticleDirection(new THREE.Vector3());
  assert.ok(nose.distanceTo(new THREE.Vector3(Math.sin(.5), 0, Math.cos(.5))) < 1e-9, 'the drawn frame, which the chase camera follows');
  f.v.flying = false;
  const view = f.driver.reticleDirection(new THREE.Vector3()), expected = f.driver.viewDirection(new THREE.Vector3());
  assert.ok(view.distanceTo(expected) < 1e-9);
});

test('a craft\'s seat draws the weapon\'s own reticle, and leaving it draws none of the craft', () => {
  const weapon = { def: { ReticuleTexture: 'reticule_rifle', ShotDelay: 0 }, wait: 0 };
  const craft = { flying: true, seatIndex: 0, seats: [{ weapons: [] }], flightState: 'Flying' };
  const aboard = hudDrawing(craft, weapon);
  aboard.hud.drawReticle();
  assert.deepEqual(aboard.quads, ['reticule_rifle']);
  const off = hudDrawing(null, null);
  off.hud.drawReticle();
  assert.deepEqual(off.quads, []);
});

test('a craft\'s pilot is prompted from the craft\'s own state, and nobody else is', () => {
  const craft = (fields) => ({ flying: true, alive: true, seatIndex: 0, flightState: 'Flying', flightWarning: '', landingReason: 'Too fast', ...fields });
  // Landed: the original's tool tip, after the takeoff key.
  assert.deepEqual(flightPrompt(craft({ flightState: 'Landed' })), { action: 'takeoff', key: 'game.tooltips.flyertakeoff', text: 'TO TAKE OFF' });
  // Low and slow over ground he can land on: the same key lands it.
  assert.deepEqual(flightPrompt(craft({ landingReason: 'Press SPACE to land' })), { action: 'takeoff', key: null, text: 'TO LAND' });
  // A refused press says why, for as long as the craft keeps the reason.
  assert.equal(flightPrompt(craft({ flightWarning: 'No landing zone' })).text, 'NO LANDING ZONE');
  assert.equal(flightPrompt(craft({ flightState: 'Landed', flightWarning: 'Takeoff path blocked' })).text, 'TAKEOFF PATH BLOCKED');
  // Nothing in flight otherwise, on the way up or down, at the edge of the
  // battlefield (the original's LEAVING BATTLEFIELD says so), in another
  // seat, in a ground vehicle, a wreck, or on foot.
  for (const v of [craft({}), craft({ flightState: 'TakingOff' }), craft({ flightState: 'Landing' }), craft({ flightWarning: 'Leaving battlefield' }),
    craft({ flightState: 'Landed', seatIndex: 1 }), craft({ flightState: 'Landed', flying: false }), craft({ flightState: 'Landed', alive: false }), null]) {
    assert.equal(flightPrompt(v), null);
  }
});

test('the prompt names the bound key in the HUD\'s font, and goes with the seat', () => {
  const landed = { flying: true, alive: true, seatIndex: 0, flightState: 'Landed', flightWarning: '', landingReason: '' };
  const { hud, texts } = hudDrawing(landed, null);
  hud.keyName = (action) => (action === 'takeoff' ? 'SPACE' : '?');
  hud.drawFlightPrompt();
  assert.deepEqual(texts, ['SPACE TO TAKE OFF']);
  hud.strings = { 'game.tooltips.flyertakeoff': 'POUR DECOLLER' };
  texts.length = 0; hud.drawFlightPrompt();
  assert.deepEqual(texts, ['SPACE POUR DECOLLER'], 'the localized tool tip where the import has it');
  hud.vehicle = null;
  texts.length = 0; hud.drawFlightPrompt();
  assert.deepEqual(texts, [], 'off the seat, no prompt');
});

test('seated, his own bar keeps hud_health and the hull takes the second panel, over a pip per seat', () => {
  const me = { id: 1 }, bot = { id: 2, character: {} };
  const seats = [{ lease: { unit: me } }, { lease: { unit: bot } }, { lease: null }];
  const draw = (vehicle, hull) => {
    const d = hudDrawing(vehicle, null);
    Object.assign(d.hud, { arsenal: {}, health: 1, healthIcon: 'hud_health', healthFader: { alpha: 1 }, supply: { health: false },
      healthFlash: 0, hullFlash: 0, second: null, enemy: null, enemyLeft: 0, unitContext: { viewer: me } });
    if (hull) d.hud.setHull(...hull);
    d.hud.drawHealth();
    return d;
  };
  const seated = draw({ seats }, ['HUD_all_xwing_icon', .5]);
  // Three pips: three slots, fills on the two held seats (his white, the bot's green).
  assert.deepEqual(seated.quads.slice(0, 5), ['hud2', 'hud2', 'hud2', 'hud2', 'hud2']);
  assert.deepEqual(seated.tints[1].slice(0, 3), [1, 1, 1]);
  assert.deepEqual(seated.tints[3].slice(0, 3).map((c) => Math.round(c * 255)), [32, 223, 32]);
  assert.ok(seated.quads.includes('hud_health'), 'his own icon on the top bar');
  assert.ok(seated.quads.includes('HUD_all_xwing_icon'), 'the hull\'s on the second panel');
  // A class with no HealthTexture shows the vehicle's own icon.
  assert.ok(draw({ seats }, ['hud_vehiclehealth', 1]).quads.includes('hud_vehiclehealth'));
  // On foot: no pips, no hull.
  const foot = draw(null, null);
  assert.equal(foot.quads.filter((name) => name === 'hud2').length, 0);
  assert.ok(!foot.quads.includes('HUD_all_xwing_icon'));
});

test('a seat\'s panels are named for the seat, by its VehiclePosition', () => {
  const v = (key, seatIndex = 0) => ({ seatIndex, seats: [{ settings: key ? { da1b8c0c: key } : {} }] });
  assert.equal(seatName(v('common.vehiclepositions.pilot')), 'PILOT');
  assert.equal(seatName(v('common.vehiclepositions.gunner')), 'MAIN GUNNER');
  assert.equal(seatName(v('common.vehiclepositions.pilot'), { 'common.vehiclepositions.pilot': 'PILOTE' }), 'PILOTE');
  assert.equal(seatName(v(null)), '', 'a seat with none (the snowspeeder\'s rear gun) shows none');
});

test('each channel\'s reticle takes the colour of what its own weapon is on', () => {
  const { hud } = hudDrawing({ seats: [] }, null);
  hud.setReticleTarget(-1, null, 1);
  assert.deepEqual(hud.reticleColor(0).slice(0, 3).map((c) => Math.round(c * 255)), [223, 32, 32]);
  assert.deepEqual(hud.reticleColor(1).slice(0, 3).map((c) => Math.round(c * 255)), [32, 223, 32]);
  hud.setReticleTarget(null, null, null);
  assert.deepEqual(hud.reticleColor(1).slice(0, 3).map((c) => Math.round(c * 255)), [67, 167, 242]);
});

test('the minimap reaches farther as what he controls goes faster', () => {
  assert.equal(minimapRange(0), 125);
  assert.equal(minimapRange(10), 125);
  assert.equal(minimapRange(55), 187.5);
  assert.equal(minimapRange(100), 250);
  assert.equal(minimapRange(300), 250);
});
