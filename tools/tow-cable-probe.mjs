// The Hoth snowspeeders under Classic AI, headless: who crews them, what
// their pilots fly against the AT-ATs, and what their rear gunners do with
// the harpoon (docs/vehicles.md "The snowspeeder's harpoon and tow cable").
//
//   node tools/tow-cable-probe.mjs [--seeds 1,2] [--minutes 5] [--side all]
//     [--tier medium] [--crew] [--detail] [--trace-orbit] [--json out.json]
//
// Per run it reports, over every snowspeeder: seconds flown by a bot pilot
// alone, with a bot gunner behind him, a gunner with no pilot and nobody
// aboard; boardings by seat; the pilots' attack patterns (13 the attack run,
// 15 the AT-AT tow orbit) by seconds, and how near the walker the tow orbit
// flew (towing or not); the rear gunners' targets and selected weapons by
// seconds; and every harpoon (held a leg or missed, how far from the
// nearest walker) and every cable's end (tripped, snapped, destroyed: its
// winding, its span, what lay between, the pilot's pattern). --crew seats
// a second Rebel bot behind every bot pilot still on his pad (to measure
// the run itself, not how often a crew forms). --detail prints each crewed
// speeder's track every 5 s; --trace-orbit the tow orbit's point, height
// share, throttle and the pilot's input every 0.25 s.
//
// docs/ai-vehicles.md "Pattern 15: the tow-cable run" has the numbers it
// gave for the 2026-10-23 fix.
import fs from 'node:fs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const seeds = String(opt('seeds', process.env.SEED || '1')).split(',').map(Number);
const minutes = Number(opt('minutes', 5));
const side = opt('side', 'all');
const tier = opt('tier', null);
const detail = argv.includes('--detail');
const crew = argv.includes('--crew');
const traceOrbit = argv.includes('--trace-orbit');
const jsonOut = opt('json', null);
const THREE = await import('three');
const { orbitResult, orbitSteer } = await import('../src/ai/classic/vehicles/orbit.js');
const { headlessGame } = await import('./headless-world.mjs');

const SPEEDER = /snowspeeder/;
const inc = (o, k, n = 1) => { o[k] = (o[k] ?? 0) + n; };
const flat = (a, b) => Math.hypot(a.x - b.x, a.z - b.z);
const nameOf = (u) => u?.className ?? u?.vehicle?.name ?? u?.name ?? '?';

async function run(seed) {
  const game = await headlessGame(`map=hot1&side=${side}&ai=classic&seed=${seed}${tier ? `&tier=${tier}` : ''}`);
  await game.ready;
  game.begin?.();
  const system = game.vehicles;
  const s = {
    seconds: 0, pilotAlone: 0, crewed: 0, gunnerOnly: 0, empty: 0, humanAboard: 0,
    boards: {}, patterns: {}, pilotAgents: {}, gunnerAgents: {}, gunnerTargets: {}, gunnerWeapons: {},
    harpoons: 0, harpoonResults: {}, holds: 0, releases: {}, trips: 0, walkerNear: { 70: 0, 150: 0 },
    gunnerShots: {}, crewed15: 0, orbitNear: {}, forcedCrew: 0, events: [], track: [],
  };
  const speeders = () => system.vehicles.filter((v) => SPEEDER.test(v.name));
  const atats = () => system.vehicles.filter((v) => v.alive && v.base === 'commandwalker');
  // Boardings by seat.
  const board = system.board.bind(system);
  system.board = (unit, v, i = -1) => {
    const ok = board(unit, v, i);
    if (ok && SPEEDER.test(v.name)) {
      const seat = v.seats.findIndex((x) => x.lease?.unit === unit);
      inc(s.boards, `seat${seat}`);
    }
    return ok;
  };
  // Every harpoon and how it ended.
  const launchTow = system.launchTow.bind(system);
  system.launchTow = (source, def, origin, direction, seatIndex = 0, lease = null) => {
    s.harpoons++;
    const walker = system.nearestWalker(source, 1e9);
    const range = walker ? flat(walker.position, source.position) : Infinity;
    const ok = launchTow(source, def, origin, direction, seatIndex, lease);
    const band = range <= 70 ? 'within 70 m' : range <= 150 ? '70-150 m' : 'past 150 m';
    inc(s.harpoonResults, `${ok ? 'held' : 'missed'} (${band})`);
    if (ok) s.holds++;
    s.events.push(`t${(game.worldClock.tick / 60).toFixed(1)} #${system.vehicles.indexOf(source)} harpoon ${ok ? `held ${source.towLeg}` : 'missed'} at ${range.toFixed(0)} m (pilot ${source.seats[0]?.lease ? (source.seats[0].lease.human ? 'human' : 'bot') : '-'})`);
    return ok;
  };
  const releaseTow = system.releaseTow.bind(system);
  system.releaseTow = (source, reason = 'released') => {
    if (source.towTarget >= 0) {
      inc(s.releases, reason);
      if (reason === 'tripped') s.trips++;
      // Why it let go: the span against 1.2 x CableLength, and what lies
      // between the gun and the anchor.
      const tether = system.tethers.get(source);
      const target = system.vehicles[source.towTarget];
      let why = '';
      if (tether && target) {
        const start = system.towGun(source, tether.start.clone());
        const end = source.towLocal.clone().applyMatrix4(target.matrix);
        const span = start.distanceTo(end);
        const dir = end.clone().sub(start).normalize();
        const hit = game.collision.cast(start, dir, span, { point: end.clone(), normal: end.clone(), distance: 0, body: null, terrain: false }, source.body);
        const owner = hit?.body?.owner;
        why = ` span ${span.toFixed(1)}/${(tether.length * 1.2).toFixed(0)} m${hit ? ` hit ${hit.terrain ? 'terrain' : owner?.name ?? owner?.desc?.class ?? (hit.body === target.body ? 'the walker' : 'a body')} at ${hit.distance.toFixed(1)} m${hit.body === target.body ? ' (walker body)' : ''}` : ''}; speeder y ${source.position.y.toFixed(1)} walker y ${target.position.y.toFixed(1)}; pilot ${(() => { const a = source.seats[0]?.lease?.controllable?.controller?.agent; return a ? `${a.constructor.name}/${a.state}${a.patterns && a.state === 4 ? `/p${a.patterns.state}` : ''}` : '-'; })()}`;
      }
      s.events.push(`t${(game.worldClock.tick / 60).toFixed(1)} #${system.vehicles.indexOf(source)} cable ${reason} after ${source.towWinding.toFixed(2)} rad${why}`);
    }
    return releaseTow(source, reason);
  };
  const onFired = system.onFired;
  system.onFired = (v, seat, shot, owner, muzzle, direction) => {
    if (SPEEDER.test(v.name) && seat === 1) inc(s.gunnerShots, shot.weapon?.def?.class ?? '?');
    return onFired?.(v, seat, shot, owner, muzzle, direction);
  };
  const ticks = Math.round(minutes * 3600);
  for (let n = 1; n <= ticks; n++) {
    const tick = game.worldClock.tick + 1;
    game.step({ epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] });
    if (traceOrbit && n % 15 === 0) {
      for (const v of speeders()) {
        const seat = v.seats[0]?.lease?.controllable;
        const orbit = seat?.steer?.orbit;
        const hull = orbit?.target?.unit?.vehicle;
        if (!v.alive || !hull) continue;
        const out = orbitSteer(v.position, new THREE.Vector3(0, 0, 1).applyQuaternion(v.quaternion), hull, orbit, orbitResult());
        const intent = seat.intent ?? {};
        const towHull = v.towTarget >= 0 ? system.vehicles[v.towTarget] : null;
        s.events.push(`t${(game.worldClock.tick / 60).toFixed(2)} #${system.vehicles.indexOf(v)} orbit #${system.vehicles.indexOf(hull)}${towHull ? ` cable #${system.vehicles.indexOf(towHull)}` : ''} r=${orbit.radius} k=${orbit.factor} P=(${v.position.x.toFixed(0)},${v.position.y.toFixed(1)},${v.position.z.toFixed(0)}) pt=(${out.point.x.toFixed(0)},${out.point.y.toFixed(1)},${out.point.z.toFixed(0)}) d=${Math.hypot(v.position.x - hull.position.x, v.position.z - hull.position.z).toFixed(1)} blend=${out.blend.toFixed(2)} thr=${out.throttle.toFixed(2)} spd=${v.airspeed?.toFixed(1)} warn=${v.terrainWarning ? 1 : 0} in(f=${(intent.forward ?? 0).toFixed(2)},p=${(intent.pitch ?? 0).toFixed(2)},t=${(intent.turn ?? 0).toFixed(2)}) wound=${v.towWinding.toFixed(2)}`);
      }
    }
    if (n % 60) continue;
    const t = n / 60;
    s.seconds++;
    for (const v of speeders()) {
      if (!v.alive) continue;
      if (crew && v.seats[0]?.lease && !v.seats[0].lease.human && !v.seats[1]?.lease && v.flightState !== 'Flying') {
        const bot = game.units.all().find((u) => u.alive && u.kind === 0 && u.character && !u.seat && u !== game.playerUnit && u.team === v.controllingTeam);
        if (bot && system.board(bot, v, 1)) s.forcedCrew++;
      }
      const pilot = v.seats[0]?.lease, gunner = v.seats[1]?.lease;
      if (pilot?.human || gunner?.human) s.humanAboard++;
      if (pilot && gunner) s.crewed++;
      else if (pilot) s.pilotAlone++;
      else if (gunner) s.gunnerOnly++;
      else { s.empty++; continue; }
      const walkers = atats();
      const near = walkers.reduce((m, w) => Math.min(m, flat(w.position, v.position)), Infinity);
      if (near <= 70) s.walkerNear[70]++;
      if (near <= 150) s.walkerNear[150]++;
      const pc = pilot && !pilot.human ? pilot.controllable?.controller : null;
      const pa = pc?.agent;
      if (pa) {
        inc(s.pilotAgents, `${pa.constructor.name}/${pa.state}`);
        if (pa.patterns && pa.state === 4) inc(s.patterns, `p${pa.patterns.state}${gunner ? ' (gunner aboard)' : ''}`);
        if (pa.patterns && pa.state === 4 && pa.patterns.state === 15) {
          s.crewed15++;
          const band = near <= 40 ? '<=40' : near <= 70 ? '40-70' : near <= 150 ? '70-150' : '>150';
          inc(s.orbitNear, `${band}${v.towTarget >= 0 ? ' towing' : ''}`);
        }
      }
      const gc = gunner && !gunner.human ? gunner.controllable?.controller : null;
      const ga = gc?.agent;
      let gtarget = null, gweapon = null;
      if (ga) {
        inc(s.gunnerAgents, `${ga.constructor.name}/${ga.state}`);
        const tu = ga.tracker?.target?.unit ?? ga.tracker?.target ?? null;
        gtarget = tu ? nameOf(tu) : 'none';
        inc(s.gunnerTargets, gtarget);
        const ch = gc.perception?.fire?.channel ?? 0;
        gweapon = gunner.controllable.combat?.arsenal?.current?.(ch)?.def?.class ?? '?';
        inc(s.gunnerWeapons, gweapon);
      }
      if (detail && t % 5 === 0) {
        const seat = v.seats[1];
        const pt = pa?.tracker?.target?.unit ?? pa?.tracker?.target ?? null;
        s.track.push(`t${t} #${system.vehicles.indexOf(v)} (${v.position.x.toFixed(0)},${v.position.y.toFixed(0)},${v.position.z.toFixed(0)}) ${v.flightState ?? ''} spd=${v.velocity.length().toFixed(0)}`
          + ` pilot=${pilot ? (pilot.human ? 'human' : `${pa?.constructor.name}/${pa?.state}${pa?.patterns && pa.state === 4 ? `/p${pa.patterns.state}` : ''} tgt=${pt ? nameOf(pt) : '-'}`) : '-'}`
          + ` gunner=${gunner ? (gunner.human ? 'human' : `${ga?.constructor.name}/${ga?.state} tgt=${gtarget} w=${gweapon} aimYaw=${(((seat.aimYaw - v.yaw) * 180 / Math.PI) % 360).toFixed(0)}`) : '-'}`
          + ` atat@${Number.isFinite(near) ? near.toFixed(0) : '-'} tow=${v.towTarget}`);
      }
    }
    if (game.battle.gameOver) break;
  }
  return { seed, minutes, side, tier, tickets: game.battle.reinforcements, gameOver: Boolean(game.battle.gameOver), ...s };
}

const results = [];
for (const seed of seeds) {
  const r = await run(seed);
  results.push(r);
  const { track, events, ...rest } = r;
  console.log(`== hot1 side=${r.side} seed=${r.seed} ${r.minutes} min tickets ${JSON.stringify(r.tickets)}${r.gameOver ? ' (game over)' : ''}`);
  console.log(JSON.stringify(rest, null, 1));
  if (detail) for (const line of track) console.log('   ', line);
  for (const line of r.events) console.log('  *', line);
}
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(results, null, 1));
process.exit(0);
