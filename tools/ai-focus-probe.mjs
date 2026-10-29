// Where bots spend their time, why they leave their objectives, and how well
// they shoot: whole headless battles (the world worker's code), every bot
// sampled once a game second. For comparing Classic and Enhanced
// (docs/ai-enhanced.md, "Measuring").
//
//   BFE_CONTENT_DIR=<game-data> node tools/ai-focus-probe.mjs \
//     [--ai classic|enhanced] [--tier easy|medium|hard] [--maps nab2,tat2] [--seeds 1,2]
//     [--minutes 5] [--endless] [--json out.json]
//
// Single player with the player idle at his post (team 1's side); --endless
// gives both teams a million reinforcements so no battle ends early. Per
// battle, then pooled per side (the player's team, "allies", and the other):
//
//   time       share of bot-seconds by what the bot was doing: on its
//              objective (heading to the post, capturing, patrolling a
//              friendly post, sniping, mining, a repair job), in combat
//              (investigating, looking round, fighting, evading; "+sees" when
//              it had a fresh target), mounted, boarding, recovery
//   reached    lives that ever stood in their objective's capture region
//   captures   posts taken per battle-minute, by side
//   hits/shot  damage events on enemies per round fired, on-foot bots, by
//              weapon family (splash and pellets count once per victim)
//   interrupts per battle-minute, by what was interrupted and why
//
// Without BFE_CONTENT_DIR it reads public/.
import fs from 'node:fs';
import path from 'node:path';
import { headlessGame, content } from './headless-world.mjs';

const argv = process.argv.slice(2);
const opt = (name, fallback) => { const i = argv.indexOf(`--${name}`); return i < 0 ? fallback : argv[i + 1]; };
const maps = opt('maps', 'nab2').split(',');
const seeds = opt('seeds', '1').split(',').map(Number);
const minutes = Number(opt('minutes', 5));
const tier = opt('tier', 'medium');
const aiMode = opt('ai', 'classic');
const endless = argv.includes('--endless');
const jsonOut = opt('json', null);

function sideOf(map) {
  const mission = JSON.parse(fs.readFileSync(path.join(content, 'maps', map, 'map.json'))).mission ?? {};
  const first = mission.teams?.[1]?.classes?.[0]?.[0] ?? mission.teams?.[1]?.classes?.[0];
  const prefix = typeof first === 'string' ? first.split('_')[0] : null;
  return prefix && (mission.sides ?? []).includes(prefix) ? prefix : (mission.sides?.[0] ?? 'rep');
}

const COMBAT_STATE = { 1: 'start', 2: 'investigate', 3: 'look-round', 4: 'fight', 5: 'evade', 6: 'prone', 100: 'done' };
const PRIORITY = { 1: 'grenade', 2: 'shot-at/hit', 3: 'saw-target', 4: 'heard-enemy', 5: 'squad-cmd', 6: 'board', 7: 'crew-wait', 8: 'stuck-recovery' };
const OTHER_STATE = { 1: 'wait', 4: 'crew-wait', 5: 'boarding', 6: 'unboarding', 9: 'recovery', 10: 'follow', 11: 'hold' };

function bucket(game, character, body, scratch) {
  const c = character.controller;
  if (body.seat) return body.seat.vehicle && body.seat.index === 0 ? 'mounted:driver' : 'mounted:gunner';
  const agent = c.agent;
  if (c.state === 2) return `combat:${COMBAT_STATE[agent?.state] ?? agent?.state}${agent?.tracker?.freshTarget ? '+sees' : ''}`;
  if (c.state !== 3) return OTHER_STATE[c.state] ?? `state${c.state}`;
  const inner = agent?.activeHelper?.() ?? null;
  const post = character.objective;
  const inside = Boolean(post && typeof post.contains === 'function' && post.contains(body.position, scratch));
  if (inner && inner === agent.patrol) return inner.state === 2 ? (inside ? 'objective:capturing' : 'objective:heading') : 'objective:patrolling-friendly';
  if (inner && inner === agent.snipe) return 'objective:sniping';
  if (inner && inner === agent.mine) return 'objective:mining';
  // The repair agent (approach state 2) services a job in state 3.
  if (agent?.approachState === 2 && agent.state === 3) return 'objective:repair-job';
  return inside ? 'objective:capturing' : 'objective:heading';
}

const family = (w) => !w ? 'other'
  : /sniper/i.test(w) ? 'sniper'
  : /pistol/i.test(w) ? 'pistol'
  : /rifle|carbine|blaster|wrist/i.test(w) ? 'rifle'
  : /launcher|rocket|missile|torpedo/i.test(w) ? 'launcher'
  : 'other';

async function run(map, seed) {
  const side = sideOf(map);
  const game = await headlessGame(`map=${map}&side=${side}&ai=${aiMode}&tier=${tier}&seed=${seed}`);
  await game.ready;
  game.begin?.();
  if (endless) game.battle.reinforcements[1] = game.battle.reinforcements[2] = 1e6;
  const world = game.ai.classic.ai.world;
  const playerTeam = game.playerUnit.team;
  const sideKey = (team) => (team === playerTeam ? 'ally' : 'enemy');
  const scratch = new game.THREE.Vector3();

  // Interrupts, by what they interrupted and why (the controller's trace
  // hook, which the interrupt helper calls whether tracing is on or not).
  const interrupts = {};
  let hooked = false;
  const hook = (controller) => {
    if (hooked) return;
    hooked = true;
    const proto = Object.getPrototypeOf(controller);
    const original = proto.trace;
    proto.trace = function (kind, fields) {
      if (kind === 'interrupt') {
        const from = this.state === 3 ? 'objective' : this.state === 2 ? 'combat' : `state${this.state}`;
        const key = `${from} <- ${PRIORITY[fields.priority] ?? fields.priority}`;
        (interrupts[key] ??= { ally: 0, enemy: 0 })[sideKey(this.unit?.team)]++;
      }
      return original.call(this, kind, fields);
    };
  };

  const shots = { ally: {}, enemy: {} };
  const hits = { ally: {}, enemy: {} };
  const kills = { ally: 0, enemy: 0 };
  const held = new Map();
  game.simEvents.listen((e) => {
    if (e.kind === 'shot') {
      const u = e.source?.unit;
      if (!u || u.human || e.mount || u.seat || e.repair) return;
      const f = family(e.weapon);
      held.set(u, f);
      const s = shots[sideKey(u.team)];
      s[f] = (s[f] ?? 0) + 1;
    } else if (e.kind === 'damage') {
      const a = e.attacker?.unit;
      const v = e.target?.unit;
      if (!a || a.human || a.seat || !v || !(e.amount > 0) || game.units.relation(a.team, v.team) >= 0) return;
      const f = held.get(a) ?? 'other';
      const h = hits[sideKey(a.team)];
      h[f] = (h[f] ?? 0) + 1;
    } else if (e.kind === 'death') {
      const a = e.attacker?.unit;
      if (a && !a.human) kills[sideKey(a.team)]++;
    }
  });

  const seconds = { ally: {}, enemy: {} };
  const lives = new Map();
  const captures = { ally: 0, enemy: 0, neutralized: 0 };
  let owners = null;
  const ticks = Math.round(minutes * 3600);
  let seq = 0, endedAt = null;
  const started = performance.now();
  for (let n = 1; n <= ticks; n++) {
    const tick = game.worldClock.tick + 1;
    seq = Math.max(seq + 1, tick);
    game.step({ epoch: 1, sequence: seq, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [] });
    if (game.battle.gameOver) { endedAt = n / 60; break; }
    if (n % 60) continue;
    const now = (world.posts.all ? world.posts.all() : world.posts.posts).map((p) => p.team);
    if (owners) now.forEach((o, i) => { if (o !== owners[i]) captures[o === 0 ? 'neutralized' : sideKey(o)]++; });
    owners = now;
    for (const character of game.battle.characters) {
      const body = character.unit;
      const c = character.controller;
      if (character.human || !body?.alive || !c?.unit) continue;
      hook(c);
      const t = sideKey(body.team);
      const b = bucket(game, character, body, scratch);
      seconds[t][b] = (seconds[t][b] ?? 0) + 1;
      const key = `${body.id}:${body.generation}`;
      let life = lives.get(key);
      if (!life) lives.set(key, (life = { side: t, reached: false }));
      const post = character.objective;
      if (post && !body.seat && typeof post.contains === 'function' && post.contains(body.position, scratch)) life.reached = true;
    }
  }
  const played = (endedAt ?? ticks / 60) / 60;
  const per = (o) => Object.fromEntries(Object.entries(o).map(([k, v]) => [k, +(v / played).toFixed(2)]));
  const sideResult = (t) => {
    const l = [...lives.values()].filter((x) => x.side === t);
    return { seconds: seconds[t], lives: l.length, reached: l.filter((x) => x.reached).length, shots: shots[t], hits: hits[t], kills: kills[t] };
  };
  return {
    map, seed, side, ai: aiMode, tier, minutes: +played.toFixed(2), wall: +((performance.now() - started) / 1000).toFixed(0),
    captures: per(captures), ally: sideResult('ally'), enemy: sideResult('enemy'),
    interrupts: Object.fromEntries(Object.entries(interrupts).map(([k, v]) => [k, { ally: +(v.ally / played).toFixed(1), enemy: +(v.enemy / played).toFixed(1) }])),
  };
}

const pct = (a, b) => (b ? `${((100 * a) / b).toFixed(1)}%` : '-');

function report(results) {
  const minutesTotal = results.reduce((s, r) => s + r.minutes, 0);
  console.log(`\n${aiMode} ${tier}: ${results.length} battles, ${minutesTotal.toFixed(0)} battle-minutes`);
  for (const t of ['ally', 'enemy']) {
    const time = {}, top = {}, shots = {}, hits = {};
    let lives = 0, reached = 0, kills = 0;
    for (const r of results) {
      const s = r[t];
      for (const [k, v] of Object.entries(s.seconds)) { time[k] = (time[k] ?? 0) + v; const g = k.split(':')[0]; top[g] = (top[g] ?? 0) + v; }
      for (const [k, v] of Object.entries(s.shots)) shots[k] = (shots[k] ?? 0) + v;
      for (const [k, v] of Object.entries(s.hits)) hits[k] = (hits[k] ?? 0) + v;
      lives += s.lives; reached += s.reached; kills += s.kills;
    }
    const total = Object.values(time).reduce((a, b) => a + b, 0);
    const fmt = (o) => Object.entries(o).sort((a, b) => b[1] - a[1]).map(([k, v]) => `${k} ${pct(v, total)}`).join(', ');
    console.log(`  ${t === 'ally' ? 'allies' : 'enemies'}: ${fmt(top)}`);
    console.log(`    ${fmt(time)}`);
    console.log(`    reached their objective ${pct(reached, lives)} of ${lives} lives; ${(kills / minutesTotal).toFixed(1)} kills a minute`);
    console.log(`    hits/shot ${['rifle', 'pistol', 'sniper', 'launcher'].filter((f) => shots[f]).map((f) => `${f} ${pct(hits[f] ?? 0, shots[f])}`).join(', ')}`);
  }
  const cap = { ally: 0, enemy: 0, neutralized: 0 };
  for (const r of results) for (const k of Object.keys(cap)) cap[k] += (r.captures[k] ?? 0) / results.length;
  console.log(`  captures a minute: allies ${cap.ally.toFixed(2)}, enemies ${cap.enemy.toFixed(2)}, neutralized ${cap.neutralized.toFixed(2)}`);
  const ints = {};
  for (const r of results) for (const [k, v] of Object.entries(r.interrupts)) {
    ints[k] ??= { ally: 0, enemy: 0 };
    ints[k].ally += v.ally / results.length;
    ints[k].enemy += v.enemy / results.length;
  }
  console.log('  interrupts a minute (allies / enemies):');
  for (const [k, v] of Object.entries(ints).sort((a, b) => b[1].ally + b[1].enemy - a[1].ally - a[1].enemy).slice(0, 8)) {
    console.log(`    ${k.padEnd(30)} ${v.ally.toFixed(1)} / ${v.enemy.toFixed(1)}`);
  }
}

const results = [];
for (const map of maps) for (const seed of seeds) {
  const r = await run(map, seed);
  results.push(r);
  console.log(`${map} seed ${seed}: ${r.minutes} min, ${r.wall} s wall; reached ${pct(r.ally.reached, r.ally.lives)} / ${pct(r.enemy.reached, r.enemy.lives)}; captures ${r.captures.ally ?? 0} / ${r.captures.enemy ?? 0} a minute`);
}
report(results);
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(results, null, 1));
process.exit(0);
