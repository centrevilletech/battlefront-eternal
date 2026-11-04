import test from 'node:test';
import assert from 'node:assert/strict';
import { Vector3 } from 'three';
import { HitHistory } from '../src/runtime/hit-history.js';
import { attachHitHistorySim } from '../src/runtime/hit-history-sim.js';
import { loadSim } from '../src/sim/module.js';

const xyz = (x = 0, y = 0, z = 0) => new Vector3(x, y, z);
const result = h => h && [h.unit.id, h.part, h.distance, ...h.point.toArray(), ...h.normal.toArray()];
const sampled = frame => frame && ({ tick: frame.tick, entries: frame.entries.slice(0, frame.count).map(e => ({
  unit: e.unit.id, generation: e.generation, position: e.position.toArray(), center: e.center.toArray(), radius: e.radius,
  shapes: e.shapes.slice(0, e.shapeCount).map(s => ({ type: s.type, part: s.part, center: s.center.toArray(), radius: s.radius, yaw: s.yaw, half: s.half?.isVector3 ? s.half.toArray() : s.half })),
})) });

async function pair(units, maxTicks = 48, instance = null) {
  const list = { all: () => units.filter(u => !u.gone) };
  const js = new HitHistory(list, maxTicks), rust = new HitHistory(list, maxTicks);
  instance ??= await loadSim();
  const adapter = attachHitHistorySim(instance, rust);
  return { js, rust, adapter, instance, capture(tick) { js.capture(tick); rust.capture(tick); } };
}

const body = (id, x = 0, z = 5) => ({
  id, generation: 1, alive: true, removed: false, networkHuman: true, position: xyz(x, 0, z),
  get sphere() { return { center: this.position, radius: 2 }; },
  hitShapes() { return [{ type: 'sphere', part: 'body', center: this.position, radius: .5 }]; },
});

test('native history matches 49-frame retention, fractional samples and life discontinuities', async () => {
  const target = body(1), p = await pair([target]);
  try {
    for (let tick = 0; tick <= 80; tick++) {
      target.position.x = tick / 60;
      p.capture(tick);
      for (const age of [0, .25, 1, 2.5, 47, 48, 48.5, 49, 60]) {
        const at = tick - age;
        assert.deepEqual(sampled(p.rust.sample(at)), sampled(p.js.sample(at)));
        assert.deepEqual(p.rust.position(target, at)?.toArray(), p.js.position(target, at)?.toArray());
      }
    }
    assert.equal(p.rust.frame(31), null);
    assert.equal(p.rust.frames.length, 49);
    target.position.x += 10; p.capture(81);
    assert.deepEqual(sampled(p.rust.sample(80.5)), sampled(p.js.sample(80.5)));
    target.generation++; p.capture(82);
    assert.equal(p.rust.position(target, 81), undefined);
    assert.equal(p.rust.sample(81.5).count, 0);
    target.alive = false; p.capture(83);
    assert.equal(p.rust.sample(82.5).count, 0);
    for (const tick of [NaN, Infinity, -Infinity]) assert.equal(p.rust.sample(tick), null);
  } finally { p.adapter.dispose(); }
});

test('native rewind respects per-channel stamps, limits and current shooter eligibility', async () => {
  const owner = body(1), p = await pair([owner]);
  try {
    for (let tick = 0; tick <= 80; tick++) p.capture(tick);
    for (const lead of [undefined, NaN, -4, 0, 6, 24, 100]) for (const playout of [undefined, Infinity, -1, 4, 10, 99]) {
      owner.networkInputLeadTicks = lead; owner.networkPlayoutMaxTicks = playout;
      for (const view of [undefined, NaN, Infinity, -1, 0, 30.25, 60.75, 80, 81]) {
        owner.networkShot = { viewTick: view };
        assert.equal(p.rust.shotTick(owner), p.js.shotTick(owner));
      }
    }
    owner.networkShots = [{ button: 0, viewTick: 70.5 }, { button: 1, viewTick: 74.25 }];
    for (const channel of [0, 1]) assert.equal(p.rust.shotTick(owner, channel), p.js.shotTick(owner, channel));
    for (const change of [{ seat: {} }, { seat: null, alive: false }, { alive: true, networkHuman: false }, { networkHuman: true, generation: 2 }]) {
      Object.assign(owner, change);
      assert.equal(p.rust.shotTick(owner), p.js.shotTick(owner));
    }
  } finally { p.adapter.dispose(); }
});

test('native history preserves topology, optional fields, arbitrary parts and signed zero', async () => {
  const unit = body(1);
  unit.position = xyz(-0, 0, -0);
  unit.shapes = [
    { type: 'sphere', center: xyz(-0), radius: .5, part: undefined },
    { type: 'cylinder', center: xyz(), radius: .3, half: .7, part: 'legs' },
    { type: 'box', center: xyz(), half: xyz(.2, .3, .4), yaw: 3.1, part: null },
    { type: 'capsule', center: xyz(), half: xyz(.1, .4, -.1), radius: .3, part: 'arm' },
  ];
  unit.hitShapes = () => unit.shapes;
  const p = await pair([unit]);
  try {
    p.capture(0);
    assert.deepEqual(sampled(p.rust.frame(0)), sampled(p.js.frame(0)));
    unit.shapes[2].yaw = -3.1; unit.position.x = .1;
    for (const shape of unit.shapes) shape.center.x += .1;
    p.capture(1);
    for (const at of [.01, .25, .5, .999]) assert.deepEqual(sampled(p.rust.sample(at)), sampled(p.js.sample(at)));
    unit.shapes[1].part = 'head'; p.capture(2);
    assert.deepEqual(sampled(p.rust.sample(1.5)), sampled(p.js.sample(1.5)));
    unit.shapes.pop(); p.capture(3);
    assert.deepEqual(sampled(p.rust.sample(2.5)), sampled(p.js.sample(2.5)));
  } finally { p.adapter.dispose(); }
});

test('native history twins mixed-shape raycasts and fractional poses over 600 captures', async () => {
  let seed = 37;
  const rand = () => (seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0) / 4294967296;
  const parts = ['head', 'body', 'legs'];
  const shapeOf = (kind, at) => {
    const center = at.clone().add(xyz(rand() - .5, rand() * 1.8, rand() - .5)), part = parts[Math.floor(rand() * 3)];
    if (kind === 0) return { type: 'sphere', part, center, radius: .2 + rand() * .3 };
    if (kind === 1) return { type: 'capsule', part, center, half: xyz(0, .2 + rand() * .4, 0), radius: .15 + rand() * .2 };
    if (kind === 2) return { type: 'box', part, center, half: xyz(.2 + rand() * .3, .3 + rand() * .5, .2 + rand() * .3), yaw: rand() * 6 };
    return { type: 'cylinder', part, center, radius: .3 + rand() * .3, half: .4 + rand() * .5 };
  };
  const units = Array.from({ length: 30 }, (_, id) => ({ ...body(id),
    alive: rand() < .8, seat: null, position: xyz(rand() * 40 - 20, 0, rand() * 40 - 20),
    sphere: { center: xyz(), radius: 2 }, shapes: [], hitShapes() { return this.shapes; },
  }));
  const reshape = u => { u.shapes = Array.from({ length: Math.floor(rand() * 7) }, () => shapeOf(Math.floor(rand() * 4), u.position)); };
  for (const u of units) { u.sphere.center.copy(u.position); reshape(u); }
  const p = await pair(units);
  let hits = 0, rewound = 0;
  try {
    for (let tick = 0; tick < 600; tick++) {
      for (const u of units) {
        const r = rand();
        if (r < .02) { u.alive = !u.alive; u.dead = !u.alive; if (u.alive) u.generation++; }
        else if (r < .03) u.position.x += 8;
        else if (r < .05) reshape(u);
        else if (r < .06) u.seat = u.seat ? null : { exposed: rand() < .5 };
        else if (r < .065) u.gone = !u.gone;
        u.position.x += (rand() - .5) * .3; u.position.z += (rand() - .5) * .3;
        u.sphere.center.copy(u.position);
        for (const s of u.shapes) s.center.x += (rand() - .5) * .3;
      }
      if (tick % 31 === 0) units.reverse();
      p.capture(tick);
      for (let q = 0; q < 12; q++) {
        const back = rand() < .5 ? Math.floor(rand() * 40) : rand() * 40, at = Math.max(0, tick - back);
        const target = units[Math.floor(rand() * units.length)], origin = xyz(rand() * 60 - 30, 1, rand() * 60 - 30);
        const direction = target.position.clone().add(xyz(0, 1, 0)).sub(origin).add(xyz(rand() - .5, rand() - .5, rand() - .5).multiplyScalar(.2)).normalize();
        const exclude = units[Math.floor(rand() * units.length)], riders = rand() < .5;
        const a = result(p.rust.raycast(at, origin, direction, 80, exclude, undefined, null, riders));
        assert.deepEqual(a, result(p.js.raycast(at, origin, direction, 80, exclude, undefined, null, riders)), `ray at ${at} after ${tick}`);
        if (a) hits++;
        assert.deepEqual(p.rust.position(target, at)?.toArray(), p.js.position(target, at)?.toArray());
        target.networkViewTick = at;
        const shot = p.rust.shotTick(target);
        assert.equal(shot, p.js.shotTick(target));
        if (shot !== null) rewound++;
      }
    }
    assert.ok(hits > 300 && rewound > 500, `${hits} hits and ${rewound} rewound shots`);
  } finally { p.adapter.dispose(); }
});

test('history instances, callbacks, memory growth and disposal stay isolated', async () => {
  const first = body(1), second = body(2), third = body(3, 0, 7);
  const p = await pair([first, second, third]);
  const other = await pair([body(9, 5)], 48, p.instance);
  try {
    p.capture(0); p.capture(1); other.capture(0);
    p.instance.exports.memory.grow(1);
    assert.deepEqual(sampled(p.rust.sample(.5)), sampled(p.js.sample(.5)));
    const trace = [];
    const hit = p.rust.raycast(.5, xyz(), xyz(0, 0, 1), 20, null, undefined, unit => {
      trace.push(unit.id);
      assert.deepEqual(sampled(other.rust.sample(0)), sampled(other.js.sample(0)));
      if (unit === first) second.alive = false;
      return true;
    });
    assert.equal(hit.unit, first, 'equal distances keep the first entry');
    assert.deepEqual(trace, [1, 3], 'later live eligibility observes earlier callbacks');
    assert.throws(() => p.rust.raycast(0, xyz(), xyz(0,0,1), 20, null, undefined, () => { throw Error('filter'); }), /filter/);
    assert.equal(p.rust.raycast(0, xyz(), xyz(0,0,1), 20).unit, first, 'a thrown filter releases its query');
    p.adapter.dispose(); p.adapter.dispose();
    assert.throws(() => p.rust.capture(2), /disposed/);
    assert.equal(other.rust.frame(0).entries[0].unit.id, 9);
  } finally { p.adapter.dispose(); other.adapter.dispose(); }
});
