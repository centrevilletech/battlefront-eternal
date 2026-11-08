import {sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";


import { weaponMuzzle } from "./muzzles.js";

import * as THREE from "three";
import { prepareMelee, attachBlade } from "./heroes.js";
import { actorMaterial } from "./materials.js";
import { modelGroup } from "./models.js";
import { motionVectors } from "./motion-vectors.js";
import { HEALTH_SCALES } from "./ordnance.js";
import { Arsenal } from "./weapons.js";
import {clientExports as x} from "./runtime/fixed-clock.js";
import {CombatViewLayout as C} from "./sim/gen/combat-view.js";
import {PageTargetFrameLayout as F} from "./sim/gen/page-targeting.js";
import {targetFrame,runTargetFrame,prepareTargeting} from "./targeting-facts.js";
import { targetingWeapon } from "./weapon-targeting.js";
import { CombatWorld } from "./combat-world.js";

// Page presentation for a soldier's published combat state: camera aim,
// held weapons, charge glows, muzzle flashes and HUD. The native owner
// advances weapons, targeting, damage and blasts before publishing mirrors.

const AIM_REACH = 60; // m: the existing crosshair collision query
const ZOOM_LEVELS=new WeakMap();
const FLASH_SCALE = 0.6;
// The soldier a blast measures to, by stance (the original game's soldier
// sphere): its centre above the feet and its radius, and the point above the
// feet its line of sight is tested to.
export const BLAST_BODY = { stand: [1.0, 1.4626, 1.4], crouch: [0.75, 1.1023, 0.6], prone: [0.35, 1.622, 0.2] };
// The animation banks a charge doesn't put on alert.
const UNALERTED_BANKS = new Set(["tool", "lightsabre", "sabre", "gam"]);
// Private native decisions are keyed by the combat resource identity. Cold
// admission owns the fixed record; a frame only refreshes detached views.
const views=new WeakMap();
const viewFinalizers=new FinalizationRegistry(handle=>x.combat_view_drop(handle));
function view(combat){
  let state=views.get(combat);
  if(!state){const handle=x.combat_view_create();state={handle,pointer:x.combat_view_words(handle),words:null};views.set(combat,state);viewFinalizers.register(combat,handle);}
  if(!state.words||state.words.length===0)state.words=new Float64Array(x.memory.buffer,state.pointer,C.words);
  return state;
}
function decide(combat,op){const state=view(combat);x.combat_view_run(state.handle,op);return view(combat).words;}
const zoomView={};
const WHITE=[255,255,255,255],BAR_LOOKS=['normal','low','pulse'];
function projection(origin,direction,camera){
  camera.updateMatrixWorld();const f=targetFrame();
  f[F.origin]=origin.x;f[F.origin+1]=origin.y;f[F.origin+2]=origin.z;
  f[F.direction]=direction.x;f[F.direction+1]=direction.y;f[F.direction+2]=direction.z;
  const matrix=camera.matrixWorldInverse.elements;for(let i=0;i<16;i++)f[F.matrix+i]=matrix[i];
  f[F.fov]=camera.fov;f[F.aspect]=camera.aspect;return runTargetFrame(3);
}

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
    prepareTargeting(ordnance?.units);
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
    // Zoom levels are immutable class data. Resolve every charge level while
    // cold so a displayed frame never builds the zoom array on first use.
    for(const weapon of this.arsenal.weapons)for(const level of weapon.levels)PlayerCombat.zooms(level);

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
      sceneProperty(group,"visible",false);
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
        sceneTransform(muzzle.position.fromArray(point));
        group.add(muzzle);
      }
      if (point && flashModel) {
        flash = flashModel();
        sceneTransform(flash.position.fromArray(point));
        sceneTransform(flash.scale.setScalar(FLASH_SCALE));
        sceneProperty(flash,"visible",false);
        group.add(flash);
      }
      this.held.set(name, { group, flash, muzzle, flashLeft: 0, flashLength: 0, flashAlpha: 1, lightRGB: [0,0,0] });
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
    // Unit presentation publishes this displayed basis alongside its pose.
    // Gameplay origins retain their separate tick/predicted-shot clock.
    this.viewAim={origin:new THREE.Vector3(),pivot:new THREE.Vector3(),left:new THREE.Vector3(),up:new THREE.Vector3(),aimDir:new THREE.Vector3(),eyePoint:new THREE.Vector3()};
    this.crosshairUnit={unit:null,distance:0,point:new THREE.Vector3(),normal:new THREE.Vector3(),part:null};
    this.crosshairFilter=unit=>unit.combat!==this&&unit.alive;
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
    let out=ZOOM_LEVELS.get(def);
    if(out)return out;
    const f=view(zoomView).words;f[C.zoomMin]=def.ZoomMin||1;f[C.zoomMax]=def.ZoomMax||1;f[C.zoomRate]=def.ZoomRate||0;decide(zoomView,6);
    out=[];for(let i=0;i<f[C.zoomCount];i++)out.push(f[C.zooms+i]);
    ZOOM_LEVELS.set(def,out);return out;
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
    const zoom=this.zoom,f=view(this).words;f[C.firstPerson]=+!!this.firstPerson;f[C.zoom]=zoom;f[C.zoomFirstPerson]=+!!this.arsenal.current(0)?.def.ZoomFirstPerson;decide(this,3);return !!f[C.eye];
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
    for(const weapon of this.arsenal.weapons)for(const level of weapon.levels)PlayerCombat.zooms(level);
    this.updateHeld();
  }

  // commandBodyAim has already previewed the complete pending look block.
  // Only the existing world/unit collision query crosses back here; the
  // native resume resolves gun-to-crosshair parallax and its final angles.
  convergedAim(preview) {
    if(!preview.needsReach)return preview.angles;
    return preview.resolve(this.crosshairReach(preview.from,preview.direction));
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

  // Which weapon is in the hand: a grenade for the whole throw (its model
  // gone once it leaves the hand), a repair tool at work, otherwise the
  // primary. A page drawing a soldier whose world is elsewhere knows his
  // tool is at work from its ticks (presentRepairTick).
  updateHeld() {
    const thrown = this.arsenal.current(1),primary=this.arsenal.current(0);
    const f=view(this).words;f[C.primaryEmpty]=+!!primary?.handEmpty;f[C.secondaryEmpty]=+!!thrown?.handEmpty;f[C.secondaryBusy]=+!!thrown?.busy;f[C.secondaryWork]=thrown?.shownAtWork??0;f[C.dead]=+!!this.dead;f[C.mounted]=+!!this.mounted;decide(this,0);
    const weapon=f[C.slot]?thrown:primary;
    const name=weapon?.def.GeometryName||null;
    // The one model in the hand, if any. Every remote body's record calls
    // this; its models are made hidden and shown only here, so they are
    // touched only when that model changes.
    const model = f[C.model] ? name : null;
    if (model !== this.heldModel) {
      for (const [key, held] of this.held) sceneProperty(held.group,"visible",key === model);
      this.heldModel = model;
    }
    // The weapon in the hand, for the first-person view, and its bank for
    // the body's upper layer.
    this.shown = f[C.shown] ? weapon : null;
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
    const tool=this.arsenal.current(1),primary=this.arsenal.current(0),f=view(this).words;
    f[C.dt]=dt;f[C.secondaryWork]=tool?.shownAtWork??0;f[C.dead]=+!!this.dead;f[C.mounted]=+!!this.mounted;f[C.remote]=+!!this.remotes?.active;
    f[C.repairLock]=+!!this.repairLock;f[C.primaryRepair]=+!!primary?.repair;f[C.secondaryRepair]=+!!tool?.repair;
    f[C.primary]=+!!primary;f[C.charging]=+!!primary?.charging;f[C.thrown]=+!!primary?.thrown;f[C.unalerted]=+UNALERTED_BANKS.has(primary?.bank);f[C.held]=primary?.held??0;f[C.maxPressed]=primary?.levels?.[0]?.MaxPressedTime||1;
    decide(this,1);if(tool&&tool.shownAtWork>0)tool.shownAtWork=f[C.secondaryWork];if(f[C.refreshHeld])this.updateHeld();
    if(f[C.repairAlert])this.animator.fire?.();
    const weapon=primary,charging=!!f[C.glow];
    if(f[C.chargeAction]===1)this.weaponAction('charge',weapon.bank,this.soldier.posture);else if(f[C.chargeAction]===2)this.endWeaponAction('charge');
    if(f[C.holdAlert])this.animator.holdAlert?.();
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
    this.glow.tint = f[C.tint];
  }

  // Nothing of a charge left on show (a new life, a body taken off the field).
  clearCharge() {
    this.glow?.kill();
    this.glow = null;
    if(decide(this,2)[C.chargeAction]===2)this.endWeaponAction('charge');
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
    return out.copy(this.viewAim.origin);
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
      const f=view(this).words;f[C.repairDelay]=weapon.def.ShotDelay||0;decide(this,5);weapon.shownAtWork=f[C.secondaryWork];
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
    sceneTransform(held.muzzle.matrixWorld);
    const f=targetFrame(),matrix=held.muzzle.matrixWorld.elements;for(let i=0;i<16;i++)f[F.matrix+i]=matrix[i];runTargetFrame(5);
    this.at.position.fromArray(f,F.point);this.at.quaternion.fromArray(f,F.rotation);this.at.scale.fromArray(f,F.scale);
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
    const f=view(this).words,color=d.FlashColor||WHITE,light=d.FlashLightColor||WHITE;
    for(let i=0;i<4;i++){f[C.flashColor+i]=color[i]??255;f[C.lightColor+i]=light[i]??255;}decide(this,4);
    held.flashAlpha=f[C.flashAlpha];held.flash.userData.material.color.setRGB(f[C.flashRGB],f[C.flashRGB+1],f[C.flashRGB+2]);
    for(let i=0;i<3;i++)held.lightRGB[i]=f[C.lightRGB+i];
    if (held.light && this.lights.pending(held.light)) return;
    held.flash.getWorldPosition(this.worldPoint);
    held.light = this.lights.flash(this.worldPoint, held.lightRGB, d.FlashLightRadius || 0, d.FlashLightDuration || 0);
  }

  // The HUD while flying a droid: its weapons and health, its reticle, and
  // the blaster's scope zoomed in first person.
  droidHud(droid) {
    const f=view(this).words;f[C.health]=droid.health;f[C.maxHealth]=droid.def.MaxHealth||40;decide(this,3);
    this.hud.setPlayer(droid.arsenal,f[C.healthFill],droid.def.HealthTexture);
    this.hud.setSecondBar(null);
    const blaster = droid.arsenal.current(0);
    const zoom = this.remotes.zoom;
    this.hud.setScope(zoom > 1 && this.firstPerson ? blaster?.def.ScopeTexture : null);
    const p=projection(droid.eye,droid.aimDir,this.hudCamera||this.camera);
    if(!p[F.value]){this.hud.setReticle(null);return;}
    this.hud.setReticle(p[F.y],blaster);

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
    const s=this.soldier,jet=s.jet,target=this.lockedTarget(),weaponForScope=turret?targetingWeapon(arsenal):this.arsenal.current(0),zoom=this.zoom,f=view(this).words;
    f[C.health]=this.health;f[C.maxHealth]=this.maxHealth;f[C.dead]=+!!this.dead;f[C.turret]=+!!turret;f[C.hullHealth]=turret?.health??0;f[C.hullMax]=turret?.maxHealth??1;
    f[C.target]=+!!target;f[C.targetHealth]=target?.health??0;f[C.targetMax]=target?.maxHealth??1;f[C.shield]=+!!s.shield;f[C.shieldValue]=s.shield?.value??0;f[C.shieldMax]=s.shield?.max??0;
    f[C.jetRecharge]=jet?.recharge??0;f[C.fuel]=s.fuel;f[C.jetBorder]=jet?.minBorder??0;f[C.jetting]=+(s.air==='jet');f[C.zoom]=zoom;f[C.firstPerson]=+!!this.firstPerson;f[C.zoomFirstPerson]=+!!weaponForScope?.def.ZoomFirstPerson;decide(this,3);
    this.hud.setPlayer(arsenal,f[C.healthFill]);
    this.hud.setHull?.(turret?turret.desc.settings.HealthTexture||'hud_vehiclehealth':null,f[C.hullFill]);
    const station=!turret&&!this.dead?this.live?.supplying?.(s.position):null;
    this.hud.setSupply?.(Boolean(station?.servedHealth),Boolean(station?.servedAmmo));
    const icon=f[C.second]===1?target.desc.settings.HealthTexture||'hud_active_repair_icon':f[C.second]===2?'hud_shield_icon':f[C.second]===3?'hud_jetpack_icon':null;
    this.hud.setSecondBar(icon,f[C.fill],BAR_LOOKS[f[C.look]]);
    const weapon = turret ? targetingWeapon(arsenal) : this.arsenal.current(0);
    const scoped = !!f[C.scope];
    this.hud.setScope(scoped ? weapon.def.ScopeTexture : null);
    if (this.dead) {
      this.hud.setReticle(null);
      return;
    }
    let origin,direction;
    if(turret){origin=(this.hudCamera||this.camera).position;direction=aimPreview?.reticleDirection?.(this.side)??aimPreview?.viewDirection(this.side)??turret.viewDirection(this.side);}
    else{const aim=pose?this.viewAim:this;origin=this.usesEyeView?aim.eyePoint:aim.origin;direction=aim.aimDir;}
    const p=projection(origin,direction,this.hudCamera||this.camera);
    if(!p[F.value]){this.hud.setReticle(null);return;}
    // A vehicle seat shows both channels' reticles (src/hud.js drawReticle).
    // A craft's pilot's stays on the screen's centre line, as the
    // original game keeps every reticle, while his chase camera swings after
    // the nose in a turn; the port projects a ground seat's across.
    const weapons = turret?.slot ? this.hudWeapons : null;
    if (weapons) { weapons[0] = arsenal.current(0); weapons[1] = arsenal.current(1); }
    const piloting = turret?.flying && turret.seatIndex === 0;
    this.hud.setReticle(p[F.y], weapon, turret?.slot && !piloting ? p[F.x] : 0.5, weapons);
  }
}
