import * as THREE from "three";
import { buildRegion } from "../../regions.js";
import { DONE, Fsm } from "./controller.js";

// The patrol behavior an objective agent runs at (or on its way to) its
// command post, and the command-post geometry it shares with the agents
// (docs/ai-controller.md).
//
// States:
//   1 choose: a post that is not friendly is walked to (2). A friendly post
//     draws 1..4; a soldier that drew 2..4 tries a patrol hint (3), anything
//     else roams (4). A unit sitting in a vehicle takes the mounted branch.
//   2 walk to the post's center plus its random offset, until it turns
//     friendly (then choose again).
//   3 walk to the chosen hint for 30..45 s, marking it in use; arriving
//     settles at it (5).
//   4 roam: 20..30 s, a flat random point 8..14-scaled around the post (a
//     non-soldier, non-droid unit that has already arrived just stops, 6).
//   5 at the hint: look 10 units along its forward, take one of its allowed
//     stances, keep it marked, until the timer runs out.
//   6 stop and wait out the timer.
//   100 done.
// More than 5 s after it starts, once, the helper checks its post: a post
// without a capture region, or one where its team can now spawn, sends for a
// new objective (excluding this one) and ends the helper.

const _v = new THREE.Vector3();
const _w = new THREE.Vector3();
const _scratch = new THREE.Vector3();

const HINT_PATROL = 2; // patrol hint type bit
const QUERY_HALF_EXTENT = 300;
const QUERY_BROAD_CAP = 50;
const QUERY_OUTPUT_CAP = 10;
const HINT_RESERVE_TIME = 4; // a marked hint is unavailable for this long
const REVALIDATE_SENTINEL = 1e8;

// -- command-post geometry -------------------------------------------------------------

// The capture region a post's AI destinations use: { origin, sizeX, sizeZ,
// contains(point) }, or null when the post has none. Population's post data
// (postAI(post).capture, from ai-mission.json) wins; the map's capture block
// is the fallback.
export function captureRegion(ai, post) {
  if (!post) return null;
  const pop = ai?.population?.postAI?.(post)?.capture;
  const capture = pop ?? post.desc?.capture ?? post.capture ?? null;
  if (!capture) return null;
  const origin = new THREE.Vector3();
  const c = capture.center;
  const m = capture.matrix;
  if (Array.isArray(c)) origin.fromArray(c);
  else if (Array.isArray(m) && m.length >= 16) origin.set(m[12], m[13], m[14]);
  else if (Array.isArray(m) && m.length === 12) origin.set(m[9], m[10], m[11]);
  else if (m?.isMatrix4) origin.setFromMatrixPosition(m);
  else origin.copy(post.position);
  // The built size, as the 2004 game's offset reads it (src/regions.js).
  const size = capture.half ?? (Array.isArray(capture.matrix) && capture.size ? buildRegion(capture).half : capture.size) ?? [3, 3, 3];
  return {
    origin,
    sizeX: size[0] ?? 3,
    sizeZ: size[2] ?? size[0] ?? 3,
    contains: (point) => (typeof post.contains === "function" ? post.contains(point, _scratch) : false),
  };
}

// Where the AI aims for a post: its capture region's origin, else the post
// entity's own position.
export function cpCenter(ai, post, out = new THREE.Vector3()) {
  if (!post) return out.set(0, 0, 0);
  const center = ai?.population?.objectiveCenter;
  if (center) {
    const c = center(post);
    return out.set(c.x, c.y, c.z);
  }
  const region = captureRegion(ai, post);
  return out.copy(region ? region.origin : post.position);
}

// A random offset in world axes: z then x drawn across the capture region's
// extent (+-3 without one) from the AI generator; y is 0. No rotation, no
// containment test.
export function cpOffset(ai, post, out = new THREE.Vector3()) {
  const random = ai.randomA;
  const offset = ai.population?.objectiveOffset;
  if (offset) {
    const o = offset(post, random);
    return out.set(o.x, o.y ?? 0, o.z);
  }
  const region = captureRegion(ai, post);
  const sx = region ? region.sizeX : 3;
  const sz = region ? region.sizeZ : 3;
  const z = random.range(-sz, sz);
  const x = random.range(-sx, sx);
  return out.set(x, 0, z);
}

// Has the unit arrived at `post` (+ offset)? Inside the capture region
// counts; otherwise horizontally within half the unit's nominal speed. A post
// without a region uses max(10, unit radius + post radius + 5), plus 20 when
// the post itself moves.
const _zero = new THREE.Vector3();
export function arrivedAtObjective(ai, unit, post, offset = null) {
  if (!unit || !post) return false;
  const position = unit.position;
  const forwardSpeed = unit.soldier?.maxSpeed ?? unit.classDef?.MaxSpeed ?? 0;
  const radius = unit.sphere?.radius ?? 0.5;
  const arrived = ai?.population?.objectiveArrived;
  if (arrived) {
    return Boolean(
      arrived(post, offset ?? _zero, { position, forwardSpeed, radius }, {
        inside: (point) => (typeof post.contains === "function" ? post.contains(point, _scratch) : false),
        postRadius: post.radius ?? 0,
        postSpeed: post.speed ?? 0,
      }),
    );
  }
  cpCenter(ai, post, _w);
  if (offset) _w.add(offset);
  const dx = _w.x - position.x;
  const dz = _w.z - position.z;
  const d2 = dx * dx + dz * dz;
  const region = captureRegion(ai, post);
  let r;
  if (region) {
    if (region.contains(position)) return true;
    r = 0.5 * forwardSpeed;
  } else {
    r = Math.max(10, radius + (post.radius ?? 0) + 5);
    if ((post.speed ?? 0) > 0) return d2 < (r + 20) * (r + 20);
  }
  return d2 < r * r;
}

// The 2004 game's friendly-spawn test, used when Population does not supply
// its own: owned by the team with a spawn path or class spawn points, or owned
// by a friend with an ally path.
export function friendlySpawnEligible(ai, post, team) {
  const own = ai.population?.friendlySpawnEligible;
  if (own) return Boolean(own(post, team, ai.world));
  if (!post?.team) return false;
  const desc = post.desc ?? {};
  if (post.team === team && (desc.spawnPath || desc.spawnPoints?.length)) return true;
  return ai.relation(post.team, team) > 0 && Boolean(desc.allyPath);
}

// -- hints --------------------------------------------------------------------------------

// Field adapters over the runtime hints (docs/ai-nav.md; a hint may also be a
// raw record with `xfrm`).
export function hintBit(hint) {
  if (Number.isInteger(hint.bit)) return hint.bit;
  if (Number.isInteger(hint.typeBit)) return hint.typeBit;
  return { snipe: 1, patrol: 2, cover: 4, jetjump: 8, minefield: 16, land: 32 }[hint.kind] ?? 0;
}

export function hintPosition(hint, out = new THREE.Vector3()) {
  const p = hint.position;
  if (p?.isVector3) return out.copy(p);
  if (Array.isArray(p)) return out.fromArray(p);
  const x = hint.xfrm;
  if (x) return out.set(x[9], x[10], x[11]);
  return out.set(0, 0, 0);
}

export function hintForward(hint, out = new THREE.Vector3()) {
  const f = hint.forward ?? hint.direction;
  if (f?.isVector3) return out.copy(f);
  if (Array.isArray(f)) return out.fromArray(f);
  if (hint.matrix?.isMatrix4) {
    const e = hint.matrix.elements;
    return out.set(e[8], e[9], e[10]);
  }
  const x = hint.xfrm;
  if (x) return out.set(x[6], x[7], x[8]);
  return out.set(0, 0, 1);
}

export function hintPost(hint) {
  return hint.post ?? hint.commandPost ?? hint.commandPostName ?? hint.cp ?? null;
}

function samePost(a, b) {
  if (!a || !b) return false;
  if (a === b) return true;
  const name = typeof a === "string" ? a : a.name;
  const other = typeof b === "string" ? b : b.name;
  return Boolean(name) && name === other;
}

// Allowed stances as a bit mask (1 stand, 2 crouch, 4 prone); stand only by
// default.
export function hintStanceMask(hint) {
  if (Number.isInteger(hint.stanceMask)) return hint.stanceMask;
  if (Number.isInteger(hint.primaryStance)) return hint.primaryStance;
  if (Number.isInteger(hint.stance)) return hint.stance;
  return 1;
}

// One of the hint's allowed stances: integer draws 0..2 until one is
// allowed. An empty mask would never finish in the original; stand.
export function pickHintStance(hint, random) {
  const mask = hintStanceMask(hint) & 7;
  if (!mask) return 0;
  for (;;) {
    const stance = random.int(0, 2);
    if (mask & (1 << stance)) return stance;
  }
}

function linkedHints(hint) {
  if (Array.isArray(hint.linked)) return hint.linked;
  const out = [];
  for (let h = hint.next; h && h !== hint; h = h.next) out.push(h);
  return out;
}

// A hint marked by someone in the last 4 s is unavailable (equality passes).
export function hintAvailable(ai, hint, now) {
  const nav = ai.nav;
  if (nav?.hintAvailable) return Boolean(nav.hintAvailable(hint, now));
  return !(now < (hint.reservedAt ?? -10) + HINT_RESERVE_TIME);
}

// Mark a hint (and its radius siblings) in use by `unit` now.
export function markHintUsed(ai, hint, unit, now) {
  if (!hint) return;
  const nav = ai.nav;
  if (nav?.markHint) {
    nav.markHint(hint, unit, now);
    return;
  }
  const ref = unit ? { unit, generation: unit.generation } : null;
  hint.reservedAt = now;
  hint.reservedBy = ref;
  for (const sibling of linkedHints(hint)) {
    sibling.reservedAt = now;
    sibling.reservedBy = ref;
  }
}

// Patrol hints for a post: type patrol, inside the box 300 around the post's
// center, available, first 50 found; of those, the ones associated with this
// post, at most 10.
export function queryPatrolHints(ai, post, requester, out = []) {
  out.length = 0;
  if (!post) return out;
  const nav = ai.nav;
  const now = ai.world.time ?? 0;
  const center = cpCenter(ai, post, new THREE.Vector3());
  let broad;
  if (nav?.queryHints) {
    broad = nav.queryHints({ mask: HINT_PATROL, center, halfExtent: QUERY_HALF_EXTENT, cap: QUERY_BROAD_CAP, requester, now }) ?? [];
  } else {
    broad = [];
    for (const hint of nav?.hints ?? []) {
      if (broad.length >= QUERY_BROAD_CAP) break;
      if (!(hintBit(hint) & HINT_PATROL)) continue;
      hintPosition(hint, _v);
      if (Math.abs(_v.x - center.x) > QUERY_HALF_EXTENT || Math.abs(_v.y - center.y) > QUERY_HALF_EXTENT || Math.abs(_v.z - center.z) > QUERY_HALF_EXTENT) continue;
      if (requester && !hintAvailable(ai, hint, now)) continue;
      broad.push(hint);
    }
  }
  for (const hint of broad) {
    if (out.length >= QUERY_OUTPUT_CAP) break;
    if (samePost(hintPost(hint), post)) out.push(hint);
  }
  return out;
}

// Choose from candidates in random order without replacement: the first
// available one that is not `previous`.
export function chooseHint(ai, candidates, previous, now) {
  const list = candidates.slice();
  for (let n = list.length; n > 0; ) {
    n -= 1;
    const i = ai.randomA.int(0, n);
    const hint = list[i];
    if (hintAvailable(ai, hint, now) && hint !== previous) return hint;
    list[i] = list[n];
  }
  return null;
}

// -- the helper ---------------------------------------------------------------------------

export class PatrolHelper extends Fsm {
  constructor(agent) {
    super();
    this.agent = agent;
    this.state = 1;
    this.pending = 1;
    this.post = null;
    this.hint = null;
    this.startTime = 0;
    this.look = false;
    this.lookPoint = new THREE.Vector3();
    this.stance = 0;
    this.expires = 0;
    this.candidates = [];
  }

  get ai() {
    return this.agent.ai;
  }

  get unit() {
    return this.agent.unit;
  }

  get helper() {
    return this.agent.helper;
  }

  now() {
    return this.agent.now();
  }

  start(post) {
    this.post = post ?? null;
    this.look = false;
    this.stance = 0;
    this.hint = null;
    this.startTime = this.now();
    this.requestState(1);
    this.flush();
  }

  tick(dt) {
    this.update(dt, false);
  }

  // The embedding agent leaving its patrol state.
  exitState() {
    this.onExit();
  }

  // Alert policies while patrolling (the objective agents' policy slots
  // answer with these in their patrol state): grenade danger and shot/damage
  // alerts, known or not, are always taken; a scanner target or an acoustic
  // contact by the agent's base weapon-eligibility policy; boarding and the
  // crew-wait window never.
  acceptsHazard() {
    return true;
  }

  acceptsAlert() {
    return true;
  }

  acceptsTarget(target, agent) {
    return agent.canEngage(target);
  }

  acceptsAcoustic(source, agent) {
    return agent.canEngage(source);
  }

  acceptsBoard() {
    return false;
  }

  acceptsSecondaryWait() {
    return false;
  }

  onExit() {
    this.look = false;
    this.stance = 0;
  }

  friendlyPost() {
    const unit = this.unit;
    return Boolean(unit && this.post && this.ai.relation(unit.team, this.post.team) > 0);
  }

  onEnter() {
    const ai = this.ai;
    const unit = this.unit;
    const helper = this.helper;
    switch (this.state) {
      case 1: {
        const vehicles = ai.vehicles;
        if (unit && vehicles?.boarded?.(this.agent.controller)) {
          if (vehicles.patrolMounted) vehicles.patrolMounted(this);
          else {
            this.agent.controller.requestExit();
            this.agent.controller.reboardAt = this.now() + 5;
            this.requestState(DONE);
          }
          return;
        }
        if (!this.friendlyPost()) {
          this.requestState(2);
          return;
        }
        const draw = ai.randomA.int(1, 4);
        if (!(draw !== 1 && unit?.kind === 0)) {
          this.requestState(4);
          return;
        }
        const candidates = queryPatrolHints(ai, this.post, this.agent.controller, this.candidates);
        this.hint = chooseHint(ai, candidates, this.hint, this.now());
        this.requestState(this.hint ? 3 : 4);
        return;
      }
      case 2:
        cpCenter(ai, this.post, _v);
        cpOffset(ai, this.post, _w);
        helper?.goto?.(_v.add(_w));
        this.centered = false;
        this.expires = 0;
        return;
      case 3:
        markHintUsed(ai, this.hint, unit, this.now());
        helper?.goto?.(hintPosition(this.hint, _v));
        this.expires = this.now() + ai.randomA.int(30, 45);
        return;
      case 4: {
        this.expires = this.now() + ai.randomA.int(20, 30);
        const kind = unit?.kind;
        if (kind !== 0 && kind !== 1 && arrivedAtObjective(ai, unit, this.post)) {
          helper?.stop?.();
          this.requestState(6);
          return;
        }
        cpCenter(ai, this.post, _v);
        const z = Math.fround(ai.randomA.uniform() - 0.5);
        const y = Math.fround(ai.randomA.uniform() - 0.5);
        const x = Math.fround(ai.randomA.uniform() - 0.5);
        _w.set(x * 2, y * 2, z * 2);
        _w.y = 0;
        _w.multiplyScalar(ai.randomA.range(8, 14));
        _v.add(_w);
        helper?.goto?.(_v, { mode: "direct" });
        return;
      }
      case 5: {
        this.look = true;
        hintForward(this.hint, _w).multiplyScalar(10);
        if (helper?.destinationPoint) helper.destinationPoint(_v);
        else hintPosition(this.hint, _v);
        this.lookPoint.copy(_v).add(_w);
        this.stance = pickHintStance(this.hint, ai.randomA);
        return;
      }
      case 6:
        this.stance = 0;
        return;
      default:
    }
  }

  onTick() {
    if (this.state === DONE) return;
    const ai = this.ai;
    const now = this.now();
    if (this.startTime + 5 < now) {
      this.startTime = REVALIDATE_SENTINEL;
      const post = this.post;
      const team = this.agent.controller.character?.team ?? this.unit?.team;
      if (!post || !captureRegion(ai, post) || friendlySpawnEligible(ai, post, team)) {
        this.agent.controller.requestObjective(true);
        this.requestState(DONE);
        return;
      }
    }
    switch (this.state) {
      case 2:
        if (this.friendlyPost()) this.requestState(1);
        // Arrived outside the capture region (the offset spans the region's
        // box, past a round region's edge, unrotated): on to its center;
        // still outside there 20 s later (a region above or below where a
        // route can end: Yavin's temple top), another objective, not this
        // one. (Port: it took no part in the capture, and state 2 has no
        // timer, so it stood beside the post for as long as it held.)
        else if (this.helper?.arrived && !this.helper.failed && this.unit && !captureRegion(ai, this.post)?.contains(this.unit.position)) {
          if (!this.centered) {
            this.centered = true;
            this.helper.goto(cpCenter(ai, this.post, _v));
          } else if (!this.expires) this.expires = now + 20;
          else if (this.expires < now) {
            if (this.agent.controller.requestObjective(true)) this.requestState(DONE);
            else this.requestState(1);
          }
        }
        return;
      case 3:
        if (this.helper?.arrived) {
          this.requestState(5);
          return;
        }
        markHintUsed(ai, this.hint, this.unit, now);
        break;
      case 4:
        if (this.helper?.arrived) {
          this.requestState(6);
          return;
        }
        break;
      case 5:
        if (this.expires < now) {
          this.requestState(1);
          return;
        }
        markHintUsed(ai, this.hint, this.unit, now);
        return;
      case 6:
        break;
      default:
        return;
    }
    if (this.expires < now) this.requestState(1);
  }
}
