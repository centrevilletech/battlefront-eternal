// Building lifecycle and supply pulses on the battle's simulation module.
// Constructor geometry is sent once. The objects retain their identity,
// collision bodies and page rigs; callbacks see each native write before
// they run, including a death that reenters another building command.
import * as THREE from "three";
import { emitSound } from "./audio/events.js";
import { bridgeFor } from "./sim/runtime.js";
import { putPbConfiguration, putPbInput, PbResultMirror, syncPbResult } from "./sim/gen/props.js";

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
