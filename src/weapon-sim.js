// Ordinary authoritative weapons retain their object identity. Definitions,
// generators and lock queries belong to their holder; the native core owns
// timers, ammunition, heat, salvos, charges, throws and lock progress.
import { bridgeFor } from './sim/runtime.js';
import { registerWeaponClass } from './weapon-content-sim.js';
import { putWpInput, WpOutputMirror, syncWpOutput } from './sim/gen/weapon-runtime.js';
import { lockPick } from './weapon-targeting.js';
import { legacyRandom } from './ai/random.js';

const spread = legacyRandom('weapon-spread'), swing = legacyRandom('melee-swing');
const adapters = new WeakMap(), owners = new WeakMap();
const mutable = ['deployed', 'firing', 'timer', 'bottomless', 'perShot', 'clip', 'reserve', 'heat', 'overheated',
  'floor', 'kick', 'spreadScale', 'sniperScale', 'yawSpreadOverride', 'wait', 'reloadLeft', 'throwTime',
  'released', 'hidden', 'charging', 'held', 'charge', 'limit', 'pattern', 'salvosLeft', 'salvoWait',
  'salvoSeed', 'salvoCount', 'lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'];
const target = t => t ? { id: t.id, generation: t.generation ?? 0, alive: Boolean(t.alive) } : null;
const posture = p => ['stand', 'crouch', 'prone'].includes(p) ? p : undefined;

export function attachWeaponSim(instance) {
  const bridge = instance.x ? instance : bridgeFor(instance);
  let sim = adapters.get(bridge);
  if (!sim) { sim = new WeaponSim(bridge); adapters.set(bridge, sim); }
  return sim;
}

export class WeaponSim {
  constructor(bridge) {
    this.bridge = bridge; this.x = bridge.x; this.contexts = [];
    this.input = { words: this.x.weapon_words(0), put: putWpInput };
    this.output = { words: this.x.weapon_words(1), Mirror: WpOutputMirror, sync: syncWpOutput };
    bridge.register({ weaponAction: a => this.action(a) });
  }
  attach(weapon) {
    const prior = owners.get(weapon);
    if (prior) { if (prior !== this) throw Error('weapon already belongs to another simulation'); return weapon; }
    registerWeaponClass(this.bridge, weapon.levels[0]); owners.set(weapon, this);
    weapon.update = (dt, selected, trigger, reload, aim = {}) => this.run(weapon, 'update', {
      dt, selected: Boolean(selected), held: Boolean(trigger.held), pressed: Boolean(trigger.pressed), seed: trigger.seed ?? null,
      late: trigger.late || 0, reload: Boolean(reload), stick: aim.stick, posture: posture(aim.posture), zoomed: Boolean(aim.zoomed), repairTarget: Boolean(aim.repairTarget),
    }).shots;
    for (const [method, op] of [['deselect', 'deselect'], ['clearLock', 'clear'], ['baseLevel', 'base']]) weapon[method] = () => { this.run(weapon, op); };
    weapon.startReload = () => this.run(weapon, 'reload').flag;
    weapon.updateLock = (dt, picked) => { this.run(weapon, 'lock', { dt, target: target(picked) }); };
    weapon.spread = (stick, pose, zoomed) => this.run(weapon, 'spread', { stick, posture: posture(pose), zoomed: Boolean(zoomed) }).spread;
    return weapon;
  }
  snapshot(w) {
    const level = w.levels.indexOf(w.def);
    if (level < 0) throw Error('weapon definition is not one of its prepared levels');
    return { numericLevel: w.level, numericSeed: w.salvoSeed, weapon: { ...w, level, admits: Boolean(w.admitShot), hasRandom: Boolean(w.random),
      salvoSeed: w.salvoRandom ? w.salvoSeed ?? 0 : null,
      melee: w.melee ? { time: w.melee.time, index: w.melee.index, active: Boolean(w.melee.swing) } : null } };
  }
  publish(w, value) {
    const state = value.weapon;
    for (const key of mutable) w[key] = state[key];
    w.def = w.levels[state.level]; w.level = value.numericLevel; w.salvoSeed = value.numericSeed;
    if (state.melee) {
      const swings = w.def.swings?.length ? w.def.swings.slice(0, 3) : [{ duration: 1, hit: .5, sound: null }];
      w.melee.time = state.melee.time; w.melee.index = state.melee.index; w.melee.active = state.melee.active;
      w.melee.swing = state.melee.active ? swings[state.melee.index] : null;
    }
  }
  run(weapon, op, values = {}, options = null) {
    this.attach(weapon);
    // One frame per invocation, including recursive calls on this same
    // weapon. A nested callback must not replace the outer round's `r`.
    const ctx = { weapon, options, random: null, admissions: [], swings: weapon.def.swings?.length ? weapon.def.swings.slice(0, 3) : [{ duration: 1, hit: .5, sound: null }] };
    this.contexts.push(ctx);
    try {
      this.bridge.callArgs(this.input, { op, state: this.snapshot(weapon), dt: 0, selected: false, held: false, pressed: false,
        seed: null, late: 0, reload: false, stick: 0, posture: undefined, zoomed: false, repairTarget: false, target: null, ...values });
      const result = this.bridge.callResult(this.output, this.x.weapon_run());
      this.publish(weapon, result.state);
      const shots = result.shots.map(s => {
        switch (s.kind) {
          case 'round': return { weapon, yaw: s.yaw, pitch: s.pitch, charge: s.charge, targetId: s.targetId, targetGeneration: s.targetGeneration,
            ...(s.admission === null ? {} : { launchAdmission: ctx.admissions[s.admission] }) };
          case 'repair': return { weapon, repair: true };
          case 'meleeStart': {
            weapon.melee.swing = ctx.swings[s.swing - 1];
            return { weapon, meleeStart: true, swing: s.swing, sound: weapon.melee.swing.sound };
          }
          case 'meleeHit': return { weapon, meleeHit: true };
          case 'meleeEnd': return { weapon, meleeEnd: true };
          default: throw Error(`unknown native weapon shot ${s.kind}`);
        }
      });
      return { shots, flag: result.flag, spread: [...result.spread] };
    } finally { this.contexts.pop(); }
  }
  updateLock(weapon, dt, options) { this.run(weapon, 'target', { dt }, options); }
  action(a) {
    const ctx = this.contexts.at(-1), w = ctx.weapon, kind = a.kind, first = a.first;
    // Complete publication before a callback can grow memory or reuse the
    // import record through nested commands. Keep no borrowed argument.
    this.publish(w, a.state);
    let value = 0, admission = 0, picked = null;
    switch (kind) {
      case 'random': case 'seeded': if (first) ctx.random = w.salvoRandom || w.random; value = ctx.random(); break;
      case 'legacy': value = (w.melee ? swing : spread)(); break;
      case 'admit': {
        const answer = w.admitShot(w);
        admission = answer === true ? 0 : answer === false ? 1 : 2;
        if (admission === 2) value = ctx.admissions.push(answer) - 1;
        break;
      }
      case 'pick': picked = target(lockPick(w, ctx.options)?.unit); break;
      case 'start': {
        const seed = w.salvoSeed; let cursor = 0;
        w.salvoRandom = seed === null ? null : () => this.x.weapon_seed(seed, cursor++);
        break;
      }
      default: throw Error(`unknown native weapon callback ${kind}`);
    }
    if (typeof value !== 'number') throw TypeError('weapon RNG must return a number');
    return { state: this.snapshot(w), value, admission, target: picked };
  }
}
