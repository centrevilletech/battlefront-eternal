// Ground hulls on slopes, through the real command path in the headless
// world (tools/headless-world.mjs): each hover class of a battle is set down
// on terrain of rising steepness, facing up the slope and then across it,
// left to settle, then driven up with W. Prints, per class and slope, the
// ground's own pitch and roll under the hull (a plane through the terrain
// at its footprint's corners), the hull's (tilt, roll), how deep its
// collision dips into the terrain (clip) and its least clearance, and its
// speed up the slope after 3 s of throttle.
//
//   BFE_CONTENT_DIR=<game data> node tools/vehicle-slope-probe.mjs [map] [era] [--json <file>]
import fs from 'node:fs';
import { headlessGame } from './headless-world.mjs';

const args = process.argv.slice(2);
const option = (name) => { const i = args.indexOf(name); return i < 0 ? null : args.splice(i, 2)[1]; };
const jsonOut = option('--json');
const [map = 'rhn1', era = 'gcw'] = args;
const units = { gcw: 'imp_inf_storm_troopersnow', cw: 'rep_inf_clone_trooper' };
const game = await headlessGame(`map=${map}&era=${era}&unit=${units[era] || units.gcw}&ai=off&seed=1`);
await game.ready; game.begin?.();
const T = game.THREE, c = game.collision;
let sequence = 0;
const step = (extra = {}) => {
  const tick = game.worldClock.tick + 1;
  sequence = Math.max(sequence + 1, tick);
  game.step({ epoch: 1, sequence, tick, generation: game.playerUnit.generation, down: [], actions: [], look: [], ...extra });
};
if (game.combat.dead || game.playerUnit.removed) { game.respawn(0); game.combat.reset(); }
step();

const ground = { height: 0, normal: new T.Vector3() };
const heightAt = (x, z, top = 2000) => c.ground(x, z, top, ground) ? ground.height : NaN;
// The plane through the terrain under a footprint (half sizes hx, hz) at
// (x, z) facing yaw: its normal, from the four corners.
function footprintNormal(x, z, yaw, hx, hz, out) {
  const s = Math.sin(yaw), k = Math.cos(yaw);
  const at = (lx, lz) => { const wx = x + k * lx + s * lz, wz = z - s * lx + k * lz; return new T.Vector3(wx, heightAt(wx, wz), wz); };
  const fl = at(hx, hz), fr = at(-hx, hz), bl = at(hx, -hz), br = at(-hx, -hz);
  if ([fl, fr, bl, br].some((p) => !Number.isFinite(p.y))) return null;
  const a = new T.Vector3().subVectors(fr, bl), b = new T.Vector3().subVectors(fl, br);
  out.crossVectors(b, a).normalize();
  if (out.y < 0) out.negate();
  return out;
}
// The (tilt, roll) a hull facing yaw takes to lie in a plane of normal n:
// Vehicle.alignGround's angles.
function planeAngles(n, yaw) {
  const s = Math.sin(yaw), k = Math.cos(yaw);
  const x = k * n.x - s * n.z, z = s * n.x + k * n.z;
  return { tilt: -Math.atan2(z, n.y), roll: -Math.atan2(x, Math.hypot(n.y, z)) };
}
// How deep the hull's collision dips into the terrain, and its least
// clearance above it: every vertex of its collision mesh, placed as drawn.
function clipping(v, positions) {
  v.root.updateMatrixWorld(true);
  const m = v.root.matrixWorld, p = new T.Vector3();
  let clip = 0, clear = Infinity;
  for (let i = 0; i < positions.length; i += 3) {
    p.set(positions[i], positions[i + 1], positions[i + 2]).applyMatrix4(m);
    const h = heightAt(p.x, p.z, p.y + 50);
    if (!Number.isFinite(h)) continue;
    clip = Math.max(clip, h - p.y);
    clear = Math.min(clear, p.y - h);
  }
  return { clip, clear };
}

// Slopes to test: terrain points within 150 m of a command post (open sky
// above: the first thing down is terrain; nothing built or blocking within
// the hull's reach and 20 m up the slope) whose footprint plane rises 4 to
// 45 degrees, a few per band, spread over the field.
const bands = [[4, 8], [8, 12], [12, 16], [16, 20], [20, 24], [24, 28], [28, 34], [34, 45]];
function slopes(v) {
  const hit = { distance: 0, point: new T.Vector3(), normal: new T.Vector3(), terrain: false, body: null };
  const n = new T.Vector3(), found = bands.map(() => []);
  const half = { x: v.size.x * .5, z: v.size.z * .5 };
  const posts = game.posts?.posts.map((p) => p.position) || [];
  // A fixed shuffle of the candidates, so each band's few spread out.
  let seed = 7;
  const random = () => (seed = (seed * 16807) % 2147483647) / 2147483647;
  const candidates = [];
  for (const post of posts) for (let i = 0; i < 400; i++) {
    const a = random() * Math.PI * 2, r = 15 + random() * 135;
    candidates.push([post.x + Math.sin(a) * r, post.z + Math.cos(a) * r]);
  }
  for (const [x, z] of candidates) {
    const h = heightAt(x, z);
    if (!Number.isFinite(h) || game.battlefield?.outside(new T.Vector3(x, h, z))) continue;
    if (!c.cast(new T.Vector3(x, h + 40, z), new T.Vector3(0, -1, 0), 60, hit) || !hit.terrain) continue;
    if (!footprintNormal(x, z, 0, half.x, half.z, n)) continue;
    const yaw = Math.atan2(-n.x, -n.z); // facing up the slope
    if (!footprintNormal(x, z, yaw, half.x, half.z, n)) continue;
    const angle = Math.acos(n.y) * 180 / Math.PI;
    const b = bands.findIndex(([lo, hi]) => angle >= lo && angle < hi);
    if (b < 0 || found[b].length >= 3) continue;
    let open = true;
    for (let d = 0; d <= 20 && open; d += 5) {
      const px = x + Math.sin(yaw) * d, pz = z + Math.cos(yaw) * d, ph = heightAt(px, pz);
      if (!Number.isFinite(ph) || game.battlefield?.outside(new T.Vector3(px, ph, pz)) || c.gather(new T.Vector3(px, ph + 3, pz), v.radius + 3, true).length) open = false;
    }
    if (open) found[b].push({ x, z, yaw, angle });
  }
  return found.flat();
}

function place(v, x, z, yaw) {
  // Each placement starts a fresh life of the hull, the player at its
  // controls (a hull left out of bounds too long, or sunk, is gone).
  if (!v.alive || game.driver.vehicle !== v) {
    if (game.driver.vehicle) game.driver.exit(true);
    if (!v.alive) v.spawn(v.team || 1);
    if (game.combat.dead || game.playerUnit.removed) { game.respawn(0); game.combat.reset(); }
    step();
    if (!game.driver.enter(v)) throw new Error(`${v.name}: not boarded`);
  }
  for (const character of game.battle?.characters || []) character.outOfBounds = -1;
  const h = heightAt(x, z);
  v.position.set(x, h + 3, z);
  v.velocity.set(0, 0, 0);
  v.yaw = v.previousYaw = yaw;
  v.tilt = v.roll = v.previousTilt = v.previousRoll = 0;
  for (const key of ['tiltRate', 'rollRate', 'yawRate']) if (key in v) v[key] = 0;
  for (const seat of v.seats) { seat.aimYaw = yaw; seat.lookYaw = seat.lookYaw === null ? null : yaw; }
  v.previous.copy(v.position); v.tickFrom?.copy(v.position);
  v.sleeping = false;
  v.syncBody();
}

const rows = [];
for (const v of game.vehicles.vehicles) {
  if (v.flying || v.walking || !v.alive) continue;
  const positions = game.vehicles.data.collision[v.def.model].positions;
  if (!game.driver.enter(v)) { console.log(`${v.name}: not boarded`); continue; }
  const half = { x: v.size.x * .5, z: v.size.z * .5 }, n = new T.Vector3();
  for (const s of slopes(v)) {
    for (const [facing, yaw] of [['up', s.yaw], ['across', s.yaw + Math.PI / 2]]) {
      place(v, s.x, s.z, yaw);
      let worstClip = 0;
      for (let i = 0; i < 180; i++) { step(); worstClip = Math.max(worstClip, clipping(v, positions).clip); }
      const plane = footprintNormal(v.position.x, v.position.z, v.yaw, half.x, half.z, n);
      if (!plane) continue;
      const want = planeAngles(plane, v.yaw), settled = clipping(v, positions);
      const row = { class: v.name, slope: +s.angle.toFixed(1), facing, at: [+s.x.toFixed(1), +s.z.toFixed(1)],
        groundTilt: +(want.tilt * 180 / Math.PI).toFixed(1), hullTilt: +(v.tilt * 180 / Math.PI).toFixed(1),
        groundRoll: +(want.roll * 180 / Math.PI).toFixed(1), hullRoll: +(v.roll * 180 / Math.PI).toFixed(1),
        clip: +settled.clip.toFixed(2), clear: +settled.clear.toFixed(2), worstClip: +worstClip.toFixed(2),
        slid: +Math.hypot(v.position.x - s.x, v.position.z - s.z).toFixed(2) };
      if (facing === 'up') {
        const start = v.position.clone();
        for (let i = 0; i < 180; i++) { step({ down: ['KeyW'] }); worstClip = Math.max(worstClip, clipping(v, positions).clip); }
        const along = new T.Vector3(Math.sin(yaw), 0, Math.cos(yaw));
        row.climbed = +v.position.clone().sub(start).dot(along).toFixed(1);
        row.alive = v.alive && game.driver.vehicle === v;
        row.speed = +v.velocity.length().toFixed(1);
        row.driveClip = +worstClip.toFixed(2);
      }
      rows.push(row);
      console.log(JSON.stringify(row));
    }
  }
  game.driver.exit(true);
  step();
}
if (jsonOut) fs.writeFileSync(jsonOut, JSON.stringify(rows, null, 1));
process.exit(0);
