import * as THREE from "three";
import { SoldierCamera } from "./camera.js";
import { blastBlocked, blastDamage, blastDistance, falloff } from "./ordnance.js";
import { Arsenal } from "./weapons.js";
import { captureOrdnanceOwner } from "./ordnance-owner.js";

// The recon droid, as the 2004 game flies it (WeaponRemote, EntityDroid,
// WeaponDestruct).
//
// The scout's remote weapon throws it like a grenade (the press, 0.3 s to
// the release, 1.4 s in all): it leaves the gun origin at 10 m/s along the
// aim plus the soldier's velocity, upright and facing the aim, and with
// SwitchImmediately the player flies it at once. One at a time: while it
// lives the remote weapon's fire button flies it again, and its reload button
// blows it up. Flying it, the soldier stands where he was, without input,
// and his death sends control back (the droid flies on, unpiloted).
//
// It hovers on a spring: its origin wants to be SetAltitude above what is
// below its collision sphere (terrain, buildings, water), lifted at
// (SetAltitude - height) x LiftSpring + 9.8 x f, less LiftDamp x its speed
// along the ground's normal, where f fades from 1 at that height to 0 nine
// times higher; a look-ahead ray along the velocity keeps it off what it is
// heading into above 5 m/s. Driving moves it toward MaxSpeed forward and
// MaxStrafeSpeed sideways at up to Acceleraton x f. It only yaws, at
// MaxTurnSpeed; the mouse tips its aim within PitchLimits. It loses AddHealth
// (1/s), so it lasts at most 40 s, and hitting something faster than 7.5 m/s
// (10 on the ground) costs 10 x the excess squared. The use key or a
// next-weapon press hands control back; it can be flown again later.
//
// Its blaster fires from its origin at the point under the crosshair. Its
// destruct weapon starts a 3 s countdown on a press (letting go doesn't stop
// it); then SalvoCount bolts of StrikeOrdnanceName fall 0.75 s apart from
// AimDistance away at AimElevation, from the world's +z, scattered by
// ScatterDistance, onto the spot under the droid, and when the salvo is done
// the droid is destroyed. Dying, it sets off its ExplosionName.

const BODY = new THREE.Vector3(0, 0.155, 0); // the collision sphere's centre in the droid's frame
export const BODY_RADIUS = 0.271;
const GRAVITY = 9.8;
const LOOKAHEAD = 5; // m/s of horizontal speed above which it looks ahead
const FRICTION = 0.28125;
const BOUNCE = 0.125;
const LOOK_SCALE = 2048;
const AIM_REACH = 512; // m
const EYE_BACK = 0.3; // m behind the eye point in first person
const EYE_BACK_ZOOMED = 0.5;
const FLOOR = -500;
const UP = new THREE.Vector3(0, 1, 0);

export class RemoteDroids {
  // model(name) makes a model; explode(explosion, position, owner) sets off
  // an explosion; fire(def, origin, dir) fires a round.
  constructor({ scene, camera, collision, water = null, lights, model, firePoints = {}, explode, fire, onFired, owner = "player" }) {
    this.onFired = onFired;
    this.owner = owner;
    this.scene = scene;
    this.camera = camera;
    this.collision = collision;
    this.water = water;
    this.lights = lights;
    this.model = model;
    this.firePoints = firePoints;
    this.explode = explode;
    this.fire = fire;
    this.droids = [];
    this.piloted = null; // the droid the player flies
    this.view = null;
    this.lookX = 0;
    this.lookY = 0;
    this.zoomStep = 0;
    this.hoverTime = 0;
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3(), terrain: false, body: null };
    this.v = [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()];
    this.closest = new THREE.Vector3();
    this.strikes = [];
  }

  // Each owner has independent control and camera state. World resources and
  // gameplay implementations remain shared; callers advance each context once.
  createContext(owner, { camera = null, aimCamera = new THREE.PerspectiveCamera(), random } = {}) {
    const context = new RemoteDroids({
      scene: this.scene, camera, collision: this.collision,
      water: this.water, lights: this.lights, model: this.model,
      firePoints: this.firePoints, explode: this.explode, fire: this.fire, onFired: this.onFired, owner,
    });
    context.aimCamera = aimCamera;
    context.random = random;
    return context;
  }

  get active() {
    return Boolean(this.piloted);
  }

  // The live droid a remote weapon launched, if any.
  of(weapon) {
    return this.droids.find((droid) => droid.weapon === weapon && !droid.dead) || null;
  }

  // A thrown droid. Returns it.
  launch(weapon, position, velocity, yaw, team, admission = undefined) {
    const token = admission === undefined ? (this.admitLaunch?.(weapon) ?? true) : admission;
    if (token === false) return null;
    let droid;
    try {
      droid = this.createDroid(weapon.def.droid, weapon, position, velocity, yaw, team);
      this.commitLaunch?.(droid, token);
    } catch (error) {
      this.cancelLaunch?.(token);
      if (droid?.mesh) this.scene.remove(droid.mesh);
      throw error;
    }
    this.droids.push(droid);
    if (weapon.def.SwitchImmediately) this.enter(droid);
    return droid;
  }

  // Shared construction for authority and semantic presentation. It does not
  // advance physics, take control, fire, or register a gameplay launch.
  createDroid(def, weapon, position, velocity, yaw, team) {
    const make = def.GeometryName ? this.model(def.GeometryName) : null;
    const mesh = make ? make() : null;
    if (mesh) this.scene.add(mesh);
    const droid = {
      def,
      owner: captureOrdnanceOwner(this.owner),
      weapon,
      team,
      mesh,
      position: position.clone(),
      velocity: velocity.clone(),
      yaw,
      pitch: 0,
      health: def.MaxHealth ?? 40,
      dead: false,
      arsenal: new Arsenal((def.weapons || []).map((w) => ({ ...w, ammo: w.ammo }))),
      countdown: -1, // s into the destruct countdown, -1 before it starts
      salvo: null,
      eye: new THREE.Vector3(),
      aimDir: new THREE.Vector3(0, 0, 1),
      target: new THREE.Vector3(),
    };
    this.place(droid);
    this.aim(droid, false);
    return droid;
  }

  enter(droid) {
    if (!droid || droid.dead) return;
    this.piloted = droid;
    const d = droid.def;
    const center = d.TrackCenter || d.EyePointOffset || [0, 0.5, 0];
    this.view = this.camera ? new SoldierCamera(this.camera, this.collision, {
      cameras: { STAND: { EyePointOffset: d.EyePointOffset || center, TrackCenter: center, TrackOffset: d.TrackOffset || [0, -0.25, 1.5], TiltValue: d.TiltValue ?? 5 } },
      ThirdPersonFOV: d.ThirdPersonFOV ?? 48,
      FirstPersonFOV: d.FirstPersonFOV ?? 48,
    }) : null;
    if (this.aimCamera) this.simulationView = new SoldierCamera(this.aimCamera, this.collision, {
      cameras: { STAND: { EyePointOffset: d.EyePointOffset || center, TrackCenter: center, TrackOffset: d.TrackOffset || [0, -0.25, 1.5], TiltValue: d.TiltValue ?? 5 } },
      ThirdPersonFOV: d.ThirdPersonFOV ?? 48,
      FirstPersonFOV: d.FirstPersonFOV ?? 48,
    });
    this.lookX = 0;
    this.lookY = 0;
    this.zoomStep = 0;
  }

  exit() {
    if (this.piloted?.mesh) this.piloted.mesh.visible = !this.piloted.dead;
    this.piloted = null;
    this.view = null;
    this.simulationView = null;
    this.zoomStep = 0;
  }

  // Blows a droid up: its death explosion, gone.
  kill(droid) {
    if (droid.dead) return;
    droid.dead = true;
    droid.health = 0;
    if (this.piloted === droid) this.exit();
    if (droid.def.explosion) this.explode(droid.def.explosion, droid.position.clone(), droid.owner);
    if (droid.mesh) this.scene.remove(droid.mesh);
  }

  look(dx, dy, sensitivity, invert) {
    this.lookX += dx * sensitivity;
    this.lookY += (invert ? dy : -dy) * sensitivity;
  }

  get zoom() {
    const blaster = this.piloted?.arsenal.current(0);
    return this.zoomStep && blaster ? blaster.def.ZoomMin || 1 : 1;
  }

  // One frame for every droid; the piloted one takes the player's input.
  // input: { forward, right, turn }; controls: the player's buttons.
  control(controls) {
    const piloted = this.piloted;
    if (piloted) {
      if (controls.take("use") || controls.take("nextPrimary") || controls.take("previousPrimary") || controls.take("nextSecondary")) this.exit();
      else if (controls.take("zoom") && (piloted.arsenal.current(0)?.def.ZoomMax || 1) > 1) this.zoomStep = this.zoomStep ? 0 : 1;
    }
  }

  update(dt, input, controls) {
    this.control(controls);
    for (const droid of this.droids) {
      if (droid.dead) continue;
      const flying = droid === this.piloted;
      this.fly(droid, dt, flying ? input : null);
      if (droid.dead) continue;
      this.aim(droid, flying);
      for (const weapon of droid.arsenal.weapons) if (!weapon.random && this.random) weapon.random = this.random;
      this.shoot(droid, dt, flying ? controls : null);
      // AddHealth drains it.
      droid.health += (droid.def.AddHealth ?? 0) * dt;
      if (droid.health <= 0 || droid.position.y < FLOOR) this.kill(droid);
      this.place(droid);
    }
    this.droids = this.droids.filter((droid) => !droid.dead || droid.salvo);
    this.updateStrikes(dt);
  }

  // The hover, the drive, the turn, and what it runs into.
  fly(droid, dt, input) {
    const d = droid.def;
    const [c, n, ray, frame, dv] = this.v;
    const v = droid.velocity;
    const altitude = d.SetAltitude ?? 1.75;
    c.copy(BODY).add(droid.position);
    const reach = BODY_RADIUS + 10 * altitude;

    // The height of its origin over what is below, and that surface's normal.
    let height = reach - BODY.y;
    n.copy(UP);
    const below = this.collision.cast(c, ray.set(0, -1, 0), reach, this.hit);
    if (below) {
      height = below.distance - BODY.y;
      n.copy(below.normal);
    }
    if (this.water?.over(c.x, c.z) && c.y - this.water.level < height + BODY.y) {
      height = Math.min(height, c.y - this.water.level - BODY.y);
      n.copy(UP);
    }
    // Looking ahead, faster than 5 m/s.
    if (Math.hypot(v.x, v.z) > LOOKAHEAD) {
      ray.set(v.x, v.y - reach, v.z);
      const length = ray.length();
      const ahead = this.collision.cast(c, ray.divideScalar(length), length, this.hit);
      if (ahead) {
        const along = frame.subVectors(ahead.point, c);
        height = Math.min(height, -along.dot(ahead.normal));
        if (ahead.normal.y > 0) n.lerp(ahead.normal, (1 - ahead.distance / length) * ahead.normal.y).normalize();
      }
    }
    const f = THREE.MathUtils.clamp(1 - (height - altitude) / (9 * altitude), 0, 1);
    v.y -= THREE.MathUtils.lerp(1, d.GravityScale ?? 1, f) * GRAVITY * dt;
    const lift = (altitude - height) * (d.LiftSpring ?? 5) + GRAVITY * f - v.dot(n) * (d.LiftDamp ?? 2);
    if (lift > 0) v.addScaledVector(n, lift * dt);

    // Driving: toward the wanted speeds in the plane of the ground.
    const forward = frame.set(Math.sin(droid.yaw), 0, Math.cos(droid.yaw));
    forward.addScaledVector(n, -forward.dot(n)).normalize();
    const right = ray.crossVectors(forward, n).normalize();
    const fwd = input?.forward || 0;
    const side = input?.right || 0;
    const along = v.dot(forward);
    const across = v.dot(right);
    const most = (d.Acceleraton ?? 70) * f * Math.max(n.y, 0) * dt;
    const step = (want, now) => THREE.MathUtils.clamp(want - now, -most, most);
    v.addScaledVector(forward, step((d.MaxSpeed ?? 9) * fwd, along));
    v.addScaledVector(right, step((d.MaxStrafeSpeed ?? 7) * side, across));

    // Turning: the keys at MaxTurnSpeed, the mouse as the soldier's does.
    const turnRate = d.MaxTurnSpeed ?? 2;
    if (input) {
      droid.yaw -= (input.turn || 0) * turnRate * dt;
      droid.yaw -= this.lookX * (turnRate / LOOK_SCALE);
      const [low, high] = (d.PitchLimits || [-50, 50]).map((x) => (x * Math.PI) / 180);
      droid.pitch = THREE.MathUtils.clamp(droid.pitch + (this.lookY * (turnRate / LOOK_SCALE)) / this.zoom, low, high);
      this.lookX = 0;
      this.lookY = 0;
    }

    droid.position.addScaledVector(v, dt);
    this.collide(droid);
  }

  // The collision sphere against buildings and terrain: crashing too fast
  // hurts, and it slides off what it meets.
  collide(droid) {
    const d = droid.def;
    const [c, n, , , rel] = this.v;
    const v = droid.velocity;
    const [base, slope] = d.CollisionThreshold || [7.5, 2.5];
    const scale = Array.isArray(d.CollisionScale) ? d.CollisionScale[0] : d.CollisionScale ?? 10;
    for (let pass = 0; pass < 4; pass++) {
      c.copy(BODY).add(droid.position);
      let depth = 0;
      let best = Infinity;
      for (const polygon of this.collision.gather(c, BODY_RADIUS)) {
        for (const tri of polygon.triangles) {
          tri.closestPointToPoint(c, this.closest);
          const distance = this.closest.distanceTo(c);
          if (distance < BODY_RADIUS && distance < best && distance > 1e-5) {
            best = distance;
            n.subVectors(c, this.closest).divideScalar(distance);
          }
        }
      }
      if (best < Infinity) depth = BODY_RADIUS - best;
      else {
        const ground = this.collision.cast(c, rel.set(0, -1, 0), BODY_RADIUS, this.hit);
        if (!ground) break;
        depth = BODY_RADIUS - ground.distance;
        n.copy(ground.normal);
      }
      if (depth <= 0) break;
      droid.position.addScaledVector(n, depth);
      const approach = v.dot(n);
      if (approach >= 0) continue;
      const speed = -approach;
      const threshold = base + slope * n.dot(UP);
      if (speed > threshold) this.hurt(droid, scale * (speed - threshold) ** 2);
      v.addScaledVector(n, -approach);
      const length = v.length();
      if (length > 1e-6) v.multiplyScalar(1 - Math.min(1, (FRICTION * speed) / length));
      v.addScaledVector(n, BOUNCE * speed);
    }
  }

  hurt(droid, amount) {
    if (droid.dead || !(amount > 0)) return;
    droid.health -= amount;
    if (droid.health <= 0) this.kill(droid);
  }

  // A blast near a droid (src/ordnance.js): damage by DroidScale and the
  // push a soldier takes, (away + 0.6 up) x Push x falloff, unless terrain or
  // a building shields it.
  blast(explosion, position) {
    for (const droid of this.droids) {
      if (droid.dead) continue;
      const c = this.v[0].copy(BODY).add(droid.position);
      const distance = blastDistance(position, c, BODY_RADIUS);
      const damage = blastDamage(explosion, distance, "droid");
      const push = (explosion.Push || 0) * falloff(distance, explosion.PushRadiusInner || 0, explosion.PushRadiusOuter || 0);
      if (!(damage > 0 || push > 0) || blastBlocked(this.collision, position, c)) continue;
      if (push > 0) {
        const away = this.v[1].subVectors(c, position);
        if (away.lengthSq() > 1e-6) droid.velocity.addScaledVector(away.normalize(), push);
        droid.velocity.y += 0.6 * push;
      }
      this.hurt(droid, damage);
    }
  }

  // Its eye and aim, and the point under the crosshair.
  aim(droid, flying) {
    const eye = droid.eye.fromArray(droid.def.EyePointOffset || [0, 0.5, 0]).add(droid.position);
    const p = droid.pitch;
    droid.aimDir.set(Math.sin(droid.yaw) * Math.cos(p), Math.sin(p), Math.cos(droid.yaw) * Math.cos(p));
    let from = eye;
    if (flying && (this.simulationView || this.view) && !this.firstPerson && this.zoom <= 1) {
      // From the camera brought level with the eye.
      from = this.v[3].copy((this.aimCamera || this.camera).position);
      from.addScaledVector(droid.aimDir, -from.clone().sub(eye).dot(droid.aimDir));
    }
    const hit = this.collision.cast(from, droid.aimDir, AIM_REACH, this.hit);
    droid.target.copy(from).addScaledVector(droid.aimDir, hit ? hit.distance : AIM_REACH);
  }

  // The blaster and the destruct countdown.
  shoot(droid, dt, controls) {
    const pressedPrimary = controls ? controls.take("firePrimary") : false;
    const pressedSecondary = controls ? controls.take("fireSecondary") : false;
    const input = {
      primary: { held: Boolean(controls?.held("firePrimary")) || pressedPrimary, pressed: pressedPrimary },
      secondary: { held: false, pressed: false },
      reload: false,
    };
    const aim = { stick: 0, posture: "stand", zoomed: this.zoom > 1, blocked: false };
    for (const shot of droid.arsenal.update(dt, input, aim)) {
      const def = shot.weapon.def;
      if (!def.ordnance) continue;
      const point = this.firePoints[droid.def.GeometryName];
      // Use simulation pose even in a worker, where there is no model.
      const at = this.v[2].set(0, 0, 0);
      if (point) at.fromArray(point).applyAxisAngle(UP, droid.yaw);
      at.add(droid.position);
      const dir = this.v[4].subVectors(droid.target, at).normalize();
      dir.applyAxisAngle(UP, shot.yaw);
      dir.applyAxisAngle(this.v[3].set(dir.z, 0, -dir.x).normalize(), -shot.pitch);
      this.fire(def.ordnance, at, dir, droid.owner);
      this.onFired?.(shot, at, droid);
      if (point) {
        this.lights.flash(at, [1, 0.75, 0.75], def.FlashLightRadius || 0, def.FlashLightDuration || 0);
      }
    }
    // The destruct weapon: a press starts the countdown.
    const destruct = droid.arsenal.current(1);
    if (destruct && droid.countdown < 0 && pressedSecondary && destruct.def.strike) droid.countdown = 0;
    if (droid.countdown >= 0 && !droid.salvo) {
      droid.countdown += dt;
      if (droid.countdown >= (destruct?.def.MaxPressedTime || 3)) this.strike(droid, destruct.def);
    }
  }

  // The orbital strike onto the spot under the droid.
  strike(droid, def) {
    const p = this.v[0].copy(BODY).add(droid.position);
    const elevation = ((def.AimElevation ?? 90) * Math.PI) / 180;
    const azimuth = ((def.AimAzimuth ?? 0) * Math.PI) / 180;
    // The strike's frame: z toward where the bolts come from.
    const z = new THREE.Vector3(Math.sin(azimuth) * Math.cos(elevation), Math.sin(elevation), Math.cos(azimuth) * Math.cos(elevation));
    const x = new THREE.Vector3(Math.cos(azimuth), 0, -Math.sin(azimuth));
    const y = new THREE.Vector3().crossVectors(z, x);
    droid.salvo = { def, at: p.clone(), x, y, z, left: def.SalvoCount || 1, wait: 0 };
    this.strikes.push(droid);
  }

  updateStrikes(dt) {
    for (const droid of this.strikes) {
      const s = droid.salvo;
      s.wait -= dt;
      while (s.left > 0 && s.wait <= 0) {
        const scatter = s.def.ScatterDistance || 0;
        const start = s.at.clone()
          .addScaledVector(s.x, scatter * ((this.random || Math.random)() - (this.random || Math.random)()))
          .addScaledVector(s.y, scatter * ((this.random || Math.random)() - (this.random || Math.random)()))
          .addScaledVector(s.z, s.def.AimDistance ?? 100);
        this.fire(s.def.strike, start, s.z.clone().negate(), droid.owner);
        s.left -= 1;
        s.wait += s.def.SalvoDelay || 0;
      }
      if (s.left <= 0) {
        droid.salvo = null;
        this.kill(droid);
      }
    }
    this.strikes = this.strikes.filter((droid) => droid.salvo);
  }

  // The model at the droid, upright, facing its way.
  place(droid) {
    if (!droid.mesh) return;
    droid.mesh.position.copy(droid.position);
    droid.mesh.rotation.set(0, droid.yaw, 0);
  }

  // Cosmetic hover only: leave the retail spring, collision and eye alone.
  present(dt, enhanced, firstPerson) {
    this.hoverTime += dt;
    for (const droid of this.droids) {
      if (!droid.mesh) continue;
      this.place(droid);
      droid.mesh.visible = !droid.dead && !(droid === this.piloted && (firstPerson || this.zoom > 1));
      if (!enhanced) continue;
      const t = this.hoverTime + droid.position.x * 0.13 + droid.position.z * 0.09;
      droid.mesh.position.y += Math.sin(t * 2.6) * 0.045;
      droid.mesh.rotation.x = Math.sin(t * 1.7) * 0.025;
      droid.mesh.rotation.z = Math.sin(t * 2.1) * 0.035;
    }
  }

  // The camera on the flown droid: its chase camera, or its eye in first
  // person (or zoomed): 0.3 m behind the eye point along the aim (0.5 m
  // zoomed), looking along the aim.
  updateCamera(dt, firstPerson, simulation = false) {
    const droid = this.piloted;
    const view = simulation ? this.simulationView : this.view;
    if (!droid || !view) return;
    this.firstPerson = firstPerson;
    const zoom = this.zoom;
    const own = firstPerson || zoom > 1;
    const eye = own ? this.v[1].copy(droid.eye).addScaledVector(droid.aimDir, zoom > 1 ? -EYE_BACK_ZOOMED : -EYE_BACK) : null;
    view.update(dt, droid.position, droid.yaw, droid.pitch, "stand", zoom, own, eye);
  }
}
