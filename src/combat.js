

import { weaponMuzzle } from "./muzzles.js";

import * as THREE from "three";
import { prepareMelee, attachBlade } from "./heroes.js";
import { actorMaterial } from "./materials.js";
import { modelGroup } from "./models.js";
import { motionVectors } from "./motion-vectors.js";
import { HEALTH_SCALES } from "./ordnance.js";
import { Arsenal } from "./weapons.js";
import { targetingWeapon } from "./weapon-targeting.js";
import { CombatWorld } from "./combat-world.js";

// Page presentation for a soldier's published combat state: camera aim,
// held weapons, charge glows, muzzle flashes and HUD. The native owner
// advances weapons, targeting, damage and blasts before publishing mirrors.

// The aim pivot and gun origin per posture: x is the soldier's left (the
// model's +x), y up, z forward.
const PIVOT = { stand: [0.06, 1.7, 0], crouch: [-0.1, 1.2, 0], prone: [0.06, 0.6, 0.4] };
const GUN = { stand: [-0.12, -0.1, 0.25], crouch: [-0.12, -0.05, 0.25], prone: [-0.12, -0.2, 0.15] };
const AIM_REACH = 60; // m
const EYE_BACK = 0.3; // m behind the gun origin in first person
const EYE_BACK_ZOOMED = 0.6;
const PRONE_EYE = -0.12; // m along the soldier's left from the prone pivot
const FLASH_SCALE = 0.6;
// The soldier a blast measures to, by stance (the original game's soldier
// sphere): its centre above the feet and its radius, and the point above the
// feet its line of sight is tested to.
export const BLAST_BODY = { stand: [1.0, 1.4626, 1.4], crouch: [0.75, 1.1023, 0.6], prone: [0.35, 1.622, 0.2] };
// The animation banks a charge doesn't put on alert.
const UNALERTED_BANKS = new Set(["tool", "lightsabre", "sabre", "gam"]);
// The soldiers whose charge presentCharge has on show: the page's own
// drawing, kept off the combat object so a world worker's state never
// carries or overwrites it.
const CHARGE_SHOWN = new WeakSet();
const RETICLE_REACH = 1024; // m along the aim to the point the reticle marks

const color01 = (rgba = [255, 255, 255, 255]) => {
  const a = (rgba[3] ?? 255) / 255;
  return [(rgba[0] / 255) * a, (rgba[1] / 255) * a, (rgba[2] / 255) * a];
};

// A repair lock as a plain key: a building's index (l3) or a vehicle's (v1)
// among what the lock can pick, "" for none; and back.
export function repairLockKey(live, target) {
  if (!live || !target) return "";
  const l = live.buildings.indexOf(target);
  if (l >= 0) return `l${l}`;
  const v = live.vehicleTargets?.indexOf(target) ?? -1;
  return v >= 0 ? `v${v}` : "";
}
export function repairLockTarget(live, key) {
  if (!live || !key) return null;
  const index = Number(key.slice(1));
  return (key[0] === "l" ? live.buildings[index] : key[0] === "v" ? live.vehicleTargets?.[index] : null) ?? null;
}

// `owner` is what its rounds and blasts are credited to (the unit).
export class PlayerCombat {
  constructor({ soldier, def, actor, animator, collision, camera, ordnance, lights, weaponModels, firePoints, flashModel, texture, hud, caster = () => {}, owner = "player", player = true }) {
    this.owner = owner;
    this.player = player;
    // Everything it asks of the rest of the battle (src/combat-world.js).
    this.worldBoundary = new CombatWorld(this);
    this.lastAttacker = undefined; // set by the first damage (one shape, as Unit's)
    this.aimOverride = null; // a bot's filtered aim direction, or null
    this.onDamaged = () => {}; // (amount, attacker, info) after health drops
    this.soldier = soldier;
    this.def = def;
    this.actor = actor;
    this.animator = animator;
    this.collision = collision;
    this.camera = camera;
    this.ordnance = ordnance;
    this.lights = lights;
    this.texture = texture;
    this.hud = hud;
    this.hudWeapons = [null, null];
    // Static turrets share a retained HUD channel view. Rebuilding this
    // wrapper every frame would allocate even while every scalar is steady.
    this.hudTurret = null;
    this.hudTurretArsenal = { current: channel => channel === 0 ? this.hudTurret?.weapon : null };
    this.maxHealth = def.MaxHealth || 300;
    this.health = this.maxHealth;
    this.dead = false;
    this.deathTime = 0;
    this.onRespawn = () => {};
    this.items = null; // where dispensed items go
    this.team = 0;
    this.live = null; // the map's buildings, for repair weapons
    this.remotes = null; // remote droids
    this.glow = null; // the charge-up effect
    this.effects = null;
    this.repairTarget = null;
    this.repairLock = ""; // the lock as a key (repairLockKey), replicated
    this.blowApart = false; // marked by a blast to blow apart at death
    this.chunks = def.chunks?.length && (def.ChunkFrequency ?? -1) > 0 ? def.ChunkFrequency : 0;
    this.zoomStep = 0; // 0: not zoomed; else which of the weapon's zooms
    this.kick = { rate: 0, hold: 0, offset: 0 };
    this.scale = HEALTH_SCALES[(def.HealthType || "person").toLowerCase()] || "PersonScale";
    this.firstPerson = false;
    this.mounted = false; // manning a turret
    this.gunUnderwater = false; // the gun is under water: it won't fire
    this.view = null; // the first-person view, when there is one
    this.deferMuzzles = false;
    this.arsenal = prepareMelee(new Arsenal(def.weapons || [], { player }));

    // The weapons in the hand, one model each, with the muzzle flash at its
    // fire point pointing down the barrel.
    this.held = new Map();
    for (const weapon of this.arsenal.weapons) {
      const name = weapon.def.GeometryName;
      if (!name || this.held.has(name) || !weaponModels.has(name)) continue;
      const group = modelGroup(weaponModels.get(name), (part) => {
        const material = actorMaterial("dynamic", { map: texture(part.texture) });
        actor.materials.push(material);
        return material;
      });
      group.visible = false;
      // Register the held model before adding its luminous muzzle flash.
      // Its bone attachment and hidden first-person layer keep the world
      // weapon's shadow aligned with the body in either camera mode.
      for (const mesh of group.children) {
        mesh.receiveShadow = true;
        caster(mesh);
        // The player's gun moves with the chase camera; bots' guns are a
        // few pixels the upscaler's history clipping handles.
        if (player) motionVectors.track(mesh);
      }
      actor.hand.add(group);
      const point = weaponMuzzle(weaponModels.get(name), firePoints[name], weapon.def);
      let flash = null;
      let muzzle = null;
      if (point) {
        muzzle = new THREE.Object3D();
        muzzle.position.fromArray(point);
        group.add(muzzle);
      }
      if (point && flashModel) {
        flash = flashModel();
        flash.position.fromArray(point);
        flash.scale.setScalar(FLASH_SCALE);
        flash.visible = false;
        group.add(flash);
      }
      this.held.set(name, { group, flash, muzzle, flashLeft: 0, flashLength: 0, flashAlpha: 1 });
      if (weapon.def.base === "melee") attachBlade(group, point, weapon.def, texture);
    }

    this.origin = new THREE.Vector3();
    this.target = new THREE.Vector3();
    this.aimDir = new THREE.Vector3();
    this.shotDir = new THREE.Vector3();
    this.fireDir = new THREE.Vector3();
    this.muzzlePoint = new THREE.Vector3();
    this.side = new THREE.Vector3();
    this.left = new THREE.Vector3();
    this.up = new THREE.Vector3();
    this.pivot = new THREE.Vector3();
    this.from = new THREE.Vector3();
    this.eyePoint = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    this.at = { position: new THREE.Vector3(), quaternion: new THREE.Quaternion(), scale: new THREE.Vector3() };
    this.gunPoint = new THREE.Vector3();
    this.screen = new THREE.Vector3();
    this.worldPoint = new THREE.Vector3();
    this.updateHeld();
  }

  // Everything it asks of the rest of the battle (src/combat-world.js),
  // made on first use where a combat was made without its constructor.
  get world() {
    return (this.worldBoundary ??= new CombatWorld(this));
  }

  // The page capture records the presence of its private prediction services.
  get hasRandom() {
    return Boolean(this.random);
  }

  get hasRemotes() {
    return Boolean(this.remotes);
  }

  // A weapon's zoom factors, from none.
  static zooms(def) {
    const out = [1];
    for (const zoom of [def.ZoomMin || 1, def.ZoomRate ? 0 : def.ZoomMax || 1]) if (zoom > out[out.length - 1]) out.push(zoom);
    return out;
  }

  get zoomed() {
    return this.zoomStep > 0;
  }

  set zoomed(on) {
    if (!on) this.zoomStep = 0;
  }

  // Sniper rifles force the eye view while scoped, even when the player's
  // preferred view is third person. Camera, aim and body visibility share it.
  get usesEyeView() {
    return Boolean(this.firstPerson || (this.zoom > 1 && this.arsenal.current(0)?.def.ZoomFirstPerson));
  }

  // The zoom factor in use (1 when not zoomed).
  get zoom() {
    const weapon = this.arsenal.current(0);
    if (!this.zoomStep || !weapon) return 1;
    return PlayerCombat.zooms(weapon.def)[this.zoomStep] || 1;
  }

  // Back to full health and a fresh loadout.
  reset() {
    this.health = this.maxHealth;
    this.dead = false;
    this.blowApart = false;
    this.zoomed = false;
    this.kick = { rate: 0, hold: 0, offset: 0 };
    this.clearCharge();
    // A new life starts on the first weapon of each channel: carrying the
    // old life's selection over spawned the soldier holding his pistol
    // forever after the first time an empty rifle auto-switched to it.
    this.arsenal = prepareMelee(new Arsenal(this.def.weapons || [], { player: this.player }));
    this.updateHeld();
  }

  // The pose's local aiming frame, without collision queries or gameplay.
  aimBasis(yaw = this.soldier.aimYaw, pitch = this.soldier.aimPitch) {
    const s = this.soldier;
    const posture = s.air ? "stand" : s.posture;
    const forward = this.aimDir.set(Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch));
    this.left.set(Math.cos(yaw), 0, -Math.sin(yaw));
    this.up.crossVectors(forward, this.left);
    const [px, py, pz] = PIVOT[posture] || PIVOT.stand;
    this.pivot.set(
      s.position.x + this.left.x * px + Math.sin(yaw) * pz,
      s.position.y + py,
      s.position.z + this.left.z * px + Math.cos(yaw) * pz,
    );
    const [gx, gy, gz] = GUN[posture] || GUN.stand;
    this.origin.copy(this.pivot).addScaledVector(forward, gz).addScaledVector(this.left, gx).addScaledVector(this.up, gy);
  }

  // The shot direction for a network fire edge, as yaw/pitch: in third
  // person the converged direction from the gun origin to the crosshair
  // ray's target, for the given (preview) body aim. The server has no
  // camera, so sending body aim instead made every round, predicted and
  // authoritative alike, fly parallel to the crosshair line, offset by
  // the gun-to-camera parallax. First person and the droideka's own eye
  // already shoot along the body aim.
  convergedAim(yaw, pitch) {
    // On the page this.camera is the simulation clone, stepped only in the
    // worker; the live render camera (hudCamera) is where the crosshair
    // actually is. In the worker there is no hudCamera and this is unused.
    const view = this.hudCamera || this.camera;
    if (!view || this.usesEyeView || this.soldier.aim) return { aimYaw: yaw, aimPitch: pitch };
    this.aimBasis(yaw, pitch);
    const forward = this.aimDir, posture = this.soldier.air ? "stand" : this.soldier.posture;
    const camera = view.position;
    const behind = this.from.subVectors(camera, this.pivot).dot(forward);
    this.from.copy(camera).addScaledVector(forward, -behind);
    const section = this.def.cameras?.[posture.toUpperCase()] || this.def.cameras?.STAND;
    const ahead = (section?.TrackOffset?.[2] ?? 3.5) + 2;
    this.from.addScaledVector(forward, ahead);
    const dir = this.from.addScaledVector(forward, this.crosshairReach(this.from, forward)).sub(this.origin).normalize();
    const angles = { aimYaw: Math.atan2(dir.x, dir.z), aimPitch: Math.asin(THREE.MathUtils.clamp(dir.y, -1, 1)) };
    this.aimBasis(); // restore the tick's basis from the soldier's own aim
    return angles;
  }

  // How far the crosshair ray reaches (60 m, mask 0x4e): the
  // world and vehicles, and soldiers, the shooter left out. Shots converge
  // on a soldier under the crosshair, his head when it is on his head;
  // casting only the world converged on the ground behind him, so a round
  // aimed at a head 40 m off passed over or under it by a third of the
  // camera's height above the gun.
  crosshairReach(from, forward) {
    const hit = this.world.cast(from, forward, AIM_REACH, this.hit);
    const reach = hit ? hit.distance : AIM_REACH;
    const units = this.ordnance?.units;
    if (!units?.raycast) return reach;
    this.crosshairUnit ||= { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), part: null };
    this.crosshairFilter ||= (unit) => unit.combat !== this && unit.alive;
    const unit = this.world.raycastUnits(from, forward, reach, this.crosshairUnit, this.crosshairFilter);
    return unit ? unit.distance : reach;
  }

  // The first-person eye for the soldier's feet at `feet` (aim() first).
  eye(feet, out) {
    const s = this.soldier;
    if (s.eye) return s.eye(feet, out);
    if (!s.air && s.posture === "prone") out.copy(this.pivot).addScaledVector(this.left, PRONE_EYE);
    else out.copy(this.origin);
    out.sub(s.position).add(feet);
    return out.addScaledVector(this.aimDir, this.zoom > 1 ? -EYE_BACK_ZOOMED : -EYE_BACK);
  }

  // Prediction moves the rendered body ahead of the worker. Its eye and
  // reticle use that pose, not the last packet's origin and aim vectors.
  viewBasis(pose) {
    if (!this.viewAim) {
      this.viewAim = Object.create(this);
      for (const key of ['origin', 'pivot', 'aimDir', 'left', 'up']) this.viewAim[key] = new THREE.Vector3();
    }
    this.viewAim.soldier = pose;
    if (pose.eye) {
      pose.eye(pose.position, this.viewAim.origin);
      const yaw = pose.aimYaw, pitch = pose.aimPitch;
      this.viewAim.aimDir.set(Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch));
    } else this.viewAim.aimBasis();
    return this.viewAim;
  }

  viewEye(feet, out, pose) {
    if (pose.eye) return pose.eye(feet, out);
    return this.viewBasis(pose).eye(feet, out);
  }

  // Which weapon is in the hand: a grenade for the whole throw (its model
  // gone once it leaves the hand), a repair tool at work, otherwise the
  // primary. A page drawing a soldier whose world is elsewhere knows his
  // tool is at work from its ticks (presentRepairTick).
  updateHeld() {
    const thrown = this.arsenal.current(1);
    const weapon = thrown?.busy || thrown?.shownAtWork > 0 ? thrown : this.arsenal.current(0);
    const name = weapon?.def.GeometryName || null;
    const hide = weapon?.handEmpty || this.mounted;
    // The one model in the hand, if any. Every remote body's record calls
    // this; its models are made hidden and shown only here, so they are
    // touched only when that model changes.
    const model = !this.dead && !hide ? name : null;
    if (model !== this.heldModel) {
      for (const [key, held] of this.held) held.group.visible = key === model;
      this.heldModel = model;
    }
    // The weapon in the hand, for the first-person view, and its bank for
    // the body's upper layer.
    this.shown = this.dead || this.mounted ? null : weapon;
    this.animator.setBank?.(weapon?.bank || "rifle");
  }

  // A charge on show, each frame the body is drawn (Unit.present for every
  // soldier, game.js for his own), as the original game draws it: a gun held at
  // a charge keeps the soldier in the alert stance, aim up (but for the tool,
  // sabre and gam banks), where a thrown weapon's windup plays its bank's
  // charge clip; and the weapon's ChargeUpEffect sits at its fire point as
  // drawn (first person, the arms' model), its colour and alpha scaled by
  // the charge. The first-person arms go on with their idle or run.
  // Presentation only: it reads the weapon's charging and held, which the
  // simulation sets, a world worker's page receives with the weapon, and a
  // client online has from the soldier's record (or his own correction,
  // src/runtime/network-entity-game.js), so a bot's charge and another
  // player's show as his own does. dt also runs down how long a repair tool
  // known to be at work only from its ticks stays in the hand.
  presentCharge(dt = 0) {
    const tool = this.arsenal.current(1);
    if (tool?.shownAtWork > 0 && (tool.shownAtWork -= dt) <= 0) this.updateHeld();
    // A repair lock keeps the soldier alert (the world's update does this
    // from its own lock); a page has the lock from the soldier's record.
    if (this.repairLock && !this.dead && !this.mounted && (this.arsenal.current(0)?.repair || tool?.repair)) this.animator.fire?.();
    const weapon = this.dead || this.mounted || this.remotes?.active ? null : this.arsenal.current(0);
    const charging = Boolean(weapon?.charging);
    if (charging !== CHARGE_SHOWN.has(this)) {
      if (charging) CHARGE_SHOWN.add(this);
      else CHARGE_SHOWN.delete(this);
      if (charging && weapon.thrown) this.weaponAction("charge", weapon.bank, this.soldier.posture);
      else if (!charging) this.endWeaponAction("charge");
    }
    if (charging && !weapon.thrown && !UNALERTED_BANKS.has(weapon.bank)) this.animator.holdAlert?.();
    const name = charging && weapon.def.ChargeUpEffect;
    const at = name ? this.firePoint(weapon) : null;
    if (!at) {
      this.glow?.kill();
      this.glow = null;
      return;
    }
    this.glow ||= this.effects?.play(name, at.position, null, null, at.quaternion) || null;
    if (!this.glow) return;
    this.glow.move(at.position, at.quaternion);
    this.glow.tint = Math.min(weapon.held / (weapon.levels[0].MaxPressedTime || 1), 1);
  }

  // Nothing of a charge left on show (a new life, a body taken off the field).
  clearCharge() {
    this.glow?.kill();
    this.glow = null;
    if (CHARGE_SHOWN.delete(this)) this.endWeaponAction("charge");
  }

  weaponAction(action, bank, posture) {
    this.animator.weaponAction(action, bank, posture);
    this.world.presentation({kind:'weaponAction',action,bank,posture,prepare:action.startsWith('shoot')&&this.arsenal.weapons.some(w=>w.bank===bank&&w.thrown&&w.busy)});
  }
  endWeaponAction(action) {
    this.animator.endWeaponAction(action);
    this.world.presentation({kind:'endWeaponAction',action:action??null});
  }
  // The gun origin where the soldier is drawn now (a page's presented pose;
  // its own player's as rendered, first person included), or null.
  drawnGunOrigin(out) {
    const pose = this.owner?.presentation?.state;
    if (!pose || this.dead || this.mounted) return null;
    return out.copy(this.viewBasis(pose).origin);
  }

  // Shared local/replicated projectile presentation. It deliberately contains
  // no recoil, weapon advancement, ordnance creation or damage.
  presentProjectileShot(weapon, gun = null) {
    // The first-person arms play their shot when the round leaves (a throw
    // for the grenade bank).
    if (this.firstPerson) this.view?.play(weapon.bank === "grenade" ? "throw" : "shoot", weapon);
    if (!weapon.thrown) {
      this.animator.weaponAction("shoot", weapon.bank, this.soldier.posture);
      this.animator.fire();
    }
    this.flash(weapon);
    if (this.firstPerson) this.view?.flash(weapon, gun);
  }

  // A repair tick as drawn: the weapon's MuzzleFlashEffect (the fusion
  // cutter's repairkit) at its fire point, on the tool in the hand or, in
  // first person, on the arms' tool. A secondary tool stays in the hand
  // until the interval after the tick has run out (as `firing` holds it
  // where the soldier's world runs), with a little to spare for the next
  // tick's way here.
  presentRepairTick(weapon) {
    if (weapon.channel === 1) {
      weapon.shownAtWork = (weapon.def.ShotDelay || 0) + 0.25;
      this.updateHeld();
    }
    // Each tick is the tool firing: the first-person arms play its bank's
    // shoot clip (the arms' update plays a weapon's shoot slot on
    // the frame it fires, _1st_tool_shoot for the tool set, and takes a
    // weapon that fires into the hand without lowering the arms first), and
    // the body holds the alert stance, the tool raised on the aim.
    if (this.firstPerson) this.view?.play("shoot", weapon);
    this.animator.fire?.();
    const name = weapon.def.MuzzleFlashEffect;
    const at = name ? this.firePoint(weapon) : null;
    if (at) this.effects?.play(name, at.position, null, null, at.quaternion);
  }

  // The repair weapon's lock: this world's, or the one its key names (a page
  // showing a worker's battle never locks on itself).
  lockedTarget() {
    return this.repairTarget || repairLockTarget(this.live, this.repairLock);
  }

  // Where a weapon's fire point is now, in the view the player sees.
  firePoint(weapon) {
    if (this.firstPerson && this.view) return this.view.firePoint(weapon.def);
    const held = this.held.get(weapon.def.GeometryName);
    if (!held?.muzzle) return null;
    held.muzzle.updateWorldMatrix(true, false);
    held.muzzle.matrixWorld.decompose(this.at.position, this.at.quaternion, this.at.scale);
    return this.at;
  }

  // The muzzle flash: the flash mesh for FlashLength, at full alpha for its
  // first half and fading through the second, and a light for
  // FlashLightDuration. Each round restarts its barrel's flash (it sets the
  // barrel's timer): the five rounds of a blast cannon's salvo light the
  // muzzle once, not five lights deep.
  flash(weapon) {
    const held = this.held.get(weapon.def.GeometryName);
    if (!held?.flash) return;
    const d = weapon.def;
    held.flashLength = d.FlashLength || 0.1;
    held.flashLeft = held.flashLength;
    const [r, g, b, a = 255] = d.FlashColor || [255, 255, 255, 255];
    held.flashAlpha = a / 255;
    held.flash.userData.material.color.setRGB(r / 255, g / 255, b / 255);
    if (held.light && held.light.left === held.light.duration) return;
    held.flash.getWorldPosition(this.worldPoint);
    held.light = this.lights.flash(this.worldPoint, color01(d.FlashLightColor), d.FlashLightRadius || 0, d.FlashLightDuration || 0);
  }

  updateFlashes(dt) {
    for (const held of this.held.values()) {
      if (!held.flash) continue;
      held.flashLeft -= dt;
      const t = held.flashLeft / Math.max(held.flashLength, 1e-3);
      held.flash.visible = held.flashLeft > 0 && held.group.visible;
      held.flash.userData.material.opacity = held.flashAlpha * Math.min(2 * Math.max(t, 0), 1);
    }
  }

  // The HUD while flying a droid: its weapons and health, its reticle, and
  // the blaster's scope zoomed in first person.
  droidHud(droid) {
    this.hud.setPlayer(droid.arsenal, droid.health / (droid.def.MaxHealth || 40), droid.def.HealthTexture);
    this.hud.setSecondBar(null);
    const blaster = droid.arsenal.current(0);
    const zoom = this.remotes.zoom;
    this.hud.setScope(zoom > 1 && this.firstPerson ? blaster?.def.ScopeTexture : null);
    const far = this.worldPoint.copy(droid.eye).addScaledVector(droid.aimDir, RETICLE_REACH);
    (this.hudCamera || this.camera).updateMatrixWorld();
    const p = this.screen.copy(far).applyMatrix4((this.hudCamera || this.camera).matrixWorldInverse);
    if (p.z >= 0) {
      this.hud.setReticle(null);
      return;
    }
    const ndcY = p.y / (-p.z * Math.tan(THREE.MathUtils.degToRad((this.hudCamera || this.camera).fov) / 2));
    this.hud.setReticle(0.5 - 0.5 * ndcY, blaster);
  }

  // The HUD: the weapons and health, the reticle at the height of the point
  // 1024 m along the aim, and the scope overlay zoomed in first person.
  // turret: the turret being manned, whose weapon the HUD shows instead.
  updateHud(turret = null, pose = null, aimPreview = null) {
    this.hud.setVehicle(turret?.slot ? turret : null, turret);
    const droid = this.remotes?.piloted;
    if (droid && !turret) {
      this.hud.setHull?.(null);
      this.droidHud(droid);
      return;
    }
    this.hudTurret = turret;
    const arsenal = turret?.arsenal || (turret?.weapon ? this.hudTurretArsenal : this.arsenal);
    // His own health on the top bar, seated or not, with hud_health (a
    // class's HealthTexture is the icon others see it by); what he sits in
    // (a vehicle, from any of its seats, or a turret) on the panel under it.
    this.hud.setPlayer(arsenal, this.health / this.maxHealth);
    this.hud.setHull?.(turret ? turret.desc.settings.HealthTexture || "hud_vehiclehealth" : null, turret ? turret.health / turret.maxHealth : 0);
    // A jetpack's fuel, on foot.
    const s = this.soldier;
    const jet = s.jet;
    const target = this.lockedTarget();
    // A droid serving him: the health bar and the rounds pulse.
    const station = !turret && !this.dead ? this.live?.supplying?.(s.position) : null;
    this.hud.setSupply?.(Boolean(station?.servedHealth), Boolean(station?.servedAmmo));
    if (!turret && !this.dead && target) {
      // A repair weapon at work: its target's health.
      const f = THREE.MathUtils.clamp(target.health / target.maxHealth, 0, 1);
      this.hud.setSecondBar(target.desc.settings.HealthTexture || "hud_active_repair_icon", f, f <= 0.5 ? "low" : "normal");
    } else if (!turret && !this.dead && s.shield) {
      // A droideka's shield.
      const f = s.shield.max > 0 ? s.shield.value / s.shield.max : 0;
      this.hud.setSecondBar("hud_shield_icon", f, f <= 0.25 ? "low" : "normal");
    } else if (!turret && !this.dead && jet.recharge > 0) {
      const low = s.fuel <= jet.minBorder;
      this.hud.setSecondBar("hud_jetpack_icon", s.fuel, low ? (s.air === "jet" ? "pulse" : "low") : "normal");
    } else {
      this.hud.setSecondBar(null);
    }
    const weapon = turret ? targetingWeapon(arsenal) : this.arsenal.current(0);
    const scoped = !turret && this.zoom > 1 && (this.firstPerson || weapon?.def.ZoomFirstPerson);
    this.hud.setScope(scoped ? weapon.def.ScopeTexture : null);
    if (this.dead) {
      this.hud.setReticle(null);
      return;
    }
    const far = this.worldPoint;
    if (turret) far.copy((this.hudCamera || this.camera).position).addScaledVector((aimPreview?.reticleDirection?.(this.side) ?? aimPreview?.viewDirection(this.side) ?? turret.viewDirection(this.side)), RETICLE_REACH);
    else {
      const aim = pose ? this.viewBasis(pose) : this;
      // Eye-view sights follow the eye ray, including the prone eye offset.
      // Projecting a parallel ray from the gun leaves a visible scope offset.
      if (this.usesEyeView) aim.eye(aim.soldier.position, far);
      else far.copy(aim.origin);
      far.addScaledVector(aim.aimDir, RETICLE_REACH);
    }
    (this.hudCamera || this.camera).updateMatrixWorld();
    const p = this.screen.copy(far).applyMatrix4((this.hudCamera || this.camera).matrixWorldInverse);
    if (p.z >= 0) {
      this.hud.setReticle(null);
      return;
    }
    const ndcY = p.y / (-p.z * Math.tan(THREE.MathUtils.degToRad((this.hudCamera || this.camera).fov) / 2));
    const ndcX = p.x / (-p.z * Math.tan(THREE.MathUtils.degToRad((this.hudCamera || this.camera).fov) / 2) * (this.hudCamera || this.camera).aspect);
    // A vehicle seat shows both channels' reticles (src/hud.js drawReticle).
    // A craft's pilot's stays on the screen's centre line, as the
    // original game keeps every reticle, while his chase camera swings after
    // the nose in a turn; the port projects a ground seat's across.
    const weapons = turret?.slot ? this.hudWeapons : null;
    if (weapons) { weapons[0] = arsenal.current(0); weapons[1] = arsenal.current(1); }
    const piloting = turret?.flying && turret.seatIndex === 0;
    this.hud.setReticle(0.5 - 0.5 * ndcY, weapon, turret?.slot && !piloting ? 0.5 + 0.5 * ndcX : 0.5, weapons);
  }
}
