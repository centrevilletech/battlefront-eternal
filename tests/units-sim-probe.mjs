// World-hash plugin for units-sim.test.js: exercise both roll transitions
// and the death clip, and compare the body's unquantized state and muzzles.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';

let game, server, rust, jsTicks = 0, jsSamples = 0, jsQueries = 0, action = 0;
const modes = new Set();

export function battle({ game: world, server: local, options }) {
  game = world;
  server = local;
  rust = options.units === 'rust';
  if (rust) assert.ok(game.unitsSim, 'units are on the module');
  assert.ok(server.actor.unit.walker, 'the player is a droideka');
  const proto = Object.getPrototypeOf(server.actor.unit.soldier), tick = proto.tick;
  proto.tick = function (...args) { jsTicks++; return tick.apply(this, args); };
  const sample = proto.sampleMuzzles;
  proto.sampleMuzzles = function (dt) {
    if (dt > 0 && game.factory.built.some(u => u.soldier === this)) jsSamples++;
    return sample.call(this, dt);
  };
  for (const name of ['nextGun', 'gunAt']) {
    const query = proto[name];
    proto[name] = function (...a) {
      if (game.factory.built.some(u => u.soldier === this)) jsQueries++;
      return query.apply(this, a);
    };
  }
}

export function tick(tick) {
  if (tick === 30 || tick === 180) assert.ok(server.world.actions(server.id, {
    epoch: 1, id: ++action, tick, generation: server.actor.unit.generation, actions: ['use'],
  }), 'the world accepts the roll action');
  if (tick === 400) {
    assert.ok(server.actor.unit.alive && !server.actor.unit.soldier.rolled, 'the deployed droideka reaches its death');
    server.actor.unit.combat.damage(Infinity);
  }
}

export function ticked(tick) {
  modes.add(server.actor.unit.soldier.mode);
  const numbers = [];
  for (const unit of game.factory.built) {
    if (!unit.walker) continue;
    const s = unit.soldier;
    numbers.push(unit.id, unit.tickCount, unit.deadTime, +unit.alive, +unit.dead, +unit.removed,
      s.mode, s.time, +s.reverse, s.legs, s.spin, s.roll, s.lean, s.aimYaw, s.aimPitch,
      s.stateTime, +s.grounded, s.blastClock, s.collisionDamage, s.shield?.value ?? 0, +(s.shield?.on ?? false),
      s.gun, s.lastGun?.index ?? -1, s.lastGun?.barrel ?? -1,
      ...s.position.toArray(), ...s.previous.toArray(), ...s.settled.toArray(), ...s.velocity.toArray(), ...s.avoidance.toArray());
    if (s.muzzleActor) {
      const a = s.muzzleAnimator;
      numbers.push(a.facing, a.legs, a.mode, +a.wentUpRolled, a.rollOrigin, a.transition, ...a.pivot.toArray(), ...a.rollCenter.toArray());
      for (const p of a.from) numbers.push(...p.q.toArray(), ...p.p.toArray());
      for (const n of [s.muzzleActor.root, ...s.muzzleActor.bones, ...s.physicalBarrels.flat()]) numbers.push(
        ...n.position.toArray(), ...n.quaternion.toArray(), ...n.scale.toArray(), ...n.matrix.elements, ...n.matrixWorld.elements);
      if (a.pose.kernel) a.pose.refresh();
      numbers.push(...a.pose.q, ...a.pose.t, ...a.pose.w);
    }
  }
  const hash = createHash('sha256').update(Buffer.from(new Float64Array(numbers).buffer)).digest('hex');
  console.error(`droideka-body ${tick} ${hash}`);
}

export function finish() {
  for (const mode of [11, 12, 13, 3, 4]) assert.ok(modes.has(mode), `the player visits droideka mode ${mode}`);
  if (rust) {
    assert.equal(jsTicks, 0, 'droideka movement never falls back to JavaScript');
    assert.equal(jsSamples, 0, 'physical muzzle posing never falls back to JavaScript');
    assert.equal(jsQueries, 0, 'physical gun queries never fall back to JavaScript');
  } else {
    assert.ok(jsTicks > 0, 'the reference exercises JavaScript movement');
    assert.ok(jsSamples > 0, 'the reference exercises physical muzzle posing');
    assert.ok(jsQueries > 0, 'the reference exercises physical gun queries');
  }
}
