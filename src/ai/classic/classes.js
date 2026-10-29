import { Vector3 } from "three";
import { SnipeHelper as SnipePolicies, MineHelper as MinePolicies } from "./agents.js";
import { refOf } from "./controller.js";
import { pickStance } from "./nav/hints.js";
import { effectiveRange, firingCone, selectedWeapon, switchWeapon } from "./weapons-ai.js";
import { PriorityHeap } from "./perception.js";
import { friendlySpawnEligible } from "./patrol.js";

// Class jobs use the ordinary movement and weapon APIs. No healing, ammunition
// grants, mine creation, or jet impulse is performed by these decisions.
const FLT_MAX = 3.4028234663852886e38;
const INT_MAX = 2147483647;
const now = (agent) => agent.world.time ?? 0;
const relation = (world, a, b) => world.relation?.(a, b) ?? world.units?.relation?.(a, b) ?? (a === b && a ? 1 : a && b ? -1 : 0);
const horizontal2 = (a, b) => (a.x - b.x) ** 2 + (a.z - b.z) ** 2;
const weapons = (unit) => unit?.combat?.arsenal?.weapons ?? [];
const health = (unit) => unit.combat?.health ?? unit.health ?? 0;
const maximum = (unit) => unit.combat?.maxHealth ?? unit.maxHealth ?? unit.classDef?.MaxHealth ?? 0;
const radius = (unit) => unit.sphere?.radius ?? unit.radius ?? 0;

export function lowTickets(world, team) {
  const count = world.spawning?.tickets?.(team) ?? world.battle?.reinforcements?.[team];
  return (team === 1 || team === 2) && count != null && count <= 25;
}

function query(world, center, range, cap, predicate, includeDead = false) {
  const candidates = includeDead && world.serviceCandidates ? world.serviceCandidates(center, range)
    : world.units?.query?.(center, range, [], predicate) ?? [];
  return candidates.filter((u) => u.position && u.position.distanceToSquared(center) <= (range + radius(u)) ** 2 && predicate(u)).slice(0, cap);
}

function hints(agent, mask, halfExtent, cap, requester = null) {
  return agent.ai.nav?.queryHints?.({ mask, center: agent.unit.position, halfExtent, cap, requester, now: now(agent) })
    ?? agent.world.nav?.queryHints?.({ mask, center: agent.unit.position, halfExtent, cap, requester, now: now(agent) }) ?? [];
}

function mark(agent, hint) {
  if (agent.world.nav?.markHint) agent.world.nav.markHint(hint, agent.unit, now(agent));
  else agent.ai.nav?.markHint?.(hint, agent.unit, now(agent));
}

function available(agent, hint) {
  return !(now(agent) < (hint.reservedAt ?? -10) + 4);
}

export function snipeTarget(world, owner, candidate) {
  return candidate !== owner && candidate.alive && candidate.targetable !== false
    && candidate.kind !== 10 && !candidate.mine && !["mine", "item", "powerupitem"].includes(candidate.classDef?.base)
    && (candidate.classDef?.MaxHealth ?? candidate.maxHealth ?? candidate.combat?.maxHealth ?? 1) < FLT_MAX
    && relation(world, owner.team, candidate.team) < 0;
}

export function snipeSuitable(agent, hint) {
  const { world, unit } = agent;
  if (lowTickets(world, unit.team)) return false;
  const controls = world.missionControls ?? {};
  const attacker = relation(world, unit.team, controls.attackingTeam ?? 1) > 0;
  const r = hint.mode ? (controls.snipe?.[attacker ? "attacker" : "defender"] ?? 140) / 2 : 70;
  const center = hint.position.clone().addScaledVector(hint.forward, r);
  return query(world, center, r, 175, (u) => relation(world, unit.team, u.team) < 0).some((u) => snipeTarget(world, unit, u));
}

export function snipeCorridorClear(agent, hint) {
  const { world, unit } = agent;
  if (world.network && world.secondaryWorldActive) return true;
  const delta = new Vector3().subVectors(hint.position, unit.position);
  const length = delta.length();
  const center = unit.position.clone().addScaledVector(delta, 0.5);
  delta.normalize();
  return !query(world, center, length / 2, 175, (u) => relation(world, unit.team, u.team) < 0)
    .some((u) => new Vector3().subVectors(u.position, unit.position).cross(delta).lengthSq() < 144 && snipeTarget(world, unit, u));
}

export function selectSnipe(agent, helper) {
  const list = hints(agent, 1, 100, 20).slice();
  const attacker = relation(agent.world, agent.unit.team, agent.world.missionControls?.attackingTeam ?? 1) > 0;
  // The 2004 game's mode filter removes by swapping with the final entry.
  for (let i = 0; i < list.length;) {
    const mode = list[i].mode ?? 0;
    if (mode === 0 ? !helper.scout : mode === 1 ? !attacker : mode === 2 ? attacker : false) {
      list[i] = list[list.length - 1]; list.pop();
    } else i++;
  }
  const heap = new PriorityHeap(20, 0);
  // This hint heap stores float distances; perception's heaps store ints.
  heap.priority = new Float32Array(21);
  for (const hint of list) heap.insert(-hint.position.distanceToSquared(agent.unit.position), hint);
  let attempts = 0;
  while (heap.size) {
    const hint = heap.pop();
    if (!available(agent, hint)) continue;
    if (attempts++ > 10) break;
    if (snipeSuitable(agent, hint) && snipeCorridorClear(agent, hint)) { helper.hint = hint; return true; }
  }
  return false;
}

export class SnipeHelper extends SnipePolicies {
  select() {
    if (!(now(this.agent) > (this.retryAt ?? -1))) return false;
    this.retryAt = now(this.agent) + 8 + this.agent.ai.randomA.range(0, 4);
    const found = selectSnipe(this.agent, this);
    if (found) mark(this.agent, this.hint);
    return found;
  }
  get busy() { return this.state === 3 && this.blockAim; }
  onEnter() {
    if (this.state === 2) this.agent.helper.goto(this.hint.position);
    if (this.state === 3) {
      this.stance = pickStance(this.hint.primaryStance ?? this.hint.primaryMask ?? 1, this.agent.ai.randomA);
      this.revalidateAt = 0;
    }
  }
  inForwardCone(point) {
    if (!point || !this.hint) return false;
    return point.clone().sub(this.agent.unit.position).normalize().dot(this.hint.forward.clone().normalize()) > 0.21643958985805511;
  }
  retry() {
    this.requestState(selectSnipe(this.agent, this) ? 2 : 100);
  }
  onTick() {
    const a = this.agent;
    if (this.state === 2) {
      mark(a, this.hint);
      if (a.helper.arrived) {
        if (a.helper.status || a.helper.failed) this.retry();
        else this.requestState(3);
      }
    } else if (this.state === 3) {
      if (now(a) > this.revalidateAt) {
        this.revalidateAt = now(a) + 15;
        if (!snipeSuitable(a, this.hint)) { this.retry(); return; }
      }
      mark(a, this.hint);
      this.lookPoint.copy(a.unit.position).addScaledVector(this.hint.forward.clone().normalize(), 10);
      const target = a.ai.combat?.currentTarget?.(a);
      const point = target?.position ?? a.ai.combat?.memoryPoint?.(a) ?? this.lookPoint;
      this.blockAim = !this.inForwardCone(point);
      this.look = !this.blockAim;
      if (this.blockAim) { a.controller.lookAt(this.lookPoint); a.controller.setStance(this.stance); }
      a.controller.rangeScale = 2;
    }
  }
  exitState() { this.blockAim = false; this.look = false; }
}

export function remainingAmmo(weapon) {
  if (weapon.bottomless || weapon.rounds >= INT_MAX || weapon.reserve >= FLT_MAX) return INT_MAX;
  return Math.trunc(Math.fround(Math.fround((weapon.clip ?? 0) + (weapon.reserve ?? 0)) * (weapon.rounds ?? 1)));
}

function itemList(world) { return world.items?.items ?? world.items ?? []; }
function dispenserAvailable(agent, weapon) {
  if (remainingAmmo(weapon) === 0) return false;
  const max = weapon.def.MaxItems ?? 0;
  if (!max) return true;
  const owned = agent.world.items?.ownedBy?.(weapon.def.class, agent.unit);
  const count = owned ? owned.filter((item) => !item.gone).length
    : itemList(agent.world).filter((item) => !item.gone && item.owner === agent.unit && item.def?.class === weapon.def.item?.class).length;
  return count < max;
}

export function mineWeapon(agent) {
  return weapons(agent.unit).find((w) => w.def.base === "dispenser" && w.def.item?.base === "mine"
    && (w.def.item.LifeSpan == null || w.def.item.LifeSpan === Infinity || w.def.item.LifeSpan === FLT_MAX) && dispenserAvailable(agent, w)) ?? null;
}

export function minePlacement(agent, hint) {
  const r = hint.radius;
  if (!(r > 0)) return null;
  const p = hint.position;
  const nearby = itemList(agent.world).filter((item) => !item.gone && relation(agent.world, agent.unit.team, item.team) > 0
    && item.position.distanceToSquared(p) <= (r + 6 + (item.radius ?? 0)) ** 2).slice(0, 30).filter((item) => item.mine || item.def?.base === "mine");
  for (let x = p.x - r; x < p.x + r; x += 3) {
    for (let z = p.z - r; z < p.z + r; z += 3) {
      const candidate = new Vector3(x, p.y, z);
      if (horizontal2(candidate, p) >= r * r) continue;
      if (nearby.every((mine) => horizontal2(candidate, mine.position) >= 5.0625)) return candidate;
    }
  }
  return null;
}

export function selectMine(agent, helper) {
  if (!(now(agent) > (helper.retryAt ?? -1))) return false;
  helper.retryAt = now(agent) + 10;
  const candidates = hints(agent, 16, 100, 3, agent.unit).slice();
  for (let i = 0; i < candidates.length;) {
    const post = candidates[i].commandPost;
    if (post && !friendlySpawnEligible(agent.ai, post, agent.unit.team)) {
      candidates[i] = candidates[candidates.length - 1]; candidates.pop();
    } else i++;
  }
  for (const hint of candidates) {
    if (!available(agent, hint)) continue;
    const center = hint.position.clone();
    if (agent.world.mineGroundHeight) center.y = agent.world.mineGroundHeight(center);
    // Only the first hostile broadphase candidate is inspected.
    const hostile = agent.world.classCandidates?.(center, hint.radius + 6, 1, agent.unit, -1)
      ?? [...query(agent.world, center, hint.radius + 6, 1, (u) => relation(agent.world, agent.unit.team, u.team) < 0),
        ...itemList(agent.world).filter((item) => !item.gone && relation(agent.world, agent.unit.team, item.team) < 0
          && item.position.distanceToSquared(center) <= (hint.radius + 6 + (item.radius ?? 0)) ** 2)].slice(0, 1);
    if (hostile[0]?.mine || hostile[0]?.def?.base === "mine") continue;
    const placement = minePlacement(agent, hint);
    if (placement) { helper.hint = hint; helper.placement = placement; mark(agent, hint); return true; }
  }
  return false;
}

function chooseWeapon(agent, weapon) {
  if (agent.controller.perception?.fire) return switchWeapon(agent.controller, agent.unit, weapons(agent.unit).indexOf(weapon));
  return agent.unit.combat.arsenal.select(weapon);
}

export class MineHelper extends MinePolicies {
  eligible() { return Boolean(mineWeapon(this.agent)); }
  select() { return selectMine(this.agent, this); }
  onEnter() {
    if (this.state === 2) {
      this.aimPoint = this.placement.clone();
      if (this.agent.world.mineGroundHeight) this.aimPoint.y = this.agent.world.mineGroundHeight(this.placement) + 1.5;
      this.agent.helper.goto(this.placement);
    } else if (this.state === 3) { this.agent.helper.stop(); this.firedAt = null; }
  }
  onTick() {
    if (this.state !== 2 && this.state !== 3) return;
    const a = this.agent;
    mark(a, this.hint);
    if (this.state === 2) {
      if (a.helper.arrived) {
        if (a.helper.status || a.helper.failed) { this.hint.reservedAt = now(a) + 30; this.requestState(100); }
        else this.requestState(3);
      }
      return;
    }
    const weapon = mineWeapon(a);
    if (!weapon) { this.requestState(100); return; }
    if (selectedWeapon(a.unit, a.controller) !== weapon) { chooseWeapon(a, weapon); return; }
    a.controller.lookAt(this.aimPoint);
    if (this.firedAt == null) { a.controller.fireA = true; this.firedAt = now(a); return; }
    if (now(a) > this.firedAt + 3) {
      const placement = minePlacement(a, this.hint);
      if (placement) { this.placement = placement; this.requestState(2); }
      else this.requestState(100);
    }
  }
}

const HEALTH_TYPES = ["Person", "Animal", "Droid", "Vehicle", "Building", "BuildingRebuild", "BuildingBuild"];
export function healthType(unit) {
  const type = unit.healthType ?? unit.classDef?.HealthType ?? "person";
  return typeof type === "number" ? type : Math.max(0, HEALTH_TYPES.findIndex((name) => name.toLowerCase() === type.toLowerCase()));
}
function quantity(def, type, kind) {
  if (def.service?.[kind]) return def.service[kind][type] ?? 0;
  if (type >= 5 && kind === "health") return def[HEALTH_TYPES[type]] ?? 0;
  return def[`${HEALTH_TYPES[type]}${kind === "health" ? "Health" : "Ammo"}`] ?? 0;
}
function serviceWeapon(agent, target, kind) {
  return weapons(agent.unit).find((w) => {
    const def = w.def.base === "repair" ? w.def : w.def.base === "dispenser" && dispenserAvailable(agent, w) ? w.def.item : null;
    return def && quantity(def, healthType(target), kind) > 0;
  }) ?? null;
}
function needs(target, kind) {
  if (kind === "health") return health(target) < maximum(target);
  return weapons(target).some((w) => w.reserve < (w.def.ammo > 0 ? w.def.ammo : FLT_MAX) - 1);
}
function serviceAlive(target) { return target && !target.removed && (target.alive || healthType(target) >= 5); }
function resolveJob(job) {
  const ref = job?.target;
  return ref?.unit?.generation === ref?.generation && serviceAlive(ref.unit) ? ref.unit : null;
}

export class RepairServices {
  constructor(ai) { this.ai = ai; this.reservations = []; this.futile = []; }
  reserved(target, kind, agent) {
    this.reservations = this.reservations.filter((r) => now(agent) < r.time + 3 && resolveJob(r) && r.owner.unit?.alive && r.owner.unit.generation === r.owner.generation);
    return this.reservations.some((r) => r.target.unit === target && r.kind === kind && r.owner.unit !== agent.unit);
  }
  // A job this life gave up on (no way there in 20 s, or 8 s of work with
  // nothing to show) stays off its list for 30 s. (Port: the 2004 game's
  // next search, five seconds on, picks the nearest job again, the same one,
  // and the bot goes back to stand at it, for good.)
  futileFor(agent, target, kind) {
    this.futile = this.futile.filter((r) => now(agent) < r.until);
    return this.futile.some((r) => r.owner === agent.unit && r.generation === agent.unit.generation && r.target === target && r.kind === kind);
  }
  giveUp(agent, target, kind) {
    this.futile.push({ owner: agent.unit, generation: agent.unit.generation, target, kind, until: now(agent) + 30 });
    return null;
  }
  useful(agent, target) {
    return ["health", "ammo"].filter((kind) => needs(target, kind) && serviceWeapon(agent, target, kind) && !this.reserved(target, kind, agent) && !this.futileFor(agent, target, kind));
  }
  // Walking to the job: after a living target; to a wreck, which doesn't
  // move, by a walk planned once. Every goto starts the plan over and stops
  // the bot until it comes, so one a service would keep it standing (and
  // the planner busy) for good.
  approach(agent, target, s) {
    if (agent.helper.follow && target.alive) agent.helper.follow(refOf(target));
    else if (!s.toward || horizontal2(s.toward, target.position) > 1e-4) {
      s.toward = target.position.clone();
      agent.helper.goto(target.position);
    }
  }
  // The last metres into the tool's reach, once, in a straight line (the
  // planning barriers around a turret keep a route's end outside it), to
  // the target's bounds and 1.5 m short of its center, where the aim can
  // still reach down to it.
  closeIn(agent, target, s) {
    if (s.closing) return;
    s.closing = true;
    s.toward = null;
    const p = agent.unit.position;
    const c = target.position;
    const stop = (target.sphere?.radius ?? 0) + 1.5;
    const dx = p.x - c.x;
    const dz = p.z - c.z;
    const d = Math.hypot(dx, dz);
    if (d <= stop) agent.helper.stop();
    else agent.helper.goto(new Vector3(c.x + (dx * stop) / d, p.y, c.z + (dz * stop) / d), { mode: "straight" });
  }
  nearbyService(target, agent, kind) {
    // Nearby dropped powerups supply service masks irrespective
    // of faction. Their pickup and quantities remain shared gameplay.
    return itemList(agent.world).filter((item) => !item.gone && item.position.distanceToSquared(target.position) <= (15 + (item.radius ?? 0)) ** 2)
      .slice(0, 10).some((item) => item.def?.base === "powerupitem" && quantity(item.def, healthType(target), kind) > 0);
  }
  findRepairJob = (agent) => {
    if (lowTickets(agent.world, agent.unit.team)) return null;
    const candidates = query(agent.world, agent.unit.position, 75, 50, (target) => target !== agent.unit && serviceAlive(target)
      && relation(agent.world, agent.unit.team, target.team) >= 0, true);
    let best = null;
    let bestD = Infinity;
    for (const target of candidates) {
      const useful = this.useful(agent, target);
      if (!useful.length) continue;
      const kind = useful.length === 2 ? useful[agent.ai.randomA.int(0, 1)] : useful[0];
      const distance = target.position.distanceToSquared(agent.unit.position);
      const speed = target.velocity?.lengthSq() ?? 0;
      const human = target.human || target.character?.human || target.humanPiloted;
      const forward = typeof target.forward === "function" ? target.forward(new Vector3()) : target.forward ?? new Vector3(0, 0, 1);
      const facing = forward.dot(agent.unit.position.clone().sub(target.position).normalize()) > Math.cos(15);
      const job = { target: refOf(target), kind };
      if (human && distance < 900 && (speed < 0.25 || facing)) return job;
      if (speed < 0.25 && (human || target.kind !== 0) && distance < bestD) { best = job; bestD = distance; }
    }
    return best;
  };
  startRepairService = (agent) => {
    agent.service = { approachAt: now(agent), progressAt: Infinity, savedHealth: 0 };
    agent.helper.stop();
  };
  repairService = (agent) => {
    const target = resolveJob(agent.job);
    if (!target) return null;
    const kind = agent.job.kind;
    if (!needs(target, kind) || this.nearbyService(target, agent, kind)) return null;
    const weapon = serviceWeapon(agent, target, kind);
    if (!weapon) return null;
    const s = agent.service;
    const distance = horizontal2(agent.unit.position, target.position);
    const current = agent.unit.combat?.repairTarget ?? agent.controller.target?.unit;
    const range = effectiveRange(agent.unit, agent.controller, selectedWeapon(agent.unit, agent.controller)?.def ?? weapon.def);
    // Locked on means the tool holds this target. With no lock, `current` is
    // empty and must not match the wrapper's missing building or vehicle:
    // that made every job "close" inside the tool's default 60 m, and the
    // bot stood that far off, cutting at nothing.
    const locked = current != null && (current === target || current === target.building || current === target.vehicle);
    const close = distance < range * range && (locked || distance < 25);
    const existing = this.reservations.find((r) => r.owner.unit === agent.unit && r.target.unit === target && r.kind === kind);
    if (existing) existing.time = now(agent);
    else this.reservations.push({ ...agent.job, owner: refOf(agent.unit), time: now(agent) });
    agent.controller.target = refOf(target);
    if (target.alive) agent.controller.aimAt(refOf(target));
    else agent.controller.lookAt(target.aimPoint?.(new Vector3()) ?? target.position);
    if (!close) {
      const hasPath = agent.controller.hasExecutor ?? agent.helper.hasPath ?? Boolean(agent.controller.navControl?.executor);
      if (now(agent) > s.approachAt + 20 && (!hasPath || agent.helper.status)) return this.giveUp(agent, target, kind);
      this.approach(agent, target, s);
      s.closing = false;
      s.workable = null;
      s.progressAt = Infinity;
      return { target, near: false };
    }
    // Close: stand and work. A repair tool works only once it has locked on,
    // which it does within its own reach (its lock range past the target's
    // bounds, from the eye). Close by the 5 m rule but not locked yet, the
    // bot walks in the last metres (closeIn); else it would cut at the air
    // until the progress check gave up.
    if (weapon.def.base === "repair" && !locked) this.closeIn(agent, target, s);
    else {
      agent.helper.stop();
      s.toward = null;
      s.closing = false;
    }
    s.approachAt = now(agent);
    s.workable ??= now(agent);
    const selected = selectedWeapon(agent.unit, agent.controller) === weapon;
    if (!selected) { chooseWeapon(agent, weapon); s.progressAt = Infinity; return { target, near: true }; }
    if (agent.controller.perception?.aimer && agent.unit.origin && agent.unit.forward
      && !firingCone(agent.controller, agent.unit, target.aimPoint?.(new Vector3()) ?? target.position)) {
      s.progressAt = Infinity;
      // Never able to bring the tool to bear (the target past its pitch, or
      // behind something): no work starts, so no progress check ever runs.
      // (Port: give up after the progress check's 8 s, not stand for good.)
      if (now(agent) > s.workable + 8) return this.giveUp(agent, target, kind);
      return { target, near: true };
    }
    s.workable = now(agent);
    const firstAction = s.progressAt > now(agent);
    if (s.progressAt > now(agent)) { s.progressAt = now(agent); s.savedHealth = health(target); }
    if (now(agent) > s.progressAt + 8) {
      if (health(target) <= s.savedHealth) return this.giveUp(agent, target, kind);
      s.savedHealth = health(target); s.progressAt = now(agent);
    }
    // Repair tools hold their trigger; a service dispenser throws only at
    // the beginning of a progress interval.
    if (weapon.def.base === "repair" || firstAction) agent.controller.fireA = true;
    return { target, near: true };
  };
}

export const helpers = { snipe: SnipeHelper, mine: MineHelper };
export function classicBinding(ai) {
  const repair = new RepairServices(ai);
  return { selectSnipe, canMine: (agent) => Boolean(mineWeapon(agent)), selectMine,
    findRepairJob: repair.findRepairJob, startRepairService: repair.startRepairService, repairService: repair.repairService };
}
