import { hashKey } from "./hash.js";
import { soldierVoice } from "./soldiers.js";
import { fireWeapon, updateWeapon, weaponSlot } from "./weapons.js";

// The player's own unit: its weapons, its footsteps and other foley, its
// voice, the jetpack, and the droideka's engine, legs, and shield, as the
// 2004 game plays them (docs/formats.md, "Sound"). Every sound here is
// placed at the unit's origin (its feet) and follows it; none is 2D except
// the scope's zoom. Other soldiers' weapons and voices are SoldierSounds'
// (soldiers.js), by the same rules.
//
// Weapons keep three voices each, as the game does (weapons.js): one for
// the state sound (charge, overheat, reload, the fusion cutter's loop), one
// for firing and the empty click, one for movement.
//
// Footsteps come from two detectors sharing a flag per foot. The feet: a
// foot dropping faster than 0.3 m/s is flagged; flagged and then rising as
// fast, it is unflagged, and steps if it rose from under 0.17 m (its
// plant). The stride, a fallback: every half stride (FootstepStride, 1.75 m
// unless set) of distance flags the next foot (right first); a full stride,
// or stopping, steps the flagged foot.
// Flagging a foot plays the unit's ClothingRustleSound (no class on Theed
// sets one). A step plays the foley entry's FootstepLeftRun or
// FootstepRightRun (the walk variants need RunSpeed, which no class sets),
// plus the weapon's movement slot for it.
//
// The 2004 game runs both detectors side by side. Run literally on this
// port's animation (blended clips whose planted foot wobbles a few
// millimetres a frame), they gave two or three steps a footfall, so here a
// foot must lift 5 cm above its last plant before it can step again, and
// the stride only steps while the feet haven't planted for 0.75 s (no
// skeleton, a still pose, the air). In the 2004 game the stride counts
// the full 3D speed in every state, so the original mode steps in the air
// too, a step every 1.75 m of a jump or a jetpack flight; the enhanced mode
// steps only on the ground.
// The entry is the FoleyFXSoldier of the surface's material group
// (terrain_foley, water_foley while wading, or the object's FoleyFXGroup)
// whose Class is the unit's FoleyFXClass; most CIS droids have none, so they
// make no foley at all.

const DROP = 0.3; // m/s a foot must fall (or rise) to count
const PLANT = 0.17; // m: a foot rising from under this has stepped
const LIFT = 0.05; // m a foot must rise above its last plant to step again
const STRIDE = 1.75; // m, FootstepStride's default
const BONES_IDLE = 0.75; // s without a planted step before the stride steps
const DROIDEKA_LEG = 0.48; // m: a leg2 bone dropping under this steps
const DROIDEKA_TURN_OFF = 3; // s, the engine class's TurnOffTime default
const SPEED = hashKey("Speed");
const ACCELERATION = hashKey("Acceleration");
const HEALTH = hashKey("Health");
const WEAPON_STEP = ["0xb77f3981", "0xb989fc18", "0x1064c0a7", "0x10a991e4"]; // left/right walk, left/right run

export class PlayerSounds {
  constructor(audio) {
    this.audio = audio;
    this.weapons = new Map(); // weapon -> { states, voices }
    this.feet = { flags: [false, false], prev: [null, null], next: 1, stride: 0, rearm: [-Infinity, -Infinity], idle: 0 };
    this.moves = null;
    this.zoom = { shown: false, level: 0 };
    this.voice = null; // hurt and death share one
    this.jet = null;
    this.engine = null;
    this.legs = [null, null, null];
    this.material = "terrain_foley";
    this.stepFrom = { x: 0, y: 0, z: 0 };
  }

  get game() {
    return this.audio.game;
  }

  cls(key) {
    return this.audio.data.classSounds(this.game.unit)?.[key];
  }

  play(className, key, options) {
    return this.audio.playClass(className, key, options);
  }

  // Follows the unit's feet (its origin) with its velocity.
  at() {
    const s = this.game.soldier;
    return { follow: () => s.position };
  }

  update(dt) {
    const g = this.game;
    if (!g.combat || !g.soldier) return;
    this.updateWeapons(dt);
    this.updateZoom();
    if (g.soldier.rolled !== undefined) this.updateDroideka(dt);
    else {
      this.updateMoves();
      this.updateFeet(dt);
      this.updateJet(dt);
    }
  }

  // --- weapons -----------------------------------------------------------

  slot(weapon) {
    let s = this.weapons.get(weapon);
    if (!s) this.weapons.set(weapon, (s = weaponSlot()));
    return s;
  }

  updateWeapons(dt) {
    const { combat, controls } = this.game;
    const arsenal = combat.arsenal;
    // Reload, overheat, charge, and the fire loop (weapons.js).
    const at = this.at();
    for (const weapon of arsenal.weapons) updateWeapon(this.audio, this.slot(weapon), weapon, at);
    // Selecting a weapon.
    const selected = arsenal.selected.join(",");
    if (this.lastSelected !== undefined && selected !== this.lastSelected && !combat.dead) {
      const before = this.lastSelected.split(",").map(Number);
      for (const channel of [0, 1]) {
        if (arsenal.selected[channel] === before[channel]) continue;
        const w = arsenal.current(channel);
        if (w) this.play(w.def.class, "WeaponChangeSound", this.at());
      }
    }
    this.lastSelected = selected;
    // The empty click: the trigger pressed while the weapon is empty,
    // overheated, reloading, or more than a second into a long cycle.
    for (const [channel, control] of [[0, "firePrimary"], [1, "fireSecondary"]]) {
      const held = controls.held(control);
      const w = arsenal.current(channel);
      if (held && !this[`held${channel}`] && w && !combat.dead && !combat.mounted) {
        const late = (w.def.ShotDelay || 0) > 1 && w.wait > 0 && w.def.ShotDelay - w.wait > 1;
        if (w.empty || w.overheated || w.reloading || late) {
          const s = this.slot(w);
          s.v1 = this.play(w.def.class, "FireEmptySound", this.at());
        }
      }
      this[`held${channel}`] = held;
    }
  }

  // A round (or a salvo) leaves a weapon, or a repair tick lands.
  fire(combat, shot) {
    fireWeapon(this.audio, this.slot(shot.weapon), shot, this.at());
  }

  // --- the scope ----------------------------------------------------------

  // The zoom sounds come with the scope overlay (zoomed in first person, or
  // a weapon that always shows its scope): on showing it, on hiding it, and
  // on each zoom step while it shows. They are the interface's, 2D.
  updateZoom() {
    const { combat } = this.game;
    const weapon = combat.arsenal.current(0);
    const level = combat.zoomStep;
    const shown = !combat.dead && !combat.mounted && combat.zoom > 1 && (combat.firstPerson || Boolean(weapon?.def.ZoomFirstPerson));
    const effects = this.audio.data.mission.effects || {};
    const z = this.zoom;
    let sound = null;
    if (shown && !z.shown) sound = effects.ScopeDisplayZoomIn;
    else if (!shown && z.shown) sound = effects.ScopeDisplayZoomOut;
    else if (shown && level > z.level) sound = effects.ScopeDisplayZoomIn;
    else if (shown && level < z.level) sound = effects.ScopeDisplayZoomOut;
    if (sound) this.audio.engine.play(sound, { category: "interface" });
    z.shown = shown;
    z.level = level;
  }

  // --- voice --------------------------------------------------------------

  // Hurt when a hit takes more than 2% of the health left; the death sound
  // at 0. They share a voice: each cuts off the one before (soldiers.js).
  damage(combat, amount, dead) {
    this.voice = soldierVoice(this.audio, this.voice, this.game.unit, amount, combat.health, dead, this.at());
  }

  // --- foley ---------------------------------------------------------------

  // The FoleyFXSoldier entry for the surface underfoot.
  entry() {
    const data = this.audio.data;
    const groupKey = hashKey(this.material);
    const group = data.object("FoleyFXGroup", groupKey) || data.object("a1c9cbc5", groupKey);
    const fxClass = this.cls("FoleyFXClass");
    if (!group || !fxClass) return null;
    const classKey = hashKey(fxClass);
    for (const [name] of group.FoleyFX || []) {
      const entry = data.object("FoleyFXSoldier", name);
      if (entry && entry.Class === classKey) return entry;
    }
    return null;
  }

  // Which material group is underfoot: water while wading, the terrain's,
  // or the object's own (left as it was over an object with none).
  surface() {
    const g = this.game;
    const p = g.soldier.position;
    const water = g.water;
    if (water && water.level > p.y && water.over(p.x, p.z)) {
      this.material = "water_foley";
      return;
    }
    const hit = this.audio.groundUnder(p);
    if (!hit) return;
    if (hit.terrain) {
      this.material = "terrain_foley";
      return;
    }
    const box = this.audio.materialAt(hit.point);
    if (box) this.material = box.group;
  }

  foley(key, weaponKey) {
    const entry = this.entry();
    if (entry?.[key]) this.audio.engine.play(entry[key], { ...this.at(), category: "foley" });
    if (weaponKey) {
      const w = this.game.combat.arsenal.current(0);
      if (w) {
        const s = this.slot(w);
        s.v2 = this.play(w.def.class, weaponKey, { ...this.at(), category: "foley" }) || s.v2;
      }
    }
  }

  // Jumps, landings, dives, posture changes, and being thrown down.
  updateMoves() {
    const { soldier: s, combat } = this.game;
    const was = this.moves;
    this.moves = { air: s.air, posture: s.posture, dive: s.dive };
    if (!was || combat.dead) return;
    if (s.air !== was.air || s.posture !== was.posture || s.dive !== was.dive) this.surface();
    if (s.air === "jump" && was.air !== "jump") this.foley("jump", "JumpSound");
    if (!s.air && was.air === "thrown") this.foley("bodyfall", null);
    else if (!s.air && was.air) this.foley("Land", "LandSound");
    if (s.dive && !was.dive) this.foley("roll", "RollSound");
    if (!s.air && !was.air && !s.dive && !was.dive && s.posture !== was.posture) {
      if (s.posture === "crouch") this.foley("squat", "SquatSound");
      else if (s.posture === "prone") this.foley("prone", "ProneSound");
      else if (s.posture === "stand") this.foley("stand", "StandSound");
    }
  }

  // The two footstep detectors (see the top of the file).
  updateFeet(dt) {
    const { soldier: s, combat, actor } = this.game;
    const f = this.feet;
    if (combat.dead || dt <= 0 || s.dive || combat.mounted) return;
    const enhanced = Boolean(this.audio.engine?.renderer?.enhanced);
    if (enhanced && (s.air || s.grounded === false)) {
      // Off the ground: no steps, and nothing carried over to the landing.
      f.flags[0] = f.flags[1] = false;
      f.prev[0] = f.prev[1] = null;
      f.stride = 0;
      f.idle = 0;
      return;
    }
    // The feet, by their height above the unit's origin.
    const bones = [actor?.byName?.bone_l_foot, actor?.byName?.bone_r_foot];
    const threshold = DROP * dt;
    let planted = false;
    bones.forEach((bone, foot) => {
      if (!bone) return;
      bone.getWorldPosition(this.audio.scratch);
      const height = this.audio.scratch.y - s.position.y;
      const prev = f.prev[foot];
      f.prev[foot] = height;
      if (prev === null) return;
      if (prev - height > threshold) {
        if (!f.flags[foot] && height > f.rearm[foot]) this.flag(foot);
      } else if (f.flags[foot] && height - prev > threshold) {
        f.flags[foot] = false;
        if (prev < PLANT) {
          this.step(foot);
          f.rearm[foot] = prev + LIFT;
          planted = true;
        }
      }
      // Lifted clear of its last plant: it may step again.
      if (height > f.rearm[foot] + LIFT) f.rearm[foot] = -Infinity;
    });
    // The stride: the full speed, as the original game has it.
    const v = s.velocity;
    const speed = Math.hypot(v.x, v.y || 0, v.z);
    const stopped = speed < 0.1;
    if (planted) {
      f.stride = 0;
      f.idle = 0;
      return;
    }
    if (!stopped) f.idle += dt;
    if (bones[0] && bones[1] && f.idle < BONES_IDLE) return;
    const stride = Number(this.cls("FootstepStride") ?? STRIDE) || STRIDE;
    f.stride += speed * dt;
    const flagged = f.flags[0] ? 0 : f.flags[1] ? 1 : -1;
    if (flagged < 0 && f.stride > stride / 2) {
      this.flag(f.next);
      f.next = 1 - f.next;
    } else if (flagged >= 0 && (f.stride > stride || (stopped && f.stride > 0.0001))) {
      f.stride = stopped ? 0 : f.stride - stride;
      this.step(flagged);
    }
  }

  flag(foot) {
    this.feet.flags[foot] = true;
    this.play(this.game.unit, "ClothingRustleSound", { ...this.at(), category: "foley" });
  }

  step(foot) {
    this.feet.flags[foot] = false;
    this.surface();
    // Left and right, run (RunSpeed is never set, so every step runs).
    this.foley(foot === 0 ? "FootstepLeftRun" : "FootstepRightRun", WEAPON_STEP[2 + foot]);
  }

  // --- the jetpack -------------------------------------------------------

  // On: TurnOnSound and the engine (a parameterized sound fed Speed, the
  // speed; Acceleration, as the original game works it out: the last speed
  // less this speed over the frame time; and Health). Off: after
  // TurnOffTime (0 for the jet trooper), TurnOffSound, and the engine stops.
  updateJet(dt) {
    const { soldier: s, combat } = this.game;
    const unit = this.game.unit;
    const on = s.air === "jet" && !combat.dead;
    if (on && !this.jet) {
      this.play(unit, "TurnOnSound", { ...this.at(), category: "foley" });
      const ref = this.audio.classSound(unit, "EngineSound");
      this.jet = { engine: ref ? this.audio.engine.playParameterized(ref.name, { ...this.at(), category: "foley" }) : null, speed: 0, off: null };
    }
    if (!this.jet) return;
    const speed = Math.hypot(s.velocity.x, s.velocity.y, s.velocity.z);
    this.jet.engine?.set(SPEED, speed);
    this.jet.engine?.set(ACCELERATION, this.jet.speed - speed / Math.max(dt, 1e-3));
    this.jet.engine?.set(HEALTH, combat.health / (combat.maxHealth || 1));
    this.jet.speed = speed;
    if (!on) {
      this.jet.off = (this.jet.off ?? Number(this.cls("TurnOffTime") ?? DROIDEKA_TURN_OFF)) - dt;
      if (this.jet.off <= 0) {
        this.play(unit, "TurnOffSound", { ...this.at(), category: "foley" });
        this.jet.engine?.stop(0.05);
        this.jet = null;
      }
    } else {
      this.jet.off = null;
    }
  }

  // --- the droideka ---------------------------------------------------------

  updateDroideka(dt) {
    const { soldier: s, combat, actor } = this.game;
    const unit = this.game.unit;
    // Folding or rolled: TurnOnSound and the engine; unfolding starts the
    // turn-off, which lands TurnOffTime (3 s) later.
    const balled = s.mode === 11 || s.mode === 12;
    if (balled && !this.engine && !combat.dead) {
      this.play(unit, "TurnOnSound", { ...this.at(), category: "foley" });
      const ref = this.audio.classSound(unit, "EngineSound");
      this.engine = { sound: ref ? this.audio.engine.playParameterized(ref.name, { ...this.at(), category: "foley" }) : null, speed: 0, off: null };
    }
    if (this.engine) {
      const speed = Math.hypot(s.velocity.x, s.velocity.y, s.velocity.z);
      this.engine.sound?.set(SPEED, speed);
      this.engine.sound?.set(ACCELERATION, this.engine.speed - speed / Math.max(dt, 1e-3));
      this.engine.sound?.set(HEALTH, combat.health / (combat.maxHealth || 1));
      this.engine.speed = speed;
      if (!balled) {
        this.engine.off = (this.engine.off ?? Number(this.cls("TurnOffTime") ?? DROIDEKA_TURN_OFF)) - dt;
        if (this.engine.off <= 0) {
          this.play(unit, "TurnOffSound", { ...this.at(), category: "foley" });
          this.engine.sound?.stop(0.1);
          this.engine = null;
        }
      } else {
        this.engine.off = null;
      }
    }
    // Its three legs: each leg2 bone dropping under 0.48 m steps.
    ["bone_l_leg2", "bone_r_leg2", "bone_b_leg2"].forEach((name, i) => {
      const bone = actor?.byName?.[name];
      if (!bone || combat.dead || balled) {
        this.legs[i] = null;
        return;
      }
      bone.getWorldPosition(this.audio.scratch);
      const height = this.audio.scratch.y - s.position.y;
      const prev = this.legs[i];
      this.legs[i] = height;
      if (prev !== null && prev >= DROIDEKA_LEG && height < DROIDEKA_LEG) this.play(unit, `Footstep${i}Sound`, { ...this.at(), category: "foley" });
    });
    // The shield going up plays its weapon's FireSound.
    const shield = s.shield;
    if (shield) {
      if (shield.on && !this.shieldOn && shield.def) this.play(shield.def.class, "FireSound", this.at());
      this.shieldOn = shield.on;
    }
  }
}
