import * as THREE from "three";

// The enhanced mode's acoustics, worked out from the map's own collision
// (the same BVH the game's rays use). None of this is in the 2004 game,
// which has one reverb "space" per sub-level (Theed's is all but dry); the
// original mode skips it.
//
// - Air absorption: high frequencies fall off with distance, a lowpass whose
//   corner stays at 20 kHz to 20 m and then drops, to 9 kHz at 100 m and
//   4 kHz at 300 m: about half of what ISO 9613-1 gives at 20 C and 50%
//   humidity, so a battle across the plaza keeps its edge.
// - Occlusion: three rays from the listener to each heard voice, aimed a
//   metre above its origin (a soldier's origin is at his feet, where the
//   ground's bumps would block everything) and half a metre to either side
//   of it. The share blocked eases the voice toward -4.4 dB and a lowpass
//   near 2.8 kHz: muffled, not buried. Six rays a frame, each voice's next
//   in turn, so every voice is looked at again every few frames.
// - Rooms: a fan of 19 rays from the listener, two a frame (the whole fan
//   about six times a second), measures the space: how many walls are
//   within 60 m, how far they are, and whether there is a roof. Walls give early reflections; a roof gives the long,
//   wet tail. Under Theed's arcades a dense, wetter room of about a second;
//   in the plaza, ringed by buildings but open above, reflections and a
//   faint, shorter tail.

const PROBE_RAYS = 2; // room rays a frame
const PROBE_REACH = 60; // m
const OCCLUSION_RAYS = 6; // occlusion rays a frame
const AIMS = [0, 1, -1]; // each voice's rays: centre, one side, the other
const OCCLUDED_GAIN = 0.6;
const OCCLUDED_CUTOFF = 2800; // Hz
const OCCLUSION_EASE = 6; // 1/s
const NEAR = 4; // m: closer voices (the player's own) are never occluded
const AIM_UP = 1; // m above a voice's origin
const AIM_SIDE = 0.5; // m to either side
const AIR_FROM = 20; // m: no air absorption closer than this
const AIR_SCALE = 70; // m: the corner halves every this much past AIR_FROM
const AIR_FLOOR = 2500; // Hz

export class Acoustics {
  constructor(collision) {
    this.collision = collision;
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    this.from = new THREE.Vector3();
    this.dir = new THREE.Vector3();
    this.side = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.turn = 0;
    this.sweep = this.newSweep();
    this.room = { enclosure: 0, meanFree: PROBE_REACH, roof: 0, nearest: PROBE_REACH };
    this.reverb = { room: 0.08, roomHF: 0.8, decay: 0.8, hfRatio: 0.6, reflections: 0.2, reflDelay: 0.02, reverb: 0.7, reverbDelay: 0.03, diffusion: 1, density: 1, hfReference: 5000 };
    // Probe directions: 12 around, 6 tipped up 50 degrees, and straight up.
    this.rays = [];
    for (let i = 0; i < 12; i++) {
      const a = (i / 12) * Math.PI * 2;
      this.rays.push({ v: new THREE.Vector3(Math.sin(a), 0, Math.cos(a)), kind: "side" });
    }
    for (let i = 0; i < 6; i++) {
      const a = (i / 6) * Math.PI * 2 + 0.3;
      const e = (50 * Math.PI) / 180;
      this.rays.push({ v: new THREE.Vector3(Math.sin(a) * Math.cos(e), Math.sin(e), Math.cos(a) * Math.cos(e)), kind: "up" });
    }
    this.rays.push({ v: new THREE.Vector3(0, 1, 0), kind: "roof" });
  }

  update(dt, listener, voices, renderer) {
    if (!this.collision) return;
    // The room, a few rays a frame.
    if (this.probe(listener.position)) renderer?.setRoom(this.reverb);
    // Occlusion: each voice's next ray, in turn, up to the frame's rays.
    const heard = voices.filter((v) => v.real && v.mode3D && v.position);
    let rays = 0;
    for (let k = 0; k < heard.length && rays < OCCLUSION_RAYS; k++) {
      const v = heard[(this.turn + k) % heard.length];
      if (!(v.distance > NEAR)) {
        v.occludedTarget = 0;
        continue;
      }
      const seen = v.occlusionRays || (v.occlusionRays = [0, 0, 0]);
      const aim = (v.occlusionAim = ((v.occlusionAim ?? -1) + 1) % AIMS.length);
      seen[aim] = this.blocked(listener.position, v.position, AIMS[aim]) ? 1 : 0;
      v.occludedTarget = (seen[0] + seen[1] + seen[2]) / AIMS.length;
      rays++;
    }
    this.turn = heard.length ? (this.turn + Math.max(rays, 1)) % heard.length : 0;
    for (const v of heard) {
      const goal = v.occludedTarget ?? 0;
      const now = v.occluded ?? goal;
      v.occluded = now + (goal - now) * Math.min(OCCLUSION_EASE * dt, 1);
    }
  }

  // Whether something blocks the ray to a metre above a voice's origin,
  // `aim` half metres to one side (0 is straight at it).
  blocked(from, at, aim) {
    this.from.set(from.x, from.y, from.z);
    this.side.set(at.z - from.z, 0, from.x - at.x);
    const span = this.side.length();
    if (span > 1e-3) this.side.multiplyScalar((AIM_SIDE * aim) / span);
    else this.side.set(0, 0, 0);
    this.target.set(at.x + this.side.x, at.y + AIM_UP, at.z + this.side.z);
    const distance = this.target.distanceTo(this.from);
    this.dir.subVectors(this.target, this.from).divideScalar(Math.max(distance, 1e-3));
    const hit = this.collision.cast(this.from, this.dir, distance, this.hit);
    return Boolean(hit && hit.distance < distance - 0.5);
  }

  newSweep() {
    return { next: 0, side: 0, sides: 0, up: 0, ups: 0, sum: 0, hits: 0, nearest: PROBE_REACH };
  }

  // The room fan's next rays from where the listener is now; true when the
  // fan is done and the room and reverb are new.
  probe(at) {
    this.from.set(at.x, at.y, at.z);
    const s = this.sweep;
    for (let k = 0; k < PROBE_RAYS && s.next < this.rays.length; k++) {
      const ray = this.rays[s.next++];
      const hit = this.collision.cast(this.from, ray.v, PROBE_REACH, this.hit);
      const d = hit ? hit.distance : PROBE_REACH;
      if (hit) {
        s.sum += d;
        s.hits++;
        s.nearest = Math.min(s.nearest, d);
      }
      if (ray.kind === "side") {
        s.sides++;
        if (hit) s.side++;
      } else {
        s.ups++;
        if (hit && d < 25) s.up++;
      }
    }
    if (s.next < this.rays.length) return false;
    this.sweep = this.newSweep();
    const { side, sides, up, ups, sum, hits, nearest } = s;
    const walls = side / Math.max(sides, 1);
    const roof = up / Math.max(ups, 1);
    const enclosure = 0.6 * walls + 0.4 * roof;
    const meanFree = hits ? sum / hits : PROBE_REACH;
    this.room = { enclosure, meanFree, roof, nearest, walls };
    const size = Math.min(meanFree / 30, 1);
    const r = this.reverb;
    // Walls make reflections; a roof makes the tail long and wet.
    r.room = 0.04 + 0.22 * roof + 0.08 * walls;
    r.decay = 0.4 + (0.5 + 1.2 * size) * roof + 0.35 * walls * size;
    r.reflections = 0.15 + 0.55 * walls;
    r.reflDelay = Math.min(Math.max((2 * nearest) / 343, 0.004), 0.09);
    r.reverbDelay = Math.min(r.reflDelay + 0.012 + 0.02 * size, 0.12);
    r.hfRatio = 0.5 + 0.2 * (1 - roof);
    r.roomHF = 0.7;
    r.density = 0.6 + 0.4 * enclosure;
    return true;
  }

  // The extras for one voice: air and occlusion lowpass, its gain, the
  // reverb send.
  shape(voice) {
    const extra = voice.extra;
    const d = voice.mode3D ? voice.distance : 0;
    let cutoff = d > AIR_FROM ? Math.min(Math.max(20000 / (1 + (d - AIR_FROM) / AIR_SCALE), AIR_FLOOR), 20000) : 20000;
    const occluded = voice.occluded || 0;
    if (occluded > 0) cutoff = cutoff * (1 - occluded) + Math.min(cutoff, OCCLUDED_CUTOFF) * occluded;
    extra.cutoff = cutoff;
    extra.gain = 1 - (1 - OCCLUDED_GAIN) * occluded;
    extra.send = 1;
    extra.lfe = voice.category === "explosions" ? 1 : 0;
    return extra;
  }
}
