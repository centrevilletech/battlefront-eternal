// Cold command-post content shared by native world loading and the page's
// spawn screen. This constructs authored records; it never assigns bot goals.
import { buildRegion } from './regions.js';

const DEFAULT_VALUES = () => [100, 100, 100, 100, 100, 100];

// A post's AI properties: the attached ai-mission.json record, or what can be
// said from map.json alone (class-default weights of 100, no bans, the spawn
// path if map.json resolved it).
export function postAI(post) {
  if (post.ai) return post.ai;
  const desc = post.desc || {};
  post.ai = {
    order: null,
    atk: DEFAULT_VALUES(),
    def: DEFAULT_VALUES(),
    bans: 0,
    valueBleed: desc.valueBleed ?? 100,
    spawnPath: desc.spawnPath ? { name: desc.spawnPath, present: !!desc.spawnPoints?.length, points: desc.spawnPoints || [] } : null,
    allyPath: null,
    allyCount: 0,
    classSpawnPoints: 0,
    classSpawnWorld: [],
    capture: desc.capture ? captureOf(desc.capture) : null,
    radius: 0,
    yaw: desc.matrix ? Math.atan2(desc.matrix[8], desc.matrix[10]) : 0,
  };
  return post.ai;
}

// half: the size as the 2004 game builds the region (src/regions.js), which
// the objective offset reads: a cylinder's x and z are its radius.
function captureOf(region) {
  const m = region.matrix;
  return { type: region.type, size: region.size, half: buildRegion(region).half, matrix: m, center: region.center || [m[12], m[13], m[14]] };
}

// Joins ai-mission.json's post records to the runtime posts by name (and,
// for unnamed posts, by position). `spawns` (map.json) supplies spawn path
// points for posts without a record.
export function attachPostAI(posts, mission, spawns = {}) {
  const records = mission?.posts || [];
  const byName = new Map(records.filter((r) => r.name).map((r) => [r.name.toLowerCase(), r]));
  const lower = Object.fromEntries(Object.entries(spawns || {}).map(([k, v]) => [k.toLowerCase(), v]));
  const at = (post) =>
    records.find((r) => !r.name && r.position && post.position &&
      Math.hypot(r.position[0] - post.position.x, r.position[1] - post.position.y, r.position[2] - post.position.z) < 0.01);
  posts.forEach((post, i) => {
    // By its world name; an unnamed one (Hoth's probe droid drop, which the
    // network calls "#5") by where it stands. Its runtime name used to be
    // looked up instead: no record matched, the unnamed post took the
    // default weights (100 for every side, where it says 0), and most of
    // the Empire's bots, its AT-ATs included, were sent to it for good.
    const worldName = typeof post.desc?.name === "string" ? post.desc.name : post.name;
    const record = (worldName ? byName.get(worldName.toLowerCase()) : at(post)) || null;
    if (!record) {
      delete post.ai;
      const ai = postAI(post);
      ai.order = i;
      if (ai.spawnPath && !ai.spawnPath.points.length) {
        const points = lower[ai.spawnPath.name.toLowerCase()] || [];
        ai.spawnPath = { name: ai.spawnPath.name, present: points.length > 0, points };
      }
      return;
    }
    const path = (p) => (p ? { name: p.name, present: !!p.present, points: p.points || [] } : null);
    post.ai = {
      order: record.order,
      atk: record.values.atk.slice(),
      def: record.values.def.slice(),
      bans: record.bans | 0,
      valueBleed: record.valueBleed,
      spawnPath: path(record.spawnPath),
      allyPath: path(record.allyPath),
      allyCount: record.allyCount | 0,
      classSpawnPoints: record.classSpawnPoints | 0,
      classSpawnWorld: record.classSpawnWorld || [],
      capture: record.captureRegion?.present ? captureOf(record.captureRegion) : null,
      radius: record.radius || 0,
      yaw: record.yaw ?? 0,
    };
  });
  return posts;
}

