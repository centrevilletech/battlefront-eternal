// Classic AI bots that stop moving: whole battles headless (the world
// worker's and the server's code), every bot sampled once a game second.
// A bot is stuck when it has lived a whole window (10 s) without leaving a
// 1 m ball, outside the states that hold still on purpose: a turret or
// passenger seat, a snipe hint, a patrol hint or the roam's wait, a hold
// order, standing in a capture region of a post it is taking, the crew wait,
// and anyone who fired in the window. A stuck driver is counted by its hull.
//
//   BFE_CONTENT_DIR=<game-data> node tools/ai-stuck-probe.mjs \
//     [--maps nab2,tat2] [--seeds 1,2] [--minutes 5] [--window 10] [--radius 1]
//     [--mode sp|mp] [--army 32] [--ai classic|enhanced] [--tier medium] [--endless] [--json out.json] [--detail 3]
//
// Without BFE_CONTENT_DIR it reads public/, or the desktop app's imported
// game data when public/ has no maps. Single player spawns the player (team
// 1's side) and leaves him idle at his post; --mode mp runs the dedicated
// server's world (src/runtime/multiplayer-world.js, armies of --army, no
// players). --endless gives both teams a million reinforcements so a match
// outlasts the run. Per map and seed it prints stuck bot-seconds per minute,
// the most stuck at once (and what they were doing then), and the stuck bots
// grouped by AI state (agent, its state, the patrol/snipe machine, the
// movement helper's state, the path executor and the planner), with --detail
// examples of each (goal, queue, path, planner slot, target, posture,
// contacts).
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const flag = (name) => argv.includes(`--${name}`);
const maps = opt('maps', 'nab2').split(',');
const seeds = opt('seeds', '1').split(',').map(Number);
const minutes = Number(opt('minutes', 5));
const WINDOW = Number(opt('window', 10));
const RADIUS = Number(opt('radius', 1));
const mode = opt('mode', 'sp');
const army = Number(opt('army', 32));
const tier = opt('tier', 'medium');
const aiMode = opt('ai', 'classic');
const detail = Number(opt('detail', 2));
const jsonOut = opt('json', null);
const endless = flag('endless');

const desktop = path.join(os.homedir(), '.config/Battlefront Eternal/game-data');
if (!process.env.BFE_CONTENT_DIR && !fs.existsSync(new URL('../public/maps', import.meta.url)) && fs.existsSync(desktop)) process.env.BFE_CONTENT_DIR = desktop;
const { headlessGame, content } = await import('./headless-world.mjs');

// The player's side for single player: a side of team 1.
function sideOf(map) {
  const mission = JSON.parse(fs.readFileSync(path.join(content, 'maps', map, 'map.json'))).mission ?? {};
  const first = mission.teams?.[1]?.classes?.[0]?.[0] ?? mission.teams?.[1]?.classes?.[0];
  const prefix = typeof first === 'string' ? first.split('_')[0] : null;
  return prefix && (mission.sides ?? []).includes(prefix) ? prefix : (mission.sides?.[0] ?? 'rep');
}

const horizontal = (a, b) => Math.hypot(a.x - b.x, a.z - b.z);

// What a stuck bot's AI is doing, for grouping (sig) and for reading (state).
function describe(game, controller, body, hull) {
  const unit = controller.unit;
  const agent = controller.agent;
  const helper = agent?.helper;
  const control = controller.navControl;
  const exec = control?.executor;
  const planner = game.ai.nav?.planner;
  const goal = controller.commands.current;
  const inner = agent?.activeHelper?.() ?? null;
  const innerName = inner === agent?.patrol ? 'patrol' : inner === agent?.snipe ? 'snipe' : inner === agent?.mine ? 'mine' : null;
  const queued = planner ? planner.queue.findIndex((r) => r.owner === control) : -1;
  const request = control?.request ?? controller.request;
  const p = (hull ?? body).position;
  const near = game.units.living().filter((u) => u !== body && !u.seat && u.position.distanceTo(body.position) < 1.2).length;
  const target = agent?.tracker?.target ?? null;
  const post = controller.character?.objective ?? null;
  const helperState = helper?.state ?? null;
  const sig = [
    agent?.constructor.name ?? `state${controller.state}`, agent?.state,
    innerName ? `${innerName}${inner.state}` : null,
    agent?.patterns && agent.state === 4 ? `pattern${agent.patterns.state}` : null,
    `goto${helperState}${helper?.failed ? 'F' : ''}`,
    exec ? `exec${exec.status}${exec.complete ? 'C' : ''}${exec.hold ? 'H' : ''}` : 'noexec',
    queued >= 0 ? `queued${planner.queue[queued].priority}` : planner?.current?.owner === control ? 'searching' : null,
    hull ? 'driver' : null,
  ].filter((x) => x != null && x !== '').join(' ');
  const state = {
    class: body.className, team: body.team, kind: unit?.kind,
    controller: { state: controller.state, pending: controller.pending, goal: { kind: goal.kind, mode: goal.mode, post: goal.post?.name ?? null }, queue: controller.commands.queue.map((g) => g.kind), latch: controller.commands.latch, done: controller.commands.currentDone },
    agent: agent ? { name: agent.constructor.name, state: agent.state, pending: agent.pending, inner: innerName, innerState: inner?.state, pattern: agent.pattern, patternState: agent.patterns?.state } : null,
    helper: helper ? { state: helperState, pending: helper.pending, failed: helper.failed, cursor: Boolean(helper.cursor), retries: helper.retries, destination: helper.destination ? +horizontal(helper.destination, p).toFixed(1) : null, arrived: helper.arrived } : null,
    path: exec ? { status: exec.status, complete: exec.complete, hold: exec.hold, nodes: exec.path.nodes.length, index: exec.index, waypoint: +horizontal(exec.waypoint, p).toFixed(1), stationary: +exec.stationary.toFixed(1), noProgress: +exec.noProgress.toFixed(1) } : null,
    planner: planner ? { queue: planner.queue.length, slot: queued, priority: queued >= 0 ? planner.queue[queued].priority : null, searching: planner.current?.owner === control, ready: control?.ready, published: Boolean(control?.published), segments: control?.segments, served: planner.served } : null,
    request: { valid: request.valid, steer: request.valid ? +horizontal(request.steeringTarget, p).toFixed(1) : null },
    intent: { forward: +(unit?.intent?.forward ?? 0).toFixed(2), right: +(unit?.intent?.right ?? 0).toFixed(2) },
    speed: +(hull ?? body).velocity.length().toFixed(2),
    soldier: hull ? null : { posture: body.soldier.posture, air: body.soldier.air, lock: body.soldier.lock, dive: Boolean(body.soldier.dive), avoidance: +(body.soldier.avoidance?.length?.() ?? 0).toFixed(2) },
    target: target ? { class: target.className, distance: +target.position.distanceTo(p).toFixed(1), fresh: Boolean(agent.tracker.freshTarget), weapon: unit?.combat?.arsenal?.current?.(controller.perception?.fire?.channel ?? 0)?.def?.class ?? null, weaponTarget: Boolean(controller.perception?.fire?.weaponTarget?.unit) } : null,
    job: agent?.job?.target?.unit ? { class: agent.job.target.unit.className, kind: agent.job.kind, targetKind: agent.job.target.unit.kind, alive: agent.job.target.unit.alive, distance: +horizontal(agent.job.target.unit.position, p).toFixed(1), resolves: Boolean(game.units.resolve(agent.job.target)), since: agent.service ? +(game.ai.classic.ai.world.time - agent.service.approachAt).toFixed(1) : null } : null,
    leader: controller.leader?.unit ?{ class: controller.leader.unit.className, alive: controller.leader.unit.alive, distance: +controller.leader.unit.position.distanceTo(p).toFixed(1) } : null,
    objective: post ? { name: post.name, team: post.team, distance: +horizontal(post.position, p).toFixed(1), inside: typeof post.contains === 'function' ? post.contains(p, new game.THREE.Vector3()) : null } : null,
    crowd: near,
    position: [p.x, p.y, p.z].map((v) => +v.toFixed(1)),
  };
  return { sig, state };
}

// Holding still on purpose (null: not).
function holding(game, controller, body) {
  const seat = body.seat;
  if (seat && !(seat.vehicle && seat.index === 0)) return 'mounted';
  const agent = controller.agent;
  if (!agent) return null;
  const name = agent.constructor.name;
  if (name === 'UnitHoldAgent') return 'hold';
  if (name === 'UnitWaitSecondaryAgent') return 'crew-wait';
  const inner = agent.activeHelper?.();
  if (inner && inner === agent.snipe && inner.state === 3) return 'snipe';
  if (inner && inner === agent.patrol && (inner.state === 5 || inner.state === 6)) return 'patrol-post';
  if (inner && inner === agent.patrol && inner.state === 2 && !seat) {
    const post = inner.post;
    if (post && typeof post.contains === 'function' && post.contains(body.position, new game.THREE.Vector3())) return 'capturing';
  }
  return null;
}

async function run(map, seed) {
  const side = sideOf(map);
  const query = `map=${map}&side=${side}&ai=${aiMode}&tier=${tier}&seed=${seed}`;
  const game = await headlessGame(query);
  await game.ready;
  game.begin?.();
  let world = null;
  if (mode === 'mp') {
    const { createMultiplayerWorld } = await import('../src/runtime/multiplayer-world.js');
    world = createMultiplayerWorld(game, { maxPlayers: 64, armySize: army });
  }
  if (endless) game.battle.reinforcements[1] = game.battle.reinforcements[2] = 1e6;
  const fired = new Map(); // unit -> last shot time
  game.simEvents.listen((e) => { if (e.kind === 'shot' && e.source?.unit) fired.set(e.source.unit, e.time); });
  const tracks = new Map(); // character -> { key, samples: [{t, x, y, z}] }
  const episodes = new Map(); // character -> open episode
  const closed = [];
  const groups = new Map(); // sig -> { seconds, examples }
  const reasons = {};
  let stuckSeconds = 0, peak = 0, peakAt = 0, peakSigs = {}, sampled = 0, bots = 0;
  const ticks = Math.round(minutes * 60 * 60);
  const started = performance.now();
  let seq = 0, endedAt = null;
  for (let n = 1; n <= ticks; n++) {
    if (world) world.step();
    else {
      const tick = game.worldClock.tick + 1;
      seq = Math.max(seq + 1, tick);
      game.step({ epoch: 1, sequence: seq, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] });
    }
    if (game.battle.gameOver && endedAt == null) endedAt = n / 60;
    if (n % 60 || game.battle.gameOver) continue;
    const t = n / 60;
    sampled++;
    let stuckNow = 0;
    const nowSigs = {};
    const live = new Set();
    for (const character of game.battle.characters) {
      if (character.human) continue;
      const body = character.unit;
      const controller = character.controller;
      if (!body?.alive || !controller?.unit) { tracks.delete(character); continue; }
      live.add(character);
      bots++;
      const hull = body.seat?.vehicle && body.seat.index === 0 ? body.seat.vehicle : null;
      const key = `${body.id}:${body.generation}:${hull ? 'v' + game.vehicles.vehicles.indexOf(hull) : body.seat ? 's' : 'f'}`;
      let track = tracks.get(character);
      if (!track || track.key !== key) tracks.set(character, (track = { key, samples: [] }));
      const p = (hull ?? body).position;
      track.samples.push({ t, x: p.x, y: p.y, z: p.z });
      while (track.samples.length && track.samples[0].t < t - WINDOW) track.samples.shift();
      let stuck = false;
      if (track.samples[0].t <= t - WINDOW) {
        let far = 0;
        for (const s of track.samples) far = Math.max(far, Math.hypot(s.x - p.x, s.y - p.y, s.z - p.z));
        if (far <= RADIUS) {
          const shot = fired.get(controller.unit) ?? fired.get(body) ?? -Infinity;
          const why = holding(game, controller, body) ?? (shot > t - WINDOW ? 'fighting' : null);
          if (why) reasons[why] = (reasons[why] ?? 0) + 1;
          else stuck = true;
        }
      }
      if (!stuck) {
        const open = episodes.get(character);
        if (open) { open.end = t; open.cause = 'moved'; closed.push(open); episodes.delete(character); }
        continue;
      }
      stuckNow++;
      const { sig, state } = describe(game, controller, body, hull);
      nowSigs[sig] = (nowSigs[sig] ?? 0) + 1;
      let group = groups.get(sig);
      if (!group) groups.set(sig, (group = { seconds: 0, bots: new Set(), examples: [] }));
      group.seconds++;
      group.bots.add(character);
      if (!episodes.has(character)) {
        const episode = { character: character.index, team: character.team, start: t, sig, state };
        episodes.set(character, episode);
        if (group.examples.length < detail) group.examples.push(episode);
      }
    }
    for (const [character, open] of episodes) {
      if (!live.has(character)) { open.end = t; open.cause = 'died'; closed.push(open); episodes.delete(character); }
    }
    stuckSeconds += stuckNow;
    if (stuckNow > peak) { peak = stuckNow; peakAt = t; peakSigs = nowSigs; }
  }
  for (const open of episodes.values()) { open.end = null; open.cause = 'open'; closed.push(open); }
  const played = (endedAt ?? ticks / 60) / 60;
  const result = {
    map, seed, side, mode, tier, minutes: +played.toFixed(2), ended: endedAt, wallSeconds: +((performance.now() - started) / 1000).toFixed(1),
    meanBots: +(bots / Math.max(1, sampled)).toFixed(1),
    stuckBotSecondsPerMinute: +(stuckSeconds / played).toFixed(1), peak, peakAt, peakSigs,
    episodes: closed.length, longest: closed.reduce((m, e) => Math.max(m, (e.end ?? played * 60) - e.start), 0),
    holding: reasons,
    groups: [...groups].sort((a, b) => b[1].seconds - a[1].seconds).map(([sig, g]) => ({ sig, seconds: g.seconds, bots: g.bots.size, examples: g.examples.map((e) => ({ start: e.start, end: e.end, cause: e.cause, ...e.state })) })),
  };
  return result;
}

const results = [];
for (const map of maps) {
  for (const seed of seeds) {
    const r = await run(map, seed);
    results.push(r);
    console.log(`\n${map} seed ${seed} (${r.mode}, ${r.side}, ${r.minutes} min${r.ended != null ? `, ended ${r.ended.toFixed(0)} s` : ''}, ~${r.meanBots} bots, ${r.wallSeconds} s wall): ` +
      `${r.stuckBotSecondsPerMinute} stuck bot-s/min, peak ${r.peak} at ${r.peakAt} s, ${r.episodes} episodes, longest ${r.longest.toFixed(0)} s`);
    console.log(`  holding still on purpose (bot-s): ${JSON.stringify(r.holding)}`);
    console.log(`  at the peak: ${JSON.stringify(r.peakSigs)}`);
    for (const g of r.groups.slice(0, 12)) {
      console.log(`  ${String(g.seconds).padStart(5)} bot-s ${String(g.bots).padStart(3)} bots  ${g.sig}`);
      for (const e of g.examples) console.log(`      ${JSON.stringify(e)}`);
    }
  }
}
console.log('\nmap seed: stuck bot-s/min, peak');
for (const r of results) console.log(`${r.map} ${r.seed}: ${r.stuckBotSecondsPerMinute}, ${r.peak}`);
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(results, null, 1));
process.exit(0);
