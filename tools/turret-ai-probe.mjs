// Bots at placed turrets (armed buildings) under Classic AI, headless: whole
// battles, every manned turret sampled four times a game second, and
// scripted duels in which a bot at a turret is shot at from a chosen side.
//
//   BFE_CONTENT_DIR=<game-data> node tools/turret-ai-probe.mjs \
//     [--maps kas2,hot1] [--seeds 1] [--minutes 4] [--tier medium] [--endless]
//     [--range 80] [--json out.json] [--detail]
//   ... --scenario [--maps kas2] [--turrets 0,3] [--bearings 0,90,180] [--distances 15,40]
//     [--seconds 12]
//
// Battle mode, per turret: seconds a bot manned it, manned episodes and
// their length, exits by reason (the AI caller of the exit request as
// file:line, or killed / destroyed / out of the seat); while manned with an
// enemy in sight within --range of the gun (a clear line from the muzzle
// to the enemy's aim point): seconds, the angle between the barrel and the
// enemy (median, and seconds within 10, 30 and past 90 degrees), seconds
// with an AI target and with that enemy as the target; shots and hits
// (damage to enemies credited to the gunner); damage taken while manned
// (to the turret and to an exposed gunner), the attacks (an attacker's
// first hit after 5 s of quiet) and of those the share the gunner made his
// target within 3 s and fired at within 6 s with the barrel within 15
// degrees of him; and bots on foot within 4 m of it that kept to a 1 m ball
// for 10 s (stuck by the turret). Seconds manned with no enemy in sight are
// split into turning (the drum moving) and idle.
//
// Scenario mode, per turret, bearing (degrees from the drum's facing,
// clockwise from above) and distance: a bot of the turret's team is seated,
// every other bot is kept dead, and the player, an enemy, is held at that
// spot and fires a rifle bolt at the gunner's turret every half second. It
// reports the seconds to the first target, to the barrel within 15 degrees,
// to the first shot at him and to the first hit, his shots and hits, the
// turret's damage and whether (and why) the gunner left.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const flag = (name) => argv.includes(`--${name}`);
const maps = opt('maps', 'kas2,hot1').split(',');
const seeds = opt('seeds', '1').split(',').map(Number);
const minutes = Number(opt('minutes', 4));
const tier = opt('tier', 'medium');
const RANGE = Number(opt('range', 80));
const jsonOut = opt('json', null);
const detail = flag('detail');
const scenario = flag('scenario');

const desktop = path.join(os.homedir(), '.config/Battlefront Eternal/game-data');
if (!process.env.BFE_CONTENT_DIR && !fs.existsSync(new URL('../public/maps', import.meta.url)) && fs.existsSync(desktop)) process.env.BFE_CONTENT_DIR = desktop;
const { headlessGame, content } = await import('./headless-world.mjs');
const { UnitController } = await import('../src/ai/classic/controller.js');
const THREE = await import('three');

// The player's side: team 1's (its first class's prefix) in the map's
// first mission.
function sideOf(map) {
  const data = JSON.parse(fs.readFileSync(path.join(content, 'maps', map, 'map.json')));
  const mission = data.mission ?? Object.values(data.missions ?? {})[0]?.mission ?? {};
  const first = mission.teams?.[1]?.classes?.[0]?.[0] ?? mission.teams?.[1]?.classes?.[0];
  return typeof first === 'string' ? first.split('_')[0] : 'rep';
}

const inc = (o, k, n = 1) => { o[k] = (o[k] ?? 0) + n; };
const flat = (a, b) => Math.hypot(a.x - b.x, a.z - b.z);
const median = (a) => { if (!a.length) return null; const s = [...a].sort((x, y) => x - y); return s[s.length >> 1]; };
const DEG = 180 / Math.PI;

// The AI caller of an exit request (as tools/walker-probe.mjs has it).
const requestExit = UnitController.prototype.requestExit;
UnitController.prototype.requestExit = function (...args) {
  const frames = String(new Error().stack).split('\n').slice(2);
  const frame = frames.find((f) => !/controller\.js:66\d|vehicles\/index\.js:\d+/.test(f)) ?? frames[0] ?? '';
  const m = frame.match(/src\/(.*?):(\d+):\d+/);
  const lease = this.unit?.lease ?? this.unit?.seat ?? null;
  if (lease) lease.probeExitCaller = m ? `${m[1]}:${m[2]}` : 'unknown';
  return requestExit.apply(this, args);
};

const _m = new THREE.Vector3(), _d = new THREE.Vector3(), _a = new THREE.Vector3(), _to = new THREE.Vector3();
const _hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };

// The enemy's body as the gun would meet it: a soldier, or a vehicle hull
// (its registry entry).
function bodyOf(u) { return u?.vehicle?.body ?? u?.turret?.bodies?.built ?? null; }

// Angle (degrees) between the barrel and the line from the muzzle to u.
function barrelError(turret, u) {
  turret.muzzle(_m, _d);
  (u.aimPoint ? u.aimPoint(_a) : _a.copy(u.position));
  _to.subVectors(_a, _m);
  const len = _to.length();
  if (len < 1e-6) return 0;
  return Math.acos(Math.max(-1, Math.min(1, _to.dot(_d) / len))) * DEG;
}

// The nearest enemy of `team` the gun could see within range: a clear line
// from the muzzle to its aim point.
function nearestEnemy(game, turret, team, range) {
  const units = game.units;
  turret.muzzle(_m);
  let best = null, bestD = range;
  for (const u of units.living()) {
    if (u === turret.aiTarget || u.targetable === false || u.removed) continue;
    if (!(units.relation(team, u.team) < 0)) continue;
    if (u.category === 4 && !u.vehicle) continue; // buildings and turrets are not who shoots back
    (u.aimPoint ? u.aimPoint(_a) : _a.copy(u.position));
    const d = _a.distanceTo(_m);
    if (d >= bestD) continue;
    _to.subVectors(_a, _m).divideScalar(d || 1);
    const hit = game.collision.cast(_m, _to, d, _hit, turret.bodies?.built || null);
    if (hit && hit.distance < d - 1.5 && !(bodyOf(u) && hit.body === bodyOf(u))) continue;
    best = u; bestD = d;
  }
  return best ? { unit: best, distance: bestD } : null;
}

// The barrel's and the drum view's yaw and pitch error to u, in degrees.
function splitError(turret, u) {
  turret.muzzle(_m, _d);
  (u.aimPoint ? u.aimPoint(_a) : _a.copy(u.position));
  _to.subVectors(_a, _m);
  const yawTo = Math.atan2(_to.x, _to.z), pitchTo = Math.atan2(_to.y, Math.hypot(_to.x, _to.z));
  const w = (x) => Math.atan2(Math.sin(x), Math.cos(x)) * DEG;
  const view = turret.facing + turret.yaw;
  return `bY=${w(Math.atan2(_d.x, _d.z) - yawTo).toFixed(1)} bP=${w(Math.asin(_d.y) - pitchTo).toFixed(1)} vY=${w(view - yawTo).toFixed(1)} vP=${w(turret.pitch - pitchTo).toFixed(1)} gY=${(turret.gunYaw * DEG).toFixed(1)} gP=${(turret.gunPitch * DEG).toFixed(1)}`;
}

// Who an attacker is, as a unit the gunner could aim at.
function attackerUnit(game, attacker) {
  if (!attacker) return null;
  if (attacker === 'player') return game.playerUnit;
  const u = attacker.alive !== undefined ? attacker : game.units.resolve(attacker);
  return u ?? null;
}
function sameFoe(game, a, b) {
  if (!a || !b) return false;
  if (a === b) return true;
  const ca = a.character ?? a.lease?.unit?.character, cb = b.character ?? b.lease?.unit?.character;
  return Boolean(ca && ca === cb);
}
function targetUnit(game, controller) {
  const ref = controller?.target ?? controller?.agent?.tracker?.target ?? null;
  const t = ref?.unit ?? ref;
  return t?.alive ? t : null;
}

function instrument(game, stats, clock) {
  const seats = game.turretSeats;
  const turrets = seats.turrets;
  const stat = (t) => stats.get(t);
  for (const t of turrets) stats.set(t, {
    index: turrets.indexOf(t), class: t.desc.class, label: t.label, position: [t.position.x, t.position.y, t.position.z].map((v) => Math.round(v)),
    manned: 0, episodes: 0, episodeLengths: [], exits: {}, mannedBy: {},
    engage: 0, errors: [], within10: 0, within30: 0, past90: 0, targeted: 0, targetedFoe: 0,
    noEnemyTurning: 0, noEnemyIdle: 0,
    shots: 0, hits: 0, damageDealt: 0, damageTaken: 0, attacks: [], stuckNear: 0, stuckBots: new Set(), stuckSigs: {}, repairing: 0, deaths: 0,
    track: [],
  });
  const vacate = seats.vacate.bind(seats);
  seats.vacate = (lease, opts = {}) => {
    const t = lease.turret;
    const was = !lease.released && !lease.human;
    const s = stat(t);
    const r = vacate(lease, opts);
    if (was && s) {
      const reason = opts.kill ? 'destroyed' : opts.reason === 'killed' ? 'killed' : lease.probeExitCaller ?? (lease.live === false ? 'unseated' : `${opts.reason ?? 'exit'} (no AI request)`);
      inc(s.exits, reason);
      s.episodeLengths.push(+(clock.t - (lease.probeSince ?? clock.t)).toFixed(1));
    }
    return r;
  };
  const man = seats.man.bind(seats);
  seats.man = (unit, turret) => {
    const ok = man(unit, turret);
    if (ok && turret.lease && !turret.lease.human) {
      turret.lease.probeSince = clock.t;
      const s = stat(turret);
      if (s) { s.episodes++; inc(s.mannedBy, `team${unit.team}`); }
    }
    return ok;
  };
  for (const t of turrets) {
    const damage = t.damage.bind(t);
    t.damage = (amount, attacker, ...rest) => {
      const lease = t.lease;
      if (lease && !lease.human && t.alive && amount > 0) noteHurt(game, stat(t), lease, amount, attacker, clock);
      return damage(amount, attacker, ...rest);
    };
  }
  game.simEvents.listen((e) => {
    if (e.kind === 'shot' && e.mount) {
      const body = game.units.resolve(e.mount);
      const t = body?.turret;
      if (t && t.lease && !t.lease.human && stat(t)) {
        stat(t).shots++;
        const lease = t.lease;
        for (const a of lease.probeAttacks ?? []) {
          if (a.shotAt != null || clock.t - a.t > 6) continue;
          const foe = attackerUnit(game, a.attacker);
          if (foe?.alive && barrelError(t, foe) < 15) a.shotAt = +(clock.t - a.t).toFixed(2);
        }
      }
    } else if (e.kind === 'damage') {
      const attacker = e.attacker ? game.units.resolve(e.attacker) : null;
      const lease = attacker?.seat;
      if (lease?.turret && !lease.human && e.attackerTeam !== e.team && stat(lease.turret)) {
        const a = Number(e.amount);
        stat(lease.turret).hits++;
        if (Number.isFinite(a)) stat(lease.turret).damageDealt += a;
      }
      const victim = e.target ? game.units.resolve(e.target) : null;
      const vl = victim?.seat;
      if (vl?.turret && !vl.human && stat(vl.turret)) noteHurt(game, stat(vl.turret), vl, Number(e.amount) || 0, attacker, clock);
    }
  });
}

function noteHurt(game, s, lease, amount, attacker, clock) {
  s.damageTaken += amount;
  const foe = attackerUnit(game, attacker);
  if (!foe) return;
  const list = (lease.probeAttacks ||= []);
  const last = list.findLast((a) => sameFoe(game, attackerUnit(game, a.attacker), foe));
  if (last && clock.t - last.lastHit < 5) { last.lastHit = clock.t; return; }
  const turret = lease.turret;
  const controller = lease.controllable?.controller;
  const held = targetUnit(game, controller);
  const a = { t: +clock.t.toFixed(2), lastHit: clock.t, attacker: foe, foeClass: foe.className, bearing: Math.round(barrelError(turret, foe)), distance: Math.round(foe.position.distanceTo(turret.position)), targetAt: null, shotAt: null, hitAt: null, exited: null,
    agent: `${controller?.agent?.constructor.name ?? '-'}/${controller?.agent?.state ?? '-'}`, busy: held && !sameFoe(game, held, foe) ? held.className : null };
  list.push(a);
  s.attacks.push(a);
}

function sample(game, stats, clock, dt) {
  const seats = game.turretSeats;
  for (const t of seats.turrets) {
    const s = stats.get(t);
    if (!t.alive) { if (!s.dead) { s.dead = true; s.deaths++; } continue; }
    s.dead = false;
    const lease = t.lease;
    if (!lease || lease.human || lease.released) continue;
    s.manned += dt;
    const controller = lease.controllable?.controller;
    const target = targetUnit(game, controller);
    for (const a of lease.probeAttacks ?? []) {
      if (a.targetAt == null && clock.t - a.t <= 3 && sameFoe(game, target, attackerUnit(game, a.attacker))) a.targetAt = +(clock.t - a.t).toFixed(2);
    }
    const enemy = nearestEnemy(game, t, lease.team, RANGE);
    const yawVel = Math.abs(t.yawVel);
    if (!enemy) {
      if (yawVel > 0.05) s.noEnemyTurning += dt; else s.noEnemyIdle += dt;
    } else {
      s.engage += dt;
      const err = barrelError(t, enemy.unit);
      s.errors.push(err);
      if (err < 10) s.within10 += dt;
      if (err < 30) s.within30 += dt;
      if (err > 90) s.past90 += dt;
      if (target) s.targeted += dt;
      if (target && sameFoe(game, target, enemy.unit)) s.targetedFoe += dt;
    }
    if (detail && Math.abs(clock.t - Math.round(clock.t)) < 1e-6 && Math.round(clock.t) % 5 === 0) {
      const agent = controller?.agent;
      s.track.push(`t${Math.round(clock.t)} ${agent?.constructor.name ?? '-'}/${agent?.state ?? '-'}${agent?.patterns ? `/p${agent.patterns.state}` : ''} aim${controller?.aimState} yaw=${((t.facing + t.yaw) * DEG).toFixed(0)} vel=${t.yawVel.toFixed(2)} turn=${(lease.input.turn).toFixed(2)} tgt=${target ? `${target.className}@${target.position.distanceTo(t.position).toFixed(0)} err=${barrelError(t, target).toFixed(0)}` : '-'} enemy=${enemy ? `${enemy.unit.className}@${enemy.distance.toFixed(0)} err=${barrelError(t, enemy.unit).toFixed(0)}` : '-'} hp=${Math.round(t.health)}`);
    }
  }
}

// Bots on foot by a turret who keep still for 10 s.
function stuckNear(game, stats, tracks, clock) {
  const seats = game.turretSeats;
  for (const character of game.battle.characters) {
    if (character.human) continue;
    const u = character.unit;
    if (!u?.alive || u.seat) { tracks.delete(character); continue; }
    let near = null;
    for (const t of seats.turrets) if (flat(t.position, u.position) < 4) { near = t; break; }
    if (!near) { tracks.delete(character); continue; }
    let tr = tracks.get(character);
    if (!tr || tr.turret !== near || tr.id !== u.id) tracks.set(character, (tr = { turret: near, id: u.id, samples: [] }));
    tr.samples.push({ t: clock.t, x: u.position.x, z: u.position.z });
    while (tr.samples[0].t < clock.t - 10) tr.samples.shift();
    if (tr.samples[0].t > clock.t - 10 + 1e-6) continue;
    const far = Math.max(...tr.samples.map((p) => Math.hypot(p.x - u.position.x, p.z - u.position.z)));
    if (far < 1) {
      const s = stats.get(near);
      const agent = character.controller?.agent;
      const sig = `${agent?.constructor.name ?? '-'}/${agent?.state ?? '-'}${agent?.helper ? ` goto${agent.helper.state}` : ''}`;
      // Repairing it (a pilot's fusion cutter at work) holds still on purpose.
      if (agent?.constructor.name === 'UnitRepairAgent') { s.repairing += 1; continue; }
      inc(s.stuckSigs, sig);
      s.stuckNear += 1;
      s.stuckBots.add(character);
      if (detail && s.stuckNear % 10 === 1) {
        const agent = character.controller?.agent;
        s.track.push(`t${Math.round(clock.t)} stuck-near ${u.className} ${agent?.constructor.name}/${agent?.state} helper=${agent?.helper?.state}`);
      }
    }
  }
}

function summarize(s) {
  const attacks = s.attacks;
  const n = attacks.length;
  const pct = (k) => (n ? Math.round((100 * attacks.filter((a) => a[k] != null).length) / n) : null);
  return {
    i: s.index, class: s.class, at: s.position, deaths: s.deaths,
    manned: Math.round(s.manned), episodes: s.episodes, medianEpisode: median(s.episodeLengths), mannedBy: s.mannedBy, exits: s.exits,
    engage: Math.round(s.engage), medianErr: s.errors.length ? Math.round(median(s.errors)) : null,
    within10: Math.round(s.within10), within30: Math.round(s.within30), past90: Math.round(s.past90),
    targeted: Math.round(s.targeted), targetedFoe: Math.round(s.targetedFoe),
    noEnemyTurning: Math.round(s.noEnemyTurning), noEnemyIdle: Math.round(s.noEnemyIdle),
    shots: s.shots, hits: s.hits, damageDealt: Math.round(s.damageDealt), damageTaken: Math.round(s.damageTaken),
    attacks: n, reactedPct: pct('targetAt'), returnedPct: pct('shotAt'),
    stuckNear: s.stuckNear, stuckBots: s.stuckBots.size, stuckSigs: s.stuckSigs, repairing: s.repairing,
    attackList: s.attacks.map(({ attacker, ...a }) => a),
  };
}

function totals(rows) {
  const sum = (k) => rows.reduce((a, r) => a + (r[k] ?? 0), 0);
  const attacks = sum('attacks');
  const reacted = rows.reduce((a, r) => a + (r.attacks ? (r.reactedPct * r.attacks) / 100 : 0), 0);
  const returned = rows.reduce((a, r) => a + (r.attacks ? (r.returnedPct * r.attacks) / 100 : 0), 0);
  const engage = sum('engage');
  const exits = {};
  for (const r of rows) for (const [k, v] of Object.entries(r.exits)) inc(exits, k, v);
  return {
    manned: sum('manned'), episodes: sum('episodes'), engage,
    within10Pct: engage ? Math.round((100 * sum('within10')) / engage) : null,
    within30Pct: engage ? Math.round((100 * sum('within30')) / engage) : null,
    past90Pct: engage ? Math.round((100 * sum('past90')) / engage) : null,
    targetedPct: engage ? Math.round((100 * sum('targeted')) / engage) : null,
    noEnemyIdle: sum('noEnemyIdle'), noEnemyTurning: sum('noEnemyTurning'),
    shots: sum('shots'), hits: sum('hits'), damageDealt: sum('damageDealt'), damageTaken: sum('damageTaken'),
    attacks, reactedPct: attacks ? Math.round((100 * reacted) / attacks) : null, returnedPct: attacks ? Math.round((100 * returned) / attacks) : null,
    stuckNear: sum('stuckNear'), exits,
  };
}

async function battle(map, seed) {
  const side = sideOf(map);
  const game = await headlessGame(`map=${map}&side=${side}&ai=classic&tier=${tier}&seed=${seed}`);
  await game.ready;
  game.begin?.();
  if (flag('endless')) game.battle.reinforcements[1] = game.battle.reinforcements[2] = 1e6;
  const clock = { t: 0 };
  const stats = new Map();
  instrument(game, stats, clock);
  const tracks = new Map();
  const ticks = Math.round(minutes * 3600);
  const started = performance.now();
  let endedAt = null;
  for (let n = 1; n <= ticks; n++) {
    const tick = game.worldClock.tick + 1;
    game.step({ epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] });
    clock.t = n / 60;
    if (game.battle.gameOver) { endedAt = clock.t; break; }
    if (n % 15) continue;
    sample(game, stats, clock, 0.25);
    if (n % 60 === 0) stuckNear(game, stats, tracks, clock);
  }
  const rows = [...stats.values()].map(summarize);
  return { map, seed, side, tier, minutes: +((endedAt ?? ticks / 60) / 60).toFixed(2), wall: Math.round((performance.now() - started) / 1000), totals: totals(rows), turrets: rows, tracks: detail ? [...stats.values()].map((s) => s.track) : null };
}

// -- scripted duels ------------------------------------------------------------------------

// Floor near bearing a (radians, world yaw) and distance d of the turret
// with a clear line from eye height (1.6 m) to the turret's middle.
function findSpot(game, t, a, d) {
  const centre = new THREE.Vector3();
  t.sphere(centre);
  const down = new THREE.Vector3(0, -1, 0), dir = new THREE.Vector3(), eye = new THREE.Vector3();
  for (const dd of [0, 0.1, -0.1, 0.2, -0.2, 0.3, -0.3]) for (const da of [0, 5, -5, 10, -10]) {
    const r = d * (1 + dd), yaw = a + da / DEG;
    for (const lift of [3, 8, 20]) {
      const top = new THREE.Vector3(t.position.x + Math.sin(yaw) * r, t.position.y + lift, t.position.z + Math.cos(yaw) * r);
      const g = game.collision.cast(top, down, lift + 40, _hit, null);
      if (!g) continue;
      const spot = g.point.clone().add(new THREE.Vector3(0, 0.05, 0));
      eye.copy(spot).add(new THREE.Vector3(0, 1.6, 0));
      const len = eye.distanceTo(centre);
      dir.subVectors(centre, eye).divideScalar(len);
      const h = game.collision.cast(eye, dir, len, _hit, null);
      if (h && h.body !== t.bodies?.built && h.distance < len - 2.5) continue;
      return spot;
    }
  }
  return null;
}

async function duel(map, turretIndex, bearing, distance, seconds) {
  const side = sideOf(map);
  const game = await headlessGame(`map=${map}&side=${side}&ai=classic&tier=${tier}&seed=1`);
  await game.ready;
  game.begin?.();
  game.battle.reinforcements[1] = game.battle.reinforcements[2] = 1e6;
  const seats = game.turretSeats;
  const t = seats.turrets[turretIndex];
  const step = () => {
    const tick = game.worldClock.tick + 1;
    game.step({ epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] });
  };
  if (t && !t.alive) t.setHealth(t.maxHealth);
  // Let the battle spawn its armies.
  let gunner = null;
  for (let n = 0; n < 600 && !gunner; n++) {
    step();
    if (n < 30) continue;
    const enemyTeam = game.playerUnit.team;
    gunner = [...game.units.living()].find((u) => u.kind === 0 && !u.human && !u.seat && u.alive && game.units.relation(u.team, enemyTeam) < 0 && seats.canMan(u, t)) ?? null;
  }
  if (!gunner || !t?.alive) return { map, turret: turretIndex, bearing, distance, error: `no gunner (${Boolean(gunner)}) or turret (${t?.state})` };
  const clock = { t: 0 };
  const stats = new Map();
  instrument(game, stats, clock);
  const keep = new Set([gunner]);
  const clearOthers = () => {
    for (const u of game.units.living()) {
      if (keep.has(u) || u.human || u.kind !== 0 || !u.combat?.damage || u.seat?.turret === t) continue;
      u.combat.damage(Infinity);
    }
  };
  clearOthers();
  seats.man(gunner, t);
  if (gunner.seat?.turret !== t) return { map, turret: turretIndex, error: 'could not seat' };
  // The shooter: the player, held at the bearing from the drum's facing.
  const player = game.playerUnit;
  const facing = t.facing + t.yaw;
  const a = facing - bearing / DEG; // clockwise from above lowers yaw
  // The spot: floor near the bearing and distance (within 10 degrees and
  // 30 %) from which a line at eye height meets the turret first.
  const spot = findSpot(game, t, a, distance);
  if (!spot) return { map, turret: turretIndex, bearing, distance, error: 'no clear spot' };
  const rifle = game.combat?.arsenal?.weapons?.find((w) => w.def?.ordnance && /rifle|blaster/i.test(w.def.class || w.name || '')) ?? game.combat?.arsenal?.weapons?.[0];
  const ordDef = rifle?.def?.ordnance;
  const res = { map, turret: turretIndex, class: t.desc.class, bearing, distance, gunnerClass: gunner.className, firstTarget: null, aimed15: null, firstShot: null, firstHit: null, shots: 0, hits: 0, turretDamage: 0, exit: null, playerDied: null };
  const origin = new THREE.Vector3(), dir = new THREE.Vector3(), aimAt = new THREE.Vector3();
  const ticks = Math.round(seconds * 60);
  const s = stats.get(t);
  for (let n = 1; n <= ticks; n++) {
    if (!player.alive) { if (res.playerDied == null) res.playerDied = +clock.t.toFixed(2); }
    else {
      player.soldier.position.copy(spot);
      player.soldier.velocity.set(0, 0, 0);
    }
    if (n % 60 === 0) clearOthers();
    if (player.alive && n > 2 && n % 30 === 3 && ordDef && t.alive) {
      player.origin ? player.origin(origin) : origin.copy(spot).add(new THREE.Vector3(0, 1.6, 0));
      t.sphere(aimAt);
      dir.subVectors(aimAt, origin).normalize();
      game.ordnance.fire(ordDef, origin, dir, player, null, null);
      game.simEvents.emit('shot', { source: game.units.ref(player), team: player.team, position: origin.clone(), direction: dir.clone(), weapon: rifle.name, target: t.lease?.controllable ? null : null });
    }
    step();
    clock.t = n / 60;
    const lease = t.lease;
    if (!lease || lease.unit !== gunner) { res.exit = { at: +clock.t.toFixed(2), reasons: s.exits }; break; }
    const controller = lease.controllable?.controller;
    const target = targetUnit(game, controller);
    if (detail && n % 15 === 0) {
      const agent = controller?.agent;
      (res.trace ||= []).push(`${clock.t.toFixed(2)} ${agent?.constructor.name ?? '-'}/${agent?.state ?? '-'}${agent?.patterns ? `/p${agent.patterns.state}` : ''} aim${controller?.aimState} yaw=${((t.facing + t.yaw) * DEG).toFixed(0)} turn=${lease.input.turn.toFixed(2)} pitch=${(t.pitch * DEG).toFixed(0)} tgt=${target ? target.className : '-'} err=${player.alive ? barrelError(t, player).toFixed(0) : '-'} ${player.alive ? splitError(t, player) : ''} shot=${controller?.shot?.pending ? 1 : 0} fire=${controller?.fireA ? 'A' : ''}${controller?.fireB ? 'B' : ''} burst=${controller?.perception?.fire?.burst?.state ?? '-'}:${controller?.perception?.fire?.burst?.count ?? '-'} heat=${(t.weapon?.heat ?? 0).toFixed(2)}`);
    }
    if (res.firstTarget == null && target && sameFoe(game, target, player)) res.firstTarget = +clock.t.toFixed(2);
    if (res.aimed15 == null && player.alive && barrelError(t, player) < 15) res.aimed15 = +clock.t.toFixed(2);
    if (res.firstShot == null && s.shots > 0) res.firstShot = +clock.t.toFixed(2);
    if (res.firstHit == null && s.hits > 0) res.firstHit = +clock.t.toFixed(2);
  }
  res.shots = s.shots; res.hits = s.hits; res.turretDamage = Math.round(s.damageTaken);
  res.agent = (() => { const c = t.lease?.controllable?.controller; return c ? `${c.agent?.constructor.name}/${c.agent?.state}${c.agent?.patterns ? `/p${c.agent.patterns.state}` : ''}` : null; })();
  return res;
}

const results = [];
if (scenario) {
  const bearings = opt('bearings', '0,90,180').split(',').map(Number);
  const distances = opt('distances', '15,40').split(',').map(Number);
  const seconds = Number(opt('seconds', 12));
  for (const map of maps) {
    const turretList = opt('turrets', '0').split(',').map(Number);
    for (const ti of turretList) for (const b of bearings) for (const d of distances) {
      const r = await duel(map, ti, b, d, seconds);
      results.push(r);
      const { trace, ...rest } = r;
      console.log(JSON.stringify(rest));
      if (trace) for (const l of trace) console.log('   ', l);
    }
  }
  const ok = results.filter((r) => !r.error);
  const share = (k) => `${ok.filter((r) => r[k] != null).length}/${ok.length}`;
  console.log(`\nduels: targeted ${share('firstTarget')}, aimed ${share('aimed15')}, fired ${share('firstShot')}, hit ${share('firstHit')}, exited ${share('exit')}; median s to aim ${median(ok.map((r) => r.aimed15).filter((x) => x != null))}, to shot ${median(ok.map((r) => r.firstShot).filter((x) => x != null))}`);
} else {
  for (const map of maps) for (const seed of seeds) {
    const r = await battle(map, seed);
    results.push(r);
    console.log(`\n== ${map} seed ${seed} (${r.side}, ${r.minutes} min, ${r.wall} s wall)`);
    console.log('totals', JSON.stringify(r.totals));
    for (const row of r.turrets) if (row.manned || row.stuckNear) console.log(JSON.stringify(row));
    if (r.tracks) r.tracks.forEach((tr, i) => { if (tr.length) { console.log(`  turret ${i}:`); for (const l of tr) console.log('   ', l); } });
  }
  console.log('\nmap seed: manned s, engaged s, within30%, past90%, targeted%, shots, hits, attacks, reacted%, returned%, idle-no-enemy s, stuck-near s, exits');
  for (const r of results) {
    const x = r.totals;
    console.log(`${r.map} ${r.seed}: ${x.manned} ${x.engage} ${x.within30Pct} ${x.past90Pct} ${x.targetedPct} ${x.shots} ${x.hits} ${x.attacks} ${x.reactedPct} ${x.returnedPct} ${x.noEnemyIdle} ${x.stuckNear} ${JSON.stringify(x.exits)}`);
  }
}
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(results, (k, v) => (v instanceof Set ? v.size : k === 'attacker' ? undefined : v), 1));
process.exit(0);
