// A droideka's prepared hidden skeleton on the battle's module. Asset
// lookup and construction happen once in Droideka.arm; the native rig
// owns posing and physical gun queries after it is attached here.
const HEADER = 5;
const ORDERS = ['XYZ', 'YXZ', 'ZXY', 'ZYX', 'YZX', 'XZY'];

export class UnitMuzzles {
  constructor(bridge) {
    this.bridge = bridge;
    this.bodies = new WeakMap();
  }

  attach(unit, cls) {
    const s = unit.soldier, actor = s.muzzleActor, a = s.muzzleAnimator;
    if (!actor || !a) return;
    const old = this.bodies.get(s);
    if (old?.actor === actor && old.id === unit.id) return;
    const nodes = [actor.root, ...actor.bones, ...s.physicalBarrels.flat()];
    const words = [actor.bones.length, a.root, a.ball, a.spine.length, ...a.spine,
      s.physicalBarrels.length, ...s.physicalBarrels.map(g => g.length)];
    for (const b of a.bind) words.push(...b.p.toArray(), ...b.q.toArray());
    for (const node of nodes) words.push(nodes.indexOf(node.parent), ...node.position.toArray(), ...node.quaternion.toArray(),
      ...node.scale.toArray(), node.rotation.x, node.rotation.y, node.rotation.z, ORDERS.indexOf(node.rotation.order),
      ...node.matrix.elements, ...node.matrixWorld.elements, +node.matrixAutoUpdate, +node.matrixWorldAutoUpdate, +node.matrixWorldNeedsUpdate);
    if (a.pose.kernel) a.pose.refresh();
    words.push(...a.pose.q, ...a.pose.t, ...a.pose.w);
    const b = this.bridge, ptr = b.x.sim_call_room(words.length, 0);
    b.views();
    b.f.set(words, ptr / 8 + HEADER);
    b.x.units_muzzle_attach(unit.id, cls);
    const kept = { id: unit.id, actor, nodes, version: -1,
      nextGun: old?.nextGun ?? s.nextGun, gunAt: old?.gunAt ?? s.gunAt };
    this.bodies.set(s, kept);
    s.nextGun = out => {
      // Unusual externally-written counters retain JavaScript's property
      // lookup rules. Normal counters and their wrap are native.
      if (!Number.isSafeInteger(s.gun) || s.gun < 0) return kept.nextGun.call(s, out);
      return this.query(unit, kept, out, true, s.gun, 0);
    };
    s.gunAt = (out, selected = s.lastGun) => {
      const i = selected?.index, k = selected?.barrel;
      if (!Number.isSafeInteger(i) || !Number.isSafeInteger(k) || !s.physicalBarrels[i]?.[k]) return kept.gunAt.call(s, out, selected);
      return this.query(unit, kept, out, false, i, k);
    };
  }

  query(unit, kept, out, next, index, barrel) {
    const b = this.bridge, s = unit.soldier;
    const at = b.x.units_muzzle_query(unit.id, +next, index, barrel) / 8;
    b.views();
    const f = b.f, found = f[at] !== 0;
    if (next) {
      s.gun = f[at + 4];
      if (f[at + 5]) s.lastGun = { index: f[at + 6], barrel: f[at + 7] };
    }
    if (found) out.set(f[at + 1], f[at + 2], f[at + 3]);
    this.sync(unit, kept);
    return found ? out : null;
  }

  sync(unit, kept = this.bodies.get(unit.soldier)) {
    if (!kept) return;
    const b = this.bridge, ptr = b.x.units_muzzle_out(unit.id, kept.version);
    if (!ptr) return;
    b.views();
    const f = b.f, base = ptr / 8, count = f[base + 1], stride = f[base + 2];
    kept.version = f[base];
    if (count !== kept.nodes.length) throw Error('the native muzzle hierarchy changed');
    let at = base + 3;
    for (const node of kept.nodes) {
      node.position.fromArray(f, at);
      // Copy both paired values without invoking host trigonometry or a
      // callback that overwrites one from the other.
      const q = node.quaternion, r = node.rotation;
      q._x = f[at + 3]; q._y = f[at + 4]; q._z = f[at + 5]; q._w = f[at + 6];
      node.scale.fromArray(f, at + 7);
      r._x = f[at + 10]; r._y = f[at + 11]; r._z = f[at + 12]; r._order = ORDERS[f[at + 13]];
      node.matrix.fromArray(f, at + 14); node.matrixWorld.fromArray(f, at + 30);
      node.matrixAutoUpdate = f[at + 46] !== 0;
      node.matrixWorldAutoUpdate = f[at + 47] !== 0;
      node.matrixWorldNeedsUpdate = f[at + 48] !== 0;
      at += stride;
    }
    const pose = unit.soldier.muzzleAnimator.pose;
    if (pose.kernel) pose.refresh();
    // Refresh may recreate a view after memory growth, but does no calls.
    for (const values of [pose.q, pose.t, pose.w]) {
      for (let i = 0; i < values.length; i++) values[i] = f[at++];
    }
  }
}
