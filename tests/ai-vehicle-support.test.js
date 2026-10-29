import test from 'node:test';
import assert from 'node:assert/strict';
import { hubsAt, outOfReach, pickSupport, routes, SUPPORT_RANGE, supportHubs } from '../src/ai/classic/vehicles/support.js';

// A post a hull cannot stand on (port; docs/ai-vehicles.md "Posts out of a
// hull's reach"): synthetic graph and barriers.

const hub = (index, x, z, mask = 8) => ({ index, x, y: 0, z, radius: 4, mask });
function graph(hubs, links) {
  // links: [[from, to]] routable on table 3 (mask 8), both ways.
  const ok = new Set(links.flatMap(([a, b]) => [`${a}>${b}`, `${b}>${a}`]));
  return {
    hubs,
    table: (mask) => (mask & 8 ? 3 : -1),
    word: (from, table, to) => (table === 3 && ok.has(`${from.index}>${to.index}`) ? 1 : 0),
    attach: (p, mask, out) => {
      const h = hubs.find((x) => (x.mask & mask) && Math.hypot(x.x - p.x, x.z - p.z) < x.radius);
      out.type = h ? 1 : 0; out.hub = h ?? null; out.arc = null;
      return out;
    },
    nearestArc: () => null,
  };
}
// One barrier: |x| < 20 and |z| < 20 for vehicles (mask 8), pushed out to
// the edge at x = 20 + r when push is set, or nowhere.
const barriers = (push) => ({
  inside: (p, r) => Math.abs(p.x) < 20 + r && Math.abs(p.z) < 20 + r,
  pointClear(p, r) { return !this.inside(p, r); },
  correct(p, r) { if (!this.inside(p, r)) return true; if (!push) return false; p.x = 20 + r + 0.01; return true; },
});

test('a post is out of reach when the correction fails or moves it past the reach', () => {
  assert.equal(outOfReach(barriers(false), { x: 0, y: 0, z: 0 }, 15, 2, 8), true, 'correction fails');
  assert.equal(outOfReach(barriers(true), { x: 0, y: 0, z: 0 }, 15, 2, 8), true, 'pushed 22 m');
  assert.equal(outOfReach(barriers(true), { x: 0, y: 0, z: 0 }, 30, 2, 8), false, 'pushed within the reach');
  assert.equal(outOfReach(barriers(true), { x: 50, y: 0, z: 0 }, 15, 2, 8), false, 'clear already');
  assert.equal(outOfReach(null, { x: 0, y: 0, z: 0 }, 15, 2, 8), false, 'no barriers');
});

test('support hubs: the mask, the range, clear of barriers, in sight; nearest first', () => {
  const hubs = [hub(0, 300, 0), hub(1, 40, 0), hub(2, 0, 10), hub(3, 0, 60), hub(4, -70, 0), hub(5, 30, 30, 1), hub(6, 0, -SUPPORT_RANGE - 5)];
  const nav = { graph: graph(hubs, []), barriers: barriers(false) };
  const blind = new Set([3]);
  const los = (eye) => !blind.has(hubs.find((h) => h.x === eye.x && h.z === eye.z)?.index);
  const found = supportHubs(nav, los, { x: 0, y: 0, z: 0 }, 2, 8, 4);
  assert.deepEqual(found.map((c) => c.hub.index), [1, 4], 'not 0 (range), 2 (barrier), 3 (sight), 5 (mask), 6 (range)');
  assert.equal(found[0].distance, 40);
});

test('pickSupport: only hubs a route reaches from the pilot; a spread among the nearest', () => {
  const hubs = [hub(0, 200, 0), hub(1, 40, 0), hub(2, -45, 0), hub(3, 0, 90)];
  const g = graph(hubs, [[0, 2], [0, 3]]);
  const cands = [{ hub: hubs[1], distance: 40 }, { hub: hubs[2], distance: 45 }, { hub: hubs[3], distance: 90 }];
  assert.deepEqual(hubsAt(g, { x: 201, z: 0 }, 8).map((h) => h.index), [0]);
  assert.equal(routes(g, [hubs[0]], hubs[1], 8), false);
  assert.equal(pickSupport(g, cands, { x: 201, y: 0, z: 0 }, 8, null), hubs[2], 'hub 1 has no route; hub 2 is the nearest routable');
  const draws = new Set();
  const all = graph(hubs, [[0, 1], [0, 2], [0, 3]]);
  for (let i = 0; i < 2; i++) draws.add(pickSupport(all, cands, { x: 201, y: 0, z: 0 }, 8, { int: () => i }).index);
  assert.deepEqual([...draws].sort(), [1, 2], 'within 15 m of the nearest: either; never the one 50 m farther');
  assert.equal(pickSupport(all, cands, { x: 1000, y: 0, z: 0 }, 8, null), null, 'off the graph');
});
