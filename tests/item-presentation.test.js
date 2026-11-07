import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import { Scene, Vector3, Mesh, BoxGeometry, MeshBasicMaterial } from 'three';
import { Items } from '../src/items.js';
import { ItemPresentation } from '../src/presentation/items.js';
import { ItemHeader as H, ItemLimits as LIMIT } from '../src/presentation/items-layout.js';
import { loaded } from './helpers/item-kernel.js';

// The page's side of the item phase over synthetic worlds that need no game
// files: planes and a water level answer the kernel's rays and crossings.
// reference.json holds what the page's JavaScript item presentation drew
// over the same scripted authority runs, as bit patterns, recorded before
// the kernel replaced it. The native test (rust/bfe-presentation/tests/items.rs)
// replays the same file.
const reference = JSON.parse(fs.readFileSync(new URL('./fixtures/items-presentation/reference.json', import.meta.url)));
const buf = Buffer.alloc(8);
const hx = (n) => { buf.writeDoubleBE(n); return buf.toString('hex'); };
const un = (h) => Buffer.from(h, 'hex').readDoubleBE(0);
const hv = (v) => [hx(v.x), hx(v.y), hx(v.z)];
const unv = (a) => new Vector3(un(a[0]), un(a[1]), un(a[2]));

const SIZES = { mine: [0.4, 0.12, 0.4], health: [0.3, 0.3, 0.3], ammo: null, bomb: [0.5, 0.5, 0.2] };
const COLORS = { mine: [255, 40, 40, 255], ammo: [0, 255, 0, 128] };
const model = (name) => {
  const box = SIZES[name === 'dual' ? 'health' : name];
  return box ? () => { const m = new Mesh(new BoxGeometry(...box), new MeshBasicMaterial()); m.geometry.computeBoundingSphere(); return m; } : null;
};
// The size a model measures, without making one (Items.size).
const size = (name) => {
  const box = SIZES[name === 'dual' ? 'health' : name];
  if (!box) return null;
  const geometry = new BoxGeometry(...box);
  geometry.computeBoundingSphere();
  return geometry.boundingSphere;
};
const definitions = reference.definitions.map((d) => ({
  name: d.name, base: d.mine ? 'mine' : 'powerupitem', GeometryName: d.mesh ? (d.name === 'health' ? 'dual' : d.name) : undefined,
  Gravity: un(d.gravity), Rebound: un(d.rebound), Friction: un(d.friction), LightRadius: un(d.lightRadius) || undefined, LightColor: COLORS[d.name],
}));
const registry = { definitions: new Map(definitions.map((d, i) => [i + 1, d])) };

function world(client) {
  const planes = client.planes.map((p) => ({ point: unv(p.point), normal: unv(p.normal) })), ripples = [];
  const level = client.level === null ? null : un(client.level);
  return {
    ripples,
    collision: {
      cast(start, dir, length, hit) {
        let best = null, at = Infinity;
        for (const p of planes) {
          const d = p.normal.dot(dir);
          if (d >= 0) continue;
          const t = p.normal.dot(p.point.clone().sub(start)) / d;
          if (t < 0 || t > length || t >= at) continue;
          at = t; best = p.normal;
        }
        if (!best) return null;
        hit.distance = at; hit.point.copy(start).addScaledVector(dir, at); hit.normal.copy(best);
        return hit;
      },
    },
    water: level === null ? null : {
      crossing(from, to, out) {
        if (!(from.y >= level && to.y < level)) return null;
        const t = (from.y - level) / (from.y - to.y);
        out.set(from.x + (to.x - from.x) * t, level, from.z + (to.z - from.z) * t);
        return out;
      },
      ripple(position) { ripples.push(hv(position)); },
    },
  };
}

function page(client, pools = { mine: 60, powerupitem: 30 }) {
  const w = world(client), lights = [];
  const items = new Items({ scene: new Scene(), collision: w.collision, water: w.water, effects: {}, lights: { add(position, rgb, radius) { lights.push({ position: hv(position), rgb: [...rgb], radius: hx(radius) }); } }, model, size, explode() {}, pools });
  return { items, w, lights, presentation: new ItemPresentation(items, registry, loaded) };
}

const recordOf = (r) => (r.active
  ? { kind: 'item', id: r.id, generation: r.generation, tick: r.tick, active: true, definitionId: r.definition, position: r.position.map(un), velocity: r.velocity.map(un), yaw: un(r.yaw), up: r.up.map(un) }
  : { kind: 'item', id: r.id, generation: r.generation, tick: r.tick, active: false });

for (const scenario of reference.scenarios) {
  test(`item presentation draws ${scenario.name} as the JavaScript it replaced did, bit for bit`, () => {
    const p = page(scenario.client);
    scenario.frames.forEach((frame, f) => {
      for (const r of frame.records) p.presentation.receive(recordOf(r), r.epoch);
      p.lights.length = 0; p.w.ripples.length = 0;
      p.presentation.present(un(frame.tick));
      const at = `${scenario.name} frame ${f}`, want = frame.expect;
      const views = [...p.presentation.slots].filter(([, entry]) => entry.view);
      assert.equal(views.length, want.views.length, `${at}: views`);
      views.forEach(([id, entry], i) => {
        const w = want.views[i], view = entry.view;
        assert.equal(id, w.id, `${at}: slot`);
        assert.deepEqual(hv(view.position), w.position, `${at}: position of ${id}`);
        if (w.quaternion) {
          const q = view.mesh.quaternion;
          assert.deepEqual([hx(q.x), hx(q.y), hx(q.z), hx(q.w)], w.quaternion, `${at}: orientation of ${id}`);
          assert.deepEqual(hv(view.mesh.position), w.position, `${at}: model of ${id}`);
        } else assert.equal(view.mesh, null);
      });
      assert.equal(p.items.scene.children.length, want.meshes, `${at}: models in the scene`);
      assert.deepEqual(p.lights.map((l) => ({ position: l.position, radius: l.radius })), want.lights, `${at}: lights`);
      assert.deepEqual(p.w.ripples, want.ripples, `${at}: ripples`);
    });
    assert.equal(p.presentation.header[H.REFUSED_RECORDS], 0);
  });
}

test('definitions reach the kernel as the page measured them', () => {
  const p = page({ planes: [], level: null });
  reference.definitions.forEach((d, i) => {
    p.presentation.receive({ kind: 'item', id: i, generation: 1, tick: 1, active: true, definitionId: i + 1, position: [0, 5, 0], velocity: [0, 1, 0], yaw: 0, up: [0, 1, 0] }, 1);
    const at = p.presentation.indices.get(i + 1) * 7;
    assert.equal(hx(p.presentation.definitions[at + 4]), d.radius, `${d.name} radius`);
    assert.deepEqual(p.presentation.table[i].rgb.map(hx), d.lightColor, `${d.name} light colour`);
  });
});

test('a full store, a burst of records and a stalled clock degrade without throwing', () => {
  const warn = console.warn, warnings = [];
  console.warn = (m) => warnings.push(m);
  try {
    const p = page({ planes: [{ point: [0, 0, 0].map(hx), normal: [0, 1, 0].map(hx) }], level: hx(-2) });
    const record = (id, generation, tick, definitionId = 1) => ({ kind: 'item', id, generation, tick, active: true, definitionId, position: [id % 7, 3, 0], velocity: [1, 2, 0], yaw: 0, up: [0, 1, 0] });
    // More items than any map has, in one packet past the batch block.
    for (let id = 0; id < LIMIT.SLOTS + 80; id++) p.presentation.receive(record(id, 1, 10, 1 + (id % 4)), 1);
    p.presentation.present(10);
    assert.equal(p.items.views.size, LIMIT.SLOTS);
    p.presentation.present(11.5);
    assert.equal(p.items.views.size, LIMIT.SLOTS);
    assert.equal(warnings.length, 1);
    assert.match(warnings[0], /80 records refused/);
    p.presentation.present(12);
    assert.equal(warnings.length, 1, 'reported once');
    // A stalled, jumping and reversed clock.
    for (const tick of [12, 12, 5, 0, 1e6, 3, Infinity, 20, NaN, 21]) p.presentation.present(tick);
    // More definitions than the kernel's table holds.
    const many = Array.from({ length: LIMIT.DEFINITIONS + 5 }, (_, i) => [1000 + i, { base: 'powerupitem', Gravity: 1 }]);
    for (const [id, def] of many) registry.definitions.set(id, def);
    try {
      for (const [n, [id]] of many.entries()) p.presentation.receive(record(n % 3, 100 + n, 30, id), 1);
      p.presentation.present(31);
    } finally {
      for (const [id] of many) registry.definitions.delete(id);
    }
  } finally {
    console.warn = warn;
  }
});

test('an unknown definition is a content mismatch the page reports', () => {
  const p = page({ planes: [], level: null });
  assert.throws(() => p.presentation.receive({ kind: 'item', id: 0, generation: 1, tick: 1, active: true, definitionId: 999, position: [0, 0, 0], velocity: [0, 0, 0], yaw: 0, up: [0, 1, 0] }, 1), /missing item definition/);
});

// The module's memory is shared by every presentation phase: a unit joining
// between two frames can grow it, which replaces each phase's views of it.
test('items follow the clock after another phase has grown the module memory', async () => {
  const { Items } = await import('../src/items.js');
  const items = new Items({ scene: new Scene(), collision: { cast: () => null }, effects: {}, water: null, lights: { add() {} }, model: () => null, explode() {}, pools: { mine: 2, powerupitem: 2 } });
  const def = { base: 'powerupitem', Gravity: 1 };
  const presentation = new ItemPresentation(items, { definitions: new Map([[1, def]]) }, loaded);
  presentation.receive({ kind: 'item', id: 0, generation: 1, tick: 1, active: true, definitionId: 1, position: [0, 50, 0], velocity: [0, 1, 0], yaw: 0, up: [0, 1, 0] }, 1);
  presentation.present(3);
  const at = () => presentation.slots.get(0).view.position.y;
  const before = at();
  loaded.memory.grow(1);
  presentation.present(12);
  assert.ok(at() < before, `the item flew on to tick 12 (${before} to ${at()})`);
});
