import * as THREE from "three";
import { Pose } from "./anim.js";
import { barrelTips, weaponMuzzle } from "./muzzles.js";
import { lighting } from "./materials.js";
import { modelGroup, partMaterial } from "./models.js";

// The first-person view, built the way the 2004 game builds it: the unit
// class's FirstPerson arms rig (rep_1st_trooper for the clone trooper, from
// FPM/REP/reptroop.lvl) holds the shown weapon's first-person model
// (HighResGeometry) in hp_weapons. The rig hangs from the camera at a fixed
// offset, a little below and in front, turned to face forward; its root's own
// lift is dropped.
//
// The first-person clips come in sets by the weapon's animation bank: rifle,
// "tool" (the pistol), and grenade. Every frame one slot plays:
//
//   - shoot when a round leaves (restarted each shot, never blended), or the
//     grenade set's shoot2 when a grenade leaves the hand;
//   - reload while reloading, cut when the reload ends;
//   - jump in the air after a jump or in a dive, flail in a fall; after
//     landing, jump_land for 0.5 s (from a jump) or handsdown for 1 s (from a
//     fall);
//   - the camera motions for a dive, going prone from a crouch, and getting
//     up from prone ease into jump_land for
//     as long as the motion lasts;
//   - on a jetpack the arms hold whatever clip they had, without looping it,
//     and land as the jump or fall that started the flight;
//   - handsdown crawling prone, else run when moving (played at 0.75 x
//     min(v^2 / 20, 1)), else idle.
//
// A change of clip blends from the captured outgoing pose over 1/3 second,
// independent of render rate. Switching weapons lowers
// the arms (handsdown) with the old weapon, then raises them with the new.
// A grenade is in the hand for its whole throw, its model gone once it has
// left. Entering first person starts from handsdown and raises into idle over
// 1 s. The view model isn't drawn while zoomed.
//
// No shadow map reaches the arms: as in the game, a 500 m ray from the gun
// toward the sun decides, the sun's share moving halfway each frame.

const OFFSET = new THREE.Matrix4()
  .makeTranslation(0, 0.13, -0.3645)
  .multiply(new THREE.Matrix4().makeRotationZ(0.05))
  .multiply(new THREE.Matrix4().makeRotationY(3.0));
const BLEND_RATE = 3; // 1/s
const MOVING = 0.1; // v^2 (m^2/s^2) above which the arms run
const RUN_RATE = 0.75;
const LAND_FORCE = 0.5; // s of jump_land after a jump
const FALL_FORCE = 1.0; // s of handsdown after a fall
const RAISE = 1.0; // s of idle when first person starts
const FLASH_SCALE = 0.6;
const SUN_RAY = 500; // m
const SUN_EASE = 0.5; // share of the way to lit or dark per 1/30 s

// The clip for each slot, by set.
const SETS = {
  rifle: {
    idle: "_1st_rifle_idle", run: "_1st_rifle_run", shoot: "_1st_rifle_shoot", shoot2: "_1st_tool_idle",
    reload: "_1st_rifle_reload", jump: "_1st_rifle_jump", flail: "_1st_rifle_flail", jump_land: "_1st_rifle_jump_land",
  },
  tool: {
    idle: "_1st_tool_idle", run: "_1st_tool_run", shoot: "_1st_tool_shoot", shoot2: "_1st_tool_idle",
    reload: "_1st_tool_reload", jump: "_1st_tool_jump", flail: "_1st_tool_flail", jump_land: "_1st_tool_jump_land",
  },
  grenade: {
    idle: "_1st_tool_idle", run: "_1st_tool_run", shoot: "_1st_grenade_shoot", shoot2: "_1st_grenade_shoot2",
    reload: "_1st_tool_reload", jump: "_1st_tool_jump", flail: "_1st_tool_flail", jump_land: "_1st_rifle_jump_land",
  },
  bazooka: {
    idle: "_1st_bazooka_idle", run: "_1st_bazooka_run", shoot: "_1st_bazooka_shoot", shoot2: "_1st_bazooka_idle",
    reload: "_1st_bazooka_reload", jump: "_1st_bazooka_jump", flail: "_1st_bazooka_flail", jump_land: "_1st_bazooka_jump_land",
  },
};
// The droideka's arms have one set of their own, whatever it fires: shoot
// loops at twice the rate while firing, walk plays with forward input.
SETS.droideka = {
  idle: "_1st_droideka_idle", run: "_1st_droideka_walk", shoot: "_1st_droideka_shoot", shoot2: "_1st_droideka_shoot",
  reload: "_1st_droideka_reload", jump: "_1st_droideka_idle", flail: "_1st_droideka_idle", jump_land: "_1st_droideka_idle",
};
// Its rig hangs turned 3.2 rad about y and offset (0.01, 0.35, 0) (the
// order of the two wasn't traced).
export const DROIDEKA_OFFSET = new THREE.Matrix4().makeTranslation(0.01, 0.35, 0).multiply(new THREE.Matrix4().makeRotationY(3.2));
const BANK_SET = { rifle: "rifle", bdroid: "rifle", pistol: "tool", tool: "tool", lightsabre: "tool", sbdroid: "tool", grenade: "grenade", bazooka: "bazooka" };
const LOOPING = new Set(["idle", "run", "flail", "handsdown"]);

export class FirstPersonView {
  // rig: firstperson.json; parts: its model pack (model name -> parts);
  // clips: the first-person clips by name; weaponModels and firePoints: the
  // side's weapons pack; flashModel makes a muzzle flash mesh.
  // set: a clip set to use whatever the weapon (the droideka's); offset:
  // where the rig hangs from the camera, if not where a soldier's does.
  constructor({ rig, parts, clips, texture, weaponModels, firePoints, flashModel, collision, set = null, offset = null }) {
    this.clips = clips;
    this.forcedSet = set;
    this.offset = offset || OFFSET;
    this.root = new THREE.Group();
    this.root.matrixAutoUpdate = false;
    this.root.visible = false;
    this.materials = [];
    this.collision = collision;
    this.sunSeen = 1;
    this.toSun = new THREE.Vector3();
    this.gunAt = new THREE.Vector3();
    this.sunHit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };

    // Bones from the rig's bind pose.
    this.bones = rig.bones.map((desc, index) => {
      const bone = new THREE.Bone();
      bone.name = desc.name;
      bone.userData.index = index;
      new THREE.Matrix4().fromArray(desc.local).decompose(bone.position, bone.quaternion, bone.scale);
      return bone;
    });
    rig.bones.forEach((desc, i) => (desc.parent >= 0 ? this.bones[desc.parent] : this.root).add(this.bones[i]));
    this.byName = Object.fromEntries(this.bones.map((bone) => [bone.name.toLowerCase(), bone]));
    this.rigParts = [];
    for (const part of parts.get(rig.model) || []) {
      const material = partMaterial(part, texture);
      this.materials.push(material);
      const mesh = new THREE.Mesh(part.geometry, material);
      mesh.frustumCulled = false;
      (this.byName[(part.bone || "").toLowerCase()] || this.root).add(mesh);
      this.rigParts.push(mesh);
    }
    this.rootBone = this.bones.find((bone) => bone.parent === this.root) || null;

    // The first-person weapons in the hand.
    this.hand = this.byName.hp_weapons || this.root;
    this.weapons = new Map();
    this.models = weaponModels;
    this.firePoints = firePoints;
    this.flashModel = flashModel;
    this.texture = texture;
    this.builtInGuns = set === "droideka" ? ["bone_l_forearm", "bone_r_forearm"].map((name, index) => {
      const bone = this.byName[name], points = [];
      for (const mesh of this.rigParts) if (mesh.parent === bone) {
        const position = mesh.geometry.attributes.position;
        for (let i = 0; i < position.count; i++) points.push(new THREE.Vector3().fromBufferAttribute(position, i));
      }
      return barrelTips(points, "z", index === 0 ? 1 : -1, "x").map(point => {
        const fire = new THREE.Object3D(); fire.position.copy(point); bone.add(fire);
        let flash = null;
        if (flashModel) {
          flash = flashModel(); flash.scale.setScalar(FLASH_SCALE * 0.5); flash.visible = false;
          if (index === 1) flash.rotation.y = Math.PI;
          fire.add(flash);
        }
        return { fire, flash, group: this.root, flashLeft: 0, flashLength: 0, flashAlpha: 1 };
      });
    }) : [];

    this.pose = new Pose(this.bones.length, -1);
    this.transitionFrom = this.bones.map((bone) => ({ position: bone.position.clone(), quaternion: bone.quaternion.clone() }));
    this.target = new THREE.Quaternion();
    this.p = new THREE.Vector3();
    this.slot = null; // the slot playing
    this.clip = null;
    this.time = 0;
    this.blend = 0;
    this.action = null; // "shoot" | "shoot2": a shot's clip under way
    this.forced = null; // { slot, left }
    this.wasAir = null;
    this.airTime = 0;
    this.impactSpeed = 0;
    this.wasActive = false;
    this.holding = null; // the weapon whose model is in the hand
    this.flashes = this.builtInGuns.flat();
  }

  // The first-person model for a weapon, built the first time it's needed.
  // The muzzle flash sits at the weapon class's fire point, which is the
  // displayed model's barrel mouth.
  weapon(def) {
    const name = def?.HighResGeometry || def?.GeometryName;
    if (!name || !this.models.has(name)) return null;
    if (!this.weapons.has(name)) {
      const group = modelGroup(this.models.get(name), (part) => {
        const material = partMaterial(part, this.texture);
        this.materials.push(material);
        return material;
      });
      group.traverse((child) => (child.frustumCulled = false));
      group.visible = false;
      const point = weaponMuzzle(this.models.get(name), this.firePoints[name] || this.firePoints[def.GeometryName], def);
      const fire = new THREE.Object3D();
      if (point) fire.position.fromArray(point);
      group.add(fire);
      let flash = null;
      if (point && this.flashModel) {
        flash = this.flashModel();
        flash.position.fromArray(point);
        flash.scale.setScalar(FLASH_SCALE);
        flash.visible = false;
        flash.traverse((child) => (child.frustumCulled = false));
        group.add(flash);
      }
      this.hand.add(group);
      const held = { group, fire, flash, flashLeft: 0, flashLength: 0, flashAlpha: 1 };
      this.weapons.set(name, held);
      this.flashes.push(held);
    }
    return this.weapons.get(name);
  }

  setFor(weapon) {
    if (this.forcedSet) return SETS[this.forcedSet];
    return SETS[BANK_SET[weapon?.bank] || "rifle"] || SETS.rifle;
  }

  // Begin the grenade windup when the throw starts, then play its short
  // release when it leaves the hand. Firing finishes a weapon switch at once.
  play(event, weapon) {
    if (!this.wasActive || !["shoot", "throw", "prepareThrow"].includes(event)) return;
    this.holding = weapon;
    if (event === "prepareThrow") {
      this.action = "shoot";
      this.start(this.action, this.setFor(weapon)[this.action]);
      return;
    }
    // The droideka's shoot loops on while it keeps firing.
    if (this.forcedSet && this.action === "shoot" && this.clip && this.time < this.clip.duration) {
      this.firing = 0.25;
      return;
    }
    this.action = event === "throw" ? "shoot2" : "shoot";
    this.firing = 0.25;
    this.start(this.action, this.setFor(weapon)[this.action], true);
  }

  // Start a clip, blending in from where the arms are, or not.
  start(slot, name, snap = false) {
    const clip = this.clips.get(name);
    if (!clip) return;
    if (clip !== this.clip || snap) {
      this.bones.forEach((bone, i) => {
        this.transitionFrom[i].position.copy(bone.position);
        this.transitionFrom[i].quaternion.copy(bone.quaternion);
      });
      this.clip = clip;
      this.time = 0;
      this.blend = snap ? 0 : 1;
    }
    this.slot = slot;
  }

  // A camera motion that drops the arms and brings them back up.
  cue(seconds) {
    if (!this.wasActive) return;
    this.action = null;
    // The landing clip already starts with lowered arms. Crossfade into it
    // instead of teleporting both hands down on the first roll/posture frame.
    this.start("jump_land", this.setFor(this.holding).jump_land);
    this.forced = { slot: "jump_land", left: seconds };
  }

  // Move the bones toward a clip's pose by w of the way.
  apply(clip, time, loop, w) {
    const pose = this.pose;
    pose.clear();
    pose.add(clip, time, 1, loop);
    this.bones.forEach((bone, i) => {
      const weight = pose.w[i];
      if (weight <= 1e-6) return;
      this.target.set(pose.q[i * 4], pose.q[i * 4 + 1], pose.q[i * 4 + 2], pose.q[i * 4 + 3]).normalize();
      this.p.set(pose.t[i * 3] / weight, pose.t[i * 3 + 1] / weight, pose.t[i * 3 + 2] / weight);
      const from = this.transitionFrom[i];
      bone.quaternion.slerpQuaternions(from.quaternion, this.target, w);
      bone.position.lerpVectors(from.position, this.p, w);
    });
  }

  // Where a weapon's fire point is on the first-person model, in the world.
  firePoint(def) {
    const held = this.weapon(def);
    if (!held) return null;
    held.fire.updateWorldMatrix(true, false);
    const at = (this.at ||= { position: new THREE.Vector3(), quaternion: new THREE.Quaternion(), scale: new THREE.Vector3() });
    held.fire.matrixWorld.decompose(at.position, at.quaternion, at.scale);
    return at;
  }

  muzzlePoint(selected, out) {
    if (!this.root.visible) return null;
    const muzzle = this.builtInGuns[selected?.index]?.[selected?.barrel];
    if (!muzzle) return null;
    return muzzle.fire.getWorldPosition(out);
  }

  // The muzzle flash on the first-person gun.
  flash(weapon, selected = null) {
    const held = this.builtInGuns[selected?.index]?.[selected?.barrel] || this.weapon(weapon.def);
    if (!held?.flash) return;
    const d = weapon.def;
    held.flashLength = d.FlashLength || 0.1;
    held.flashLeft = held.flashLength;
    held.flashFresh = true;
    held.flash.visible = held.group.visible;
    const [r, g, b, a = 255] = d.FlashColor || [255, 255, 255, 255];
    held.flashAlpha = a / 255;
    held.flash.userData.material.color.setRGB(r / 255, g / 255, b / 255);
    held.flash.userData.material.opacity = held.flashAlpha;
  }

  // The soldier's light (light regions, LUMI), with the sun as the ray to it
  // finds it.
  light(soldier) {
    for (const material of this.materials) {
      material.uniforms.sunScale.value = soldier.sun * this.sunSeen;
      material.uniforms.brightness.value = soldier.brightness;
      material.uniforms.ambientPull.value.set(soldier.darken, soldier.lighten);
    }
  }

  // shown: the weapon in the hand (null for none); zoomed hides the view
  // model.
  update(dt, { active, camera, soldier, shown, zoomed }) {
    this.root.visible = active && !zoomed;
    if (!active) {
      this.wasActive = false;
      return;
    }
    const v2 = soldier.velocity.x ** 2 + soldier.velocity.z ** 2;
    const air = soldier.air;
    if (air) {
      this.airTime += dt;
      this.impactSpeed = Math.max(this.impactSpeed, -soldier.velocity.y);
    }

    // Entering first person: from handsdown, raised into idle over 1 s.
    if (!this.wasActive) {
      this.wasActive = true;
      this.holding = shown;
      this.action = null;
      this.wasAir = air;
      this.start("handsdown", "_1st_handsdown", true);
      if (this.clip) this.apply(this.clip, 0, true, 1);
      this.forced = { slot: "idle", left: RAISE };
    }
    // Landing: jump_land after a jump, handsdown after a fall (a jetpack
    // flight counts as whichever began it).
    if (this.wasAir && !air && (this.wasAir === "jump" || this.impactSpeed >= 3 || this.airTime > 0.35)) {
      this.forced = this.wasAir === "jump" ? { slot: "jump_land", left: LAND_FORCE } : { slot: "handsdown", left: FALL_FORCE };
    }
    if (!air) this.airTime = this.impactSpeed = 0;
    if (air !== "jet") this.wasAir = air;
    if (this.forced && (this.forced.left -= dt) <= 0) this.forced = null;

    // A switch lowers the arms with the old weapon, then raises the new one.
    // A weapon is its class: the page plays the rounds its weapon prediction
    // fires from that prediction's own copy of the arsenal
    // (src/runtime/weapon-prediction.js), the weapon shown and not a switch.
    if (shown && this.holding !== shown && this.holding?.name === shown.name) this.holding = shown;
    let slot;
    if (shown !== this.holding) {
      slot = "handsdown";
      if (this.slot === "handsdown" && this.blend <= 0) {
        this.holding = shown;
        this.action = null;
      }
    } else if (this.action && this.clip && this.time < this.clip.duration) {
      slot = this.action;
    } else {
      this.action = null;
      if (this.holding?.reloading) slot = "reload";
      else if (this.forced) slot = this.forced.slot;
      else if (air === "jet" && this.slot) slot = this.slot;
      else if (air === "jump" || soldier.dive) slot = "jump";
      else if (air) slot = "flail";
      else if (soldier.posture === "prone" && v2 >= MOVING) slot = "handsdown";
      else slot = v2 >= MOVING ? "run" : "idle";
    }
    const name = slot === "handsdown" ? "_1st_handsdown" : this.setFor(this.holding)[slot];
    if (slot !== this.slot || this.clip?.name !== name) this.start(slot, name);

    // The model in the hand; a thrown grenade has left it.
    const model = this.holding && !this.holding.handEmpty ? this.holding.def.HighResGeometry || this.holding.def.GeometryName : null;
    if (this.holding) this.weapon(this.holding.def);
    for (const [name, held] of this.weapons) held.group.visible = name === model;

    // Blend from a fixed source pose, not recursively from last frame: the
    // latter changed transition speed with FPS and moved even on dt=0 draws.
    // The droideka's shoot runs at twice the rate and loops while it fires.
    this.firing = Math.max((this.firing || 0) - dt, 0);
    if (this.forcedSet && slot === "shoot" && this.firing > 0 && this.clip && this.time >= this.clip.duration) this.time = 0;
    this.time += dt * (slot === "run" ? (this.forcedSet ? 1 : RUN_RATE * Math.min(v2 / 20, 1)) : this.forcedSet && slot === "shoot" ? 2 : 1);
    this.blend = Math.max(this.blend - BLEND_RATE * dt, 0);
    const progress = 1 - this.blend;
    if (this.clip) this.apply(this.clip, this.time, LOOPING.has(this.slot) && air !== "jet", progress * progress * (3 - 2 * progress));
    // The rig's root keeps its rotation but not its lift.
    if (this.rootBone) this.rootBone.position.set(0, 0, 0);

    // Hung from the camera.
    this.root.matrix.multiplyMatrices(camera.matrixWorld, this.offset);
    this.root.updateMatrixWorld(true);

    // In the sun? A ray from the gun toward it.
    if (this.collision) {
      this.hand.getWorldPosition(this.gunAt);
      this.toSun.copy(lighting.sunDirection.value).negate();
      const blocked = this.collision.cast(this.gunAt, this.toSun, SUN_RAY, this.sunHit);
      this.sunSeen += ((blocked ? 0 : 1) - this.sunSeen) * (1 - Math.pow(1 - SUN_EASE, dt * 30));
    }

    for (const held of this.flashes) {
      if (!held.flash) continue;
      if (!held.flashFresh) held.flashLeft -= dt;
      held.flashFresh = false;
      const t = held.flashLeft / Math.max(held.flashLength, 1e-3);
      held.flash.visible = held.flashLeft > 0 && held.group.visible;
      held.flash.userData.material.opacity = held.flashAlpha * Math.min(2 * Math.max(t, 0), 1);
    }
  }
}
