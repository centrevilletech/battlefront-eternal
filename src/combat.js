import {latePresses,readWeaponInput,seedPresses,selectCombatWeapons} from './combat-weapon-input.js';
import { networkShotDirection, networkShotFor, networkShotTarget } from './runtime/network-shot-aim.js';
import { weaponMuzzle } from "./muzzles.js";
import * as THREE from "three";
import { prepareMelee, meleeStrike, regenerate, attachBlade } from "./heroes.js";
import { zeroMaterial } from "./materials.js";
import { modelGroup } from "./models.js";
import { motionVectors } from "./motion-vectors.js";
import { falloff, blastDistance, blastShare, blastBlocked, HEALTH_SCALES } from "./ordnance.js";
import { Arsenal } from "./weapons.js";
import { updateWeaponLock, hasMissileLock, targetingWeapon, botLockTarget } from "./weapon-targeting.js";
import { emitSound } from "./audio/events.js";
import { infantryFireBlocked, damageDirection } from './combat-input.js';

// The player's side of the fight, following the 2004 game: triggers and
// switching, where shots start and where they go, the weapon in the hand and
// its muzzle flash, and what explosions do to the player.
//
// Aim: shots start at a gun origin beside the head, set per posture in the
// aim's frame, and converge on the point under the crosshair. That point is
// found by a ray along the aim, up to 60 m, from where the camera sits level
// with the soldier plus TrackOffset.z + 2 m, so nothing right in front of the
// soldier steers it. The ray meets the world, vehicles and soldiers (mask
// 0x4e), so a soldier under the crosshair is where the shots converge. Grenades leave along that line tipped up by ShotElevate.
//
// In first person the eye sits 0.3 m (0.6 m zoomed) behind the gun origin
// along the aim, or prone behind a point beside the pivot, so shots leave
// from the middle of the screen and go straight along the aim.
//
// Firing is held off in a jump or fall, mid-dive, while getting up, and
// crawling prone; a jetpack hover fires freely. Zoom needs a weapon that
// zooms, and drops while jumping or falling, diving, or reloading. The zoom
// key steps through the weapon's zooms and back out: ZoomMin, then ZoomMax
// when that differs (the sniper rifle's 2x and 8x).
//
// Aim kick (KickStrength, in degrees, KickBuildup): each round adds 10 x
// KickStrength per second to a kick rate that falls off at 20/s, and holds
// it for (1 / KickBuildup + 2) x ShotDelay; while held the aim climbs at that
// rate (a sniper shot lifts it about half the kick). Shots also go up by an
// offset that gathers the rate and fades at 6/s.
//
// Explosions (src/ordnance.js, "What an explosion does"): damage and push
// fall off from the distance to the soldier's blast sphere, its centre 1.0 m
// up and radius 1.46 standing (0.75 and 1.10 crouched, 0.35 and 1.62 prone
// or lying dead), less half that radius; terrain or a building (never a
// soldier or a vehicle) between the blast and a point 1.4 m up (0.6, 0.2)
// blocks both. A push adds (away + 0.6 up) x Push x falloff to the velocity,
// and over 2 m/s up throws the soldier (src/soldier.js). A soldier the blast
// kills is thrown all the same, dying whole, and a body already lying on the
// field is thrown too: in the retail data every pushing explosion's push
// radius lies inside its full-damage radius, which kills any trooper, so it
// is the killed soldier who flies, landing in the thrown rest pose. Damage
// is scaled for what the soldier is (its HealthType: a droid takes the
// DroidScale share).
//
// A repair weapon (the fusion cutter) locks onto the building it would work
// on, each frame, and each tick gives it the weapon's amount for what it is,
// heating the weapon when it gave something, with the weapon's
// MuzzleFlashEffect (repairkit sparks) either way, at the tool's fire point
// (the tick starts it at the weapon's fire matrix). A
// world worker has no posed hand, so the page that draws the soldier plays
// it at the tool as drawn when the tick's shot reaches it
// (presentRepairTick). It plays no shot clip; holding a lock keeps the
// soldier alert. The health bar's second bar shows the target's health (a
// pad's is its build progress); the lock travels as a plain key
// (repairLock) so a page showing a worker's battle has it too.
//
// The arc caster (emitter ordnance) throws lightning on
// release from the gun origin along the shot. It gathers soldiers (droids
// and droidekas too) and mines in a box ConeLength across each way about
// the point 0.9 x ConeLength down the shot (the 2004 game gathers a
// droideka's shield as a target of its own; here the droideka is linked and
// its shield takes the damage first), and keeps those whose origin is closer
// along the shot than ConeLength and inside the cone of half angle
// ConeAngle / 2 about it. The cone is a double cone: nothing requires the
// target to be ahead, so one in the box behind the shooter on the line
// back counts, and comes first. Nearest first along the shot, each link
// must be within ConeLength (the first) or MaxJumpDistance (later ones)
// further along than the last, have a clear line (terrain, buildings,
// vehicles) from where its arc starts to its origin + 1 m, and the first
// that point within FirstRadius of the shot's line. Damage is Damage +
// (MaxDamage - Damage) x charge, the same for every kind of target, less
// DamageDropPerTarget a link; the chain stops below DamageThreshold or at
// MaxTargets. Each link's damage comes from where its arc starts (a shield
// ripples on that side; a player struck sees which side it came from).
// From the second link on, the next arc starts from the same
// point 30% of the time instead of from the last target. With nothing to
// link, one bolt runs to what the shot's line hits within ConeLength. The
// first bolt (or that lone one) is held to the shooter: its start follows
// his gun origin for as long as it lasts. While
// charging, the weapon's ChargeUpEffect glows at its fire point, brighter
// with the charge.
//
// Droids blow apart (a class with CHUNKSECTIONs and a ChunkFrequency): each
// blast that would push one first rolls against ChunkFrequency, and a hit
// marks it to blow apart when it dies and leaves it unpushed. At death it
// rolls again; either way it then goes up in its ExplosionName instead of
// playing a death clip, its pieces flying (see debris.js). A soldier thrown
// through the air dies whole. All of this is the same with either setting of
// Destruction effects: Enhanced (destruction.js) only changes how it looks,
// and also blows apart on screen a droid that plays its death clip here.

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
const SIGHT_POINT = new THREE.Vector3();
// The animation banks a charge doesn't put on alert.
const UNALERTED_BANKS = new Set(["tool", "lightsabre", "sabre", "gam"]);
// The soldiers whose charge presentCharge has on show: the page's own
// drawing, kept off the combat object so a world worker's state never
// carries or overwrites it.
const CHARGE_SHOWN = new WeakSet();
const RESPAWN_DELAY = 4; // s: a Character's respawn timer; the spawn screen opens then
const THROWN_SPEED = 2; // m/s up after a push that throws the soldier
const RETICLE_REACH = 1024; // m along the aim to the point the reticle marks
const DEG = Math.PI / 180;
const KICK_RATE = 10; // kick rate per round, x KickStrength
const KICK_FADE = 20; // 1/s
const KICK_OFFSET_FADE = 6; // 1/s
const KICK_HOLD_DRAIN = 2; // 1/s

const color01 = (rgba = [255, 255, 255, 255]) => {
  const a = (rgba[3] ?? 255) / 255;
  return [(rgba[0] / 255) * a, (rgba[1] / 255) * a, (rgba[2] / 255) * a];
};

// The same code runs a bot's weapons. A bot has no camera or HUD: it fires
// along the direction its controller's aimer gives (aimOverride), or straight
// along its aim, and `player: false` gives its weapons the AI spread rules.
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
    this.maxHealth = def.MaxHealth || 300;
    this.health = this.maxHealth;
    this.dead = false;
    this.deathTime = 0;
    this.onDeath = () => {};
    this.onRespawn = () => {};
    this.onBlowApart = () => {}; // (health at death)
    this.items = null; // where dispensed items go
    this.team = 0;
    this.live = null; // the map's buildings, for repair weapons
    this.remotes = null; // remote droids
    this.admitRemoteLaunch = weapon => this.remotes?.admitLaunch?.(weapon) ?? true;
    this.lightning = null; // the lightning bolts the arc caster throws
    this.glow = null; // the charge-up effect
    this.arcTargets = () => []; // what lightning can link (owner): { origin, hurt(amount) }
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
    this.pendingMuzzles = [];
    this.arsenal = prepareMelee(new Arsenal(def.weapons || [], { player }));
    this.seen = new Map(); // weapon -> { reloading, throwing } last frame

    // The weapons in the hand, one model each, with the muzzle flash at its
    // fire point pointing down the barrel.
    this.held = new Map();
    for (const weapon of this.arsenal.weapons) {
      const name = weapon.def.GeometryName;
      if (!name || this.held.has(name) || !weaponModels.has(name)) continue;
      const group = modelGroup(weaponModels.get(name), (part) => {
        const material = zeroMaterial("dynamic", { map: texture(part.texture) });
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
    this.seen.clear();
    this.pendingMuzzles.length = 0;
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

  // The yaw/pitch of the shot direction as last computed by aim().
  shotAngles() {
    return { aimYaw: Math.atan2(this.shotDir.x, this.shotDir.z), aimPitch: Math.asin(THREE.MathUtils.clamp(this.shotDir.y, -1, 1)) };
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

  // The gun origin and the converged aim point for the soldier as it stands.
  aim() {
    const s = this.soldier;
    // A droideka aims from its own eye.
    if (s.aim) {
      s.aim(this);
      return;
    }
    this.aimBasis();
    const forward = this.aimDir;
    const posture = s.air ? "stand" : s.posture;

    // A bot (no camera) fires from the gun origin along its aimer's
    // direction, or straight along its aim.
    if (!this.camera) {
      this.shotDir.copy(this.aimOverride || forward).normalize();
      this.target.copy(this.origin).addScaledVector(this.shotDir, AIM_REACH);
      return;
    }
    // In first person shots go straight along the aim.
    if (this.usesEyeView) {
      const eye = this.eye(s.position, this.eyePoint);
      const hit = this.collision.cast(eye, forward, AIM_REACH, this.hit);
      this.target.copy(eye).addScaledVector(forward, hit ? hit.distance : AIM_REACH);
      this.shotDir.copy(forward);
      return;
    }
    // The crosshair ray: from the camera brought level with the pivot.
    const camera = this.camera.position;
    const behind = this.from.subVectors(camera, this.pivot).dot(forward);
    this.from.copy(camera).addScaledVector(forward, -behind);
    const section = this.def.cameras?.[posture.toUpperCase()] || this.def.cameras?.STAND;
    const ahead = (section?.TrackOffset?.[2] ?? 3.5) + 2;
    this.from.addScaledVector(forward, ahead);
    this.target.copy(this.from).addScaledVector(forward, this.crosshairReach(this.from, forward));
    this.shotDir.subVectors(this.target, this.origin).normalize();
  }

  // How far the crosshair ray reaches (60 m, mask 0x4e): the
  // world and vehicles, and soldiers, the shooter left out. Shots converge
  // on a soldier under the crosshair, his head when it is on his head;
  // casting only the world converged on the ground behind him, so a round
  // aimed at a head 40 m off passed over or under it by a third of the
  // camera's height above the gun.
  crosshairReach(from, forward) {
    const hit = this.collision.cast(from, forward, AIM_REACH, this.hit);
    const reach = hit ? hit.distance : AIM_REACH;
    const units = this.ordnance?.units;
    if (!units?.raycast) return reach;
    this.crosshairUnit ||= { unit: null, distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), part: null };
    this.crosshairFilter ||= (unit) => unit.combat !== this && unit.alive;
    const unit = units.raycast(from, forward, reach, null, this.crosshairUnit, this.crosshairFilter, true);
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
    for (const [model, held] of this.held) held.group.visible = !this.dead && !hide && model === name;
    // The weapon in the hand, for the first-person view, and its bank for
    // the body's upper layer.
    this.shown = this.dead || this.mounted ? null : weapon;
    this.animator.setBank?.(weapon?.bank || "rifle");
  }

  update(dt, controls, stick) {
    const s = this.soldier;
    regenerate(this, dt);
    // Manning a turret, the soldier's own weapons wait.
    if (this.mounted) {
      for (const weapon of this.arsenal.weapons) weapon.clearLock?.();
      this.aim();
      this.updateHeld();
      this.updateFlashes(dt);
      return;
    }
    if (this.dead) {
      for (const weapon of this.arsenal.weapons) weapon.clearLock?.();
      this.deathTime += dt;
      for (const name of ["firePrimary", "fireSecondary", "reload", "nextPrimary", "previousPrimary", "nextSecondary", "zoom"]) controls.take(name);
      if (this.deathTime >= RESPAWN_DELAY) this.onRespawn();
      this.updateFlashes(dt);
      return;
    }
    // Flying a remote droid, the soldier does nothing.
    if (this.remotes?.active) {
      for (const weapon of this.arsenal.weapons) weapon.clearLock?.();
      this.aim();
      this.updateHeld();
      this.updateFlashes(dt);
      return;
    }
    // A press the page predicted (its trigger edge's id) draws its rounds'
    // spread from the press's seed, as the page's copies did.
    const input = latePresses(seedPresses(readWeaponInput(controls), this.owner, [0, 1].map((channel) => networkShotFor(this.owner, channel)?.id)),
      [0, 1].map((channel) => networkShotFor(this.owner, channel)?.late));
    // A remote weapon whose droid is out: fire flies it again, reload blows
    // it up (AllowDestruct).
    for (const weapon of this.arsenal.weapons) {
      if (!weapon.def.droid) continue;
      weapon.admitShot = this.admitRemoteLaunch;
      const droid = this.remotes?.of(weapon) || null;
      weapon.deployed = Boolean(droid);
      if (!droid || this.arsenal.current(weapon.channel) !== weapon) continue;
      const trigger = weapon.channel === 0 ? input.primary : input.secondary;
      if (trigger.pressed) {
        this.remotes.enter(droid);
        trigger.pressed = false;
        trigger.held = false;
      } else if (input.reload && weapon.def.AllowDestruct !== 0) {
        this.remotes.kill(droid);
        input.reload = false;
      }
    }
    const primary = selectCombatWeapons(this, controls, PlayerCombat.zooms);

    const blocked = infantryFireBlocked(s, this.gunUnderwater, stick);
    // A droideka's shield goes up or down with each press of its trigger.
    const guard = this.arsenal.current(1);
    if (guard?.def.base === "shield") {
      if (input.secondary.pressed && !blocked) s.toggleShield?.();
      input.secondary = { held: false, pressed: false };
    }
    this.aim();
    // A repair weapon's lock, the trigger held widening it.
    const fixer = [this.arsenal.current(0), this.arsenal.current(1)].find((w) => w?.repair);
    const fixHeld = fixer && (fixer.channel === 0 ? input.primary.held : input.secondary.held);
    this.repairTarget = fixer && this.live ? this.live.lockOn(this.origin, this.aimDir, fixer.def, this.repairTarget, fixHeld, this.team) : null;
    this.repairLock = repairLockKey(this.live, this.repairTarget);
    if (this.repairTarget) this.animator.fire();
    for (const channel of [0, 1]) {
      const weapon = this.arsenal.current(channel);
      if (!weapon || !hasMissileLock(weapon.def)) continue;
      // The cone runs from the gun through the point under the reticle
      // (aim(): shotDir).
      updateWeaponLock(weapon, dt, { units: this.ordnance?.units, collision: this.collision,
        origin: this.origin, dir: this.shotDir, viewer: this.owner, target: botLockTarget(this.owner) });
    }
    const shots = this.arsenal.update(dt, input, { stick, posture: s.posture, zoomed: this.zoomed, blocked, repairTarget: this.repairTarget });
    // Enter the grenade bank before starting its throw: changing banks clears
    // the old weapon action, and must not erase the action we just started.
    this.updateHeld();

    // Reloads of the gun in hand and throws play their clips over the upper
    // body, for as long as the weapon stays in that state (charging is shown
    // by presentCharge). A throw plays shoot3 above 0.7 of a charge, shoot2
    // above 0.4, else shoot.
    for (const weapon of this.arsenal.weapons) {
      const was = this.seen.get(weapon) || { reloading: false, throwing: false };
      if (weapon === primary) {
        if (weapon.reloading && !was.reloading) this.weaponAction("reload", weapon.bank, s.posture);
        if (!weapon.reloading && was.reloading) this.endWeaponAction("reload");
      }
      if (weapon.thrown && weapon.busy && !was.throwing) {
        // A dispenser plays its shoot clip (FireAnim 0).
        let action = weapon.charge > 0.7 ? "shoot3" : weapon.charge > 0.4 ? "shoot2" : "shoot";
        if (weapon.def.base === "dispenser") action = "shoot";
        this.weaponAction(action, weapon.bank, s.posture);
        if (this.firstPerson) this.view?.play("prepareThrow", weapon);
        this.animator.fire();
      }
      this.seen.set(weapon, { reloading: weapon.reloading, throwing: weapon.busy });
    }
    for (const shot of shots) this.fire(shot);
    this.updateKick(dt);
    this.updateHeld();
    this.updateFlashes(dt);
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

  // The aim kick: the aim climbs while the kick holds, and the rate fades.
  updateKick(dt) {
    const k = this.kick;
    if (!k.rate && !k.offset && !k.hold) return;
    const s = this.soldier;
    s.aimPitch += Math.min(k.hold, 1) * k.rate * dt;
    const [low, high] = s.pitchRange();
    s.aimPitch = THREE.MathUtils.clamp(s.aimPitch, low, high);
    k.offset = (k.offset + k.rate * dt) * (1 - Math.min(KICK_OFFSET_FADE * dt, 1));
    k.rate *= 1 - Math.min(KICK_FADE * dt, 1);
    k.hold = Math.max(k.hold - KICK_HOLD_DRAIN * dt, 0);
    if (Math.abs(k.rate) < 1e-6 && Math.abs(k.offset) < 1e-6 && !k.hold) k.rate = k.offset = 0;
  }

  weaponAction(action, bank, posture) {
    this.animator.weaponAction(action, bank, posture);
    this.onPresentation?.({kind:'weaponAction',action,bank,posture,prepare:action.startsWith('shoot')&&this.arsenal.weapons.some(w=>w.bank===bank&&w.thrown&&w.busy)});
  }
  endWeaponAction(action) {
    this.animator.endWeaponAction(action);
    this.onPresentation?.({kind:'endWeaponAction',action:action??null});
  }
  // One bolt of the arc caster's lightning, drawn where this world is drawn
  // and sent to the pages that draw it. `held`, the weapon's channel, marks
  // the bolt held to the shooter: its start follows his gun origin as drawn.
  lightningBolt(name, def, from, to, charge, held = null) {
    this.lightning?.spawn(def, from, to, charge, held === null ? null : (out) => this.drawnGunOrigin(out));
    this.onPresentation?.({ kind: 'lightning', name, from: from.toArray(), to: to.toArray(), charge, ...(held === null ? {} : { channel: held }) });
  }
  // The gun origin where the soldier is drawn now (a page's presented pose;
  // its own player's as rendered, first person included), or null.
  drawnGunOrigin(out) {
    const pose = this.owner?.presentation?.state;
    if (!pose || this.dead || this.mounted) return null;
    return out.copy(this.viewBasis(pose).origin);
  }
  fire(shot) {
    if (shot.meleeEnd) {
      this.endWeaponAction();
      return;
    }
    if (shot.meleeHit) {
      meleeStrike(this);
      return;
    }
    emitSound(this.player ? "fire" : "actorFire", this, shot);
    this.onFired?.(shot);
    const weapon = shot.weapon;
    if (shot.meleeStart) {
      this.weaponAction(shot.swing === 1 ? "shoot" : `shoot${shot.swing}`, weapon.bank, this.soldier.posture);
      this.animator.fire();
      return;
    }
    if (shot.repair) {
      this.repairTick(weapon);
      return;
    }
    if (weapon.def.item) {
      this.dispense(shot);
      return;
    }
    if (weapon.def.droid) {
      // A remote droid leaves the gun origin along the aim at Velocity, plus
      // the soldier's own velocity.
      const speed = (weapon.def.Velocity ?? 10) * (weapon.def.MaxStrength ?? 1);
      const velocity = this.fireDir.copy(this.shotDir).multiplyScalar(speed).add(this.soldier.velocity);
      this.remotes?.launch(weapon, this.origin, velocity, this.soldier.aimYaw, this.team, shot.launchAdmission);
      if (this.firstPerson) this.view?.play("throw", weapon);
      return;
    }
    const ordnance = weapon.def.ordnance;
    if (!ordnance) return;
    if (ordnance.base === "emitterordnance") {
      this.arc(shot, ordnance);
      this.presentProjectileShot(weapon);
      return;
    }
    // Collision starts at the gameplay gun origin. Resolve the visible
    // muzzle after presentation has posed the wrist/weapon and camera;
    // a headless worker's actor transform is not a rendered muzzle.
    const dir = this.fireDir.copy(this.shotDir);
    // A droideka's guns take turns, each firing at the aim point.
    let from = this.origin;
    const gun = this.soldier.nextGun?.(this.gunPoint);
    if (gun) {
      from = gun;
      dir.subVectors(this.target, gun).normalize();
      // A close surface between the eye and barrels cannot make a shot turn
      // back through the droideka. Keep firing into the forward hemisphere.
      if (dir.dot(this.aimDir) <= 0) dir.copy(this.aimDir);
    }
    networkShotDirection(this.owner,dir,weapon.channel);
    // The spread (and a pattern's offset, and the kick): turn about the
    // vertical, then tip about the horizontal.
    dir.applyAxisAngle(THREE.Object3D.DEFAULT_UP, shot.yaw);
    const side = this.side.set(dir.z, 0, -dir.x).normalize();
    dir.applyAxisAngle(side, -(shot.pitch + this.kick.offset));
    if (weapon.def.ShotElevate) {
      dir.y += weapon.def.ShotElevate;
      dir.normalize();
    }
    if (gun && this.soldier.gunAt) {
      const payload = {
        shot, selected: { ...this.soldier.lastGun }, target: this.target.clone(),
        forward: this.aimDir.clone(), kick: this.kick.offset,
      };
      if (this.deferMuzzles) this.pendingMuzzles.push(payload);
      else this.emitGunRound(payload);
    } else {
      const round = this.ordnance.fire(ordnance, from, dir, this.owner, null, null, null, shot);
      if (round && !weapon.thrown) this.bindRoundMuzzle(round, weapon);
    }
    // Each round kicks the aim.
    if (weapon.def.KickStrength) {
      this.kick.rate += KICK_RATE * weapon.def.KickStrength * DEG;
      this.kick.hold += (1 / (weapon.def.KickBuildup || 2) + 2) * weapon.def.ShotDelay;
    }
    this.presentProjectileShot(weapon, gun ? this.soldier.lastGun : null);
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

  // Emit after the fixed tick has sampled the physical built-in gun rig.
  // Rendering must never decide when a projectile enters the world.
  flushMuzzles() {
    for (const payload of this.pendingMuzzles) this.emitGunRound(payload);
    this.pendingMuzzles.length = 0;
  }

  emitGunRound({ shot, selected, target, forward, kick }) {
    const from = this.soldier.gunAt(this.gunPoint, selected);
    if (!from) return;
    networkShotTarget(this.owner, this.origin, target, shot.weapon.channel);
    const dir = this.fireDir.subVectors(target, from).normalize();
    if (dir.dot(forward) <= 0) dir.copy(forward);
    dir.applyAxisAngle(THREE.Object3D.DEFAULT_UP, shot.yaw);
    this.side.set(dir.z, 0, -dir.x).normalize();
    dir.applyAxisAngle(this.side, -(shot.pitch + kick));
    if (shot.weapon.def.ShotElevate) { dir.y += shot.weapon.def.ShotElevate; dir.normalize(); }
    // The hidden world rig supplies physical collision, while the visible
    // first-person barrels supply the beam's start in the rendered view.
    const round = this.ordnance.fire(shot.weapon.def.ordnance, from, dir, this.owner, null, null, null, shot);
    if (round) this.bindRoundMuzzle(round, shot.weapon, selected);
  }

  bindRoundMuzzle(round, weapon, selected = null) {
    // These identities cross the worker boundary; the closure does not.
    round.weapon = weapon;
    round.muzzleGeneration = this.owner?.generation ?? null;
    round.muzzleIndex = selected?.index ?? -1;
    round.muzzleBarrel = selected?.barrel ?? -1;
    round.visualMuzzle = () => this.roundMuzzle(round);
  }

  roundMuzzle(round) {
    if (round.muzzleGeneration != null && this.owner?.generation !== round.muzzleGeneration) return null;
    if (round.muzzleIndex >= 0) {
      const selected = { index: round.muzzleIndex, barrel: round.muzzleBarrel };
      return this.firstPerson && this.view
        ? this.view.muzzlePoint?.(selected, this.muzzlePoint)
        : this.soldier.visualGunAt?.(this.muzzlePoint, selected);
    }
    return this.firePoint(round.weapon)?.position ?? null;
  }

  // The arc caster's shot (see the top of the file). It starts at
  // the gun origin, never at a held model's muzzle: a world worker's body is
  // never posed, so its hand stays where it last stood.
  arc(shot, def) {
    const charge = shot.charge ?? 1;
    const channel = shot.weapon?.channel ?? 0;
    const P = this.origin.clone();
    // Along the shot (the aimer's direction): converged on the crosshair in
    // third person, a network player's own aim at the trigger.
    const D = networkShotDirection(this.owner, this.shotDir.clone(), channel).normalize();
    const L = def.ConeLength ?? 10;
    const sine = Math.sin((((def.ConeAngle ?? 60) / 2) * Math.PI) / 180);
    // The gathering box: ConeLength each way about 0.9 x ConeLength down the
    // shot, against each target's bounds.
    const C = P.clone().addScaledVector(D, 0.9 * L);
    const toward = new THREE.Vector3();
    const candidates = [];
    for (const target of this.arcTargets(this.owner)) {
      const c = target.center ?? target.origin;
      const r = L + (target.radius ?? 0);
      if (Math.abs(c.x - C.x) > r || Math.abs(c.y - C.y) > r || Math.abs(c.z - C.z) > r) continue;
      toward.subVectors(target.origin, P);
      const a = toward.dot(D);
      if (!(a < L)) continue;
      // Inside the cone about the shot's line, either way along it.
      if (!(toward.addScaledVector(D, -a).lengthSq() < (sine * a) ** 2)) continue;
      candidates.push({ target, a });
    }
    candidates.sort((x, y) => x.a - y.a);
    const bolt = this.effects?.lightning?.[def.LightningEffect];
    let damage = (def.Damage ?? 0) + ((def.MaxDamage ?? 0) - (def.Damage ?? 0)) * charge;
    const from = P.clone();
    const T = new THREE.Vector3();
    const dir = new THREE.Vector3();
    let prev = 0;
    let links = 0;
    for (const { target, a } of candidates) {
      if (links >= (def.MaxTargets ?? 5) || damage < (def.DamageThreshold ?? 0)) break;
      if (a - prev > (links === 0 ? L : def.MaxJumpDistance ?? 10)) break;
      T.copy(target.origin);
      T.y += 1;
      // Blocked from where its arc starts, or (the first) its point too far
      // off the shot's line.
      const span = dir.subVectors(T, from).length();
      if (span > 1e-3 && this.collision.cast(from, dir.divideScalar(span), span, this.hit)) continue;
      if (links === 0 && def.FirstRadius > 0) {
        toward.subVectors(T, P);
        if (toward.addScaledVector(D, -toward.dot(D)).lengthSq() > def.FirstRadius ** 2) continue;
      }
      prev = a;
      if (damage > 0) target.hurt(damage, from.clone());
      if (bolt) {
        this.lightningBolt(def.LightningEffect, bolt, from, T, charge, links === 0 ? channel : null);
        if (links === 0 || (this.random || Math.random)() > 0.3) from.copy(T);
      }
      links += 1;
      damage -= def.DamageDropPerTarget ?? 0;
    }
    if (!links && bolt) {
      const hit = this.collision.cast(P, D, L, this.hit);
      this.lightningBolt(def.LightningEffect, bolt, P, T.copy(P).addScaledVector(D, hit ? hit.distance : L), charge, channel);
    }
  }

  // One tick of a repair weapon: the target's health raised. Its sparks are
  // the page's (presentRepairTick), when the tick's shot reaches it.
  repairTick(weapon) {
    const d = weapon.def;
    if (this.live?.repair(d, this.repairTarget, this.team)) weapon.heat += d.HeatPerShot || 0;
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

  // A dispenser's throw: its item leaves the gun origin along the aim at
  // Velocity x lerp(MinStrength, MaxStrength, charge), plus the soldier's own
  // velocity. Nothing spent if there was no room for it.
  dispense(shot) {
    const weapon = shot.weapon;
    const d = weapon.def;
    const strength = (d.MinStrength ?? 0.3) + ((d.MaxStrength ?? 1) - (d.MinStrength ?? 0.3)) * (shot.charge ?? 1);
    const velocity = this.fireDir.copy(this.shotDir).multiplyScalar((d.Velocity ?? 10) * strength).add(this.soldier.velocity);
    const made = this.items?.dispense(d.class, d.MaxItems || 0, d.item, this.origin, velocity, this.soldier.aimYaw, this.owner, this.team);
    if (!made) {
      if (!weapon.bottomless) weapon.clip = Math.min(weapon.clip + weapon.perShot, 1);
      weapon.heat = Math.max(weapon.heat - (d.HeatPerShot || 0), 0);
    }
    if (this.firstPerson) this.view?.play("throw", weapon);
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

  // An explosion at `position`: damage and push on the soldier (the camera's
  // shake is the page's: src/camera-shake.js). `owner` is credited with any
  // damage; `scale` is the battle's friendly fire factor for it (the push
  // doesn't take it). A soldier the
  // blast kills is thrown all the same; a body already dead takes the push
  // alone, so a corpse lying on the field is tossed like anyone else.
  explode(explosion, position, owner = null, scale = 1) {
    const s = this.soldier;
    const wasDead = this.dead;
    // Nothing is left to throw of a droid that went up in pieces, and a
    // droideka's hull never takes the infantry launch impulse.
    if (wasDead && (this.blowApart || this.def.base === "walkerdroid")) return;
    // His blast sphere and sight point by stance (a body lies prone, a
    // soldier in the air stands).
    const [middle, radius, sight] = BLAST_BODY[wasDead ? "prone" : s.air ? "stand" : s.posture] || BLAST_BODY.stand;
    const center = this.worldPoint.copy(s.position);
    center.y += middle;
    const away = new THREE.Vector3().subVectors(center, position);
    const span = away.length();
    const distance = blastDistance(position, center, radius);
    // Terrain or a building between the blast and his sight point shields him.
    if (blastBlocked(this.collision, position, SIGHT_POINT.set(s.position.x, s.position.y + sight, s.position.z))) return;
    if (span > 1e-3) away.divideScalar(span);
    // The push is rolled before the damage: a droid the blast would push
    // may be marked to blow apart at its death instead, and is left
    // unpushed by this blast. A turret's gunner (the only one seated a
    // blast reaches: SeatLease.blastable) is never pushed while he mans
    // it, as the 2004 game's push skips a soldier in a seat.
    let push = (explosion.Push || 0) * falloff(distance, explosion.PushRadiusInner || 0, explosion.PushRadiusOuter || 0);
    if (push > 0 && this.chunks && !wasDead && !this.mounted && s.air !== "thrown" && (this.random || Math.random)() <= this.chunks) {
      this.blowApart = true;
      push = 0;
    }
    if (!wasDead) {
      const damage = scale * (explosion.Damage || 0) * blastShare(explosion, distance);
      const info = { kind: "explosion", amount: damage, typed: explosion[this.scale] ?? 1, generic: explosion.ShieldScale ?? 1, known: false, point: null };
      this.damage(damage * info.typed, damage * info.generic, position, owner, info);
      // The HUD shows which side it came from (positive to the right).
      if (damage > 0 && this.hud) {
        const toward = this.from.subVectors(position, center);
        const yaw = s.aimYaw;
        this.hud.damageFrom(damageDirection(toward.x, toward.z, yaw));
      }
    }
    // Thrown whole, alive or killed by this blast; a droid that blew apart
    // at its death has no body left. Droidekas take blast damage but never
    // use the infantry launch impulse. A gunner it killed has left his gun
    // by now (src/boarding.js leaveKilled), and his body is thrown too.
    if (push > 0 && !this.mounted && !(this.dead && this.blowApart) && this.def.base !== "walkerdroid") {
      s.velocity.addScaledVector(away, push);
      s.velocity.y += 0.6 * push;
      if (s.velocity.y > THROWN_SPEED && s.air !== "thrown") {
        s.air = "thrown";
        s.stateTime = 0;
        s.grounded = false;
      }
    }
  }

  // amount: the damage to health; onShield: what the same attack does to a
  // shield (a droideka's takes it first). attacker: who is credited. info:
  // the hit as the AI's alerts read it ({ amount: the packet's base damage,
  // typed and generic: its health-type and shield scales, known, point }).
  damage(amount, onShield = amount, source = null, attacker = null, info = null) {
    if (this.dead || amount <= 0) return;
    if (this.soldier.absorb) amount = this.soldier.absorb(amount, onShield, source, info?.kind);
    if (amount <= 0) return;
    this.health -= amount;
    if (this.player) emitSound(this.health > 0 ? "hurt" : "death", this, amount);
    else emitSound(this.health > 0 ? "actorHurt" : "actorDeath", this, amount);
    this.lastAttacker = attacker;
    this.onDamaged(amount, attacker, { ...info, source });
    if (this.health <= 0) {
      const left = this.health;
      this.health = 0;
      this.dead = true;
      this.deathTime = 0;
      this.zoomed = false;
      const apart = this.chunks && this.soldier.air !== "thrown" && (this.blowApart || (this.random || Math.random)() <= this.chunks);
      // Dead, the mark records what became of the body: a blast reaching
      // it later finds whether anything is left to throw.
      this.blowApart = Boolean(apart);
      if (apart) this.onBlowApart(left);
      else this.animator.die();
      this.updateHeld();
      this.onDeath();
    }
  }

  // Melee invokes the defender's selected-weapon block hook. Stock cloned
  // sabers use effective CanDeflect=0; explicit custom overrides can block
  // melee. Projectile rebound is a separate, not yet wired ordnance hook.
  tryMeleeBlock(direction, point) {
    if (this.dead || this.mounted) return false;
    const weapon = this.arsenal.current(0);
    if (weapon?.def.base !== "melee" || !Number(weapon.def.CanDeflect)) return false;
    this.effects?.play?.("lightsabre_deflect", point);
    this.onPresentation?.({kind:'effect',name:'lightsabre_deflect',position:point.toArray()});
    return true;
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
    const arsenal = turret?.arsenal || (turret?.weapon ? { current: (channel) => (channel === 0 ? turret.weapon : null) } : this.arsenal);
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
    const weapons = turret?.slot ? [arsenal.current(0), arsenal.current(1)] : null;
    const piloting = turret?.flying && turret.seatIndex === 0;
    this.hud.setReticle(0.5 - 0.5 * ndcY, weapon, turret?.slot && !piloting ? 0.5 + 0.5 * ndcX : 0.5, weapons);
  }
}
