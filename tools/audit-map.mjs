// Audits a map's playability under the headless world (tools/headless-world.mjs):
// every loader warning, bots that push to move and go nowhere (alive, on
// foot, moving less than a metre in 5 s), where they cluster and what collision surrounds them, and spawns that
// land off the ground (no terrain under them, far above or below it, or inside
// a wall). Prints a report; exits nonzero when the map has stuck bots or bad
// spawns, so it can gate an extraction.
//
// Usage: BFE_CONTENT_DIR=... node tools/audit-map.mjs yav1 [yav2 kas1 ...]
//   AUDIT_TICKS=3600 AUDIT_QUERY='ai=classic&tier=medium&heroes=0&seed=1'
import { headlessGame, idleCommand } from './headless-world.mjs';

const TICKS = Number(process.env.AUDIT_TICKS || 3600);
const EXTRA = process.env.AUDIT_QUERY || 'ai=classic&tier=medium&heroes=0&seed=1';
const STUCK_TICKS = 300;      // 5 s at 60 Hz
const STUCK_RADIUS = 0.75;    // m: moved less than this over STUCK_TICKS
const CLUSTER = 6;            // m: stuck spots this close are one cluster
const PROBE_RADII = [0.35];   // m: the soldier's body sphere, at hip height

const maps = process.argv.slice(2);
if (!maps.length) { console.error('usage: node tools/audit-map.mjs <map> [map ...]'); process.exit(2); }

let failed = false;
for (const map of maps) failed = (await audit(map)) || failed;
process.exit(failed ? 1 : 0);

async function audit(map) {
  const messages = [];
  const warn = console.warn, error = console.error;
  console.warn = (...args) => messages.push(['warn', args.map(String).join(' ')]);
  console.error = (...args) => messages.push(['error', args.map(String).join(' ')]);
  const started = performance.now();
  let game, mission;
  try {
    mission = await readMission(map);
    game = await headlessGame(`map=${map}&unit=${mission.unit}&${EXTRA}`);
  } catch (e) {
    console.warn = warn; console.error = error;
    console.log(`\n=== ${map}: FAILED TO LOAD: ${e.message}`);
    for (const [kind, text] of messages) console.log(`  [${kind}] ${text}`);
    return true;
  }
  const THREE = game.THREE;
  const probe = { height: 0, normal: new THREE.Vector3() };
  const center = new THREE.Vector3();
  const origin = new THREE.Vector3();
  const DOWN = new THREE.Vector3(0, -1, 0);
  const hit = { point: new THREE.Vector3(), normal: new THREE.Vector3(), distance: 0, terrain: false, body: null };
  const spawns = [];
  const badSpawns = [];
  const patched = new WeakSet();
  let hookOn = true;
  const tracks = new Map(); // unit id -> { last: Vector3, since: tick, stuckFrom, episodes: [] }
  const stuckSpots = [];    // { unit, className, tick, position, ticks }
  const names = (u) => `${u.className}#${u.id}`;

  // Every unit class shares Unit.prototype.spawn: wrap it once.
  const watchSpawn = (unit) => {
    const proto = Object.getPrototypeOf(unit);
    if (patched.has(proto) || typeof proto?.spawn !== 'function') return;
    patched.add(proto);
    const original = proto.spawn;
    proto.spawn = function (x, y, z, yaw) {
      const u = this;
      const result = original.call(u, x, y, z, yaw);
      if (!hookOn) return result;
      const record = { unit: names(u), tick: game.worldClock.tick, position: [x, y, z], problems: [] };
      // What holds him up: the first surface (floor, prop or terrain) under
      // the spawn point; a spawn high above it drops, one with nothing
      // under it falls for good.
      const support = game.collision.cast(origin.set(x, y + 1.5, z), DOWN, 60, hit, null, false);
      if (!support) record.problems.push('nothing under spawn (no terrain or floor within 60 m)');
      else if (support.distance - 1.5 > 2.5) record.problems.push(`spawn ${(support.distance - 1.5).toFixed(1)} m above its ${support.terrain ? 'terrain' : 'floor'}`);
      else if (!game.collision.ground(x, z, y + 3, probe) && support.terrain) record.problems.push('no terrain under spawn');
      for (const r of PROBE_RADII) {
        center.set(x, y + 0.9, z);
        if (game.collision.overlapsSphere(center, r)) { record.problems.push(`body overlaps collision (r=${r})`); break; }
      }
      spawns.push(record);
      if (record.problems.length) badSpawns.push(record);
      const track = tracks.get(u.id) || { last: new THREE.Vector3(), since: 0, episodes: 0 };
      track.last.set(x, y, z); track.since = game.worldClock.tick; track.spawnY = y; track.fell = false;
      tracks.set(u.id, track);
      return result;
    };
  };

  const falls = [];
  for (let i = 1; i <= TICKS; i++) {
    for (const u of game.units.all()) watchSpawn(u);
    game.step(idleCommand(game));
    const tick = game.worldClock.tick;
    for (const u of game.units.all()) {
      if (u.human || !u.alive || u.seat) { const t = tracks.get(u.id); if (t) t.since = tick; continue; }
      const track = tracks.get(u.id);
      if (!track) continue;
      const p = u.position;
      if (!track.fell && track.spawnY - p.y > 40) { track.fell = true; falls.push({ unit: names(u), tick, position: [p.x, p.y, p.z].map(round) }); }
      if (p.distanceTo(track.last) > STUCK_RADIUS) { track.last.copy(p); track.since = tick; track.flagged = false; continue; }
      // Standing still on purpose (sniping, guarding a post) is not stuck:
      // only a bot pushing to move and going nowhere counts.
      const intent = u.intent;
      if (!intent || Math.hypot(intent.forward || 0, intent.right || 0) < 0.2) { track.since = tick; continue; }
      if (tick - track.since >= STUCK_TICKS && !track.flagged) {
        track.flagged = true; track.episodes++;
        stuckSpots.push({ unit: names(u), tick, position: [p.x, p.y, p.z], jet: u.soldier?.air === 'jet' });
      }
    }
  }
  console.warn = warn; console.error = error;
  const elapsed = ((performance.now() - started) / 1000).toFixed(1);

  // Stuck spots clustered, with the collision around each cluster's middle.
  const clusters = [];
  for (const spot of stuckSpots) {
    let found = clusters.find((c) => Math.hypot(c.center[0] - spot.position[0], c.center[2] - spot.position[2]) < CLUSTER);
    if (!found) { found = { center: [...spot.position], spots: [], units: new Set() }; clusters.push(found); }
    found.spots.push(spot); found.units.add(spot.unit);
    for (let k = 0; k < 3; k++) found.center[k] += (spot.position[k] - found.center[k]) / found.spots.length;
  }
  clusters.sort((a, b) => b.spots.length - a.spots.length);
  const describe = (c) => {
    const [x, y, z] = c.center;
    const ground = game.collision.ground(x, z, y + 3, probe);
    const walls = [];
    for (let k = 0; k < 8; k++) {
      const a = (k / 8) * Math.PI * 2;
      const dir = new THREE.Vector3(Math.sin(a), 0, Math.cos(a));
      const origin = new THREE.Vector3(x, y + 0.9, z);
      const h = game.collision.cast(origin, dir, 3, hit, null, false);
      walls.push(h ? (h.terrain ? `T${h.distance.toFixed(1)}` : `W${h.distance.toFixed(1)}`) : '-');
    }
    const above = game.collision.cast(new THREE.Vector3(x, y + 0.2, z), new THREE.Vector3(0, 1, 0), 4, hit, null, false);
    return `ground ${ground ? `${ground.height.toFixed(1)} (${(y - ground.height).toFixed(1)} above)` : 'NONE'}; walls within 3 m (8 dirs): ${walls.join(' ')}; headroom ${above ? above.distance.toFixed(1) : '>4'}`;
  };

  // Every command post's spawn path, walked: the player's body is put on
  // each point and pushed in four directions for two seconds. A point he
  // cannot leave (under a metre in any direction) is a spawn inside
  // collision; one that overlaps a wall at the start is listed too.
  const walks = [];
  hookOn = false;
  const player = game.playerUnit;
  if (player && mission.world.spawns) {
    for (const post of mission.world.commandPosts || []) {
      const points = mission.world.spawns[post.spawnPath] || [];
      const bad = [], moved = [];
      for (const point of points) {
        const [x, y, z, yaw] = point;
        const overlap = game.collision.overlapsSphere(center.set(x, y + 0.9, z), 0.35);
        let best = 0;
        for (const heading of [0, Math.PI / 2, Math.PI, -Math.PI / 2]) {
          player.spawn(x, y, z, yaw || 0);
          const start = player.position.clone();
          for (let t = 0; t < 120; t++) {
            const c = idleCommand(game);
            c.down = ['KeyW'];
            player.soldier.yaw = heading; player.soldier.aimYaw = heading;
            game.step(c);
          }
          best = Math.max(best, Math.hypot(player.position.x - start.x, player.position.z - start.z));
        }
        moved.push(best);
        if (overlap || best < 1) bad.push({ point: [x, y, z].map(round), overlap, moved: round(best) });
      }
      moved.sort((a, b) => a - b);
      walks.push({ post: (post.displayName || post.name).trim(), path: post.spawnPath, points: points.length, bad,
        least: round(moved[0] ?? 0), median: round(moved[moved.length >> 1] ?? 0) });
    }
  }
  const bots = game.units.all().filter((u) => !u.human);
  const alive = bots.filter((u) => u.alive).length;
  console.log(`\n=== ${map}: ${TICKS} ticks in ${elapsed}s; ${bots.length} bots (${alive} alive at the end); ${spawns.length} spawns`);
  const posts = game.posts?.list?.() || game.posts?.posts || [];
  if (messages.length) {
    console.log(`--- loader messages (${messages.length})`);
    const counts = new Map();
    for (const [kind, text] of messages) counts.set(`[${kind}] ${text}`, (counts.get(`[${kind}] ${text}`) || 0) + 1);
    for (const [text, n] of counts) console.log(`  ${n > 1 ? `${n}x ` : ''}${text}`);
  } else console.log('--- no loader warnings or errors');
  const nav = game.ai?.nav || game.ai?.navigation;
  const issues = nav?.issues || nav?.data?.issues;
  if (issues?.length) { console.log(`--- nav issues (${issues.length})`); for (const i of issues.slice(0, 20)) console.log(`  ${JSON.stringify(i)}`); }
  if (walks.length) {
    console.log('--- spawn paths walked by the player (points stuck at spawn / points)');
    for (const w of walks) {
      console.log(`  ${w.post} (${w.path}): ${w.bad.length} / ${w.points}; walked in 2 s: least ${w.least} m, median ${w.median} m`);
      for (const b of w.bad) console.log(`     ${b.point.join(', ')}: ${b.overlap ? 'overlaps collision; ' : ''}moved ${b.moved} m at best`);
    }
  }
  console.log(`--- bad spawns: ${badSpawns.length} of ${spawns.length}`);
  for (const s of badSpawns.slice(0, 30)) console.log(`  t${s.tick} ${s.unit} at ${s.position.map(round).join(', ')}: ${s.problems.join('; ')}`);
  if (falls.length) { console.log(`--- long falls: ${falls.length}`); for (const f of falls.slice(0, 20)) console.log(`  t${f.tick} ${f.unit} fell to ${f.position.join(', ')}`); }
  console.log(`--- stuck episodes: ${stuckSpots.length} (${[...new Set(stuckSpots.map((s) => s.unit))].length} bots), ${clusters.length} clusters`);
  for (const c of clusters.slice(0, 25)) {
    console.log(`  ${c.spots.length} episodes, ${c.units.size} bots at ${c.center.map(round).join(', ')}${c.spots.some((s) => s.jet) ? ' (jet)' : ''}`);
    console.log(`     ${describe(c)}`);
  }
  const stuckShare = bots.length ? [...new Set(stuckSpots.map((s) => s.unit))].length / bots.length : 0;
  return badSpawns.length > 0 || stuckShare > 0.25 || walks.some((w) => w.bad.length);
}

function round(v) { return Math.round(v * 10) / 10; }

// The battle's first playable class on the first side, so the audit asks for
// a unit the map fields: the battle the audit's query plays (AUDIT_QUERY may
// name an era=, side= or mission=; src/mission.js).
async function readMission(map) {
  const { selectMission } = await import('../src/mission.js');
  const world = selectMission(await fetch(`/maps/${map}/map.json`).then((r) => { if (!r.ok) throw new Error(`no map.json for ${map}`); return r.json(); }), EXTRA);
  const teams = world.mission?.teams || {};
  const first = Object.values(teams)[0];
  const unit = first?.classes?.[0]?.[0];
  if (!unit) throw new Error(`${map} has no mission classes`);
  return { unit, world };
}
