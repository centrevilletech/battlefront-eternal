import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { CommandPosts } from '../src/commandposts.js';
import { MobilePost } from '../src/mobile-posts.js';
import { attachPostsSim } from '../src/posts-sim.js';
import { loadSim } from '../src/sim/module.js';
import { ExportContext, exportState } from '../src/sim/runtime.js';

const at = (team, x = 0) => ({ team, position: new THREE.Vector3(x, 0, 0) });
const event = e => ({ kind: e.kind, post: e.post?.name, vehicle: e.vehicle?.id, from: e.from, by: e.by, to: e.to, destroyed: e.destroyed });

async function pair() {
  const make = async rust => {
    const posts = new CommandPosts(null, { commandPost: { neutralizeTime: 1, captureTime: 1 }, commandPosts: [
      { name: 'cp', team: 1, matrix: new THREE.Matrix4().toArray(), hologram: [0, 0, 0], capture: { type: 'sphere', size: [5, 5, 5], matrix: new THREE.Matrix4().toArray() } },
    ] }, { headless: true, playerTeam: 1, teams: { captureTeam: team => team === 7 ? 1 : team } });
    const hull = id => ({ id, name: `command${id}`, label: `Command ${id}`, team: 2, controllingTeam: 2, alive: true, yaw: 0.5,
      def: { base: 'commandwalker', settings: { ValueBleed: '37.9' } },
      aiTarget: { generation: 1 }, position: new THREE.Vector3(20, 0, 3), size: new THREE.Vector3(3, 4, 5), speed: 2,
      seats: [{ lease: null }] });
    const one = hull(0), two = hull(1); two.alive = false;
    const slot = { desc: { classes: { 1: one.name, 2: two.name } }, variants: new Map([[one.name, one], [two.name, two]]), vehicle: one };
    posts.attachVehicles({ slots: [slot], data: { classes: { [one.name]: one.def, [two.name]: two.def } } }, {
      [two.name]: { valueBleed: 17, values: { atk: [1, 2], def: [] }, spawnPoints: [[1, 2, 3, 4]] },
    });
    const building = { id: 0, alive: true };
    if (rust) attachPostsSim(await loadSim(), posts);
    const snapshot = () => {
      const cx = new ExportContext({ classes: { Post: posts.posts[0].constructor, MobilePost }, ref: (kind, v) => ({ kind, id: v.id ?? v.name }) });
      return { posts: exportState('commandPosts', posts, cx), placed: posts.posts.map(p => exportState('post', p, cx)), mobile: posts.mobile.posts.map(p => exportState('mobilePost', p, cx)) };
    };
    return { posts, one, two, slot, building, snapshot };
  };
  const js = await make(false), rust = await make(true);
  return { js, rust, step(fn) {
    const a = fn(js), b = fn(rust);
    assert.deepEqual(b, a);
    assert.deepEqual(rust.snapshot(), js.snapshot());
  } };
}

test('native posts capture, contest, neutralize and lose with exact event order', async () => {
  const { step } = await pair();
  for (const [dt, soldiers] of [[0.25, [at(2)]], [0.25, [at(2), at(2)]], [1, [at(2)]], [1, [at(2)]], [0.5, [at(7), at(2)]], [1, [at(7)]]]) {
    step(({ posts }) => {
      const counts = posts.posts[0].counts;
      posts.update(dt, soldiers);
      assert.equal(posts.posts[0].counts, counts);
      return posts.events.splice(0).map(event);
    });
  }
  step(({ posts, building }) => { posts.posts[0].building = building; building.alive = false; posts.update(1, []); return posts.events.splice(0).map(event); });
  step(({ posts }) => posts.at(new THREE.Vector3())?.name ?? null);
});

test('native post direct methods preserve containment and external count changes', async () => {
  const { step } = await pair();
  for (const x of [0, 5, 5 + Number.EPSILON * 4, Infinity, NaN]) step(({ posts }) => posts.posts[0].contains(new THREE.Vector3(x, 0, 0), posts.scratch));
  step(({ posts }) => { posts.posts[0].counts.set(2, 3); return posts.posts[0].remaining(); });
  step(({ posts }) => posts.posts[0].update(0.2));
  step(({ posts }) => posts.posts[0].destroy());
});

test('native mobile posts keep pad selection, owner, replacement and loss behavior', async () => {
  const { step } = await pair();
  step(({ posts }) => { assert.equal(posts.all(), posts.listed); return posts.owned(2).map(p => p.name); });
  step(({ posts, one }) => { one.seats[0].lease = { released: false }; one.controllingTeam = 1; return posts.mobileEvents().map(event); });
  step(({ posts, one }) => { one.seats[0].lease.released = true; return posts.mobileEvents().map(event); });
  step(({ posts, two }) => { two.alive = true; return posts.mobileEvents().map(event); });
  step(({ posts, two }) => { two.aiTarget.generation++; return posts.mobileEvents().map(event); });
  step(({ posts, one, two }) => { one.alive = false; two.alive = false; return posts.mobileEvents().map(event); });
  step(({ posts }) => {
    const p = posts.mobile.posts[0], ai = p.ai, position = p.lastPosition;
    assert.equal(p.refresh(), p);
    assert.equal(p.ai, ai); assert.equal(p.lastPosition, position);
    return posts.owned(2).map(p => p.name);
  });
});
