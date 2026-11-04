// Firing geometry shares the hull phase's retained rig. Prepared node and
// aimer identities stay with the Vehicle; commands own their arithmetic,
// matrix updates, barrel counters and kick times. The clock can re-enter.
import { putVgInput, VgOutputMirror, syncVgOutput } from './sim/gen/vehicle-firing.js';

const methods = ['muzzleOf', 'muzzleOfSeat', 'heldByLimits', 'reticleRay', 'botAim', 'nose', 'shotDirection', 'beamDirection', 'spread', 'towHeld'];
const zero = Object.freeze({ x: 0, y: 0, z: 0 });
const num = value => Number.isFinite(parseFloat(value)) ? parseFloat(value) : 0;

export class VehicleFiringSim {
  constructor(owner, nodesOf) {
    this.owner = owner; this.bridge = owner.bridge; this.x = owner.x; this.nodesOf = nodesOf;
    this.attached = new WeakSet(); this.contexts = [];
    this.input = { words: this.x.vehicle_geometry_words(0), put: putVgInput };
    this.output = { words: this.x.vehicle_geometry_words(1), Mirror: VgOutputMirror, sync: syncVgOutput };
    this.bridge.register({ vehicleGeometryClock: args => this.clock(args) });
  }

  attach(v) {
    if (this.attached.has(v)) return;
    this.attached.add(v);
    const sim = this;
    for (const name of methods) {
      const original = v[name];
      v[name] = function (...args) { return sim.invoke(this, name, args, original); };
    }
  }

  context(v) {
    const nodes = [...this.nodesOf(v), v.root.parent], index = new Map(nodes.map((node, i) => [node, i]));
    return { v, nodes, index, node: n => n == null ? -1 : index.get(n) ?? -1 };
  }

  snapshot(c) {
    const v = c.v, named = name => v.byName[(name || '').toLowerCase()];
    return { flying: v.flying, center: v.center, size: v.size, matrix: v.matrix, quaternion: v.quaternion, seatIndex: v.seatIndex,
      seats: v.seats.map(s => ({ yaw: s.aimYaw, pitch: s.pitch, target: s.target, leased: Boolean(s.lease), human: Boolean(s.lease?.human),
        aiming: Boolean(s.lease?.input?.aiming), aim: s.lease?.input?.aim ?? zero, turret: c.node(s.view.turret),
        weapons: s.weapons.map(w => ({ barrel: w.barrel, weaponNode: c.node(named(w.def.FirePointName)),
          fallbackNode: c.node(named(w.aimers[0]?.FirePointName || w.aimers[0]?.node || w.def.FirePointName)),
          aimers: w.aimers.map(a => ({ bone: c.node(a.bone), directionNode: c.node(named(a.FirePointName)), facing: a.facing,
            posed: Boolean(a.posed), onTarget: Boolean(a.onTarget), inLimits: a.inLimits, outside: num(a.FireOutsideLimits ?? s.settings?.FireOutsideLimits) })),
          muzzles: w.muzzles.map(m => ({ node: c.node(m.fireNode), aimer: w.aimers.indexOf(m.sourceAimer), barrel: c.node(m.barrel), kickAt: m.kickAt })) })) })),
      nodes: c.nodes.map(n => ({ position: n.position, quaternion: n.quaternion, scale: n.scale, matrix: n.matrix, matrixWorld: n.matrixWorld,
        auto: n.matrixAutoUpdate, worldAuto: n.matrixWorldAutoUpdate, needs: n.matrixWorldNeedsUpdate })) };
  }

  // Only geometry-owned fields are published. In particular this does not
  // turn a quaternion back into an Euler or update untouched child matrices.
  publish(c, state) {
    for (let i = 0; i < state.seats.length; i++) for (let j = 0; j < state.seats[i].weapons.length; j++) {
      const w = c.v.seats[i].weapons[j], s = state.seats[i].weapons[j]; w.barrel = s.barrel;
      for (let k = 0; k < s.muzzles.length; k++) {
        if (s.muzzles[k].kickAt === undefined) delete w.muzzles[k].kickAt;
        else w.muzzles[k].kickAt = s.muzzles[k].kickAt;
      }
    }
    for (let i = 0; i < c.nodes.length; i++) {
      const n = c.nodes[i], s = state.nodes[i]; n.matrix.copy(s.matrix); n.matrixWorld.copy(s.matrixWorld); n.matrixWorldNeedsUpdate = s.needs;
    }
  }

  clock(args) {
    const c = this.contexts.at(-1);
    this.publish(c, args.state);
    const value = c.v.system.world.clock();
    return { state: this.snapshot(c), value };
  }

  invoke(v, name, args, original) {
    if (!this.attached.has(v) || !this.owner.posed(v)) return original.apply(v, args);
    const c = this.context(v), data = { hull: v, state: this.snapshot(c), seat: v.seatIndex, weapon: 0, aimer: -1,
      from: zero, target: null, fallback: null, out: zero, normal: v.normal, beamFrom: v.beamFrom, node: -1, yaw: 0, pitch: 0, tow: false, towTarget: v.towTarget };
    let out, weapon = null, aimer = null;
    switch (name) {
      case 'muzzleOf': data.op = 'muzzle'; [weapon, out] = args; break;
      case 'muzzleOfSeat': data.op = 'muzzleSeat'; [data.seat, weapon, out] = args; break;
      case 'heldByLimits': data.op = 'limits'; [data.seat, aimer] = args; break;
      case 'reticleRay': data.op = 'reticle'; [data.seat, data.from, out] = args; break;
      case 'botAim': data.op = 'bot'; [data.seat, out] = args; break;
      case 'nose': data.op = 'nose'; [out] = args; break;
      case 'shotDirection': data.op = 'direction'; [aimer, data.from, data.target, data.fallback, out] = args; break;
      case 'beamDirection': data.op = 'beam'; [data.seat, , aimer, out] = args; data.node = c.node(args[1]);
        if (args[1] && data.node < 0) return original.apply(v, args); break;
      case 'spread': data.op = 'spread'; out = args[1]; data.yaw = args[0].yaw ?? NaN; data.pitch = args[0].pitch ?? NaN; break;
      case 'towHeld': data.op = 'tow'; data.tow = args[0].def.ordnance?.base === 'towcable'; break;
    }
    if (weapon || aimer) {
      let found = false;
      for (let i = 0; i < v.seats.length; i++) for (let j = 0; j < v.seats[i].weapons.length; j++) {
        const w = v.seats[i].weapons[j], k = w.aimers.indexOf(aimer);
        if (weapon ? w === weapon : k >= 0) {
          if (name === 'muzzleOf' || name === 'shotDirection') data.seat = i;
          if (data.seat !== i) continue;
          data.weapon = j; data.aimer = k; found = true; break;
        }
      }
      if (!found) return original.apply(v, args);
    }
    data.out = out ?? zero;
    this.contexts.push(c);
    try {
      this.bridge.callArgs(this.input, data);
      const r = this.bridge.callResult(this.output, this.x.vehicle_geometry_run());
      this.publish(c, r.state);
      if (out) out.copy(r.out);
      if (name === 'spread') v.normal.copy(r.normal);
      if (name === 'beamDirection') v.beamFrom.copy(r.beamFrom);
      if (name === 'muzzleOf' || name === 'muzzleOfSeat') {
        v.shotNode = c.nodes[r.node] ?? null;
        v.shotEmitter = r.emitter < 0 ? (weapon.muzzles.length ? weapon.emitter : weapon.aimers[0]?.emitter ?? weapon.emitter)
          : weapon.muzzles[r.emitter]?.emitter ?? weapon.emitter;
        return weapon.aimers[r.aimer] ?? null;
      }
      if (name === 'heldByLimits' || name === 'towHeld') return r.flag;
      if (name !== 'spread') return out;
    } finally { this.contexts.pop(); }
  }
}
