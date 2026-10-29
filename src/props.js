import * as THREE from "three";
import { descendantMuzzles } from "./muzzles.js";
import { gradientTexture } from "./commandposts.js";
import { blastBlocked, blastDamage, blastDistance, healthScale } from "./ordnance.js";
import { Weapon } from "./weapons.js";
import { emitSound } from "./audio/events.js";
import { vehicleBatches, attachVehicleBatches } from "./vehicle-rig.js";
import { loadClips, Pose } from "./anim.js";
import { makeRiderPoints, riderDropPoint, riderHitShapes, riderPoints } from "./rider-pose.js";

// Placed objects that come to life, following the 2004 game: the
// power-up droids (powerupstation) and the turrets (armedbuilding). Each is a
// rig: its model's skeleton with every part hung from its bone, posed in code.
//
// Both are buildings, and a building is built, destroyed, or unbuilt:
//
// - A class with an UnbuiltGeometryName starts unbuilt (every turret does):
//   its build pad, without collision, at 0 health, and a hologram over it
//   that only a soldier carrying a repair weapon sees. Health raised to its
//   MaxHealth builds it: its own model and collision, full health.
// - At 0 health a built one is destroyed: whoever mans it dies, its
//   ExplosionName goes off, and it becomes its wreck (DestroyedGeometryName,
//   which collides) or, without one, its pad again (hologram fading back in).
//   It then comes back by itself at MaxHealth / RespawnTime per second (120 s
//   for a turret), if it is an armed building; power-up droids wait for a
//   repair weapon.
// - Only a built one can be entered, serves soldiers, or takes damage: blasts
//   (by BuildingScale, falling off from its size) and rounds that hit it.
//
// A repair weapon's tick gives a building the weapon's BuildingHealth (built),
// BuildingRebuild (a wreck) or BuildingBuild (a pad).

const num = (value, fallback = 0) => {
  const n = Number.parseFloat(Array.isArray(value) ? value[0] : value);
  return Number.isFinite(n) ? n : fallback;
};
const pair = (value, fallback) => {
  const parts = String(value ?? "").split(/\s+/).map(Number).filter(Number.isFinite);
  return parts.length >= 2 ? parts : fallback;
};
const list = (value) => (value === undefined ? [] : Array.isArray(value) ? value : [value]);
const DEG = Math.PI / 180;
// A vector key as the 2004 game's parsers read one, sscanf "%f %f %f": each
// %f takes the longest float it can from where the last stopped, a space in
// the format skips any whitespace, and a value that does not start a float
// ends the scan. So the Hoth dish turret's malformed "0.0 1.5.0 -2.0" reads
// 0, 1.5 and then ".0" as 0; its -2.0 is never reached. Missing fields read
// 0 here (the 2004 game leaves them whatever its stack held). A repeated
// key keeps its last value, as the parsers read each anew. Null when not
// even the first float parses.
const FLOAT = /^\s*[+-]?(?:\d+\.?\d*|\.\d+)(?:[eE][+-]?\d+)?/;
export function scanVector(value) {
  let rest = String(Array.isArray(value) ? value[value.length - 1] : value ?? "");
  const out = [];
  while (out.length < 3) {
    const m = FLOAT.exec(rest);
    if (!m) break;
    out.push(Number.parseFloat(m[0]));
    rest = rest.slice(m[0].length);
  }
  if (!out.length) return null;
  while (out.length < 3) out.push(0);
  return out;
}

const HEALTH_TYPE = { built: "building", destroyed: "rebuild", unbuilt: "build" };
// What a repair weapon gives each, and the target bit that also allows it.
const REPAIR = {
  building: ["BuildingHealth", "TargetBuilding"],
  vehicle: ["VehicleHealth", "TargetVehicle"],
  rebuild: ["BuildingRebuild", "TargetBuildingDead"],
  build: ["BuildingBuild", "TargetBuildingUnbuilt"],
};
const LOCK_HELD = 0.7854; // extra slope keeping a lock while the trigger is held
const PITCH_POSE = 45 * DEG; // a gunner's 9-pose spans 45 degrees up and down
const TURRET_FOV = 48; // degrees across: every controllable's default, both views
const AIMED = DEG; // rad: a gun this close to its target fires straight at it (as a vehicle's barrel)
const GUNNER_STANDIN = "human_minigun_9pose";
// World ticks a second, and the ticks without a turret record after which a
// page's copy of its drum stands still (records come at least every other
// tick while its aim changes).
const TICK_RATE = 60;
const TURN_SEEN = 8;

// A model's rig: bones from the bind pose, and its parts in one skinned
// draw per material (vehicle-rig.js), each vertex following its part's bone,
// lit as dynamic objects (these classes say "Lighting dynamic"). A droid was
// a mesh per part (six draws, and six per shadow cascade); its parts share
// a texture. batches: the model's batches, shared by every rig of the model
// (its name -> batches); separate: bone names whose parts get batches of
// their own under those bones (a turret's drum and gun, hidden in first
// person by walking them).
function buildRig(rig, parts, texture, castShadow, { batches = null, separate = [], caster = null } = {}) {
  const root = new THREE.Group();
  const bones = rig.bones.map((desc) => {
    const bone = new THREE.Object3D();
    bone.name = desc.name;
    new THREE.Matrix4().fromArray(desc.local).decompose(bone.position, bone.quaternion, bone.scale);
    return bone;
  });
  rig.bones.forEach((desc, i) => (desc.parent >= 0 ? bones[desc.parent] : root).add(bones[i]));
  const byName = Object.fromEntries(bones.map((bone) => [bone.name.toLowerCase(), bone]));
  const meshes = [];
  const made = batches || vehicleBatches(rig, parts, texture, { separate, dispose: false });
  attachVehicleBatches(root, bones, made, castShadow ? caster : null, { byName, motion: false, meshes });
  const bind = new Map(bones.map((bone) => [bone, { q: bone.quaternion.clone(), p: bone.position.clone() }]));
  return { root, byName, bind, batches: made, meshes, bones };
}

// The hologram over a pad: the class's image spinning in the class's Color
// over a faint beam, flickering, fading in over HoloFadeInTime when the pad
// appears, gone at once when the building is built.
class BuildHologram {
  constructor(at, look, image) {
    this.look = look;
    this.group = new THREE.Group();
    this.group.position.fromArray(at);
    this.color = new THREE.Color().setRGB(...(look.color || [255, 255, 255]).map((c) => c / 255));
    this.icon = new THREE.Group();
    this.icon.position.y = look.height;
    this.icon.scale.setScalar(look.size);
    this.group.add(this.icon);
    this.materials = [];
    if (image) {
      for (const part of image) {
        const material = new THREE.MeshBasicMaterial({
          map: part.map,
          color: this.color,
          transparent: true,
          blending: THREE.AdditiveBlending,
          depthWrite: false,
          side: THREE.DoubleSide,
        });
        this.materials.push(material);
        this.icon.add(new THREE.Mesh(part.geometry, material));
      }
    }
    this.beam = new THREE.Mesh(
      new THREE.CylinderGeometry(0.35, 0.05, look.height, 16, 1, true).translate(0, look.height / 2, 0),
      new THREE.MeshBasicMaterial({
        map: gradientTexture(),
        color: this.color,
        transparent: true,
        opacity: 0.35 * look.beamIntensity,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
        side: THREE.DoubleSide,
      }),
    );
    this.group.add(this.beam);
    this.fade = 0;
    this.flicker = 1;
    this.on = false;
  }

  update(dt, time, shown, camera) {
    const look = this.look;
    if (!this.on) this.fade = 0;
    else this.fade = Math.min(this.fade + dt / Math.max(look.fadeInTime, 1e-3), 1);
    if (Math.random() < look.flickerRate * 60 * dt) {
      this.flicker = look.flickerAlpha[0] + Math.random() * (look.flickerAlpha[1] - look.flickerAlpha[0]);
    }
    const distance = camera.position.distanceTo(this.group.position);
    const reach = THREE.MathUtils.clamp((look.visibleDistance - distance) / (look.visibleDistance * 0.1), 0, 1);
    this.group.visible = this.on && shown && reach > 0;
    this.icon.rotation.y = time * look.rotateRate * Math.PI * 2;
    for (const material of this.materials) material.opacity = look.imageIntensity * this.flicker * this.fade * reach;
    this.beam.material.opacity = 0.35 * look.beamIntensity * this.fade * reach;
  }
}

// A building's looks and life (see the top of the file). looks: the rigs for
// "built", "destroyed" and "unbuilt"; bodies: their collision.
export class Building {
  constructor(desc, looks, bodies, hologram) {
    const s = desc.settings;
    this.desc = desc;
    this.looks = looks;
    this.bodies = bodies;
    this.hologram = hologram;
    this.matrix = new THREE.Matrix4().fromArray(desc.matrix);
    this.position = new THREE.Vector3().setFromMatrixPosition(this.matrix);
    this.maxHealth = num(s.MaxHealth, Infinity);
    this.respawnTime = num(s.RespawnTime, Infinity);
    this.addHealth = 0;
    this.regenerates = desc.base === "armedbuilding";
    this.explosion = desc.explosion || null;
    this.team = desc.team || 0;
    this.onExplode = () => {};
    this.onDestroyed = () => {};
    this.state = null;
    this.health = 0;
    if (looks.unbuilt) this.show("unbuilt");
    else {
      this.show("built");
      this.health = this.maxHealth;
    }
  }

  get alive() {
    return this.state === "built";
  }

  // What a repair weapon counts this as: "building", "rebuild" or "build".
  get healthType() {
    return HEALTH_TYPE[this.state];
  }

  show(state) {
    this.state = state;
    for (const [name, look] of Object.entries(this.looks)) if (look) look.root.visible = name === state;
    for (const [name, body] of Object.entries(this.bodies)) if (body) body.enabled = name === state;
    if (this.hologram) {
      const on = state === "unbuilt";
      if (on && !this.hologram.on) this.hologram.fade = 0;
      this.hologram.on = on;
    }
  }

  setHealth(value) {
    if (value > 0) {
      this.health = Math.min(value, this.maxHealth);
      if (this.health >= this.maxHealth && this.state !== "built") this.built();
    } else {
      this.health = 0;
      if (this.state === "built") this.destroyed();
    }
  }

  built() {
    this.show("built");
    this.health = this.maxHealth;
    this.addHealth = num(this.desc.settings.AddHealth);
  }

  destroyed() {
    // A bot manning it dies first (src/seats.js TurretSeats).
    this.crew?.destroyed?.(this);
    this.onDestroyed(this);
    if (this.looks.destroyed) this.show("destroyed");
    else if (this.looks.unbuilt) this.show("unbuilt");
    if (this.explosion) this.onExplode(this.explosion, this.center(new THREE.Vector3()));
    this.addHealth = Number.isFinite(this.respawnTime) && this.respawnTime > 0 ? this.maxHealth / this.respawnTime : 0;
  }

  // Damage, which only a built building takes.
  // attacker (optional): who is credited, for the gunner's death if it
  // goes down.
  damage(amount, attacker = null) {
    if (!this.alive || !(amount > 0)) return;
    this.lastAttacker = attacker;
    this.setHealth(this.health - amount);
  }

  // Coming back by itself.
  regenerate(dt) {
    if (this.regenerates && this.addHealth > 0 && this.health < this.maxHealth) this.setHealth(this.health + this.addHealth * dt);
  }

  // What locks and blasts go by: a pad's bounding sphere, otherwise the
  // middle of the model's box and half its diagonal.
  sphere(out) {
    const model = this.looks[this.state]?.model;
    let radius = 1;
    if (this.state === "unbuilt" && model?.sphere) {
      out.set(model.sphere[0], model.sphere[1], model.sphere[2]);
      radius = model.sphere[3];
    } else if (model?.box) {
      const [lo, hi] = model.box;
      out.set((lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2);
      radius = Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) / 2;
    } else out.set(0, 1, 0);
    out.applyMatrix4(this.matrix);
    return radius;
  }

  center(out) {
    this.sphere(out);
    return out;
  }

  // What a blast measures to (src/ordnance.js): the model's bounding sphere
  // as it stands, else the sphere above.
  blastSphere(out) {
    const sphere = this.looks[this.state]?.model?.sphere;
    if (!sphere) return this.sphere(out);
    out.set(sphere[0], sphere[1], sphere[2]).applyMatrix4(this.matrix);
    return sphere[3];
  }
}

// A health or ammo droid. Every PowerupDelay seconds it serves everything
// standing within its Radius (no team check): SoldierHealth points of health
// up to full, and SoldierAmmo clips for each weapon up to its clip count (less
// the loaded clip, while it has rounds). Idle, it waits IdleWaitTime, then
// turns the other way through 135 to 225 degrees at IdleRotateSpeed, the gonk
// rocking on gonk1 as it goes. Serving, the FX-7 swings its tester arms and
// twists its operator arm, and the gonk dips. Its wreck, with Enhanced
// destruction effects, smoulders until it is rebuilt (destruction.js watches
// its state; nothing here changes).
export class PowerupStation extends Building {
  constructor(desc, looks, bodies, hologram) {
    super(desc, looks, bodies, hologram);
    const s = desc.settings;
    const rig = looks.built;
    this.rig = rig;
    this.radius = desc.radius || 5;
    this.delay = num(s.PowerupDelay, 0.5);
    this.giveHealth = num(s.SoldierHealth);
    this.giveAmmo = num(s.SoldierAmmo);
    this.vehicleHealth = num(s.VehicleHealth);
    this.vehicleAmmo = num(s.VehicleAmmo);
    this.turnSpeed = num(s.IdleRotateSpeed, 1);
    this.waitTime = num(s.IdleWaitTime, 2);
    this.wobble = num(s.IdleWobbleFactor);
    this.wobbleNode = rig.byName[String(s.IdleWobbleNode || "").toLowerCase()] || null;
    this.arms = list(s.ActiveRotateNode).map((name) => rig.byName[String(name).toLowerCase()]).filter(Boolean);
    this.spin = rig.byName[String(s.ActiveSpinNode || "").toLowerCase()] || null;
    // Everything turns about the model's up axis, inside the placed frame.
    this.turner = new THREE.Group();
    while (rig.root.children.length) this.turner.add(rig.root.children[0]);
    rig.root.add(this.turner);
    // The tick's state, plain so a page showing a worker's battle
    // (src/runtime/replica.js) gets it: whether it is serving ("active")
    // or not ("waiting"), how long it has been serving (its arms' clock),
    // and the serve pulse. The idle cycle (waiting, then turning the other
    // way, with its random span) is the page's alone (idle, not plain:
    // it never travels) and runs in pose.
    this.mode = "waiting";
    this.active = 0;
    this.pulse = 0;
    this.idle = { waiting: true, t: this.waitTime, direction: 1, rate: 0, yaw: 0 };
    // What its last pulse did: whether the player (a target marked player)
    // was among those served, and whether it gave health or ammo.
    this.servedPlayer = false;
    this.servedHealth = false;
    this.servedAmmo = false;
    this.onServe = () => {}; // (target, gave) with gave bits 1 health, 2 ammo
    this.axisX = new THREE.Vector3(1, 0, 0);
    this.axisZ = new THREE.Vector3(0, 0, 1);
    this.q = new THREE.Quaternion();
  }

  // One tick. target: { position (feet), health, maxHealth, weapons }, or
  // a list of them (everyone on foot: a droid serves whoever is near). Only
  // state changes here, and only the serving's; the droid is posed from
  // it (pose) by whoever shows it, this world's page or a worker's.
  update(dt, target, vehicles = []) {
    this.regenerate(dt);
    if (!this.alive) return;
    if (this.mode === "active") this.active += dt;
    this.pulse += dt;
    if (this.pulse >= this.delay) {
      this.pulse -= this.delay;
      let served = 0;
      let first = null;
      this.servedPlayer = false;
      for (const one of Array.isArray(target) ? target : target ? [target] : []) {
        if (one.position.distanceTo(this.position) >= this.radius) continue;
        const gave = this.serve(one);
        if (!gave) continue;
        if (!first) first = one;
        served |= gave;
        if (one.player) this.servedPlayer = true;
        this.onServe(one, gave);
      }
      if (this.vehicleHealth || this.vehicleAmmo) for (const vehicle of vehicles) {
        if (vehicle.alive && vehicle.position.distanceTo(this.position) < this.radius + vehicle.radius * .5) served |= this.serve(vehicle);
      }
      this.servedHealth = (served & 1) !== 0;
      this.servedAmmo = (served & 2) !== 0;
      if (served) {
        emitSound("serve", this, first);
        if (this.mode !== "active") this.active = 0;
        this.mode = "active";
      } else if (this.mode === "active") this.mode = "waiting";
    }
  }

  // What it gives one target: bit 1 health, bit 2 ammo (0: nothing).
  serve(target) {
    let served = 0;
    const health = target.healthType === "vehicle" ? this.vehicleHealth : this.giveHealth;
    const ammo = target.healthType === "vehicle" ? this.vehicleAmmo : this.giveAmmo;
    if (health > 0 && target.health < target.maxHealth) {
      target.health = Math.min(target.health + health, target.maxHealth);
      served |= 1;
    }
    if (ammo > 0) {
      for (const weapon of target.weapons) {
        if (weapon.bottomless || !(weapon.def.ammo > 0)) continue;
        const loaded = weapon.clip >= weapon.perShot - 1e-6 ? 1 : 0;
        const cap = weapon.def.ammo - loaded;
        const next = THREE.MathUtils.clamp(weapon.reserve + ammo, 0, cap);
        if (Math.abs(next - weapon.reserve) > 0.0001) {
          weapon.reserve = next;
          served |= 2;
        }
      }
    }
    return served;
  }

  // The idle cycle, the page's: while the droid serves it holds; after,
  // it waits IdleWaitTime, then turns the other way at IdleRotateSpeed for
  // a random three quarters to five quarters of a half turn, and waits
  // again, as the original game does. Nothing here reaches the tick.
  step(dt) {
    const idle = this.idle;
    if (this.mode === "active") { idle.waiting = true; idle.t = this.waitTime; return; }
    idle.t -= dt;
    if (idle.waiting) {
      if (idle.t < 0) {
        idle.waiting = false;
        idle.direction = -idle.direction;
        idle.t = (Math.PI * (0.75 + 0.5 * Math.random())) / this.turnSpeed;
        idle.rate = (12 * Math.PI) / idle.t;
      }
    } else {
      idle.yaw += idle.direction * this.turnSpeed * dt;
      if (idle.t < 0) { idle.waiting = true; idle.t = this.waitTime; }
    }
  }

  // The rig for a frame: the idle cycle stepped by dt (0 poses again as
  // it is), then the turner at its yaw; turning, the gonk rocking;
  // serving, the gonk dipping and the FX-7's arms swinging and its
  // operator arm twisting on the serving clock (active, replicated).
  // Waiting keeps the last pose, as the original game does.
  pose(dt = 0) {
    if (dt > 0) this.step(dt);
    const rig = this.rig, idle = this.idle;
    this.turner.rotation.y = idle.yaw;
    if (this.mode === "active") {
      if (this.wobbleNode) {
        const bind = rig.bind.get(this.wobbleNode);
        this.wobbleNode.quaternion.copy(bind.q);
        this.wobbleNode.position.copy(bind.p);
        this.wobbleNode.position.y += 0.04 * (Math.cos(2 * Math.PI * this.active) - 1);
      }
      this.arms.forEach((arm, i) => {
        arm.quaternion.copy(rig.bind.get(arm).q).multiply(this.q.setFromAxisAngle(this.axisX, 0.15 * Math.sin(2 * Math.PI * (this.active + i))));
      });
      if (this.spin) {
        this.spin.quaternion.copy(rig.bind.get(this.spin).q).multiply(this.q.setFromAxisAngle(this.axisZ, Math.PI * Math.sin(2 * Math.PI * this.active)));
      }
    } else if (!idle.waiting && this.wobbleNode) {
      const bind = rig.bind.get(this.wobbleNode);
      this.wobbleNode.quaternion.copy(bind.q).multiply(this.q.setFromAxisAngle(this.axisZ, Math.sin(idle.rate * idle.t) * this.wobble));
      this.wobbleNode.position.copy(bind.p);
    }
  }

  // Whether it is serving `position` now: its last pulse served the player
  // and he is still within reach (the HUD's supply indicator).
  supplying(position) {
    return this.alive && this.mode === "active" && this.servedPlayer && position.distanceTo(this.position) < this.radius;
  }
}

// A turret a soldier can man: an armed building, whose turret is the same
// seat object a vehicle's TURRET section makes. The building's class parser
// hands every key after BuildingSection to the seat's parser, which reads
// the camera keys through the camera section parser and the field of view
// through the control section's; the camera is the seat's and the update
// the seat's. So, as docs/formats.md "Vehicle seats" has it:
//
// - The drum (TurretNodeName) is posed as a frame looking along the aim from
//   its own pivot, with its own up (the aim's yaw times YawTurnFactor and
//   pitch times PitchTurnFactor; every placed turret sets the pitch factor to
//   0, so drums only turn). Its bind turn is not kept: the barge turret's
//   drum, yawed half a turn in its bind pose, turns its gun along the aim
//   all the same.
// - The aim's yaw is measured from the building's heading, positive to the
//   left, and held to YawLimits when the arc lies inside +-180 degrees (the
//   barge turret's +-70); an arc from the left round past the back is held
//   in 0..360; anything wider (the default is +-10 rad) turns freely. The
//   pitch is down positive within PitchLimits (the Yavin tower's -10 35 is
//   10 degrees up to 35 down); Turret.pitch is kept up positive.
// - The aim frame hangs at the drum's pivot, looking along the aim, its up
//   the drum's (a class without a drum names the pivot with the unnamed key
//   f2aa4a90). EyePointOffset is the eye in that frame; it sets TrackCenter
//   too, and a TrackCenter after it overrides it. The chase camera looks
//   along the aim pitched down by TiltValue (0 unless set) from TrackCenter
//   in the aim frame plus TrackOffset (right, up, back) in its own; both
//   fields of view are 48 degrees. A class that names no TrackCenter (the
//   Hoth, dish and Yavin tower turrets) pivots on its eye, and one with no
//   TrackOffset sits on its pivot.
// - The gun (AimerNodeName) turns toward the point under the crosshair from
//   its rest pose at up to AimerYawRate and AimerPitchRate (2 and 1 rad/s),
//   within AimerYawLimits and AimerPitchLimits (+-30 and +-45 degrees unless
//   set; an aimer's pitch is up positive), its pitch widened in
//   the port to all the view takes, and fires the turret's weapon from its
//   fire point at what the crosshair is on. Mouse turn and pitch rates are MaxTurnSpeed,
//   eased in at TurnFilter (10/s).
export class Turret extends Building {
  constructor(desc, looks, bodies, hologram) {
    super(desc, looks, bodies, hologram);
    const s = desc.settings;
    const rig = looks.built;
    this.rig = rig;
    this.facing = Math.atan2(desc.matrix[8], desc.matrix[10]);
    this.label = desc.label || s.Label || "Turret";
    this.drum = rig.byName[String(s.TurretNodeName || "").toLowerCase()] || null;
    this.gun = rig.byName[String(s.AimerNodeName || "").toLowerCase()] || null;
    this.seat = rig.byName[String(s.PilotPosition || "").toLowerCase()] || null;
    // The gunner shows where the class names a pilot animation (Pilot9Pose,
    // or PilotAnimation, which no turret uses), at PilotPosition in human_
    // and its name: the armed building poses him through the vehicles'
    // pilot section (docs/formats.md "Riders"). Without one he is hidden
    // and out of collision, in the AAA and Hoth turrets. Rounds and blasts
    // meet a shown gunner unless the class sets hash 2b60e217 to 0 (none
    // does): shootable.
    const anim = s.Pilot9Pose || s.PilotAnimation;
    this.rider = anim ? { clip: `human_${String(anim).toLowerCase()}`, ninePose: Boolean(s.Pilot9Pose), shootable: num(s["2b60e217"], 1) !== 0 } : null;
    // The fire point, or points (the AAA turret's hp_fire1 and hp_fire2:
    // its two barrels take turns).
    this.firePoints = list(s.FirePointName || "hp_fire").map((n) => rig.byName[String(n).toLowerCase()]).filter(Boolean);
    if (!this.firePoints.length) this.firePoints = descendantMuzzles(this.gun || this.drum, rig.byName);
    this.firePoint = this.firePoints[0] || null;
    this.barrel = 0;
    // The view's pitch, down positive as written, kept up positive.
    const pitch = pair(s.PitchLimits, [-45, 45]).map((d) => d * DEG);
    this.pitchLimits = [-pitch[1], -pitch[0]];
    // The yaw's arc from the building's heading (null: free all round), and
    // whether it is one round the back, kept in 0..2pi.
    const yaw = pair(s.YawLimits, null)?.map((d) => d * DEG) ?? null;
    this.wrapYaw = Boolean(yaw && yaw[0] > 0 && yaw[0] <= Math.PI && yaw[1] >= Math.PI && yaw[1] < 2 * Math.PI);
    this.yawLimits = yaw && (this.wrapYaw || (yaw[0] > -Math.PI + 1e-4 && yaw[1] < Math.PI - 1e-4)) ? yaw : null;
    this.yawFactor = num(s.YawTurnFactor, 1);
    this.pitchFactor = num(s.PitchTurnFactor, 1);
    // The gun's own limits (up positive), widened to the view's
    // pitch: the Yavin tower's, dish turret's and AAA's classes write the
    // same numbers for both, which the 2004 game reads with opposite signs,
    // so their guns stopped short of what the view looks at (the Yavin
    // tower's at 10 degrees down, its view at 35). The port's guns reach
    // every pitch the view can take (docs/live-objects.md "Turret views and
    // aim").
    const gunPitch = pair(s.AimerPitchLimits, [-45, 45]).map((d) => d * DEG);
    this.gunPitchLimits = [Math.min(gunPitch[0], this.pitchLimits[0]), Math.max(gunPitch[1], this.pitchLimits[1])];
    this.gunYawLimits = pair(s.AimerYawLimits, [-30, 30]).map((d) => d * DEG);
    this.turnRate = num(s.MaxTurnSpeed, 2);
    this.filter = num(s.TurnFilter, 10);
    this.gunPitchRate = num(s.AimerPitchRate, 1);
    this.gunYawRate = num(s.AimerYawRate, 2);
    // The camera section, read in the class's key order (EyePointOffset sets
    // TrackCenter as well).
    const camera = { eye: [0, 0, 0], center: [0, 0, 0], offset: [0, 0, 0] };
    for (const [key, value] of Object.entries(s)) {
      const v = key === "EyePointOffset" || key === "TrackCenter" || key === "TrackOffset" ? scanVector(value) : null;
      if (!v) continue;
      if (key === "EyePointOffset") camera.eye = camera.center = v;
      else if (key === "TrackCenter") camera.center = v;
      else camera.offset = v;
    }
    this.camera = {
      eye: new THREE.Vector3().fromArray(camera.eye),
      center: new THREE.Vector3().fromArray(camera.center),
      offset: new THREE.Vector3().fromArray(camera.offset),
      tilt: num(s.TiltValue, 0) * DEG,
      thirdFov: num(s.ThirdPersonFOV, TURRET_FOV) * DEG,
      firstFov: num(s.FirstPersonFOV, TURRET_FOV) * DEG,
    };
    this.weapon = desc.weapon ? new Weapon(desc.weapon) : null;
    this.yaw = 0;
    this.pitch = 0;
    this.yawVel = 0;
    this.pitchVel = 0;
    this.gunYaw = 0;
    this.gunPitch = 0;
    this.gunner = null;
    // Seats (src/seats.js): the lease of whoever mans it, the bots' crew
    // manager, the player's Gunner while he mans it, and its entry in the
    // units registry.
    this.lease = null;
    this.crew = null;
    this.humanGunner = null;
    this.aiTarget = null;
    this.lastAttacker = null;
    // The drum's rest pose, less any heading of its own: the original game
    // swings an aimer to the aim from the hull's or the building's frame,
    // as Vehicle.poseSeat does, so yaw 0 points the gun along the facing.
    // Only the barge turret's aimer_01 is bound turned (half round); with
    // its turn kept, its gun pointed back over the barge's deck, against
    // the view (facing + yaw), and its gunner's seat (hp_active_01, behind
    // the gun) hung outside the hull. Unturned, the gun points out over the
    // rail and the gunner stands on the deck behind it.
    this.drumBind = this.drum ? withoutHeading(this.drum.quaternion) : null;
    this.gunBind = this.gun ? this.gun.quaternion.clone() : null;
    // The aim frame's pivot and up in the model's own frame: the drum's at
    // its bind pose (the seat parser keeps the node's place), or
    // the unnamed key's point without a drum. And the turn from the model's
    // frame into the drum's parent's, by which the drum is posed.
    rig.root.updateMatrixWorld(true);
    const rootInverse = new THREE.Matrix4().copy(rig.root.matrixWorld).invert();
    this.pivot = this.drum
      ? this.drum.getWorldPosition(new THREE.Vector3()).applyMatrix4(rootInverse)
      : new THREE.Vector3().fromArray(scanVector(s.f2aa4a90) || [0, 0, 0]);
    const rootTurn = rig.root.getWorldQuaternion(new THREE.Quaternion()).invert();
    this.up = this.drum
      ? new THREE.Vector3(0, 1, 0).applyQuaternion(rootTurn.clone().multiply(this.drum.getWorldQuaternion(new THREE.Quaternion()))).normalize()
      : new THREE.Vector3(0, 1, 0);
    this.drumParentTurn = this.drum?.parent
      ? rootTurn.clone().multiply(this.drum.parent.getWorldQuaternion(new THREE.Quaternion())).invert()
      : null;
    this.frameMatrix = new THREE.Matrix4();
    this.frameTurn = new THREE.Quaternion();
    this.frameDir = new THREE.Vector3();
    this.frameUp = new THREE.Vector3();
    this.framePoint = new THREE.Vector3();
    this.q = new THREE.Quaternion();
    this.e = new THREE.Euler(0, 0, 0, "YXZ");
    this.forward = new THREE.Vector3();
    this.aim = new THREE.Vector3();
  }

  built() {
    super.built();
    if (this.aiTarget) this.aiTarget.generation += 1;
    this.lastAttacker = null;
  }

  // Damage to the turret is damage to whoever mans it, for the AI: the
  // 2004 game's damage alert resolves the damaged entity's
  // controllables, which for an armed building is its mounted turret
  // control, and notifies their UnitControllers. onDamaged
  // (game.js) puts it on the event journal as a vehicle's damage is.
  // hit (optional): { direction, amount, typed, generic } of a round.
  damage(amount, attacker = null, hit = null) {
    if (!this.alive || !(amount > 0)) return;
    super.damage(amount, attacker);
    this.onDamaged?.(this, amount, attacker, hit);
  }

  destroyed() {
    super.destroyed();
    // A turret without a wreck model still loses its live collision and
    // target identity until fully rebuilt.
    if (this.state === "built") this.show("destroyed");
  }

  // The view direction: the building's heading turned by the yaw, tipped by
  // the pitch (up positive). yaw and pitch: the aim's (the turret's own by
  // default; a page passes the one it shows).
  viewDirection(out, yaw = this.yaw, pitch = this.pitch) {
    return aimVector(this.facing + yaw, pitch, out);
  }

  // A yaw from the building's heading held inside the class's arc, as the
  // seat holds its aim (an arc round the back in 0..2pi); free classes keep
  // theirs.
  holdYaw(yaw) {
    const arc = this.yawLimits;
    if (!arc) return yaw;
    let a = Math.atan2(Math.sin(yaw), Math.cos(yaw));
    if (this.wrapYaw && a < 0) a += 2 * Math.PI;
    return THREE.MathUtils.clamp(a, arc[0], arc[1]);
  }

  // Mouse turn and pitch rates (as the soldier reads them) steer the aim,
  // inside the class's yaw arc and pitch limits.
  steer(dt, turnInput, pitchInput, zoom = 1) {
    const k = Math.min(1, this.filter * dt);
    const rate = this.turnRate / zoom;
    this.yawVel += (turnInput * rate - this.yawVel) * k;
    this.pitchVel += (pitchInput * rate - this.pitchVel) * k;
    const yaw = this.yaw + this.yawVel * dt;
    this.yaw = this.yawLimits ? Turret.prototype.holdYaw.call(this, yaw) : Math.atan2(Math.sin(yaw), Math.cos(yaw));
    // Held at the arc's end, the drum stands still (its gunner's turn
    // frames and the look preview go by yawVel).
    if (this.yawLimits && Math.abs(Math.atan2(Math.sin(yaw - this.yaw), Math.cos(yaw - this.yaw))) > 1e-9) this.yawVel = 0;
    this.pitch = THREE.MathUtils.clamp(this.pitch + this.pitchVel * dt, this.pitchLimits[0], this.pitchLimits[1]);
  }

  // The aim frame (the original game's camera subject for the seat): at the
  // drum's pivot, looking along the aim (yaw from the heading, pitch up
  // positive), its up the drum's. Written to out.
  aimFrame(out, yaw = this.yaw, pitch = this.pitch) {
    const root = this.rig.root;
    root.updateMatrixWorld(true);
    this.framePoint.copy(this.pivot).applyMatrix4(root.matrixWorld);
    this.frameUp.copy(this.up).transformDirection(root.matrixWorld);
    lookBasis(this.viewDirection(this.frameDir, yaw, pitch), this.frameUp, this.frameTurn);
    return out.compose(this.framePoint, this.frameTurn, ONE);
  }

  // The eye: EyePointOffset in the aim frame. What a gunner looks from in
  // first person, and the plane the chase camera's aim starts at.
  eyePoint(out, yaw = this.yaw, pitch = this.pitch) {
    return out.copy(this.camera.eye).applyMatrix4(this.aimFrame(this.frameMatrix, yaw, pitch));
  }

  // The chase camera's turn: along the aim with the frame's up, pitched down
  // by TiltValue (a camera looks along its -z; first person, tilt 0, looks
  // straight down the aim). Written to out; leaves the aim frame in
  // frameMatrix.
  cameraTurn(out, yaw = this.yaw, pitch = this.pitch, tilt = this.camera.tilt) {
    this.aimFrame(this.frameMatrix, yaw, pitch);
    this.frameDir.negate();
    lookBasis(this.frameDir, this.frameUp, out);
    this.frameDir.negate();
    return tilt ? out.multiply(cameraTilt.setFromAxisAngle(X_AXIS, -tilt)) : out;
  }

  // Where the chase camera rests, walls and easing aside: TrackCenter in the
  // aim frame, then TrackOffset (right, up, back) in the camera's turn.
  // pivot: also given TrackCenter alone, when wanted.
  cameraRest(out, yaw = this.yaw, pitch = this.pitch, pivot = null) {
    this.cameraTurn(restTurn, yaw, pitch);
    this.framePoint.copy(this.camera.center).applyMatrix4(this.frameMatrix);
    pivot?.copy(this.framePoint);
    return out.copy(this.camera.offset).applyQuaternion(restTurn).add(this.framePoint);
  }

  // Where a person's aim at this turret starts (the original game's player aim
  // target, docs/formats.md "Vehicle seats"): his eye in first person;
  // otherwise where the chase camera rests, brought forward along the aim to
  // the eye's plane. The crosshair sits on the line from there along the
  // aim, so the point the gun turns to is the one under it. The page's
  // Gunner and the server's TurretSeats both aim from here.
  aimOrigin(out, firstPerson = false, yaw = this.yaw, pitch = this.pitch) {
    if (firstPerson) return this.eyePoint(out, yaw, pitch);
    this.cameraRest(out, yaw, pitch);
    const eye = this.eyePoint(aimEye, yaw, pitch);
    const dir = this.viewDirection(aimDir, yaw, pitch);
    return out.addScaledVector(dir, Math.max(0, aimEye.sub(out).dot(dir)));
  }

  // Whether a point (the world) lies inside the built model's box, short of
  // its faces by 0.15 m: an eye there looks from inside the turret, as from
  // a vehicle's cockpit (VehicleCamera.eyeView), and the drum and gun leave
  // the view.
  encloses(point) {
    const box = this.looks?.built?.model?.box;
    if (!box) return false;
    const p = aimEye.copy(point).applyMatrix4(inverseScratch.copy(this.matrix).invert());
    const [lo, hi] = box, m = 0.15;
    return p.x > lo[0] + m && p.x < hi[0] - m && p.y > lo[1] + m && p.y < hi[1] - m && p.z > lo[2] + m && p.z < hi[2] - m;
  }

  // The gun turned toward a world point at its own rates (the tick's
  // part), then the drum and gun posed from the angles (present's). Both
  // hosts integrate; a page showing a worker's battle poses from the
  // replicated angles alone (LiveObjects.present).
  pose(dt, target) {
    this.aimGun(dt, target);
    this.present();
  }

  // The drum and gun from the aim and the gun's own angles. The drum is the
  // aim frame's turn (by YawTurnFactor and PitchTurnFactor) about its pivot,
  // its bind turn not kept, as the seat's update writes it; the
  // gun is its rest pose turned by gunYaw and gunPitch. yaw, pitch: the
  // aim's (a page shows its previewed one).
  present(yaw = this.yaw, pitch = this.pitch) {
    if (this.drum && this.drumParentTurn) {
      lookBasis(aimVector(this.yawFactor * yaw, this.pitchFactor * pitch, this.frameDir), this.up, this.q);
      this.drum.quaternion.copy(this.drumParentTurn).multiply(this.q);
    }
    if (!this.gun) return;
    // Pitch up is a negative turn about the gun's x.
    this.e.set(-this.gunPitch, this.gunYaw, 0);
    this.gun.quaternion.copy(this.gunBind).multiply(this.q.setFromEuler(this.e));
  }

  // The gun's angles toward a world point (null: the drum's own pitch),
  // held inside its limits: { yaw, pitch } in its parent's frame from its
  // pivot, written to out.
  gunWant(target, out) {
    out.yaw = 0;
    out.pitch = this.pitch;
    if (target) {
      this.rig.root.updateMatrixWorld(true);
      this.aim.copy(target);
      this.gun.parent.worldToLocal(this.aim);
      this.aim.sub(this.gun.position);
      out.yaw = Math.atan2(this.aim.x, this.aim.z);
      out.pitch = Math.atan2(this.aim.y, Math.hypot(this.aim.x, this.aim.z));
    }
    out.yaw = THREE.MathUtils.clamp(out.yaw, this.gunYawLimits[0], this.gunYawLimits[1]);
    out.pitch = THREE.MathUtils.clamp(out.pitch, this.gunPitchLimits[0], this.gunPitchLimits[1]);
    return out;
  }

  // The gun's aim integration alone: toward the target at its yaw and pitch
  // rates, inside its limits.
  aimGun(dt, target) {
    if (!this.gun) return;
    const want = this.gunWant(target, gunWanted);
    const step = (from, to, rate) => from + THREE.MathUtils.clamp(to - from, -rate * dt, rate * dt);
    this.gunYaw = step(this.gunYaw, want.yaw, this.gunYawRate);
    this.gunPitch = step(this.gunPitch, want.pitch, this.gunPitchRate);
  }

  // The direction the gun points, and its fire point, in the world.
  muzzle(position, direction, advance = false) {
    this.rig.root.updateMatrixWorld(true);
    const point = this.firePoints?.length > 1 ? this.firePoints[this.barrel % this.firePoints.length] : this.firePoint;
    if (advance) this.barrel++;
    (point || this.gun || this.rig.root).getWorldPosition(position);
    if (direction) direction.set(0, 0, 1).transformDirection((this.gun || this.rig.root).matrixWorld);
    return position;
  }

  // Where a round from `from` (a fire point) goes: straight at the target
  // once the gun has turned as far toward it as it can, so it lands under
  // the crosshair from whichever barrel fires: when the gun points at it
  // from its pivot within a degree (as a vehicle's barrel, Vehicle.
  // shotDirection), or when it stands at its limits short of it (the AAA
  // and dish turret's guns, which never yaw; the dish turret's held 25
  // degrees up under a chase camera far above it). Along the gun (`along`,
  // as muzzle() gives it) only while it is still turning. The gun's angles
  // are all it needs, so the server, a bot's seat and the page's predicted
  // rounds agree.
  fireDirection(from, target, along, out) {
    if (target && this.gun) {
      this.gun.getWorldPosition(aimEye);
      aimDir.subVectors(target, aimEye);
      const span = aimDir.length();
      const want = this.gunWant(target, gunWanted);
      const settled = Math.abs(want.yaw - this.gunYaw) <= AIMED && Math.abs(want.pitch - this.gunPitch) <= AIMED;
      if (span > 1e-3 && (settled || aimDir.dot(along) > span * Math.cos(AIMED))) {
        out.subVectors(target, from);
        if (out.lengthSq() > 1e-8) return out.normalize();
      }
    }
    return out.copy(along);
  }

  // A page's copy of the turret, from the world's record of it ({ tick,
  // yaw, pitch, gunYaw, gunPitch }, src/runtime/network-entity-game.js).
  // The drum's turn speed picks its gunner's turn frames (riderSample), as
  // the world draws and strikes him, but isn't sent: the world's steer
  // turns the yaw by it each tick, so it is the yaw's change since the
  // last record, per tick. A drum whose record stops coming (its aim no
  // longer changing) stands still: settleTurn.
  aimFrom(record) {
    const ticks = record.tick - (this.recordTick ?? record.tick);
    if (ticks > 0) this.yawVel = ticks <= TURN_SEEN ? Math.atan2(Math.sin(record.yaw - this.yaw), Math.cos(record.yaw - this.yaw)) * TICK_RATE / ticks : 0;
    this.recordTick = record.tick;
    this.yaw = record.yaw;
    this.pitch = record.pitch;
    this.gunYaw = record.gunYaw;
    this.gunPitch = record.gunPitch;
  }

  // No record for TURN_SEEN ticks by `tick`: the drum stands still.
  settleTurn(tick) {
    if (tick - (this.recordTick ?? tick) > TURN_SEEN) this.yawVel = 0;
  }

  // The gunner's pose, written to out { clip, pitch, turn, time } (as
  // SoldierAnimator.gunner takes it): the 9-pose by the drum's pitch (-1 at
  // 45 degrees up, 1 at 45 down) and its turn speed over MaxTurnSpeed (a
  // PilotAnimation would hold its first frame). The Hoth dish turret's
  // gunner has his own clip, human_dishcannon_9pose; a bank without it (an
  // extract from before the extractor named it) has the minigun's stand
  // in, as for a hidden gunner. Null without either clip. He is drawn
  // (TurretSeats.pose, the Gunner) and struck (riderShapes) in this one
  // pose. pitch and yawVel: the drum's, unless a page draws the player's
  // own gunner at the aim it previews (Gunner.poseBody).
  riderSample(clips, out, pitch = this.pitch, yawVel = this.yawVel) {
    const clip = (this.rider && clips?.get(this.rider.clip)) || clips?.get(GUNNER_STANDIN) || null;
    if (!clip) return null;
    const timed = this.rider && !this.rider.ninePose;
    out.clip = clip;
    out.pitch = timed ? 0 : -pitch / PITCH_POSE;
    out.turn = timed || !this.turnRate ? 0 : yawVel / this.turnRate;
    out.time = timed ? 0 : null;
    return out;
  }

  // Where the gunner stands, in the world: the seat node's place, written
  // to out (the turret's root without one).
  seatPoint(out) {
    return (this.seat || this.rig.root).getWorldPosition(out);
  }

  // How the gunner is turned: the seat node's own turn in the world (out,
  // a Quaternion), as the 2004 game's pilot section hands the soldier the
  // node's whole transform and he takes it for his own matrix: the drum's
  // turn (facing + yaw, along the gun), and the lean
  // of a turret placed on a slope (one Hoth dish turret stands 9 degrees
  // off level), with any turn the node itself is bound with.
  seatTurn(out) {
    return (this.seat || this.rig.root).getWorldQuaternion(out);
  }

  // The heading of that turn: where the gunner's +z points, about the
  // vertical (the soldier's aimYaw while he mans it).
  seatYaw() {
    const node = this.seat || this.rig.root;
    node.updateWorldMatrix(true, false);
    const e = node.matrixWorld.elements;
    return Math.atan2(e[8], e[10]);
  }

  // Whether whoever mans it is out of sight (no pilot animation: rider).
  get hidesGunner() {
    return !this.rider;
  }

  // Whether rounds, swings and blasts meet the gunner (SeatLease.exposed):
  // one shows, and the class leaves him shootable.
  exposedSeat() {
    return Boolean(this.alive && this.rider?.shootable);
  }

  // The gunner's hit shapes this tick (src/rider-pose.js riderHitShapes):
  // his body in riderSample's pose at the seat, turned with the drum, as he
  // is drawn. Written to shapes; returns their count, 0 where none shows.
  // index: the seat's, always 0 (as Vehicle.riderShapes takes it).
  riderShapes(index, unit, shapes) {
    const frame = this.riderBody(unit, riderScratch);
    return frame ? riderHitShapes(riderScratch.points, frame, unit.rootScale, shapes) : 0;
  }

  // Where a gunner killed at the gun starts to fall from, in the world:
  // out, or null where none shows (src/rider-pose.js riderDropPoint).
  riderDrop(index, unit, out) {
    const frame = this.riderBody(unit, riderScratch);
    return frame ? riderDropPoint(riderScratch.points, out).applyMatrix4(frame) : null;
  }

  // His body's points (written to s.points) and the frame that carries
  // them into the world (s.frame): the seat node's place and turn
  // (seatPoint, seatTurn), where TurretSeats.pose and the Gunner put his
  // body. Null where none shows.
  riderBody(unit, s) {
    const animator = unit?.animator;
    if (!this.rider || !animator) return null;
    const sample = this.riderSample(animator.clips, s.sample);
    if (!sample || !riderPoints(animator, sample, s.points)) return null;
    this.seatPoint(s.point);
    return s.frame.compose(s.point, this.seatTurn(s.turn), ONE);
  }
}
// A direction from a yaw (from +z, positive to the left, toward +x) and
// an up-positive pitch.
function aimVector(yaw, pitch, out) {
  return out.set(Math.sin(yaw) * Math.cos(pitch), Math.sin(pitch), Math.cos(yaw) * Math.cos(pitch));
}
// The turn whose +z is `forward`, keeping up as near its +y as it can (the
// original game's look basis: right = up x forward, then up = forward x right).
const basisX = new THREE.Vector3(), basisY = new THREE.Vector3(), basisZ = new THREE.Vector3(), basisM = new THREE.Matrix4();
function lookBasis(forward, up, out) {
  basisZ.copy(forward).normalize();
  basisX.crossVectors(up, basisZ);
  if (basisX.lengthSq() < 1e-10) basisX.set(0, 0, 1).cross(basisZ);
  if (basisX.lengthSq() < 1e-10) basisX.set(1, 0, 0);
  basisX.normalize();
  basisY.crossVectors(basisZ, basisX);
  return out.setFromRotationMatrix(basisM.makeBasis(basisX, basisY, basisZ));
}
const X_AXIS = new THREE.Vector3(1, 0, 0);
const cameraTilt = new THREE.Quaternion();
const restTurn = new THREE.Quaternion();
const aimEye = new THREE.Vector3();
const aimDir = new THREE.Vector3();
const gunWanted = { yaw: 0, pitch: 0 };
const inverseScratch = new THREE.Matrix4();
// A rotation less its heading (its turn about the vertical, read from
// where it sends +z): what is left of a node's bind turn once the aim
// takes over its heading.
function withoutHeading(q) {
  const forward = new THREE.Vector3(0, 0, 1).applyQuaternion(q);
  const heading = new THREE.Quaternion().setFromAxisAngle(THREE.Object3D.DEFAULT_UP, Math.atan2(forward.x, forward.z));
  return heading.invert().multiply(q);
}
// A gunner's pose scratch (Turret.riderBody), shared: one is posed at a time.
const riderScratch = { points: makeRiderPoints(), frame: new THREE.Matrix4(), point: new THREE.Vector3(), turn: new THREE.Quaternion(),
  sample: { clip: null, pitch: 0, turn: 0, time: null } };
const ONE = new THREE.Vector3(1, 1, 1);

// Every live object on the map. collision: the world to add their bodies to,
// with their triangles (props_collision.*); explode(explosion, position) sets
// off a destroyed building's blast; onDestroyed(building) is told first.
// An animated prop (Jabba, the rancor, a sarlacc tentacle, the trapdoor
// in front of Jabba) or a door (the palace cell doors): a rig playing its
// bank's clips, as the 2004 game's animated prop runs them
// (docs/live-objects.md "Animated props and the sarlacc"):
//
// - Its Animation clips fill five slots; it idles in the first, looping,
//   from a point of its own in the clip, holding the last frame IdleDelay
//   seconds between loops.
// - Idling with no one in its grip and its idle at least half blended in,
//   it watches each AnimationTrigger's node: a soldier's collision sphere
//   within the reach's box of it (living, or a body not yet gone) plays
//   that action once, then the idle again.
// - Its pose eases into each new clip: each step it moves the weight of
//   the way from where it stood to the clip's pose, the weight rising from
//   0 by a second a second into an action, by a fifth back into the idle.
// - A creature's AttachTrigger seizes (the sarlacc's tentacles, the
//   rancor): while its action is under way, up to `window` seconds in, the
//   first soldier found within reach of its node, alive and on foot, is
//   held there to the action's end, posed in his clip (riders, src/seats.js
//   and rider-pose.js); then he dies (no one is credited), his body is gone
//   at once and the KillSoldierSound plays.
// - A door opens while anyone is in reach and closes when they leave, its
//   Open clip run both ways. An "open" action drops the prop's collision
//   while it plays: the closed trapdoor is solid, the open one is the pit.
//
// A creature that seizes is the world's to run: the page follows its
// state (src/runtime/entity-world-state.js) and draws whom it holds from
// its own pose, so he moves with the tentacle frame by frame. Doors and the
// rest step on every host from the soldiers it has (the page's own soldier
// is predicted, so his trapdoor opens under him without waiting).
const SOLDIER_MIDDLE = 1.048; // m: a soldier's collision sphere's centre above his feet (src/units.js)
const SOLDIER_RADIUS = 1.278; // m: and its radius
const TICK = 1 / 60;
// The original game's length of a clip: its frames at 30 a second (one frame
// past the port's duration, which ends on the last frame).
const length = (clip) => (clip ? clip.frames / clip.fps : 0);
// Whether a soldier with his feet at `feet` is within `reach` of `point`
// as the 2004 game's region query has it: his
// collision sphere's box against the box `reach` round the point.
// A destructible command post's building (docs/objectives.md): Hoth's
// shield generator, Geonosis' Techno Union ships and the Republic forward
// command center, Endor's bunker control panel. Any team's rounds and
// blasts hurt it (the post's own team's by the friendly fire factor); it
// never comes back by itself, and its wreck takes no repair weapon (nothing
// in the stock game triggers a rebuild). Its post (src/commandposts.js)
// goes with it.
//
// An animated one (the Techno Union ship) loops its IdleAnimation; at 0
// health its ExplosionName goes off at once and it plays its
// DeathAnimation, its random explosions and final DeathEffects drawn by
// src/vehicle-effects.js, and only then is it gone. The state that travels
// is "destroyed" from the start; each host draws the dying from the moment
// it sees it, so a late joiner sees the wreck.
export class ObjectivePost extends Building {
  constructor(desc, looks, bodies, hologram, clips = null) {
    super(desc, looks, bodies, hologram);
    this.regenerates = false;
    this.post = null; // its command post (CommandPosts.attachBuildings)
    const anim = desc.animation || {};
    const rig = looks.built;
    this.idleClip = (anim.idle && clips?.get(anim.idle)) || null;
    this.deathClip = (anim.death && clips?.get(anim.death)) || null;
    this.span = desc.fx?.dying?.span || length(this.deathClip);
    this.pose = (this.idleClip || this.deathClip) && rig?.bones?.length
      ? new Pose(rig.bones.length, Math.max(0, rig.bones.findIndex((b) => /^dummy_?root$/i.test(b.name))))
      : null;
    this.time = 0;
    this.dying = 0; // s of its death animation left (drawn while > 0)
  }

  // Gone for good: its wreck, or nothing where the class names none (the
  // Techno Union ship, whose hulk is its static first chunk).
  destroyed() {
    this.crew?.destroyed?.(this);
    this.onDestroyed(this);
    const at = this.center(new THREE.Vector3());
    this.show("destroyed");
    if (this.explosion) this.onExplode(this.explosion, at);
    this.addHealth = 0;
  }

  // A wreck is no one's to rebuild; built, it takes a repair weapon's
  // BuildingHealth like any building.
  get healthType() {
    return this.state === "built" ? "building" : null;
  }

  show(state) {
    const was = this.state;
    super.show(state);
    this.dying = was === "built" && state === "destroyed" && this.span > 0 && this.looks.built ? this.span : 0;
    if (this.dying) {
      this.looks.built.root.visible = true;
      this.time = 0;
    }
  }

  // A frame: the idle looping, or the death clip until it has played.
  present(dt) {
    const rig = this.looks.built;
    if (this.dying > 0) {
      this.dying = Math.max(this.dying - dt, 0);
      this.time += dt;
      this.posed(this.deathClip, this.time, false);
      if (!(this.dying > 0)) rig.root.visible = false;
    } else if (this.alive && this.idleClip) {
      this.time += dt;
      this.posed(this.idleClip, this.time, true);
    }
  }

  posed(clip, time, loop) {
    const pose = this.pose;
    if (!pose || !clip) return;
    pose.clear();
    pose.add(clip, time, 1, loop);
    const bones = this.looks.built.bones;
    for (let i = 0; i < bones.length; i++) {
      const w = pose.w[i];
      if (w <= 1e-6) continue;
      bones[i].quaternion.fromArray(pose.q, i * 4).normalize();
      bones[i].position.set(pose.t[i * 3] / w, pose.t[i * 3 + 1] / w, pose.t[i * 3 + 2] / w);
    }
  }
}

function within(point, reach, feet) {
  const r = reach + SOLDIER_RADIUS;
  return Math.abs(feet.x - point.x) <= r && Math.abs(feet.y + SOLDIER_MIDDLE - point.y) <= r && Math.abs(feet.z - point.z) <= r;
}
// A body in the soldier list, as triggers see it: a soldier (not a
// droideka) on foot, living or lying dead, not seated or held.
const onFoot = (unit) => Boolean(unit?.soldier && !unit.walker && !unit.removed && (unit.alive || unit.dead) && !unit.seat && !unit.held);
// A point of the idle clip of its own for each prop (the original game starts
// each at a random frame), the same on every host.
const phase = (index) => (((index + 1) * 2654435761) >>> 0) / 4294967296;

const heldScratch = { points: { ...makeRiderPoints(), root: new THREE.Vector3(), rootTurn: new THREE.Quaternion() },
  sample: { clip: null, pitch: 0, turn: 0, time: null }, frame: new THREE.Matrix4(), root: new THREE.Matrix4() };
const SCALE_ONE = new THREE.Vector3(1, 1, 1);

export class AnimatedProp {
  constructor(desc, rig, body, clips, index = 0) {
    this.desc = desc;
    this.rig = rig;
    this.body = body || null;
    this.index = index;
    this.position = new THREE.Vector3().setFromMatrixPosition(new THREE.Matrix4().fromArray(desc.matrix));
    const anim = desc.animation || {};
    this.door = desc.base === "door";
    // The slots, by name (Animation; an import before them names the idle
    // and the trigger's action alone).
    this.names = anim.clips || [anim.idle, anim.trigger?.action].filter(Boolean);
    this.slots = this.names.map((name) => clips?.get(name) || null);
    this.idleClip = this.slots[0] || null;
    rig.root.updateMatrixWorld(true);
    const node = (name) => rig.byName[String(name || "").toLowerCase()] || null;
    this.triggers = (anim.triggers || (anim.trigger ? [anim.trigger] : [])).map((trigger) => {
      const point = this.position.clone();
      node(trigger.node)?.getWorldPosition(point);
      return { ...trigger, slot: this.names.indexOf(trigger.action), point };
    });
    this.trigger = this.triggers[0] || null;
    this.doorClip = this.door ? clips?.get("open") || this.idleClip : null;
    this.grabs = (anim.attach || []).map((grab) => ({ ...grab, slot: this.names.indexOf(grab.action), node: node(grab.node) }));
    this.killSound = anim.killSound || null;
    this.idleDelay = anim.idleDelay || 0;
    // A seized player watches from his death camera at once (0550a3b2).
    this.deathCamera = Boolean(anim.deathCamera);
    this.pose = clips?.size && rig.bones?.length
      ? new Pose(rig.bones.length, Math.max(0, rig.bones.findIndex((b) => /^dummy_?root$/i.test(b.name))))
      : null;
    const n = rig.bones?.length || 0;
    // The pose it stands in (the clips ease into it), per bone, from bind.
    this.standQ = new Float32Array(n * 4);
    this.standT = new Float32Array(n * 3);
    rig.bones?.forEach((bone, i) => { bone.quaternion.toArray(this.standQ, i * 4); bone.position.toArray(this.standT, i * 3); });
    this.slot = -1; // the clip playing (-1 before the first step)
    // Seconds into it: the idle starts at a point of its own unless the
    // class says otherwise (6b014234, the carbon-freeze forklift).
    this.time = anim.fromStart ? 0 : phase(index) * length(this.idleClip);
    this.weight = 1; // how far each step eases toward it (and the triggers' gate)
    this.loop = false; // the idle loops; an action plays once
    this.hold = 0; // seconds held at the idle's end (IdleDelay)
    this.started = 0; // the world tick the clip started at
    this.startTime = this.time; // and the time into it then
    this.victim = null; // { unit, generation, grab, since }: whom it holds
    this.openness = 0; // a door's, 0 shut to 1 open
    this.point = new THREE.Vector3();
  }

  // Doors: anyone's feet within the trigger's reach of its node.
  near(positions) {
    const trigger = this.trigger;
    if (!trigger) return false;
    for (const position of positions || []) if (position.distanceToSquared(trigger.point) <= trigger.radius * trigger.radius) return true;
    return false;
  }

  // The clip in a slot, from the tick it starts on (as in the 2004 game:
  // the same slot again changes nothing).
  play(slot, loop, tick) {
    if (slot === this.slot) return;
    this.slot = slot;
    this.loop = loop;
    this.started = tick;
    this.startTime = this.time;
    if (this.body && !this.door && this.triggers.some((t) => t.action === "open")) this.body.enabled = !(!loop && this.names[slot] === "open");
  }

  // One step of the world (see the class note): its triggers and grabs
  // from `bodies` (units), its clock, its pose, and whom it holds.
  // positions: feet a door feels. tick: the world's.
  step(dt, positions, bodies = [], tick = 0) {
    if (!this.pose) return;
    if (this.door) {
      const near = this.near(positions);
      const clip = this.doorClip;
      const span = clip ? Math.max(clip.duration, 1e-3) : 1;
      this.openness = THREE.MathUtils.clamp(this.openness + (near ? dt : -dt) / span, 0, 1);
      this.pose.clear();
      if (clip) this.pose.add(clip, this.openness * clip.duration, 1, false);
      if (this.body) this.body.enabled = this.openness < 0.5;
      for (const [bone, at] of this.rig.bind) { bone.quaternion.copy(at.q); bone.position.copy(at.p); }
      this.pose.apply(this.rig.bones);
      return;
    }
    if (this.slot < 0) this.play(0, true, tick);
    if (!this.loop) {
      if (this.time > length(this.slots[this.slot])) {
        this.time = 0;
        this.weight = 0;
        this.swallow();
        this.play(0, true, tick);
      }
    } else if (this.victim) this.swallow();
    // Idling with no one held and the idle half blended in: the first
    // trigger with a soldier at its node plays its action.
    if (this.loop && !this.victim && this.weight >= 0.5) {
      for (const trigger of this.triggers) {
        if (trigger.slot < 0 || trigger.slot === this.slot) continue;
        if (!bodies.some((unit) => onFoot(unit) && within(trigger.point, trigger.radius, unit.position))) continue;
        this.time = 0;
        this.weight = 0;
        this.play(trigger.slot, false, tick);
        if (trigger.sound) emitSound("sound", trigger.sound, trigger.point);
        break;
      }
    }
    // Under way, a grab takes the first body it finds within reach of its
    // node, if he lives (the original game's query takes one).
    if (!this.victim) {
      for (const grab of this.grabs) {
        if (grab.slot !== this.slot || this.time > grab.window || !grab.node) continue;
        this.rig.root.updateMatrixWorld(true);
        grab.node.getWorldPosition(this.point);
        const unit = bodies.find((b) => onFoot(b) && within(this.point, grab.radius, b.position));
        if (!unit?.alive) continue;
        this.seize(unit, grab, tick);
        break;
      }
    }
    this.advance(dt);
    if (this.victim) this.carry(tick);
  }

  // The clock and the pose by dt (the original game's order: the time, then
  // the pose eased by the weight, then the weight).
  advance(dt) {
    let step = dt;
    if (!this.loop) this.time += dt;
    else {
      const span = length(this.slots[0]);
      if (this.time + dt < span) this.time += dt;
      else {
        this.hold += dt;
        if (this.hold > this.idleDelay) {
          this.hold = 0;
          this.time += dt;
        } else step = 0;
      }
      if (span > 0 && this.time > span) this.time -= span;
    }
    this.stand(this.weight, step);
    this.weight += (this.slot === 0 ? 0.2 : 1) * step;
  }

  // The pose eased toward the clip's at this time: at 60 steps a second the
  // weight of the way each step (a frame of `dt` takes the share as many
  // 60 Hz steps would), all of it once the weight is 1.
  stand(weight, dt) {
    const clip = this.slots[this.slot];
    const pose = this.pose;
    pose.clear();
    if (clip) pose.add(clip, this.time, 1, this.loop);
    const share = weight >= 1 ? 1 : dt > 0 ? 1 - Math.pow(1 - Math.max(weight, 0), dt / TICK) : weight;
    const q = this.standQ, t = this.standT;
    for (let i = 0; i < pose.count; i++) {
      const w = pose.w[i];
      if (w <= 1e-6) continue;
      const qi = i * 4, ti = i * 3;
      let x = pose.q[qi], y = pose.q[qi + 1], z = pose.q[qi + 2], s = pose.q[qi + 3];
      if (q[qi] * x + q[qi + 1] * y + q[qi + 2] * z + q[qi + 3] * s < 0) { x = -x; y = -y; z = -z; s = -s; }
      q[qi] += (x - q[qi]) * share;
      q[qi + 1] += (y - q[qi + 1]) * share;
      q[qi + 2] += (z - q[qi + 2]) * share;
      q[qi + 3] += (s - q[qi + 3]) * share;
      const len = Math.sqrt(q[qi] * q[qi] + q[qi + 1] * q[qi + 1] + q[qi + 2] * q[qi + 2] + q[qi + 3] * q[qi + 3]) || 1;
      q[qi] /= len; q[qi + 1] /= len; q[qi + 2] /= len; q[qi + 3] /= len;
      for (let k = 0; k < 3; k++) t[ti + k] += (pose.t[ti + k] / w - t[ti + k]) * share;
    }
    const bones = this.rig.bones;
    for (let i = 0; i < bones.length; i++) {
      bones[i].quaternion.fromArray(q, i * 4);
      bones[i].position.fromArray(t, i * 3);
    }
  }

  // Seized (the 2004 game's grab): held at the node in his
  // clip, out of his own control; his weapons wait (Unit.tick,
  // PlayerCombat.mounted). since: the world tick.
  seize(unit, grab, tick) {
    this.victim = { unit, generation: unit.generation, grab, since: tick };
    unit.held = { prop: this, grab, since: tick };
    if (unit.combat) unit.combat.mounted = true;
    const s = unit.soldier;
    s.velocity.set(0, 0, 0);
    if (s.air) s.air = null;
    s.dive = 0;
  }

  // At the action's end the one it holds dies, no one credited, and his
  // body is gone at once; the KillSoldierSound plays at the prop.
  swallow() {
    const victim = this.victim;
    this.victim = null;
    if (!victim) return;
    const unit = victim.unit;
    if (unit.generation !== victim.generation || unit.removed) return;
    release(unit);
    if (unit.alive) unit.combat?.damage(Infinity);
    unit.remove?.();
    if (this.killSound) emitSound("sound", this.killSound, this.position);
  }

  // Whom it holds, at the node this step: his body's place (his feet) and
  // the frame he is drawn and struck in.
  carry(tick) {
    const victim = this.victim;
    const unit = victim.unit;
    if (unit.generation !== victim.generation || unit.removed || unit.held?.prop !== this) {
      this.victim = null;
      return;
    }
    const frame = this.heldFrame(unit, (tick - victim.since) * TICK, heldScratch.frame);
    if (!frame) return;
    const s = unit.soldier;
    s.position.setFromMatrixPosition(frame);
    s.previous?.copy(s.position);
    s.settled?.copy(s.position);
    s.velocity.set(0, 0, 0);
    (unit.held.frame ||= new THREE.Matrix4()).copy(frame);
    unit.held.time = (tick - victim.since) * TICK;
    unit.tickCount = (unit.tickCount || 0) + 1;
  }

  // The frame a held soldier is drawn in (his actor's root) at `time`
  // seconds into his hold: his root bone (bone_root, as his clip poses it)
  // on the grab's node, turned with it (the 2004 game's inverse root
  // times the node). Null without his clip or the node.
  heldFrame(unit, time, out) {
    const grab = unit.held?.grab;
    const animator = unit.animator;
    const clip = animator?.clips?.get(grab?.clip);
    if (!grab?.node || !clip) return null;
    const sample = heldScratch.sample;
    sample.clip = clip; sample.time = Math.max(time, 0);
    if (!riderPoints(animator, sample, heldScratch.points)) return null;
    this.rig.root.updateMatrixWorld(true);
    heldScratch.root.compose(heldScratch.points.root, heldScratch.points.rootTurn, SCALE_ONE).invert();
    return out.multiplyMatrices(grab.node.matrixWorld, heldScratch.root);
  }

  // A held soldier's hit shapes (Unit.updateSpheres): his posed body in
  // the frame he hangs in this tick, as a shown rider's. Returns their
  // count.
  heldShapes(unit, shapes) {
    const held = unit.held;
    const clip = unit.animator?.clips?.get(held?.grab?.clip);
    if (!held?.frame || !clip) return 0;
    const sample = heldScratch.sample;
    sample.clip = clip; sample.time = held.time || 0;
    if (!riderPoints(unit.animator, sample, heldScratch.points)) return 0;
    return riderHitShapes(heldScratch.points, held.frame, unit.rootScale, shapes);
  }

  // The world's state of a creature that seizes, for a page to follow
  // (entity-world-state.js): its clip, the tick and time it started at,
  // and whom it holds, since when.
  state() {
    const v = this.victim;
    return [this.slot, this.started, +this.startTime.toFixed(4), v ? v.unit.id : 0, v ? v.generation : 0, v ? this.grabs.indexOf(v.grab) : -1, v ? v.since : 0];
  }

  // A page: the world's state (state()), then follow() each frame. False
  // while whom it holds isn't on this page yet.
  adopt([slot, started, startTime, victimId, victimGeneration, grab, since], units) {
    if (slot !== this.slot || started !== this.started) {
      // The first state a page has stands at once; later ones ease in.
      this.adopted = this.adopted === undefined ? "first" : true;
      this.slot = slot;
      this.loop = slot === 0;
      this.started = started;
      this.startTime = startTime;
    }
    const unit = victimId ? units?.byId?.(victimId) : null;
    const held = unit && unit.generation === victimGeneration ? unit : null;
    if (this.victim && this.victim.unit !== held) {
      const gone = this.victim;
      this.victim = null;
      if (gone.unit.held?.prop === this) release(gone.unit);
      // The swallow: the hold ended with the action, as the world's did.
      if (!held && slot === 0 && this.killSound) emitSound("sound", this.killSound, this.position);
    }
    if (held && !this.victim && this.grabs[grab]) {
      this.victim = { unit: held, generation: victimGeneration, grab: this.grabs[grab], since };
      held.held = { prop: this, grab: this.grabs[grab], since };
      if (held.combat) held.combat.mounted = true;
    }
    return !victimId || Boolean(held);
  }

  // A frame of a page following the world (adopt()): the clip at the
  // world's time for `tick` (fractional, the tick drawn), eased as the
  // world eases it, and whom it holds where it holds him.
  follow(dt, tick) {
    if (!this.pose || this.slot < 0) return;
    // The world's step on the tick it started stepped the clock once.
    const elapsed = Math.max(0, (tick - this.started + 1) * TICK);
    const wanted = this.loop ? this.startTime + elapsed : elapsed;
    const span = length(this.slots[0]);
    this.time = this.loop && span > 0 ? wanted % span : wanted;
    // The weight the world's step for this tick eased by (it rises after
    // each step); the first state a page has (it joined the battle) stands
    // at once.
    const weight = this.adopted === "first" ? 1 : Math.min(1, (this.slot === 0 ? 0.2 : 1) * Math.max(0, tick - this.started) * TICK);
    this.adopted = false;
    if (dt > 0 || weight >= 1) this.stand(weight, dt);
  }

  // Where a page draws whom it holds: his body in his clip, his root on
  // the node (heldFrame), at `time` seconds into the hold.
  presentHeld(tick) {
    const victim = this.victim;
    if (!victim) return null;
    const unit = victim.unit;
    if (unit.generation !== victim.generation || unit.removed || unit.held?.prop !== this) return null;
    // Before the tick it took him (the page draws a little behind the
    // world), he is drawn where his own record has him.
    const held = unit.held;
    if (tick < victim.since) {
      held.frame = null;
      return null;
    }
    held.time = (tick - victim.since) * TICK;
    held.frame = this.heldFrame(unit, held.time, held.matrix ||= new THREE.Matrix4());
    return held.frame;
  }
}

// Let go of a held soldier (Unit.letGo; the page's own soldier's Unit
// too).
export function release(unit) {
  if (!unit?.held) return;
  if (unit.letGo) unit.letGo();
  else unit.held = null;
}

export class LiveObjects {
  constructor(scene, data, pack, { texture, caster, collision = null, triangles = null, explode = () => {}, onDestroyed = () => {}, onServe = () => {}, animationBuffer = null, soldiers = null, bodies = null, Trap = null, padOnGround = null }) {
    this.stations = [];
    this.traps = []; // Endor's log traps (src/traps.js, passed in as Trap)
    this.turrets = [];
    this.objectives = []; // destructible command posts' buildings
    this.buildings = [];
    this.animated = [];
    this.soldiers = soldiers;
    this.bodies = bodies;
    this.holograms = [];
    this.collision = collision;
    this.time = 0; // the shown holograms' clock (present)
    this.center = new THREE.Vector3();
    this.toward = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    // Each model's batches are built once and shared by its rigs.
    const batches = new Map();
    const look = (name, matrix, separate = []) => {
      const model = data.models[name];
      if (!model) return null;
      const key = `${name}:${separate.join(",")}`;
      const rig = buildRig(model, pack.get(name) || [], texture, true, { batches: batches.get(key) || null, separate, caster });
      batches.set(key, rig.batches);
      rig.model = model;
      rig.root.matrixAutoUpdate = false;
      rig.root.matrix.fromArray(matrix);
      scene.add(rig.root);
      return rig;
    };
    const body = (range, owner) => {
      if (!range || !collision || !triangles) return null;
      const [first, count] = range;
      const indices = triangles.indices.subarray(first * 3, (first + count) * 3);
      const polygons = triangles.polygons.subarray(first, first + count);
      return collision.addBody(triangles.positions, indices, polygons, owner);
    };
    for (const desc of data.instances || []) {
      if (!data.models[desc.model]) continue;
      // A turret's drum and gun keep meshes of their own (Gunner.hideRig).
      const s = desc.settings || {};
      const separate = desc.base === "armedbuilding" ? [s.TurretNodeName, s.AimerNodeName].filter(Boolean).map((n) => String(n).toLowerCase()) : [];
      const looks = desc.base === "trap" ? {} : {
        built: look(desc.model, desc.matrix, separate),
        destroyed: desc.destroyed ? look(desc.destroyed.model, desc.matrix) : null,
        unbuilt: desc.unbuilt ? look(desc.unbuilt.model, desc.matrix) : null,
      };
      let hologram = null;
      if (desc.unbuilt?.holo) {
        const image = desc.unbuilt.holo.image ? (pack.get(desc.unbuilt.holo.image) || []).map((part) => ({ geometry: part.geometry, map: texture(part.texture) })) : null;
        hologram = new BuildHologram(desc.unbuilt.hologram, desc.unbuilt.holo, image);
        scene.add(hologram.group);
        this.holograms.push(hologram);
      }
      const bodies = {};
      if (desc.base === "trap") {
        // Its logs, always drawn, and its build pad (src/traps.js); the
        // unbuilt and wreck looks the class names are the same model.
        if (!Trap) continue;
        const pad = desc.trap?.pad;
        const padMatrix = pad ? (padOnGround ? padOnGround(desc.matrix, pad.at, collision, this.hit) : null) : null;
        const trapLooks = { built: look(desc.model, desc.matrix), pad: pad ? look(pad.model, padMatrix ? padMatrix.toArray() : desc.matrix) : null };
        const trap = new Trap({ ...desc, padMatrix: padMatrix?.toArray() }, trapLooks, bodies, hologram, this.clipsFor(data, desc.model, animationBuffer));
        trap.onExplode = explode;
        trap.onDestroyed = onDestroyed;
        this.buildings.push(trap);
        this.traps.push(trap);
        continue;
      }
      if (desc.base === "animatedprop" || desc.base === "door") {
        bodies.built = body(desc.collision, null);
        const prop = new AnimatedProp(desc, looks.built, bodies.built, this.clipsFor(data, desc.model, animationBuffer), this.animated.length);
        if (bodies.built) bodies.built.owner = prop;
        this.animated.push(prop);
        continue;
      }
      const Kind = desc.base === "powerupstation" ? PowerupStation : desc.base === "armedbuilding" ? Turret : desc.base === "commandpost" ? ObjectivePost : null;
      if (!Kind) continue;
      const building = Kind === ObjectivePost
        ? new ObjectivePost(desc, looks, bodies, hologram, this.clipsFor(data, desc.model, animationBuffer))
        : new Kind(desc, looks, bodies, hologram);
      bodies.built = body(desc.collision, building);
      bodies.destroyed = body(desc.destroyed?.collision, building);
      bodies.unbuilt = body(desc.unbuilt?.collision, building);
      building.show(building.state);
      building.onExplode = explode;
      building.onDestroyed = onDestroyed;
      this.buildings.push(building);
      if (Kind === PowerupStation) {
        building.onServe = onServe;
        this.stations.push(building);
      }
      else if (Kind === ObjectivePost) this.objectives.push(building);
      else this.turrets.push(building);
    }
  }

  // A model's clips, loaded once (props.json's animations, props_anims.bin).
  clipsFor(data, model, animationBuffer) {
    this.clipCache ||= new Map();
    if (!this.clipCache.has(model)) {
      const meta = data.animations?.[model];
      this.clipCache.set(model, meta && animationBuffer
        ? loadClips(meta, animationBuffer, (data.models[model]?.bones || []).map((b) => b.name))
        : null);
    }
    return this.clipCache.get(model);
  }

  // The world's tick. target: whoever the droids may serve (PowerupStation).
  // tick: the world's.
  update(dt, target, tick = 0) {
    for (const station of this.stations) station.update(dt, target, this.vehicleTargets);
    for (const turret of this.turrets) {
      turret.regenerate(dt);
      if (!turret.gunner && turret.alive) turret.aimGun(dt, null);
    }
    if ((this.animated || []).length) {
      const positions = this.soldiers?.() || [];
      const bodies = this.bodies?.() || [];
      for (const prop of this.animated) prop.step(dt, positions, bodies, tick);
    }
    for (const trap of this.traps || []) trap.step(dt, tick, this.vehicleTargets || []);
  }

  // The traps' sides and their DeathEffect (src/traps.js): sideOf(team)
  // gives a team's side ("all", "imp"...), teamOf(side) the team of one.
  // A trap placed with Team 0 belongs to its ResetTeam's side.
  attachTraps({ sideOf, teamOf, effect }) {
    for (const trap of this.traps || []) {
      trap.sideOf = sideOf;
      if (effect) trap.onEffect = effect;
      if (!trap.team && trap.settings.resetTeam) trap.team = teamOf(trap.settings.resetTeam) || 0;
    }
  }

  // A frame of what is shown, from the tick's state: the droids posed (and
  // their idle cycle stepped), the unmanned turrets' drums and guns from
  // their angles, and the build holograms. viewer: { camera, canRepair }
  // (the holograms show to a soldier with a repair weapon). A page showing
  // a worker's battle calls this alone; the state arrives as plain fields.
  // The animated props are presentAnimated()'s, earlier in the frame.
  present(dt, viewer = null) {
    this.time += dt;
    for (const station of this.stations) if (station.alive) station.pose(dt);
    for (const turret of this.turrets || []) if (!turret.gunner && turret.alive) turret.present();
    for (const objective of this.objectives || []) objective.present(dt);
    if (viewer) for (const hologram of this.holograms) hologram.update(dt, this.time, viewer.canRepair, viewer.camera);
  }

  // A page's frame of the animated props, before the bodies are drawn: a
  // creature that seizes follows the world's state at `tick` (the world
  // tick drawn, fractional) and places whom it holds; the rest step here
  // from the soldiers this page has.
  presentAnimated(dt, tick = 0) {
    for (const trap of this.traps || []) trap.present(tick);
    if (!(this.animated || []).length) return;
    const positions = this.soldiers?.() || [];
    const bodies = this.bodies?.() || [];
    for (const prop of this.animated) {
      // Until the world's state arrives a creature idles here, setting
      // nothing off.
      if (prop.grabs.length && prop.adopted !== undefined) {
        prop.follow(dt, tick);
        prop.presentHeld(tick);
      } else if (prop.grabs.length) prop.step(dt, positions, [], Math.floor(tick));
      else prop.step(dt, positions, bodies, Math.floor(tick));
    }
  }

  // The animated prop holding a unit, or null.
  holding(unit) {
    return unit?.held?.prop && this.animated.includes(unit.held.prop) ? unit.held.prop : null;
  }

  // The droid serving the soldier at `position` now, or null (the HUD's
  // supply indicator; see PowerupStation.supplying).
  supplying(position) {
    for (const station of this.stations) if (station.supplying(position)) return station;
    return null;
  }

  // A blast: damage to each built building in its reach by BuildingScale
  // (src/ordnance.js), unless terrain or another building shields it.
  // teamScale(building): the battle's friendly fire factor for the owner.
  blast(explosion, position, owner = null, teamScale = null) {
    for (const building of this.buildings) {
      if (!building.alive) continue;
      const radius = building.blastSphere(this.center);
      const damage = blastDamage(explosion, blastDistance(position, this.center, radius), "building");
      if (!(damage > 0) || blastBlocked(this.collision, position, this.center, building.bodies[building.state])) continue;
      building.damage(damage * (teamScale ? teamScale(building) : 1), owner);
    }
  }

  // A round that hit a building's collision.
  // direction: the round's, when known (what the AI hears its hit from).
  struck(body, def, owner = null, teamScale = null, direction = null) {
    const building = body?.owner;
    if (!(building instanceof Building)) return;
    const typed = healthScale(def, "building");
    const team = teamScale ? teamScale(building) : 1;
    building.damage((def.Damage || 0) * typed * team, owner, { direction, amount: (def.Damage || 0) * team, typed, generic: def.ShieldScale ?? 1 });
  }

  // The building a repair weapon locks onto, following the original game: of
  // those it can work on, the one scoring lowest at 16 x miss^2 + distance
  // along the aim, inside a cone of slope LockOnAngle from the eye and within
  // LockOnRange of its sphere. A held trigger keeps the current one with the
  // cone widened by 45 degrees.
  lockOn(eye, dir, def, current, held, team = this.playerTeam) {
    const range = def.LockOnRange || 0;
    const slope = ((def.LockOnAngle || 0) * Math.PI) / 180;
    const score = (building, widen) => {
      if (!(building.alive || building.state === "destroyed" || building.state === "unbuilt")) return null;
      if (!this.repairs(def, building, team)) return null;
      const r = building.sphere(this.center);
      const d = this.toward.subVectors(this.center, eye);
      const t = d.dot(dir);
      if (t < 0 || t > r + range) return null;
      if (d.length() > r + range) return null;
      const miss2 = d.addScaledVector(dir, -t).lengthSq();
      if (miss2 > (r + t * (slope + widen)) ** 2) return null;
      // Nothing in the way but the building itself.
      const to = this.toward.subVectors(this.center, eye);
      const span = to.length();
      if (span > 1e-3) {
        const hit = this.collision?.cast(eye, to.divideScalar(span), span, this.hit);
        if (hit && hit.body?.owner !== building && hit.distance < span - r) return null;
      }
      return 16 * miss2 + t;
    };
    let best = null;
    let bestScore = Infinity;
    if (current) {
      const kept = score(current, held ? LOCK_HELD : 0);
      if (kept !== null) {
        best = current;
        bestScore = kept;
      }
    }
    for (const candidates of [this.buildings, this.vehicleTargets || []]) for (const building of candidates) {
      if (building === current) continue;
      const s = score(building, 0);
      if (s !== null && s < bestScore) {
        best = building;
        bestScore = s;
      }
    }
    return best;
  }

  // Whether a repair weapon can work on a building as it is: it gives that
  // kind something, the building still needs it, and the building is on a
  // side the weapon serves (turrets and droids belong to no one).
  repairs(def, building, team = this.playerTeam) {
    const [amountKey] = REPAIR[building.healthType] || [];
    const amount = def[amountKey] || 0;
    if (!amount) return false;
    if (building.healthType === "vehicle" && (def.TargetVehicle === 0 || !building.alive || building.team !== team && def.TargetEnemy === 0)) return false;
    if (amount > 0 && building.health >= building.maxHealth) return false;
    return building.team === 0 ? def.TargetNeutral !== 0 : building.team !== team ? def.TargetEnemy !== 0 : def.TargetFriendly !== 0;
  }

  // A repair weapon's tick on a building. Returns whether it gave anything.
  repair(def, building, team = this.playerTeam) {
    if (!building || !this.repairs(def, building, team)) return false;
    const [amountKey] = REPAIR[building.healthType];
    building.setHealth(building.health + def[amountKey]);
    return true;
  }

  // The turret a soldier would get into: the original game picks, among those
  // close by, the one nearest the line of the soldier's aim, within reach
  // ahead of him (score 16 x miss^2 + distance along the aim).
  enterable(eye, dir, may = null) {
    let best = null;
    let bestScore = Infinity;
    const c = new THREE.Vector3();
    for (const turret of this.turrets) {
      if (!turret.alive || (turret.gunner && !may?.(turret))) continue;
      // The turret's bounding sphere: about 2.25 m across its drum and gun.
      c.set(0, 1.65, 0.32).applyMatrix4(turret.matrix);
      const reach = 2.25;
      const toC = c.clone().sub(eye);
      const t = toC.dot(dir);
      if (eye.distanceTo(turret.position) > reach + 3.5) continue;
      if (t < 0 || t > reach + 5) continue;
      const miss = toC.addScaledVector(dir, -t).length();
      if (miss > reach + t) continue;
      const score = 16 * miss * miss + t;
      if (score < bestScore) {
        bestScore = score;
        best = turret;
      }
    }
    return best;
  }
}
