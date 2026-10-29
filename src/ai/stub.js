import * as THREE from "three";

// A stand-in brain for exercising the multi-actor foundation (spawning,
// shooting, dying, capturing) before the Classic AI modules are wired in.
// This is NOT Classic behavior and makes no fidelity claim: it walks
// straight at the nearest post its team doesn't hold and shoots the nearest
// visible enemy. It goes away when src/ai/classic replaces it.

const RESPAWN = 3; // s after a death before the Character spawns again
const SIGHT = 60; // m
const FIRE_CONE = 0.97; // cos of the aim error that pulls the trigger

const toTarget = new THREE.Vector3();
const eye = new THREE.Vector3();
const aimAt = new THREE.Vector3();

export class StubAI {
  constructor({ battle, units, posts, spawns, spawnUnit, los, random, classesFor, time }) {
    this.time = time;
    this.battle = battle;
    this.units = units;
    this.posts = posts;
    this.spawns = spawns;
    this.spawnUnit = spawnUnit;
    this.los = los;
    this.random = random;
    this.classesFor = classesFor; // team -> [[className, count], ...]
    this.wait = new Map(); // character -> seconds until it may spawn
    this.near = [];
  }

  update(dt) {
    let stagger = 0;
    for (const character of this.battle.characters) {
      if (character.human) continue;
      const unit = character.unit;
      if (unit?.alive) {
        this.drive(unit, dt);
        continue;
      }
      if ((this.battle.reinforcements[character.team] ?? 0) <= 0 || this.battle.over) continue;
      if (character.diedAt !== undefined && this.time() - character.diedAt < RESPAWN) continue;
      const left = (this.wait.get(character) ?? stagger) - dt;
      if (left > 0) {
        this.wait.set(character, left);
        continue;
      }
      this.wait.delete(character);
      stagger += 0.25;
      this.spawn(character);
    }
  }

  spawn(character) {
    const own = this.posts.owned(character.team).filter((post) => this.spawns[post.desc.spawnPath]);
    if (!own.length) return;
    const post = own[Math.floor(this.random() * own.length)];
    const points = this.spawns[post.desc.spawnPath];
    const [x, y, z, yaw] = points[Math.floor(this.random() * points.length)];
    // Roster deficit, the simple way: the class furthest under its target.
    const roster = this.classesFor(character.team);
    const counts = new Map();
    for (const c of this.battle.team(character.team)) if (c.unit?.alive) counts.set(c.className, (counts.get(c.className) || 0) + 1);
    let pick = roster[0]?.[0];
    let deficit = -Infinity;
    for (const [className, target] of roster) {
      const d = target - (counts.get(className) || 0);
      if (d > deficit) {
        deficit = d;
        pick = className;
      }
    }
    if (pick) this.spawnUnit(character, pick, post, [x, y, z], yaw);
  }

  drive(unit, dt) {
    const intent = unit.intent;
    const s = unit.soldier;
    intent.forward = intent.right = intent.turn = intent.pitch = 0;
    intent.buttons[0] = intent.buttons[0] > 0 ? -1 : Math.max(intent.buttons[0] - 1, -100);
    intent.aim = null;
    unit.origin(eye);
    // The nearest enemy in sight.
    let target = null;
    let best = SIGHT * SIGHT;
    for (const other of this.units.query(eye, SIGHT, this.near)) {
      if (this.units.relation(unit.team, other.team) >= 0) continue;
      const d = other.sphere.center.distanceToSquared(eye);
      if (d >= best) continue;
      other.aimPoint(aimAt);
      if (!this.los(eye, aimAt, unit, other)) continue;
      best = d;
      target = other;
    }
    let goal = null;
    if (target) {
      goal = target.aimPoint(aimAt);
    } else {
      let far = Infinity;
      for (const post of this.posts.posts) {
        if (post.team === unit.team) continue;
        const d = post.position.distanceToSquared(s.position);
        if (d < far) {
          far = d;
          goal = post.position;
        }
      }
    }
    if (!goal) return;
    toTarget.subVectors(goal, eye);
    const yaw = Math.atan2(toTarget.x, toTarget.z);
    const yawError = Math.atan2(Math.sin(s.aimYaw - yaw), Math.cos(s.aimYaw - yaw));
    intent.turn = THREE.MathUtils.clamp(yawError / Math.max(s.maxTurn * dt, 1e-3), -1, 1);
    if (target) {
      const flat = Math.hypot(toTarget.x, toTarget.z);
      const pitch = Math.atan2(toTarget.y, flat);
      intent.pitch = THREE.MathUtils.clamp((pitch - s.aimPitch) / Math.max((s.maxPitch ?? 0.5) * dt, 1e-3), -1, 1);
      intent.aim = toTarget.clone().normalize();
      const facing = Math.cos(yawError);
      if (facing > FIRE_CONE) intent.buttons[0] = intent.buttons[0] > 0 ? Math.min(intent.buttons[0] + 1, 100) : 2;
      intent.forward = best > 400 ? 0.5 : 0;
    } else {
      intent.forward = Math.abs(yawError) < 1 ? 1 : 0.2;
      intent.pitch = THREE.MathUtils.clamp(-s.aimPitch * 4, -1, 1);
    }
  }
}
