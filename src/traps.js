import * as THREE from "three";
import { Pose } from "./anim.js";
import { Building } from "./props.js";

// Endor's Ewok log trap (ODF base "trap", end_weap_treesmash), as the 2004
// game's EntityTrap runs it (docs/live-objects.md "Ewok log traps"). Two
// logs hang from ropes either side of a clearing; cocked, they swing down
// and meet in the middle.
//
// - It is a building that starts unbuilt (it has an UnbuiltGeometryName):
//   the logs hang at rest, its build pad (BuildModelOdf, com_prop_buildzone)
//   stands on the ground at BuildPoint (hp_active) with the build hologram
//   over it, and its health is 0. It belongs to ResetTeam's side (a trap
//   placed with Team 0 takes that side's team), so only that
//   side's repair weapons, and its friends' (the Ewoks' tools), build it.
// - Built (health raised to MaxHealth by BuildingBuild ticks), the pad goes
//   and it plays BuildAnimation once (the logs are hauled up), then stands
//   cocked at TriggerAnimation's first frame.
// - Cocked, each update it looks for a vehicle of TriggerTeam's side whose
//   collision sphere is larger than MinEnemyRadius and whose sphere's box
//   meets the box TriggerRadius round its trigger point (its origin; the
//   region query with soldiers and vehicles, mask 6). One sets it off:
//   TriggerAnimation plays. So does losing all its health while built: the
//   logs fall wherever the shot came from.
// - DeathTime into the swing (0.666 s, the frame the logs meet) everything
//   in that box larger than MinEnemyRadius, whatever its side, is killed
//   with an empty damage record (no one is credited) and deleted at once
//   (no wreck, no death animation); if anything was, DeathEffect plays at
//   DeathEffectOffset in the trap's frame.
// - When the swing has played, it is unbuilt again at 0 health, the logs at
//   rest, the pad and hologram back.
//
// The world runs it (step); a page draws the clip from the world's state
// (schema 10, src/runtime/entity-world-state.js: phase and the tick it
// began), its built/unbuilt state, health and team arriving as any
// building's (schema 7).

const FPS = 30; // the original game's clip length: frames at 30 a second
export const TRAP_PHASES = ["down", "reset", "armed", "swing"];
const length = (clip) => (clip ? clip.frames / FPS : 0);
const _down = new THREE.Vector3(0, -1, 0);

export class Trap extends Building {
  // looks: { built: the trap's rig, pad: the build pad's rig or null }.
  constructor(desc, looks, bodies, hologram, clips = null) {
    super(desc, { built: looks.built, destroyed: null, unbuilt: null }, bodies, hologram);
    this.regenerates = false;
    this.rig = looks.built;
    this.pad = looks.pad || null;
    const t = desc.trap || {};
    this.settings = t;
    this.triggerClip = clips?.get(t.trigger) || null;
    this.buildClip = (t.build && clips?.get(t.build)) || null;
    this.reach = Number.isFinite(t.radius) ? t.radius : 3;
    this.minRadius = t.minRadius || 0;
    this.deathTime = Number.isFinite(t.deathTime) ? t.deathTime : -1;
    const rig = this.rig;
    this.pose = rig?.bones?.length && (this.triggerClip || this.buildClip)
      ? new Pose(rig.bones.length, Math.max(0, rig.bones.findIndex((b) => /^dummy_?root$/i.test(b.name))))
      : null;
    // The trigger point: TriggerNode at bind (none on the stock trap) and
    // TriggerOffset, in the trap's frame.
    this.point = new THREE.Vector3(...(t.offset || [0, 0, 0]));
    const node = t.node && rig?.byName?.[t.node];
    if (node) {
      rig.root.updateMatrixWorld(true);
      this.point.applyMatrix4(node.matrixWorld);
    } else this.point.applyMatrix4(this.matrix);
    this.effectAt = new THREE.Vector3(...(t.deathEffectOffset || [0, 0, 0])).applyMatrix4(this.matrix);
    // The pad's sphere (where repair weapons, the AI and blasts find it:
    // the 2004 game sets the trap's sphere to the pad's).
    this.origin = this.position.clone();
    this.padSphere = { center: this.position.clone(), radius: 1 };
    this.placePad(desc.padMatrix ? new THREE.Matrix4().fromArray(desc.padMatrix) : null);
    // The world's state: the phase, the tick it began, seconds into it,
    // and whether it has been shot down while built (it falls when cocked).
    this.phase = "down";
    this.started = 0;
    this.time = 0;
    this.sprung = false;
    this.tick = 0;
    this.crushed = 0; // vehicles it has killed (tests, probes)
    // It starts unbuilt, at 0 health (EntityBuilding's constructor).
    this.health = 0;
    this.show("unbuilt");
  }

  // The pad on its place (a matrix from LiveObjects, on the ground) and the
  // hologram on its hp_hologram.
  placePad(matrix) {
    const pad = this.settings.pad;
    if (!pad) return;
    const at = matrix || new THREE.Matrix4().makeTranslation(...pad.at).premultiply(this.matrix);
    this.padMatrix = at;
    if (this.pad) {
      this.pad.root.matrix.copy(at);
      this.pad.root.matrixWorldNeedsUpdate = true;
    }
    const sphere = this.pad?.model?.sphere;
    if (sphere) {
      this.padSphere.center.set(sphere[0], sphere[1], sphere[2]).applyMatrix4(at);
      this.padSphere.radius = sphere[3];
    } else this.padSphere.center.setFromMatrixPosition(at);
    // Where the AI and the HUD find it: every point the 2004 game gives
    // of a trap is the pad's (its hardpoint position included).
    this.position.copy(this.padSphere.center);
    if (this.hologram) this.hologram.group.position.set(...(pad.hologram || [0, 0, 0])).applyMatrix4(at);
  }

  // Only built and unbuilt: the logs are always drawn; the pad and the
  // hologram only while unbuilt.
  show(state) {
    if (state === "destroyed") state = "unbuilt";
    this.state = state;
    if (this.rig) this.rig.root.visible = true;
    if (this.pad) this.pad.root.visible = state === "unbuilt";
    for (const [name, body] of Object.entries(this.bodies || {})) if (body) body.enabled = name === state;
    if (this.hologram) {
      const on = state === "unbuilt";
      if (on && !this.hologram.on) this.hologram.fade = 0;
      this.hologram.on = on;
    }
  }

  get healthType() {
    return this.state === "built" ? "building" : "build";
  }

  sphere(out) {
    out.copy(this.padSphere.center);
    return this.padSphere.radius;
  }

  blastSphere(out) {
    return this.sphere(out);
  }

  // The world's state for a page (schema 10): phase, the tick it began.
  stateRecord() {
    return [TRAP_PHASES.indexOf(this.phase), this.started ?? this.tick, this.sprung ? 1 : 0];
  }

  adopt([phase, started, sprung]) {
    this.phase = TRAP_PHASES[phase] || "down";
    this.started = started;
    this.sprung = Boolean(sprung);
    this.adopted = true;
    return true;
  }
}

// The pad on the ground under BuildPoint (the 2004 game sets its height to
// the ground's there): straight down onto the world's collision.
export function padOnGround(trapMatrix, at, collision, hit) {
  const matrix = new THREE.Matrix4().makeTranslation(at[0], at[1], at[2]).premultiply(new THREE.Matrix4().fromArray(trapMatrix));
  if (!collision?.cast) return matrix;
  const p = new THREE.Vector3().setFromMatrixPosition(matrix);
  const from = p.clone();
  from.y += 20;
  const found = collision.cast(from, _down, 60, hit);
  if (found) matrix.elements[13] = from.y - hit.distance;
  return matrix;
}

// A team's side, from the mission's teams: the prefix of its classes'
// names (all_, imp_, rep_, cis_, ewk_), as each side's units are named.
export function sideOfTeams(teams) {
  const sides = {};
  for (const [team, desc] of Object.entries(teams || {})) {
    const first = desc?.classes?.[0];
    const name = Array.isArray(first) ? first[0] : first;
    if (typeof name === "string" && name.includes("_")) sides[team] = name.split("_")[0].toLowerCase();
  }
  return (team) => sides[team] ?? null;
}
