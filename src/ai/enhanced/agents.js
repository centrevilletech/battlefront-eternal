// Enhanced objective agents (docs/ai-enhanced.md): bots that keep going for
// their command post.
//
// Classic's trooper, scout and repair agents take every interrupt their
// policies allow: a bot walking to a post stops to fight anyone its sight
// scan picks up (out to about 60 m), turns to look round at gunfire it hears
// through walls, and walks off to investigate a shot from any range. In
// whole battles that pulls each bot off its walk two or three times a
// minute, and only a few lives in a hundred reach the post.
//
// These subclasses change only those policies, and only on the objective
// run: while walking to the post, and while the patrol is taking a post that
// is not yet the team's. There a bot
//
//   - stops to fight a seen enemy only within 20 m; farther ones it shoots
//     at as it walks (the objective agents already track, aim and fire);
//   - takes a heard enemy only within 12 m;
//   - takes a shot or hit only from within 25 m, and otherwise turns toward
//     the shooter for a moment so its own scan can find him;
//   - inside the capture region, stays: it fights from where it stands.
//
// Off the run (defending a friendly post, sniping, laying mines) the Classic
// policies apply, except that a heard enemy checked in the last 10 s is not
// checked again. Grenades are always dodged.

import * as THREE from "three";
import { UnitRepairAgent, UnitScoutAgent, UnitTrooperAgent } from "../classic/agents.js";

export const ENHANCED_FOCUS = Object.freeze({
  engageRadius: 20, // m: a seen enemy nearer than this is fought properly
  alertRadius: 25, // m: a shot or hit from nearer than this is answered
  hearRadius: 12, // m: a heard enemy nearer than this is checked
  lookTime: 2.5, // s spent facing a shooter it did not stop for
  recheck: 10, // s before the same heard enemy is checked again
});

const scratch = new THREE.Vector3();

function near(a, b, radius) {
  const dx = a.x - b.x;
  const dz = a.z - b.z;
  return dx * dx + dz * dz < radius * radius;
}

// What a controller remembers across its agents (they are rebuilt per goal).
function memoryOf(controller) {
  return (controller.enhanced ??= { lookUntil: -Infinity, lookPoint: new THREE.Vector3(), heard: new Map() });
}

const sourceKey = (unit) => `${unit.id}:${unit.generation}`;

function focused(Base) {
  return class extends Base {
    // Walking to the post, or the patrol taking a post not yet the team's.
    onRun() {
      if (this.state === this.approachState) return true;
      return this.activeHelper() === this.patrol && this.patrol.state === 2;
    }

    // Standing in the capture region of the post it is taking.
    capturing() {
      if (this.activeHelper() !== this.patrol || this.patrol.state !== 2) return false;
      const post = this.patrol.post;
      const unit = this.unit;
      return Boolean(unit && post && typeof post.contains === "function" && post.contains(unit.position, scratch));
    }

    acceptsTarget(target) {
      if (target && this.unit && this.onRun()) {
        if (this.capturing() || !near(target.position, this.unit.position, ENHANCED_FOCUS.engageRadius)) return false;
      }
      return super.acceptsTarget(target);
    }

    acceptsAcoustic(source) {
      if (!source || !this.unit) return false;
      if (this.onRun() && (this.capturing() || !near(source.position, this.unit.position, ENHANCED_FOCUS.hearRadius))) return false;
      const heard = memoryOf(this.controller).heard;
      const key = sourceKey(source);
      const now = this.now();
      if (now - (heard.get(key) ?? -Infinity) < ENHANCED_FOCUS.recheck) return false;
      const accepted = super.acceptsAcoustic(source);
      if (accepted) {
        heard.set(key, now);
        if (heard.size > 32) for (const [k, at] of heard) if (now - at >= ENHANCED_FOCUS.recheck) heard.delete(k);
      }
      return accepted;
    }

    acceptsAlert(known, point) {
      if (point && this.unit && this.onRun()) {
        if (this.capturing() || !near(point, this.unit.position, ENHANCED_FOCUS.alertRadius)) {
          const memory = memoryOf(this.controller);
          memory.lookPoint.copy(point);
          memory.lookUntil = this.now() + ENHANCED_FOCUS.lookTime;
          return false;
        }
      }
      return super.acceptsAlert(known, point);
    }

    // With no target in view, face a shooter it chose not to stop for.
    aim(stance = 0, look = null) {
      const memory = this.controller.enhanced;
      if (memory && memory.lookUntil > this.now()) look = memory.lookPoint;
      super.aim(stance, look);
    }
  };
}

export class EnhancedTrooperAgent extends focused(UnitTrooperAgent) {}
export class EnhancedScoutAgent extends focused(UnitScoutAgent) {}
export class EnhancedRepairAgent extends focused(UnitRepairAgent) {}

// The agent table entries Enhanced replaces (createClassicAI options.agents).
export const ENHANCED_AGENTS = Object.freeze({
  trooper: EnhancedTrooperAgent,
  scout: EnhancedScoutAgent,
  repair: EnhancedRepairAgent,
});
