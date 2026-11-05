import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { Battlefield, LeavingWarning } from '../src/battlefield.js';
import { Boundary } from '../src/ai/classic/nav/boundary.js';
import { Character } from '../src/battle.js';
import { attachBattlefieldSim } from '../src/battlefield-sim.js';
import { loadSim } from '../src/sim/module.js';

const SQUARE = [[-100, 0, -100], [-100, 0, 100], [100, 0, 100], [100, 0, -100]];
async function pair(setup = () => {}) {
  async function make(rust) {
    const field = new Battlefield(new Boundary([SQUARE])), warning = new LeavingWarning();
    const events = [], characters = [];
    const add = (x = 150) => {
      const c = new Character(characters.length, 1);
      const u = c.unit = { id: characters.length, alive: true, removed: false, position: new THREE.Vector3(x, 0, 0), seat: null,
        combat: { dead: false, remotes: null, damage(amount) {
          events.push(['kill', u.id, amount, c.outOfBounds]);
          this.dead = true; u.alive = false;
        } } };
      characters.push(c);
      return c;
    };
    add();
    const state = { field, warning, events, characters, add };
    setup(state);
    if (rust) attachBattlefieldSim(await loadSim(), field, warning);
    return state;
  }
  const js = await make(false), rust = await make(true);
  const snapshot = p => ({ characters: p.characters.map(c => [c.index, c.team, c.outOfBounds, c.kills, c.deaths, c.unit?.alive, c.unit?.combat.dead]),
    warning: { ...p.warning, alpha: p.warning.alpha }, events: p.events });
  return { js, rust, step(fn) {
    assert.deepEqual(fn(rust), fn(js));
    assert.deepEqual(snapshot(rust), snapshot(js));
  } };
}

test('native battlefield uses compiled edge order and preserves boundary special values', async () => {
  const { step } = await pair();
  for (const polygons of [[SQUARE], [], [[]], [SQUARE.slice().reverse()], [SQUARE, SQUARE.map(([x, y, z]) => [x + 250, y, z])]]) {
    step(p => { p.field.boundary = new Boundary(polygons); });
    for (const x of [-Infinity, -100, -0, 0, 100, 150, 300, Infinity, NaN]) {
      for (const z of [-101, 0, 100, NaN]) step(p => p.field.outside(new THREE.Vector3(x, 99, z)));
    }
    step(p => p.field.controlsOutside(null));
    step(p => p.field.controlsOutside(p.characters[0].unit));
    step(p => p.field.controlsOutside(p.characters[0].unit, { position: new THREE.Vector3() }, { position: new THREE.Vector3(350, 0, 0) }));
  }
});

test('native countdowns reset, skip absent bodies, and kill at the same tick', async () => {
  const { step } = await pair(({ add }) => { add(0); add(100); add(-200); });
  step(p => { p.characters[2].unit.removed = true; p.characters[3].unit = null; });
  for (let i = 0; i < 540; i++) step(p => p.field.update(1 / 60, p.characters));
  step(p => { p.characters[0].unit.position.x = 0; p.field.update(0, p.characters); });
  step(p => { p.characters[0].unit.position.x = 150; p.field.update(1 / 60, p.characters); });
  for (let i = 0; i < 600; i++) step(p => p.field.update(1 / 60, p.characters));
  for (const dt of [NaN, -0, Infinity, -Infinity, -1]) step(p => p.field.update(dt, p.characters));
});

test('native deaths observe current seats and appended Characters after nested callbacks', async () => {
  const { step } = await pair(p => {
    const c = p.characters[0], unit = c.unit;
    const hull = { id: 0, position: new THREE.Vector3(150, 0, 0), lastAttacker: 'previous', damage(amount) {
      p.events.push(['wreck', amount, this.lastAttacker, c.outOfBounds]);
      unit.combat.damage(amount);
    } };
    unit.seat = { vehicle: hull };
    const droid = { dead: false, position: new THREE.Vector3(150, 0, 0) };
    unit.combat.remotes = { piloted: droid, kill(d) {
      p.events.push(['droid', d === droid, c.outOfBounds]);
      d.dead = true; this.piloted = null;
      unit.seat = null;
      const next = p.add(150); next.outOfBounds = 0;
      c.kills++;
      p.field.update(0, [next]);
    } };
    c.outOfBounds = 0;
  });
  step(p => p.field.update(1 / 60, p.characters));
  step(p => { assert.equal(p.characters.length, 2); assert.equal(p.events.some(e => e[0] === 'wreck'), false); });
  step(p => p.field.update(1 / 60, p.characters));
});

test('native hull, turret and droid countdowns preserve death order and credit clearing', async () => {
  for (const kind of ['vehicle', 'turret', 'droid']) {
    const { step } = await pair(p => {
      const c = p.characters[0], unit = c.unit;
      unit.position.x = 0;
      const target = { id: 0, dead: false, position: new THREE.Vector3(150, 0, 0), lastAttacker: 'previous', damage(amount) {
        p.events.push(['wreck', amount, this.lastAttacker, c.outOfBounds]);
        unit.combat.damage(amount);
      } };
      // F2 clears canonical hull credit before the battlefield applies damage.
      if (kind === 'vehicle') target.system = { f2: { command(v, op) { assert.equal(op, 3); v.lastAttacker = null; } } };
      if (kind === 'droid') unit.combat.remotes = { piloted: target, kill(d) {
        p.events.push(['droid', d === target, c.outOfBounds]); d.dead = true; this.piloted = null;
      } };
      else unit.seat = { [kind]: target };
    });
    for (let i = 0; i < 602; i++) step(p => p.field.update(1 / 60, p.characters));
  }
});

test('native leaving warning preserves fractional, repeated and discontinuous tick steps', async () => {
  const { step } = await pair();
  for (let i = 0; i < 800; i++) {
    step(p => p.warning.update(i / 2, i % 91 > 0));
    step(p => p.warning.update(i / 2, i % 91 > 0));
  }
  for (const tick of [1e8, 0, -1, Infinity, Infinity, -Infinity, NaN]) step(p => p.warning.update(tick, true));
  step(p => p.warning.update(0, false));
});
