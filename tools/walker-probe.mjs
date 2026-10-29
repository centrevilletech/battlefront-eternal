// Walkers under Classic AI, headless: every walker hull (AT-AT, AT-TE, AT-ST,
// spider walker, tauntaun, kaadu) on a map, sampled once a game second.
//
//   node tools/walker-probe.mjs <map> [--seeds 1,2] [--minutes 5] [--side imp]
//     [--detail] [--json out.json]
//
// Per hull it reports: seconds with a bot at the controls, without one, and
// with nobody aboard; boardings and pilot exits by reason (the AI caller of
// the exit request, as file:line, or killed / destroyed); the pilot's goal
// posts and how often they changed; the nearest it came to an enemy post and
// to its goal post, and its goal-distance progress; shots its crew fired and
// damage they did (to objectives separately); posts its team took while the
// hull stood within 60 m; seconds outside the map boundary, held by a
// yielding contact, in the stuck recovery, turning on the spot, standing
// still (10 s within 2 m, not standing to shoot, waiting for its crew or an
// AT-AT at its post) and shoved (moved over 1.5 x its walking pace + 3 m in
// a second, and how far past its pace); and the pilot's agent and state, by
// seconds. --detail prints each hull's track every 10 s, with its target.
import fs from 'node:fs';
import path from 'node:path';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const map = argv[0] && !argv[0].startsWith('--') ? argv[0] : 'geo1';
const seeds = String(opt('seeds', process.env.SEED || '1')).split(',').map(Number);
const minutes = Number(opt('minutes', 5));
const detail = argv.includes('--detail');
const jsonOut = opt('json', null);
const { headlessGame, content } = await import('./headless-world.mjs');
const { UnitController } = await import('../src/ai/classic/controller.js');

function sideOf(name) {
  const mission = JSON.parse(fs.readFileSync(path.join(content, 'maps', name, 'map.json'))).mission ?? {};
  const first = mission.teams?.[1]?.classes?.[0]?.[0] ?? mission.teams?.[1]?.classes?.[0];
  const prefix = typeof first === 'string' ? first.split('_')[0] : null;
  return prefix && (mission.sides ?? []).includes(prefix) ? prefix : (mission.sides?.[0] ?? 'rep');
}
const side = opt('side', null) ?? { geo1: 'rep', hot1: 'imp' }[map] ?? sideOf(map);
const WALKER = /_walk_/;
const flat = (a, b) => Math.hypot(a.x - b.x, a.z - b.z);
const inc = (o, k, n = 1) => { o[k] = (o[k] ?? 0) + n; };

// The AI caller of an exit request: the first frame outside the controller
// and the vehicle service.
const requestExit = UnitController.prototype.requestExit;
UnitController.prototype.requestExit = function (...args) {
  const frames = String(new Error().stack).split('\n').slice(2);
  const frame = frames.find((f) => !/controller\.js:66\d|vehicles\/index\.js:39\d/.test(f)) ?? frames[0] ?? '';
  const m = frame.match(/src\/(.*?):(\d+):\d+/);
  // On the seat's lease, so only a request made from this seat counts.
  const lease = this.unit?.lease ?? this.unit?.seat ?? null;
  if (lease) lease.probeExitCaller = m ? `${m[1]}:${m[2]}` : 'unknown';
  return requestExit.apply(this, args);
};

async function run(seed) {
  const game = await headlessGame(`map=${map}&side=${side}&ai=classic&seed=${seed}`);
  await game.ready;
  game.begin?.();
  const system = game.vehicles;
  const nav = game.ai.nav;
  const posts = game.posts.posts;
  const objectives = game.live?.objectives ?? [];
  const hulls = new Map(); // vehicle -> stats
  const stat = (v) => {
    let s = hulls.get(v);
    if (!s) hulls.set(v, (s = {
      name: v.name, index: system.vehicles.indexOf(v), team: v.team,
      piloted: 0, crewedNoPilot: 0, empty: 0, boards: 0, exits: {}, deaths: 0,
      goals: {}, goalChanges: 0, lastGoal: null, nearestEnemyPost: Infinity, nearestGoal: Infinity,
      progress: 0, shots: 0, damage: 0, objectiveDamage: {}, assisted: [], outside: 0, held: 0,
      recovery: 0, spinning: 0, stood: 0, shoved: 0, shovedMetres: 0, states: {}, track: [], window: [],
    }));
    return s;
  };
  for (const v of system.vehicles) if (WALKER.test(v.name)) stat(v);
  const walkerOf = (unit) => {
    const v = unit?.seat?.vehicle;
    return v && WALKER.test(v.name) ? v : null;
  };
  // Boardings and exits of the controls.
  const vacate = system.vacate.bind(system);
  system.vacate = (lease, opts = {}) => {
    const v = lease.vehicle;
    const pilot = lease.index === 0 && !lease.human;
    const ok = vacate(lease, opts);
    if (ok && pilot && v && WALKER.test(v.name)) {
      const reason = opts.reason === 'killed' ? 'killed' : (lease.probeExitCaller ?? `${opts.reason ?? 'exit'} (no AI request)`);
      inc(stat(v).exits, reason);
    }
    return ok;
  };
  const board = system.board.bind(system);
  system.board = (unit, v, i = -1) => {
    const ok = board(unit, v, i);
    if (ok && WALKER.test(v.name) && v.seats[0]?.lease?.unit === unit) stat(v).boards++;
    return ok;
  };
  // Shots and damage by walker crews.
  game.simEvents.listen((e) => {
    if (e.kind === 'shot') {
      const unit = e.source ? game.units.resolve(e.source) : null;
      const v = walkerOf(unit);
      if (v) stat(v).shots++;
    } else if (e.kind === 'damage') {
      const unit = e.attacker ? game.units.resolve(e.attacker) : null;
      const v = walkerOf(unit);
      if (v && e.attackerTeam !== e.team) { const a = Number(e.amount); if (Number.isFinite(a)) stat(v).damage += a; }
    }
  });
  for (const b of objectives) {
    const damage = b.damage.bind(b);
    b.damage = (amount, attacker) => {
      const v = walkerOf(attacker);
      if (v) inc(stat(v).objectiveDamage, b.desc.post, amount);
      return damage(amount, attacker);
    };
  }
  const owner = new Map(posts.map((p) => [p, p.team]));
  const ticks = Math.round(minutes * 3600);
  const started = performance.now();
  for (let n = 1; n <= ticks; n++) {
    const tick = game.worldClock.tick + 1;
    game.step({ epoch: 1, sequence: tick, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] });
    if (n % 60) continue;
    const t = n / 60;
    for (const v of system.vehicles) {
      if (!WALKER.test(v.name)) continue;
      const s = stat(v);
      if (!v.alive) {
        if (!s.dead) { s.deaths++; s.dead = true; s.last = null; }
        continue;
      }
      s.dead = false;
      const p = v.position;
      const lease = v.seats[0]?.lease;
      const crew = v.seats.some((x) => x.lease);
      const seatUnit = lease && !lease.human ? lease.controllable : null;
      const controller = seatUnit?.controller;
      if (!crew) { s.empty++; s.window.length = 0; s.last = null; continue; }
      if (!controller) { s.crewedNoPilot++; s.window.length = 0; s.last = null; continue; }
      s.piloted++;
      if (nav?.boundary?.outside(p)) s.outside++;
      const st = seatUnit.steer ?? {};
      if (st.holding) s.held++;
      if (st.recovery) s.recovery++;
      const agent = controller.agent;
      const key = `${agent?.constructor.name ?? 'none'}/${agent?.state ?? '-'}${agent?.patterns && agent.state === 4 ? `/p${agent.patterns.state}` : ''}`;
      inc(s.states, key);
      const character = lease.unit?.character;
      const goal = character?.objective ?? null;
      const goalName = goal?.name ?? null;
      if (goalName) inc(s.goals, goalName);
      if (s.lastGoal !== null && goalName !== s.lastGoal) s.goalChanges++;
      s.lastGoal = goalName;
      for (const post of posts) {
        if (post.active === false || !post.team || post.team === v.controllingTeam) continue;
        if (game.units.relation?.(v.controllingTeam, post.team) >= 0) continue;
        s.nearestEnemyPost = Math.min(s.nearestEnemyPost, flat(post.position, p));
      }
      if (goal) {
        const d = flat(goal.position, p);
        s.nearestGoal = Math.min(s.nearestGoal, d);
        if (s.goalRef === goal && Number.isFinite(s.goalDistance)) s.progress += s.goalDistance - d;
        s.goalRef = goal;
        s.goalDistance = d;
      }
      // Turning on the spot and standing still.
      const prevYaw = s.prevYaw ?? v.yaw;
      s.prevYaw = v.yaw;
      const speed = Math.hypot(v.velocity.x, v.velocity.z);
      const turned = Math.abs(Math.atan2(Math.sin(v.yaw - prevYaw), Math.cos(v.yaw - prevYaw)));
      if (speed < 0.5 && turned > 0.2) s.spinning++;
      // Moved much farther in the second than its legs carry it.
      const top = Math.max(v.speed ?? 0, seatUnit.classDef?.MaxSpeed ?? 5);
      if (s.last) {
        const moved = Math.hypot(p.x - s.last.x, p.z - s.last.z);
        if (moved > 1.5 * top + 3) { s.shoved++; s.shovedMetres += moved - top; }
      }
      s.last = { x: p.x, z: p.z };
      // Holding still on purpose: standing to shoot, waiting for its crew, an
      // AT-AT at its post.
      const still = (agent?.patterns && agent.state === 4 && agent.patterns.state === 2) || /WaitSecondary|Hold/.test(agent?.constructor.name ?? '')
        || (/_atat/.test(v.name) && agent?.constructor.name === 'UnitTrooperAgent' && agent.state === 2);
      s.window.push({ x: p.x, z: p.z, fight: still });
      if (s.window.length > 10) s.window.shift();
      if (s.window.length === 10 && !s.window.some((w) => w.fight) && s.window.every((w) => Math.hypot(w.x - p.x, w.z - p.z) < 2)) s.stood++;
      if (detail && t % 10 === 0) {
        const helper = agent?.helper;
        const target = agent?.tracker?.target ?? null;
        const tu = target?.unit ?? target;
        const tgt = tu?.position ? ` tgt=${tu.className ?? tu.name ?? '?'}@${flat(tu.position, p).toFixed(0)}` : '';
        s.track.push(`t${t} (${p.x.toFixed(0)},${p.z.toFixed(0)}) ${key} goal=${goalName ?? '-'}${goal ? `@${flat(goal.position, p).toFixed(0)}` : ''} goto${helper?.state ?? '-'} spd=${speed.toFixed(1)} hp=${Math.round(v.health)}${tgt}${st.recovery ? ` rec${st.recovery}` : ''}${st.holding ? ' held' : ''}`);
      }
    }
    for (const post of posts) {
      if (owner.get(post) === post.team) continue;
      owner.set(post, post.team);
      if (!post.team) continue;
      for (const [v, s] of hulls) {
        if (v.alive && v.controllingTeam === post.team && v.seats.some((x) => x.lease) && flat(v.position, post.position) < 60) s.assisted.push(`${post.name}@${t}`);
      }
    }
    if (game.battle.gameOver) break;
  }
  const out = [...hulls.values()].map((s) => ({
    name: s.name, index: s.index, team: s.team, piloted: s.piloted, crewedNoPilot: s.crewedNoPilot, empty: s.empty,
    boards: s.boards, exits: s.exits, deaths: s.deaths, goals: s.goals, goalChanges: s.goalChanges,
    nearestEnemyPost: Number.isFinite(s.nearestEnemyPost) ? Math.round(s.nearestEnemyPost) : null,
    nearestGoal: Number.isFinite(s.nearestGoal) ? Math.round(s.nearestGoal) : null, progress: Math.round(s.progress),
    shots: s.shots, damage: Math.round(s.damage), objectiveDamage: Object.fromEntries(Object.entries(s.objectiveDamage).map(([k, x]) => [k, Math.round(x)])),
    assisted: s.assisted, outside: s.outside, held: s.held, recovery: s.recovery, spinning: s.spinning, stood: s.stood, shoved: s.shoved, shovedMetres: Math.round(s.shovedMetres),
    states: Object.fromEntries(Object.entries(s.states).sort((a, b) => b[1] - a[1]).slice(0, 6)),
    ...(detail ? { track: s.track } : {}),
  }));
  return {
    map, side, seed, minutes, seconds: Math.round((performance.now() - started) / 1000),
    posts: posts.map((p) => `${p.name}:${p.team}${p.active === false ? 'x' : ''}`).join(' '),
    objectives: objectives.map((b) => `${b.desc.post}:${Math.round(b.health)}`).join(' '),
    tickets: game.battle.reinforcements, gameOver: Boolean(game.battle.gameOver), hulls: out,
  };
}

const results = [];
for (const seed of seeds) {
  const r = await run(seed);
  results.push(r);
  console.log(`== ${r.map} side=${r.side} seed=${r.seed} ${r.minutes} min (${r.seconds} s) posts ${r.posts}${r.objectives ? ` objectives ${r.objectives}` : ''} tickets ${JSON.stringify(r.tickets)}${r.gameOver ? ' (game over)' : ''}`);
  for (const h of r.hulls) {
    const { track, ...rest } = h;
    console.log(JSON.stringify(rest));
    if (track) for (const line of track) console.log('   ', line);
  }
}
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(results, null, 1));
process.exit(0);
