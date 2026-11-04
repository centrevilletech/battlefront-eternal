// Real-content differential traces for the authority's movement-only entry
// and retained physical gun rig. Compare files byte for byte under Node24.
import fs from 'node:fs';
import assert from 'node:assert/strict';
import { MAGIC, RECORD, Writer } from './sim-capture/format.mjs';

const args = process.argv.slice(2), option = (name, fallback) => {
  const i = args.indexOf(`--${name}`); return i < 0 ? fallback : args[i + 1];
};
const module = args.includes('--module'), kind = option('kind', 'body');
const unitName = option('unit', 'cis_inf_droideka'), side = unitName.slice(0, 3);
const ticks = Number(option('ticks', kind === 'rig' ? 1440 : 900)), seed = Number(option('seed', 1));
const output = option('out', null); if (!output) throw Error('unit-body-twin requires --out FILE');
let now = 1000, draw = 991;
performance.now = () => now; Date.now = () => 1700000000000 + Math.floor(now);
Math.random = () => { draw = (Math.imul(draw, 1664525) + 1013904223) >>> 0; return draw / 4294967296; };
const { headlessGame, idleCommand } = await import('./headless-world.mjs');
const { ExportContext } = await import('../src/sim/runtime.js');
const { exportSoldier, exportDroideka } = await import('../src/sim/gen/index.js');
const { worldState } = await import('../src/runtime/world-state.js');
const game = await headlessGame(`?map=nab2&side=${side}&era=cw&seed=${seed}&unit=${unitName}&sim=js${module ? '&units=rust&combat=rust&hulls=rust&seats=rust&battle=rust' : ''}`);
for (let i = 0; i < 3; i++) { game.step(idleCommand(game)); await new Promise(resolve => setImmediate(resolve)); }
assert.equal(Boolean(game.unitsSim), module);
assert.ok(!game.factory.built.includes(game.playerUnit), 'the authority local body is outside the factory phase');
const fd = fs.openSync(output, 'w'); fs.writeSync(fd, MAGIC);
const writer = new Writer(); let operations = 0, callbacks = 0, samples = 0, moves = 0;
const modes = new Set(), postures = new Set();
const nodeValue = n => ({ p: n.position, q: n.quaternion, scale: n.scale, rotation: n.rotation.toArray(), matrix: n.matrix,
  world: n.matrixWorld, auto: n.matrixAutoUpdate, worldAuto: n.matrixWorldAutoUpdate, dirty: n.matrixWorldNeedsUpdate });
function bodyValue(u) {
  const s = u.soldier, cx = new ExportContext({ ref: (kind, value) => `${kind}:${value.name ?? value.id ?? ''}`,
    def: (_, value) => value.class ?? value.name ?? value.base ?? '' });
  const body = u.walker ? exportDroideka(s, cx) : exportSoldier(s, cx);
  // A mover is a native allocation handle, never gameplay state.
  delete body.mover;
  return { id: u.id, body, input: { ...u.input }, health: u.combat.health, dead: u.combat.dead,
    kick: { ...u.combat.kick }, gunUnderwater: u.combat.gunUnderwater,
    rig: s.muzzleActor ? { nodes: [s.muzzleActor.root, ...s.muzzleActor.bones, ...s.physicalBarrels.flat()].map(nodeValue),
      pose: ['q', 't', 'w'].map(k => { if (s.muzzleAnimator.pose.kernel) s.muzzleAnimator.pose.refresh(); return [...s.muzzleAnimator.pose[k]]; }) } : null };
}
function write(name, value) {
  writer.record(RECORD.frame, ++operations, name, value);
  fs.writeSync(fd, writer.bytes.subarray(0, writer.length)); writer.length = 0;
}
function sample(u, dt) { samples++; if (module) game.unitsSim.sampleMuzzles(u, dt); else u.soldier.sampleMuzzles(dt); }
function move(u, input, dt) { moves++; if (module) game.unitsSim.bodyTick(u, input, dt); else u.soldier.tick(input, dt); }
const own = game.playerUnit;
const otherClass = side === 'cis' ? 'cis_inf_battledroid' : 'rep_inf_clone_trooper';
await game.factory.prepare(otherClass);
const other = game.factory.build(otherClass);
other.team = own.team;
other.spawn(own.position.x + 5, own.position.y, own.position.z + 5, own.soldier.aimYaw);
const restInput = { forward: 0, right: 0, turn: 0, pitch: 0, jump: false, crouch: false, prone: false, use: false };
if (own.walker) for (const name of ['onDeathBlast', 'onExploded']) {
  const base = own.soldier[name];
  own.soldier[name] = (...a) => {
    callbacks++; write(name, { own: bodyValue(own), other: other ? bodyValue(other) : null });
    base.apply(own.soldier, a);
    game.combat.damage(1);
    own.soldier.aimPitch += 0.03125;
    other.combat.damage(.25);
    move(other, { ...restInput, forward: .25 }, 1 / 120);
    write(`${name}:return`, { own: bodyValue(own), other: other ? bodyValue(other) : null });
  };
}
if (kind === 'offline' && module) {
  // The game's call sites keep their order; only the owned body methods
  // are redirected. The production game can call these adapter entries.
  own.soldier.tick = (input, dt) => move(own, input, dt);
  if (own.walker) own.soldier.sampleMuzzles = dt => sample(own, dt);
}
for (let i = 0; i < ticks; i++) {
  now += 1000 / 60;
  const s = own.soldier, phase = i % 240, dt = [1 / 60, 1 / 120, 1 / 30, 0][i % 4];
  if (kind === 'rig') {
    assert.ok(own.walker);
    s.mode = [0, 1, 2, 5, 7, 8, 9, 10, 11, 12, 13, 3, 4][Math.floor(i / 40) % 13];
    s.time = (i % 40) / 60; s.dead = s.mode === 3 || s.mode === 4;
    s.position.set(i / 31, (i % 7) / 5, -i / 43);
    s.aimYaw = Math.sin(i / 50) * 4; s.aimPitch = Math.cos(i / 21) * .8;
    s.legs = Math.sin(i / 13) * .3; s.roll = i / 17; s.lean = Math.cos(i / 39) * .2;
    if (i % 211 === 0) s.muzzleAnimator.reset(s.aimYaw);
    sample(own, dt);
    const point = new game.THREE.Vector3();
    for (let shot = 0; shot < 2; shot++) {
      const result = s.nextGun(point);
      write('nextGun', { found: Boolean(result), point, state: bodyValue(own) });
    }
    for (const selected of [s.lastGun, { index: 0, barrel: 99 }, { index: -1, barrel: 0 }, undefined]) {
      const result = s.gunAt(point, selected);
      write('gunAt', { selected, found: Boolean(result), point, state: bodyValue(own) });
    }
  } else {
    if (i === 500) {
      // Force the deployed death clip even if collision prevented the
      // preceding unfold; it must emit blasts and then the final callback.
      if (own.walker) { s.mode = 0; s.time = 0; s.reverse = false; }
      game.combat.damage(Infinity);
    }
    if (kind === 'offline') {
      const command = idleCommand(game);
      command.down = phase < 120 ? ['KeyW', ...(phase > 60 ? ['KeyD'] : [])] : phase < 160 ? ['Mouse0'] : [];
      command.look = [[phase % 3 - 1, phase % 5 - 2]];
      command.actions = phase === 25 ? ['jump'] : phase === 60 ? ['crouch'] : phase === 110 ? ['prone'] : own.walker && (phase === 30 || phase === 180) ? ['use'] : [];
      game.step(command);
      await new Promise(resolve => setImmediate(resolve));
    } else {
      const input = { ...restInput, forward: phase < 120 ? .8 : -.2, right: phase < 40 ? .7 : -.3,
        turn: phase % 3 / 10, pitch: phase % 5 / 13, jump: phase === 25 || phase === 27,
        crouch: phase === 60, prone: phase === 110, use: phase === 30 || phase === 180 };
      move(own, input, dt);
      if (own.walker) sample(own, dt);
    }
    write('tick', { own: bodyValue(own), other: other ? bodyValue(other) : null,
      world: kind === 'offline' ? worldState(game) : null });
  }
  if (own.walker) modes.add(s.mode); else postures.add(s.posture);
}
if (own.walker && kind !== 'rig') assert.ok(callbacks > 0, 'real death callbacks were exercised');
if (kind !== 'rig' && module) assert.ok(moves > 0, 'the local movement entry was exercised');
fs.closeSync(fd);
console.log(JSON.stringify({ kind, unit: unitName, seed, ticks, operations, callbacks, samples, moves, modes: [...modes], postures: [...postures] }));
