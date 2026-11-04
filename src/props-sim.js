// Building lifecycle and supply pulses on the battle's simulation module.
// Constructor geometry is sent once. The objects retain their identity,
// collision bodies and page rigs; callbacks see each native write before
// they run, including a death that reenters another building command.
import * as THREE from "three";
import { emitSound } from "./audio/events.js";
import { bridgeFor } from "./sim/runtime.js";
import { putPbConfiguration, putPbInput, PbResultMirror, syncPbResult } from "./sim/gen/props.js";
import { putPaConfiguration, putPaInput, PaResultMirror, syncPaResult } from "./sim/gen/props.js";
import { makeRiderPoints, riderPoints } from "./rider-pose.js";

const num = value => {
  const n = Number.parseFloat(Array.isArray(value) ? value[0] : value);
  return Number.isFinite(n) ? n : 0;
};
const vector = value => value ? new THREE.Vector3().fromArray(value) : null;

export function attachPropsSim(instance, live) {
  return new PropsSim(bridgeFor(instance), live);
}

export class PropsSim {
  constructor(bridge, live) {
    this.bridge = bridge;
    this.x = bridge.x;
    this.live = live;
    this.buildings = live.buildings.filter(b => b.constructor.name !== "Trap");
    this.identities = new Map();
    this.objects = [];
    this.contexts = [];
    this.input = { words: this.x.props_words(1), put: putPbInput };
    this.output = { words: this.x.props_words(2), Mirror: PbResultMirror, sync: syncPbResult };
    const buildings = this.buildings.map(b => ({
      kind: b.constructor.name === "Turret" ? "turret" : b.constructor.name === "ObjectivePost" ? "objective" : "building",
      maxHealth: b.maxHealth, respawnTime: b.respawnTime, builtAddHealth: num(b.desc.settings.AddHealth), regenerates: b.regenerates,
      hasDestroyed: Boolean(b.looks.destroyed), hasUnbuilt: Boolean(b.looks.unbuilt), hasExplosion: Boolean(b.explosion), matrix: b.matrix,
      models: ["built", "destroyed", "unbuilt"].map(name => {
        const m = b.looks[name]?.model;
        return m ? { sphere: m.sphere ?? null, low: vector(m.box?.[0]), high: vector(m.box?.[1]) } : null;
      }),
    }));
    bridge.callArgs({ words: this.x.props_words(0), put: putPbConfiguration }, { buildings });
    this.x.props_configure();
    bridge.register({ pbAction: a => this.action(a) });
    this.buildings.forEach((b, index) => {
      const command = (op, value = 0, attacker = null, hit = null, soldiers = [], vehicles = []) => this.run(index, op, value, attacker, hit, soldiers, vehicles);
      Object.assign(b, {
        setHealth(value) { command("setHealth", value); },
        built() { command("built"); },
        destroyed() { command("destroyed"); },
        damage(amount, attacker = null, hit = null) { command("damage", amount, attacker, hit); },
        regenerate(dt) { command("regenerate", dt); },
        sphere(out) { const r = command("sphere"); out.copy(r.point); return r.number; },
        blastSphere(out) { const r = command("blastSphere"); out.copy(r.point); return r.number; },
      });
      if (b.constructor.name === "PowerupStation") Object.assign(b, {
        update(dt, target, vehicles = []) { command("update", dt, null, null, Array.isArray(target) ? target : target ? [target] : [], vehicles); },
        serve(target) { return command("serve", 0, null, null, [target]).number; },
      });
    });
    this.animated = new AnimatedPropsSim(bridge, live);
  }

  identity(object) {
    if (object === undefined) return { kind: 0, id: 0 };
    if (object === null) return { kind: 1, id: 0 };
    let id = this.identities.get(object);
    if (id === undefined) { id = this.objects.length; this.objects.push(object); this.identities.set(object, id); }
    return { kind: 2, id };
  }
  object(ref) { return ref.kind === 0 ? undefined : ref.kind === 1 ? null : this.objects[ref.id]; }
  state(b) {
    return { state: b.state, health: b.health, addHealth: b.addHealth, lastAttacker: this.identity(b.lastAttacker), targetGeneration: b.aiTarget?.generation ?? null };
  }
  station(b) {
    return { building: this.state(b), mode: b.mode, active: b.active, pulse: b.pulse,
      servedPlayer: b.servedPlayer, servedHealth: b.servedHealth, servedAmmo: b.servedAmmo };
  }
  target(t, id) {
    return { id, position: t.position, vehicle: t.healthType === "vehicle", alive: Boolean(t.alive), player: Boolean(t.player), radius: t.radius ?? 0,
      health: t.health, maxHealth: t.maxHealth, weapons: t.weapons.map(w => ({ bottomless: Boolean(w.bottomless), ammo: w.def.ammo ?? NaN, clip: w.clip, perShot: w.perShot, reserve: w.reserve })) };
  }
  syncState(b, state, shown = true) {
    if (shown) b.state = state.state;
    b.health = state.health;
    b.addHealth = state.addHealth;
    // Preserve the absent field until the first assignment, as Building
    // does; Turret starts with an explicit null.
    const attacker = this.object(state.lastAttacker);
    if (attacker !== undefined || Object.hasOwn(b, "lastAttacker")) b.lastAttacker = attacker;
    if (b.aiTarget && state.targetGeneration !== null) b.aiTarget.generation = state.targetGeneration;
  }
  syncStation(b, station) {
    this.syncState(b, station.building);
    for (const key of ["mode", "active", "pulse", "servedPlayer", "servedHealth", "servedAmmo"]) b[key] = station[key];
  }
  syncTarget(t, state) {
    if (!Object.is(t.health, state.health)) t.health = state.health;
    for (let i = 0; i < state.weapons.length; i++) if (!Object.is(t.weapons[i].reserve, state.weapons[i].reserve)) t.weapons[i].reserve = state.weapons[i].reserve;
  }

  run(index, op, value, attacker, hit, soldiers, vehicles) {
    const b = this.buildings[index], station = b.constructor.name === "PowerupStation";
    const targets = [...new Set([...soldiers, ...vehicles])];
    const args = { index, op, value, attacker: this.identity(attacker), hit: this.identity(hit), state: this.state(b),
      crew: typeof b.crew?.destroyed === "function", damaged: typeof b.onDamaged === "function", station: station ? this.station(b) : null,
      supply: station ? { position: b.position, radius: b.radius, delay: b.delay, health: b.giveHealth, ammo: b.giveAmmo, vehicleHealth: b.vehicleHealth, vehicleAmmo: b.vehicleAmmo } : null,
      targets: targets.map((t, id) => this.target(t, id)), soldiers: soldiers.map(t => targets.indexOf(t)), vehicles: vehicles.map(t => targets.indexOf(t)) };
    this.contexts.push(targets);
    try {
      this.bridge.callArgs(this.input, args);
      const result = this.bridge.callResult(this.output, this.x.props_run());
      if (result.station) this.syncStation(b, result.station);
      else this.syncState(b, result.state);
      for (const t of result.targets) this.syncTarget(targets[t.id], t);
      return result;
    } finally { this.contexts.pop(); }
  }

  action(a) {
    const b = this.buildings[a.index], targets = this.contexts.at(-1);
    // The bridge reuses a for a nested import: copy every argument needed
    // after a callback before it can reenter this command.
    const kind = a.kind, target = a.target ? targets[a.target.id] : null, targetId = a.target?.id;
    const amount = a.amount, gave = a.gave, point = a.point.clone(), first = a.first;
    const attacker = this.object(a.attacker), hit = this.object(a.hit), shown = a.state?.state;
    if (a.state) this.syncState(b, a.state, kind !== "show");
    if (a.station) this.syncStation(b, a.station);
    if (a.target) this.syncTarget(target, a.target);
    switch (kind) {
      // ObjectivePost.show reads the previous state for its page's dying
      // animation; publishing the new state before show would erase it.
      case "show": b.show(shown); break;
      case "crewDestroyed": b.crew.destroyed(b); break;
      case "destroyed": b.onDestroyed(b); break;
      case "explode": b.onExplode(b.explosion, point); break;
      case "damaged": b.onDamaged(b, amount, attacker, hit); break;
      case "targetChanged": break;
      case "served": b.onServe(target, gave); break;
      case "serveSound": emitSound("serve", b, first < 0 ? null : targets[first]); break;
      default: throw Error(`unknown prop action ${kind}`);
    }
    return { state: ["crewDestroyed", "destroyed", "explode", "damaged"].includes(kind) ? this.state(b) : null,
      target: kind === "served" ? this.target(target, targetId) : null };
  }
}

export function attachAnimatedPropsSim(instance, live) {
  return new AnimatedPropsSim(bridgeFor(instance), live);
}

// Prop clips and their node transforms are native. The soldier package
// supplies only its rider root sample; Rust places that root on the grab
// node and moves the held body. Page adoption and interpolation stay with
// AnimatedProp, which reads these same objects after every command.
export class AnimatedPropsSim {
  constructor(bridge, live) {
    this.bridge = bridge;
    this.x = bridge.x;
    this.props = live.animated ?? [];
    this.contexts = [];
    this.input = { words: this.x.animated_words(1), put: putPaInput };
    this.output = { words: this.x.animated_words(2), Mirror: PaResultMirror, sync: syncPaResult };
    this.points = { ...makeRiderPoints(), root: new THREE.Vector3(), rootTurn: new THREE.Quaternion() };
    const clip = c => c ? { frames: c.frames, fps: c.fps, rootMotion: c.rootMotion, tracks: c.tracks, data: c.data } : null;
    const props = this.props.map(p => ({
      door: p.door, clips: p.slots.map(clip), doorClip: clip(p.doorClip), doorDuration: p.doorClip?.duration ?? 0,
      idleDelay: p.idleDelay, openSlots: p.names.map(n => n === "open"), position: p.position, killSound: p.killSound,
      triggers: p.triggers.map(t => ({ slot: t.slot, point: t.point, radius: t.radius, sound: t.sound || null, opens: t.action === "open" })),
      grabs: p.grabs.map(g => ({ slot: g.slot, window: g.window, radius: g.radius, node: g.node ? p.rig.bones.indexOf(g.node) : null, clip: g.clip })),
      root: p.rig.root.matrix, parents: p.rig.bones.map(b => p.rig.bones.indexOf(b.parent)), bind: p.rig.bones.map(b => p.rig.bind.get(b)),
    }));
    bridge.callArgs({ words: this.x.animated_words(0), put: putPaConfiguration }, { props });
    this.x.animated_configure();
    bridge.register({ paAction: a => this.action(a) });
    this.props.forEach((p, index) => {
      p.step = (dt, positions, bodies = [], tick = 0) => { this.run(index, "step", dt, positions ?? [], bodies, tick); };
      p.state = () => this.run(index, "state", 0, [], [], 0).record;
    });
  }

  state(p, index, bodies) {
    return { index, slot: p.slot, time: p.time, weight: p.weight, loop: p.loop, hold: p.hold, started: p.started, startTime: p.startTime,
      victim: p.victim ? { unit: bodies.indexOf(p.victim.unit), generation: p.victim.generation, grab: p.grabs.indexOf(p.victim.grab), since: p.victim.since } : null,
      openness: p.openness, bodyEnabled: p.body?.enabled ?? null, standQ: p.standQ, standT: p.standT,
      pose: p.pose ? { root: p.pose.root, q: p.pose.q, t: p.pose.t, w: p.pose.w } : null, rootWorld: p.rig.root.matrixWorld,
      bones: p.rig.bones.map(b => ({ p: b.position, q: b.quaternion, s: b.scale, world: b.matrixWorld })) };
  }
  body(unit, p, key) {
    const s = unit.soldier, h = unit.held;
    return { key, id: unit.id, generation: unit.generation, soldier: Boolean(s), walker: Boolean(unit.walker), removed: Boolean(unit.removed),
      alive: Boolean(unit.alive), dead: Boolean(unit.dead), seated: Boolean(unit.seat), combat: Boolean(unit.combat), mounted: Boolean(unit.combat?.mounted),
      held: h ? { prop: this.props.indexOf(h.prop), grab: h.prop.grabs.indexOf(h.grab), since: h.since, frame: h.frame ?? null, time: h.time ?? null } : null,
      position: unit.position, previous: s?.previous ?? null, settled: s?.settled ?? null, velocity: s?.velocity ?? new THREE.Vector3(),
      air: Boolean(s?.air), dive: s?.dive ?? 0, tickCount: unit.tickCount ?? null, letGo: typeof unit.letGo === "function", remove: typeof unit.remove === "function",
      riderClips: p.grabs.map(g => Boolean(unit.animator?.clips?.get(g.clip))) };
  }
  syncState(p, value, bodies) {
    for (const key of ["slot", "time", "weight", "loop", "hold", "started", "startTime", "openness"]) p[key] = value[key];
    const v = value.victim;
    if (!v) p.victim = null;
    else if (p.victim?.unit !== bodies[v.unit] || p.victim.generation !== v.generation || p.victim.grab !== p.grabs[v.grab] || p.victim.since !== v.since) {
      p.victim = { unit: bodies[v.unit], generation: v.generation, grab: p.grabs[v.grab], since: v.since };
    }
    if (p.body && value.bodyEnabled !== null) p.body.enabled = value.bodyEnabled;
    p.standQ.set(value.standQ); p.standT.set(value.standT);
    if (p.pose && value.pose) { p.pose.q.set(value.pose.q); p.pose.t.set(value.pose.t); p.pose.w.set(value.pose.w); }
    p.rig.root.matrixWorld.copy(value.rootWorld);
    for (let i = 0; i < value.bones.length; i++) {
      const b = p.rig.bones[i], v = value.bones[i];
      b.position.copy(v.p); b.quaternion.copy(v.q); b.scale.copy(v.s); b.matrixWorld.copy(v.world);
    }
  }
  syncBody(unit, value) {
    const h = value.held;
    if (!h) { if (unit.held) unit.held = null; }
    else {
      const prop = this.props[h.prop], grab = prop.grabs[h.grab];
      if (unit.held?.prop !== prop || unit.held.grab !== grab || unit.held.since !== h.since) unit.held = { prop, grab, since: h.since };
      if (h.frame) (unit.held.frame ??= new THREE.Matrix4()).copy(h.frame);
      if (h.time !== null) unit.held.time = h.time;
    }
    if (unit.combat && unit.combat.mounted !== value.mounted) unit.combat.mounted = value.mounted;
    const s = unit.soldier;
    if (s) {
      s.position.copy(value.position); s.velocity.copy(value.velocity);
      if (s.previous && value.previous) s.previous.copy(value.previous);
      if (s.settled && value.settled) s.settled.copy(value.settled);
      if (s.air && !value.air) s.air = null;
      if (!Object.is(s.dive, value.dive)) s.dive = value.dive;
    }
    if (value.tickCount !== null) unit.tickCount = value.tickCount;
  }
  run(index, op, dt, positions, order, tick) {
    const p = this.props[index], bodies = [...new Set([...order, ...(p.victim ? [p.victim.unit] : [])])];
    const args = { op, state: this.state(p, index, bodies), bodies: bodies.map((u, key) => this.body(u, p, key)),
      order: order.map(u => bodies.indexOf(u)), positions, dt, tick };
    this.contexts.push(bodies);
    try {
      this.bridge.callArgs(this.input, args);
      const result = this.bridge.callResult(this.output, this.x.animated_run());
      this.syncState(p, result.state, bodies);
      for (const b of result.bodies) this.syncBody(bodies[b.key], b);
      return result;
    } finally { this.contexts.pop(); }
  }
  action(a) {
    const p = this.props[a.index], bodies = this.contexts.at(-1), unit = a.body ? bodies[a.body.key] : null;
    const kind = a.kind, key = a.body?.key, text = a.text, value = a.value, point = a.point.clone();
    this.syncState(p, a.state, bodies);
    if (a.body) this.syncBody(unit, a.body);
    let found = false;
    switch (kind) {
      case "sound": emitSound("sound", text, point); break;
      case "letGo": unit.letGo(); break;
      case "damage": unit.combat.damage(value); break;
      case "remove": unit.remove(); break;
      case "rider": found = Boolean(riderPoints(unit.animator, { clip: unit.animator.clips.get(text), pitch: 0, turn: 0, time: value }, this.points)); break;
      default: throw Error(`unknown animated prop action ${kind}`);
    }
    return { body: ["letGo", "damage", "remove"].includes(kind) ? this.body(unit, p, key) : null,
      found, root: this.points.root, turn: this.points.rootTurn };
  }
}
