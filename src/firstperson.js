import * as THREE from "three";
import { barrelTips, weaponMuzzle } from "./muzzles.js";
import { lighting } from "./materials.js";
import { modelGroup, partMaterial } from "./models.js";

// Browser shells for first-person arms, weapon models and muzzle resources.
// The native unit phase owns slot selection, pose clocks and blending; its
// attachment phase places this hierarchy with the pre-shake camera record.
// Lighting still writes these material shells after a collision query from
// the native hand position. Combat flash events also target these resources.

const OFFSET = new THREE.Matrix4()
  .makeTranslation(0, 0.13, -0.3645)
  .multiply(new THREE.Matrix4().makeRotationZ(0.05))
  .multiply(new THREE.Matrix4().makeRotationY(3.0));
const FLASH_SCALE = 0.6;

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

}
