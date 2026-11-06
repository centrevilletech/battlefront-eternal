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
    this.bodies.set(s, { id: unit.id, actor });
  }
}
