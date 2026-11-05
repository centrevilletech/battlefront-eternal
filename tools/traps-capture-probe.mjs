// Deterministic stimulus for a real Endor battle, as a world-hash plugin.
// Include it in all three sim-capture runs so scan/capture/plain hashes
// still prove the recording shims leave the battle alone:
// node tools/sim-capture.mjs --out DIR --specs traps --every 7 --around 1 --
//   --map end1 --side all --seed 1 --ticks 1400
//   --plugin tools/traps-capture-probe.mjs
// The map's actual trap, cutter repair path, AT-ST and death callbacks run.
// Actions happen after each published tick, while capture is still open.
import assert from 'node:assert/strict';

let game, trap, walker, stage = 'build', builtAt = 0, armedAt = 0, killedAt = 0, swungAt = 0;
let firstCrushed = 0, secondBuilt = false, forced = false;
const argv = process.argv.slice(2), at = argv.indexOf('--trap-probe-index');
const trapIndex = at < 0 ? 0 : Number(argv[at + 1]);
const build = { BuildingBuild: 30, TargetFriendly: 1, TargetNeutral: 1, TargetEnemy: 0 };

export function battle({ game: value, options }) {
  assert.equal(options.map, 'end1', 'the trap probe needs the Endor map');
  game = value;
  trap = game.live.traps[trapIndex];
  walker = game.vehicles.vehicles.find(v => /atst/.test(v.name));
  assert.ok(trap && walker, 'Endor trap and AT-ST are loaded');
  firstCrushed = trap.crushed;
}

export function ticked(tick) {
  if (stage === 'build' || stage === 'rebuild') {
    if (trap.state !== 'built') assert.ok(game.live.repair(build, trap, trap.team), 'friendly repair builds the actual trap');
    if (trap.state === 'built') {
      if (stage === 'build') { builtAt = tick; stage = 'arm'; }
      else { secondBuilt = true; stage = 'shoot'; }
    }
  } else if (stage === 'arm' && trap.phase === 'armed') {
    armedAt = tick;
    if (!walker.alive) walker.spawn(2);
    // A previous real damage source must be cleared before the crush's
    // death callback, so its kill does not credit that attacker.
    walker.damage(1, game.playerUnit);
    // Real hull geometry and its collision body; the box test and death
    // callback remain the game's. Reposition once, after the repair clip.
    walker.position.copy(trap.point);
    walker.velocity.set(0, 0, 0);
    walker.syncBody();
    stage = 'crush';
  } else if (stage === 'crush') {
    if (!swungAt && trap.phase === 'swing') swungAt = tick;
    if (!killedAt && !walker.alive) killedAt = tick;
    if (trap.state !== 'built' && swungAt) {
      assert.ok(killedAt && trap.crushed > firstCrushed, 'the live AT-ST was crushed');
      stage = 'rebuild';
    }
  } else if (stage === 'shoot') {
    // Shot down during reset: sprung must survive arming, then fall.
    trap.setHealth(0);
    forced = true;
    stage = 'finish';
  } else if (stage === 'finish' && trap.state !== 'built' && trap.phase === 'down') {
    stage = 'done';
  }
}

export function ended() {
  assert.equal(stage, 'done', `unfinished trap sequence: ${stage}`);
  assert.ok(builtAt && armedAt && swungAt && killedAt && secondBuilt && forced);
  console.error(JSON.stringify({ trapProbe: trapIndex, builtAt, armedAt, swungAt, killedAt, crushed: trap.crushed - firstCrushed, rebuilt: secondBuilt, shotDuringReset: forced }));
}
