// Operator-only functional check against locally imported gameplay data: an
// explosion's area damage and push on soldiers in a headless match, through
// the same simulation the dedicated server runs. Sets off the loaded side's
// pushing explosion (the thermal detonator's) beside a network human standing
// still and beside a bot, then follows both through the flight and the
// landing, and shows the human's compact correction carries the thrown state
// a client replays. Does not measure transport, rendering or capacity; no
// retail data is emitted. Run with BFE_DATA_DIR pointing at the imported data.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve, sep } from 'node:path';
import { execFileSync } from 'node:child_process';
import { createGame } from '../src/game.js';
import { createMultiplayerWorld } from '../src/runtime/multiplayer-world.js';
import { captureCompactSoldier, restoreCompactSoldier } from '../src/runtime/network-entity-soldier-compact.js';
import { Soldier } from '../src/soldier.js';

const root = resolve(process.env.BFE_DATA_DIR || 'public');
globalThis.fetch = async path => {
  const file = resolve(root, `.${path}`);
  assert.ok(file.startsWith(root + sep));
  try { return new Response(await readFile(file)); } catch (e) { if (e.code === 'ENOENT') return new Response('', { status: 404 }); throw e; }
};
const map = process.env.BFE_MAP || 'bes1';
const game = await createGame({ headless: true, query: `?map=${map}&side=all&ai=classic&seed=1` });
const world = createMultiplayerWorld(game, { armySize: 16 });

// Every explosion the loaded sides' infantry can set off that pushes.
const blasts = [];
for (const [sideName, side] of Object.entries(game.data.sides)) {
  for (const [className, unit] of Object.entries(side.units || {})) {
    for (const weapon of unit.weapons || []) {
      for (const key of ['ExplosionExpire', 'ExplosionImpact']) {
        const explosion = weapon.ordnance?.[key];
        if (explosion && explosion.Push > 0) blasts.push({ sideName, className, weapon: weapon.class, key, explosion });
      }
    }
  }
}
assert.ok(blasts.length, 'the loaded sides carry a pushing explosion (Push is exported)');
// The widest push the team's own classes carry (the thermal detonator's).
const roster = game.spawnPort.roster(1);
const byReach = (a, b) => (b.explosion.PushRadiusOuter || 0) - (a.explosion.PushRadiusOuter || 0);
const pick = blasts.filter(b => roster.some(c => c.className === b.className)).sort(byReach)[0] || blasts.sort(byReach)[0];
const explosion = pick.explosion;
const className = roster.some(c => c.className === pick.className) ? pick.className : roster[0].className;
// Inside the push radius and the full-damage radius, as a grenade landing beside him.
const distance = Math.min(3, 0.6 * (explosion.PushRadiusOuter || 0));
await world.addPlayer('probe', { team: 1, className });
const u = world.players.get('probe').unit;
const view = { mouseScale: 1, aspect: 16 / 9, invertMouse: false, firstPerson: false };
const step = () => {
  const tick = game.worldClock.tick + 1;
  world.input('probe', { epoch: 1, sequence: tick, tick, generation: u.generation, down: [], actions: [], look: [], ...view });
  world.step();
  return tick;
};
// Long enough for the bots' first spawn wave to be out on the field.
for (let i = 0; i < 900; i++) step();
assert.ok(u.soldier.grounded && !u.soldier.air, 'the human stands on the ground before the blast');

// The blast `distance` metres from the soldier's feet along x, just above the ground.
function blastBeside(unit, distance) {
  const at = unit.soldier.position.clone();
  at.x += distance;
  at.y += 0.1;
  game.ordnance.explosion(explosion, at, null);
  return at;
}
// Follow a unit through the flight: its state right after the blast, how
// high it rose, when it was thrown and when it landed.
function follow(unit, label, ticks, after = () => {}) {
  const feet = unit.soldier.position.y, start = unit.soldier.position.clone(), health = unit.combat.health;
  blastBeside(unit, distance);
  const justAfter = { health: unit.combat.health, dead: unit.combat.dead, air: unit.soldier.air, grounded: unit.soldier.grounded, velocity: unit.soldier.velocity.toArray().map(v => Math.round(v * 1000) / 1000) };
  after(unit);
  let rise = 0, thrownAt = null, landedAt = null;
  for (let i = 1; i <= ticks; i++) {
    const tick = step();
    rise = Math.max(rise, unit.soldier.position.y - feet);
    if (unit.soldier.air === 'thrown' && thrownAt === null) thrownAt = tick;
    if (thrownAt !== null && landedAt === null && !unit.soldier.air && unit.soldier.grounded) landedAt = tick;
  }
  const moved = unit.soldier.position.clone().sub(start);
  return { label, health: { before: health, after: justAfter.health }, justAfter, rise: Math.round(rise * 100) / 100, thrownAt, landedAt, moved: moved.toArray().map(v => Math.round(v * 100) / 100), airAtEnd: unit.soldier.air, removed: unit.removed };
}

// The human: standing still, so the push and the flight are the blast's alone.
let correction = null, replay = null;
const human = follow(u, 'network human', 240, unit => {
  // The compact correction a client restores: it must carry the thrown state.
  correction = captureCompactSoldier(unit.soldier);
  const copy = new Soldier(unit.soldier.world, unit.classDef);
  copy.spawn(...unit.soldier.position.toArray(), unit.soldier.aimYaw);
  restoreCompactSoldier(copy, correction);
  replay = { soldier: copy, air: copy.air, grounded: copy.grounded };
});
// The client's replay of the same flight from that correction, with no input.
const input = { forward: 0, right: 0, turn: 0, jump: false, crouch: false, prone: false };
let replayRise = 0, replayLanded = null;
for (let i = 1; i <= 240; i++) {
  replay.soldier.tick(input, 1 / 60);
  replayRise = Math.max(replayRise, replay.soldier.position.y - u.soldier.position.y);
  if (replayLanded === null && !replay.soldier.air && replay.soldier.grounded) replayLanded = i;
}
const replayGap = replay.soldier.position.distanceTo(u.soldier.position);

// A bot, wherever its controller has it.
const bot = game.units.all().find(x => x !== u && x.alive && x.soldier && !x.seat && !x.walker && !x.human && !x.soldier.air);
const botResult = bot ? follow(bot, `bot ${bot.className}`, 240) : null;

const report = {
  source: { revision: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(), dirty: !!execFileSync('git', ['status', '--porcelain'], { encoding: 'utf8' }).trim() },
  map, className, distance, blast: { side: pick.sideName, weapon: pick.weapon, explosion: explosion.class, Damage: explosion.Damage, DamageRadiusInner: explosion.DamageRadiusInner, DamageRadiusOuter: explosion.DamageRadiusOuter, Push: explosion.Push, PushRadiusInner: explosion.PushRadiusInner, PushRadiusOuter: explosion.PushRadiusOuter },
  human, correction: { bytes: correction.length, restoredAir: replay.air, restoredGrounded: replay.grounded, replayRise: Math.round(replayRise * 100) / 100, replayLandedTick: replayLanded, gapToAuthority: Math.round(replayGap * 1000) / 1000 },
  bot: botResult,
  scope: 'area damage, push and the thrown flight through the server simulation and the compact correction; not transport, rendering or capacity',
};
console.log(JSON.stringify(report, null, 2));
assert.ok(human.health.after < human.health.before, 'the blast hurt the human by its radius');
assert.equal(human.justAfter.air, 'thrown', 'the blast threw the human');
assert.ok(human.rise > 0.5, 'the human flew');
assert.ok(human.landedAt, 'the human landed');
assert.equal(replay.air, 'thrown', 'the correction carries the thrown state');
if (botResult) {
  assert.ok(botResult.health.after < botResult.health.before, 'the blast hurt the bot by its radius');
  assert.equal(botResult.justAfter.air, 'thrown', 'the blast threw the bot');
}
