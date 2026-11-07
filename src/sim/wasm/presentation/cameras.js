import {sceneComponent,sceneProperty,sceneTransform} from '../../../rust-render/scene-changes.js';
// One camera phase over reusable generated records. These bindings gather
// retained scene/input facts and publish the native result; no view decides
// policy or advances a clock in JavaScript.
import * as THREE from "three";
import { SOURCE as NativeView } from "../../../rust-render/camera-layout.js";
import {ShotDirector} from "../../../spawn-screen/page/director.js";
import { drawnKiller } from "../../../presentation/camera-facts.js";
import { VERSION, CameraFrame as F, CameraCommand as C, CameraCommandKind as K, CameraOutput as O, CameraLimits as L, CameraMode as Mode, CameraEye as Eye, CameraMotion as Motion } from "../../../presentation/layout.js";
const POSTURES = ["stand", "crouch", "prone"], AIR = [null, "jump", "fall", "thrown", "jet"];
const MOTIONS = { [Motion.ROLL_LEFT]:"RollLeft", [Motion.ROLL_RIGHT]:"RollRight", [Motion.STAND_TO_PRONE]:"StandToProne", [Motion.CROUCH_TO_PRONE]:"CrouchToProne", [Motion.PRONE_TO_STAND]:"ProneToStand", [Motion.CROUCH_TO_STAND]:"CrouchToStand", [Motion.LAND]:"Land" };
const DEG = Math.PI / 180;
function encode(value) {
  return JSON.stringify(value, (key, value) => Object.is(value, -0) ? "__camera_negative_zero__" : value).replaceAll('"__camera_negative_zero__"', '-0.0');
}
function vector(a, at, p) { a[at] = p.x; a[at + 1] = p.y; a[at + 2] = p.z; }
function quaternion(a, at, q) { a[at] = q.x; a[at + 1] = q.y; a[at + 2] = q.z; a[at + 3] = q.w; }
// A retained Camera is a matrix consumer. Three's unconditional camera
// update hooks must not rebuild a view that the native phase just published.
function publishedMatrix() {}
// Retain the native publication independently of scratch reused by commands.
// Rendering imports these f64 records without reading matrices back from Three.
function nativeView(camera,o,world,inverse,lens=true){
  let a=camera.userData.nativeView;if(!a)camera.userData.nativeView=a=new Float64Array(NativeView.LEN);
  for(let i=0;i<16;i++){a[NativeView.WORLD+i]=o[world+i];a[NativeView.INVERSE+i]=o[inverse+i];if(lens){a[NativeView.PROJECTION+i]=o[O.PROJECTION+i];a[NativeView.PROJECTION_INVERSE+i]=o[O.PROJECTION_INVERSE+i];}}
}

// Native Euler mirrors keep Three readers consistent without invoking its
// quaternion callbacks, which would otherwise redo matrix/trigonometric work.
function publishTurn(camera, output, at, euler) {
  const q=camera.quaternion,r=camera.rotation;
  q._x=output[at];q._y=output[at+1];q._z=output[at+2];q._w=output[at+3];
  r._x=output[euler];r._y=output[euler+1];r._z=output[euler+2];
}
function definition(def) {
  const sections = [], source = def.cameras || {};
  for (const zoom of [false, true]) for (const name of ["STAND", "CROUCH", "PRONE"]) {
    const s = zoom && source[name + "ZOOM"] || source[name] || source.STAND || {};
    sections.push({ eye: s.EyePointOffset || s.TrackCenter || [0, 1.8, 0], center: s.TrackCenter || s.EyePointOffset || [0, 1.8, 0], offset: s.TrackOffset || [0, 0, 3.5], tilt: (s.TiltValue ?? 5) * DEG });
  }
  return { third: (def.ThirdPersonFOV ?? 65) * DEG, first: (def.FirstPersonFOV ?? 70) * DEG, sections };
}
// This replaces SoldierCamera's page handle. Commands at class/spawn changes
// register content once; the frame reads scalars from the retained handle.
class SoldierView {
  constructor(owner, def) { this.owner = owner; this.camera = owner.camera; this.responsive = true; this.setDef(def); }
  setDef(def) {
    this.sections = def.cameras || {}; this.hfov = (def.ThirdPersonFOV ?? 65) * DEG; this.firstFov = (def.FirstPersonFOV ?? 70) * DEG;
    this.id = this.owner.register(def); this.reset();
  }
  reset() { this.fresh = true; this.resetPending = true; this.reach = 1; this.fov = this.hfov; }
  applyFov() { this.owner.projectLens(this.fov, this.camera); }
}
class ShakeHandle {
  constructor(owner) { this.owner = owner; this.total = 0; this.active = false; this.random = Math.random; }
  add(amount, length) {
    if (!(amount > 0)) return false;
    const o = this.owner, at = o.command(K.ADD_SHAKE); if (at < 0) return false;
    o.commands[at + C.AMOUNT] = amount; o.commands[at + C.LENGTH] = length; return amount > 0;
  }
  explode(def, position, target) {
    if (!target) return 0;
    const o = this.owner, at = o.command(K.EXPLOSION); if (at < 0) return 0;
    const a = o.commands;
    if (Array.isArray(position)) { a[at + C.POSITION] = position[0]; a[at + C.POSITION + 1] = position[1]; a[at + C.POSITION + 2] = position[2]; }
    else vector(a, at + C.POSITION, position);
    vector(a, at + C.TARGET, target.position); a[at + C.RADIUS] = target.radius || 0;
    a[at + C.AMOUNT] = def?.Shake || 0; a[at + C.LENGTH] = def?.ShakeLength ?? 0.5;
    a[at + C.INNER] = def?.ShakeRadiusInner || 0; a[at + C.OUTER] = def?.ShakeRadiusOuter || 0;
    return 0;
  }
  clear() { this.owner.commandCount = 0; this.owner.command(K.CLEAR_SHAKE); this.total = 0; this.active = false; }
}
export class PageCameras {
  constructor(instance, collision, camera, def, motions, authority = false, shots = []) {
    this.automaticEye = true; this.authority = authority; sceneComponent(camera.rotation,"order","=","YXZ"); this.bindCamera(camera);
    this.instance = instance; this.x = instance.exports; this.collision = collision; this.camera = camera;
    this.encoder = new TextEncoder(); this.ids = new WeakMap(); this.commandCount = 0; this.skipped = 0;
    const keys = Array.from({length:L.MOTIONS}, (_, index) => motions?.[MOTIONS[index]] || { Vec: [], Slope: [], Ore: [], RSlope: [], Time: [] });
    const bytes = this.encoder.encode(encode({ definitions: [], motions: keys, spawnShots: shots }));
    const at = this.x.sim_scratch(bytes.length); new Uint8Array(this.x.memory.buffer, at, bytes.length).set(bytes);
    this.handle = this.x.camera_init(at, bytes.length);
    if (!this.handle) throw Error("camera content rejected");
    this.addresses = [this.x.camera_input_ptr(this.handle), this.x.camera_commands_ptr(this.handle), this.x.camera_output_ptr(this.handle), this.x.camera_flags_ptr()];
    this.refresh(); this.view = new SoldierView(this, def); this.shake = new ShakeHandle(this);
    // The imported random primitive uses the battle's injectable cosmetic
    // stream. It draws only when the native shake actually turns the view.
    if (!authority) instance.holder.cameraRandom = () => this.shake.random();
    this.director = new ShotDirector(this, shots); this.remoteDefs = new WeakMap(); this.remote = null; this.remoteDef = -1; this.turret = null; this.turretId = 0;
    this.vehicleView = {lookAt: new THREE.Vector3()}; this.p = new THREE.Vector3();
    this.killer = { point: new THREE.Vector3(), size: 1.9, living: false, friendly: false };
    this.from = new THREE.Vector3(); this.to = new THREE.Vector3(); this.warned = false;
  }
  // graphics, foliage and effects call these Camera hooks unconditionally.
  // This camera is already published. The renderer owns derived views and
  // imports this publication without asking Three to recompute a matrix.
  bindCamera(camera) {
    camera.updateMatrixWorld=publishedMatrix;camera.updateWorldMatrix=publishedMatrix;
  }
  refresh() {
    const memory = this.x.memory.buffer; if (this.memory === memory) return;
    this.memory = memory; this.input = new Float64Array(memory, this.addresses[0], F.STRIDE);
    this.commands = new Float64Array(memory, this.addresses[1], L.COMMANDS * C.STRIDE);
    this.output = new Float64Array(memory, this.addresses[2], O.STRIDE); this.flags = new Uint8Array(memory, this.addresses[3], L.BODIES);
  }
  register(def) {
    if (this.ids.has(def)) return this.ids.get(def);
    const bytes = this.encoder.encode(encode(definition(def))), at = this.x.sim_scratch(bytes.length);
    new Uint8Array(this.x.memory.buffer, at, bytes.length).set(bytes);
    const id = this.x.camera_add_definition(this.handle, at, bytes.length); this.ids.set(def, id); this.refresh(); return id;
  }
  command(kind) {
    this.refresh();
    // Four shakes can survive a frame. A long paused event burst can fill
    // this much larger command prefix; dropping its tail cannot fail a frame.
    if (this.commandCount === L.COMMANDS) { this.skipped++; return -1; }
    const at = this.commandCount++ * C.STRIDE; this.commands.fill(0, at, at + C.STRIDE); this.commands[at + C.KIND] = kind; return at;
  }
  seat(a, view, turret = false) {
    const eye = turret ? F.TURRET_EYE : F.SEAT_EYE, center = turret ? F.TURRET_CENTER : F.SEAT_CENTER, offset = turret ? F.TURRET_OFFSET : F.SEAT_OFFSET;
    vector(a, eye, view.eye); vector(a, center, view.center); vector(a, offset, view.offset);
    a[turret ? F.TURRET_TILT : F.SEAT_TILT] = view.tilt;
    a[turret ? F.TURRET_FIRST : F.SEAT_FIRST] = view.firstFov; a[turret ? F.TURRET_THIRD : F.SEAT_THIRD] = view.thirdFov;
    const rates = turret ? this.turretRates : view.moveTension, at = turret ? F.TURRET_RATES : F.SEAT_RATES;
    for (let k = 0; k < 6; k++) a[at + k] = rates[k];
    a[turret ? F.TURRET_AIM : F.SEAT_AIM] = view.aimTension || 30;
  }
  ignore(a, bodies) {
    if (!bodies) return;
    if (!Array.isArray(bodies)) { a[F.IGNORED] = bodies.raySlot ?? -1; a[F.IGNORED_COUNT] = 1; return; }
    const count = Math.min(L.IGNORED_BODIES, bodies.length); a[F.IGNORED_COUNT] = count;
    for (let k = 0; k < count; k++) a[F.IGNORED + k] = bodies[k]?.raySlot ?? -1;
  }
  // game.js formerly selected and updated each view here. This bridge packs
  // all candidate facts once; the native phase chooses exactly one owner.
  // Death must win even during the packet that still holds a mounted lease.
  // Recon wins over a vehicle, and a vehicle wins over an emplacement.
  // Raw soldier posture and displayed posture deliberately travel separately:
  // the former selects its camera section, the latter drives motion edges.
  // Authored resource ids are resolved only when a class or lease changes.
  // The module can grow on those cold events, so typed views rebind first.
  // No vectors, matrices, lists or closures are constructed by this method.
  // The authority clock uses the same kernel with a separate retained owner.
  // It never advances the display's motion, shake or death-shot followers.
  // A network correction can replay that clock without moving the shown view.
  prepareFrame(dt, frozen, feet, state, soldier, combat, ownEyes, firstPerson, eye, deathView, deathShot, remotes, driver, gunner, ribcage, style, shot) {
    // Rebind after cold registrations, which may have grown the module.
    if (remotes.piloted !== this.remote) {
      this.remote = remotes.piloted;
      if (this.remote) {
        this.remoteDef = this.remoteDefs.get(this.remote.def) ?? -1;
      }
      this.remoteReset = true;
    }
    if (gunner.turret !== this.turret) {
      this.turret = gunner.turret; this.turretId++;
      if (this.turret) this.turretRates = this.turret.camera.moveTension;
    }
    const collision = this.collision; collision.rayBodies(); this.refresh(); const a = this.input, camera = this.camera;
    a[F.PROJECT_ONLY] = 0; a[F.WORLD_START] = 0; a[F.REMOTE_BACK] = 0; a[F.VERSION] = VERSION; a[F.AUTHORITY] = this.authority ? 1 : 0; a[F.DT] = dt; a[F.FROZEN] = frozen ? 1 : 0;
    a[F.DEATH_ACTIVE] = deathView.active ? 1 : 0; a[F.REMOTE_ACTIVE] = remotes.active ? 1 : 0;
    a[F.VEHICLE_ACTIVE] = driver?.active ? 1 : 0; a[F.TURRET_ACTIVE] = gunner.active ? 1 : 0;
    a[F.OWN_EYES] = ownEyes ? 1 : 0; a[F.FIRST_PERSON] = firstPerson ? 1 : 0;
    a[F.BODY_POSTURE] = POSTURES.indexOf(state.posture);
    vector(a, F.BODY_POSITION, state.position);
    a[F.POSTURE] = POSTURES.indexOf(soldier.air ? "stand" : soldier.posture); a[F.AIR] = AIR.indexOf(state.air); a[F.DIVE] = state.dive || 0;
    vector(a, F.VELOCITY, state.velocity); vector(a, F.FEET, feet); a[F.YAW] = state.aimYaw; a[F.PITCH] = state.aimPitch;
    a[F.ZOOM] = combat.zoom; a[F.HAS_EYE] = ownEyes ? 1 : 0; a[F.EYE_KIND] = Eye.PROVIDED;
    if (ownEyes && this.automaticEye) {
      a[F.EYE_KIND] = state.mode !== undefined ? Eye.DROIDEKA : this.authority ? Eye.BASIS : Eye.INFANTRY;
      a[F.EYE_HEIGHT] = state.eyeHeight || 0; a[F.EYE_ROLLED] = state.rolled ? 1 : 0;
      if (this.authority && state.mode === undefined) {
        vector(a, F.EYE_PIVOT, combat.pivot); vector(a, F.EYE_ORIGIN, combat.origin);
        vector(a, F.EYE_LEFT, combat.left); vector(a, F.EYE_DIRECTION, combat.aimDir);
      }
    } else if (ownEyes) vector(a, F.EYE, eye);
    a[F.SOLDIER_DEF] = this.view.id; a[F.SOLDIER_RESET] = this.view.resetPending ? 1 : 0; this.view.resetPending = false; a[F.RESPONSIVE] = this.view.responsive ? 1 : 0;
    vector(a, F.CAMERA_POSITION, camera.position); quaternion(a, F.CAMERA_QUATERNION, camera.quaternion); vector(a, F.CAMERA_UP, camera.up);
    a[F.CAMERA_FOV] = camera.fov; a[F.CAMERA_ASPECT] = camera.aspect; a[F.CAMERA_NEAR] = camera.near; a[F.CAMERA_FAR] = camera.far;
    a[F.WORLD] = collision.rayWorld; a[F.IGNORED_COUNT] = 0;
    // Body flags are keyed by stable slot. Values that did not change leave
    // the native mirror alone; ray candidates never cause another crossing.
    for (let k = 0; k < collision.bodies.length; k++) {
      const body = collision.bodies[k], id = body.raySlot;
      if (id >= 0 && id < L.BODIES) { const enabled = body.enabled ? 1 : 0; if (this.flags[id] !== enabled) this.flags[id] = enabled; }
    }
    if (this.authority && deathView.active) {
      a[F.DEATH_HFOV] = deathView.hfov; a[F.WORLD_DEATH_TIME] = deathView.time;
      vector(a, F.WORLD_DEATH_POSITION, deathView.position); quaternion(a, F.WORLD_DEATH_QUATERNION, deathView.quaternion);
      vector(a, F.WORLD_DEATH_BODY, deathView.body); vector(a, F.WORLD_DEATH_ANCHOR, deathView.anchor);
      a[F.WORLD_DEATH_KILLED] = deathView.killed ? 1 : 0; a[F.WORLD_HAS_FEET] = this.hasFeet ? 1 : 0;
      a[F.WORLD_HAS_KILLER] = this.killerEyes ? 1 : 0; if (this.killerEyes) vector(a, F.WORLD_KILLER, this.killerEyes);
    } else if (deathView.active) {
      a[F.DEATH_SERIAL] = deathView.deaths; a[F.DEATH_HFOV] = deathView.hfov; a[F.DEATH_CLASSIC] = style === "classic" ? 1 : 0;
      a[F.HAS_CHEST] = ribcage ? 1 : 0;
      if (ribcage) { this.p.setFromMatrixPosition(ribcage.matrixWorld); vector(a, F.CHEST, this.p); }
      const killer = drawnKiller(deathView, deathShot.refs, style === "classic", this.killer);
      a[F.KILLER_PRESENT] = killer ? 1 : 0;
      if (killer) { vector(a, F.KILLER_POINT, killer.point); a[F.KILLER_SIZE] = killer.size; a[F.KILLER_LIVING] = killer.living ? 1 : 0; a[F.KILLER_FRIENDLY] = killer.friendly ? 1 : 0; }
    }
    if (remotes.active && this.remote) {
      const d = this.remote, zoom = remotes.zoom, own = firstPerson || zoom > 1;
      const saved = this.authority ? remotes.simulationView : null;
      a[F.REMOTE_OVERRIDE] = saved ? 1 : 0;
      if (saved) { a[F.REMOTE_FOV] = saved.fov; a[F.REMOTE_REACH] = saved.reach; a[F.REMOTE_FRESH] = saved.fresh ? 1 : 0; }
      a[F.REMOTE_DEF] = this.remoteDef; a[F.REMOTE_RESET] = this.remoteReset ? 1 : 0; this.remoteReset = false;
      vector(a, F.REMOTE_FEET, d.position); a[F.REMOTE_YAW] = d.yaw; a[F.REMOTE_PITCH] = d.pitch; a[F.REMOTE_ZOOM] = zoom; a[F.REMOTE_FIRST] = own ? 1 : 0; a[F.REMOTE_HAS_EYE] = own ? 1 : 0;
      if (own) { vector(a, F.REMOTE_EYE, d.eye); vector(a, F.REMOTE_AIM, d.aimDir); a[F.REMOTE_BACK] = zoom > 1 ? 0.5 : 0.3; }
    }
    if (driver?.active) this.vehicle(a, driver);
    if (gunner.active) this.gunner(a, gunner);
    a[F.HAS_SHOT] = shot ? 1 : 0;
    a[F.DIRECTOR] = shot?.director ? 1 : 0;
    if (shot?.director) {a[F.DIRECTOR_AGE]=shot.age;a[F.DIRECTOR_REVISION]=shot.revision;}
    else if (shot) { vector(a, F.SHOT_POSITION, shot.position); quaternion(a, F.SHOT_QUATERNION, shot.quaternion); }
    a[F.COMMANDS] = this.commandCount; this.commandCount = 0;
  }
  frame(dt, frozen, feet, state, soldier, combat, ownEyes, firstPerson, eye, deathView, deathShot, remotes, driver, gunner, ribcage, style, shot) {
    this.prepareFrame(dt,frozen,feet,state,soldier,combat,ownEyes,firstPerson,eye,deathView,deathShot,remotes,driver,gunner,ribcage,style,shot);
    this.x.camera_frame(this.handle);
    return this.publishFrame(firstPerson,deathView,deathShot,remotes,driver,gunner,this.camera);
  }
  publishFrame(firstPerson,deathView,deathShot,remotes,driver,gunner,camera=this.camera) {
    this.refresh();
    const o = this.output;
    if (o[O.STATUS]) throw Error("camera presentation layout mismatch");
    // Publish the pre-shake view before arms, charge and the death trace.
    // Final records are applied after those consumers finish, without a call.
    if (!this.authority) {
    if (o[O.TRACE_VISIBLE]) deathShot.trace.show(this.from.fromArray(o, O.TRACE_FROM), this.to.fromArray(o, O.TRACE_TO), !!o[O.TRACE_FRIENDLY], o[O.TRACE_HEAD], o[O.TRACE_FADE], o[O.TRACE_SINCE], camera);
    else deathShot.trace.hide();
    }
    sceneTransform(camera.position.fromArray(o, O.PRE_POSITION)); publishTurn(camera,o,O.PRE_QUATERNION,O.PRE_EULER); camera.up.fromArray(o, O.UP);
    camera.fov = o[O.FOV]; camera.near = o[O.NEAR]; camera.far = o[O.FAR];
    sceneTransform(camera.matrix.fromArray(o, O.PRE_MATRIX)); sceneTransform(camera.matrixWorld.fromArray(o, O.PRE_MATRIX)); camera.matrixWorldInverse.fromArray(o, O.PRE_INVERSE);
    camera.projectionMatrix.fromArray(o, O.PROJECTION); camera.projectionMatrixInverse.fromArray(o, O.PROJECTION_INVERSE);
    nativeView(camera,o,O.PRE_MATRIX,O.PRE_INVERSE);
    this.view.fov = o[O.SOLDIER_FOV]; this.view.reach = o[O.SOLDIER_REACH]; this.view.fresh = !!o[O.SOLDIER_FRESH];
    if (this.authority && deathView.active) {
      deathView.time = o[O.WORLD_DEATH_TIME]; sceneTransform(deathView.position.fromArray(o, O.WORLD_DEATH_POSITION)); sceneTransform(deathView.quaternion.fromArray(o, O.WORLD_DEATH_QUATERNION));
      deathView.previousPosition.fromArray(o, O.WORLD_PREVIOUS_POSITION); deathView.previousQuaternion.fromArray(o, O.WORLD_PREVIOUS_QUATERNION); deathView.body.fromArray(o, O.WORLD_DEATH_BODY);
    } else if (!this.authority) { deathShot.active = !!o[O.DEATH_ACTIVE]; deathShot.time = o[O.DEATH_TIME]; deathShot.known = !!o[O.DEATH_KNOWN]; }
    if (this.authority && remotes.active && remotes.simulationView) {
      remotes.simulationView.fov=o[O.REMOTE_FOV];remotes.simulationView.reach=o[O.REMOTE_REACH];remotes.simulationView.fresh=!!o[O.REMOTE_FRESH];
    }
    this.shake.total = o[O.SHAKE_TOTAL]; this.shake.active = !!o[O.SHAKE_ACTIVE];
    if (o[O.MODE] === Mode.REMOTE) {
      remotes.firstPerson = firstPerson;
      if (!this.authority && remotes.view) {
        remotes.view.fov=o[O.REMOTE_FOV];remotes.view.reach=o[O.REMOTE_REACH];remotes.view.fresh=!!o[O.REMOTE_FRESH];
      }
    }
    if (o[O.MODE] === Mode.VEHICLE) {
      driver.vehicle.viewFirstPerson = firstPerson;
      if (this.authority) driver.vehicle.aimCamera = camera;
      else { driver.cameraView = this.vehicleView; this.vehicleView.lookAt.fromArray(o, O.LOOK_AT); }
      const handle = this.authority ? driver.aimView : driver;
      handle.cameraSet = !!o[O.VEHICLE_SET]; handle.speedEffect = o[O.SPEED_EFFECT];
      if (!this.authority) driver.hideHull(firstPerson && o[O.ENCLOSED] ? driver.vehicle : null);
    }
    if (o[O.MODE] === Mode.TURRET) {
      if (this.authority) gunner.firstPerson = firstPerson;
      else {
        gunner.viewFirstPerson = firstPerson;
        gunner.hideRig(firstPerson && o[O.TURRET_ENCLOSED] ? gunner.turret : null);
        const root = gunner.animator?.actor?.root; if (root) sceneProperty(root,"visible",!gunner.turret.hidesGunner);
      }
    }
    if (!this.warned && (this.skipped || o[O.SKIPPED])) { this.warned = true; console.warn("camera presentation capacity reached"); }
    return o[O.CUE];
  }
  // game.js applied shake, a possible spawn override, then updated matrices.
  // The phase has already calculated both endpoints in that exact order.
  // Arms and trace consume the pre-shake record before this final publication.
  // Rendering, spatial sound and camera history consume the final record.
  // There is no second native call and no matrix recomputation at this point.
  finish() {
    this.refresh();
    const o = this.output, c = this.camera;
    sceneTransform(c.position.fromArray(o, O.POSITION)); publishTurn(c,o,O.QUATERNION,O.EULER);
    sceneTransform(c.matrix.fromArray(o, O.MATRIX)); sceneTransform(c.matrixWorld.fromArray(o, O.MATRIX)); c.matrixWorldInverse.fromArray(o, O.INVERSE);
    nativeView(c,o,O.MATRIX,O.INVERSE,false);
  }
  // Remote definitions are registered by immutable content identity.
  // New droid lives reuse the same entry rather than consuming another slot.
  // Registering is cold; no class strings enter the native frame kernel.
  remoteDefinition(droid) {
    const d=droid.def;
    let id=this.remoteDefs.get(d); if(id!==undefined){if(this.displayOwner)this.displayOwner.remoteDefinition(droid);return id;}
    const center=d.TrackCenter||d.EyePointOffset||[0,0.5,0];
    id=this.register({cameras:{STAND:{EyePointOffset:d.EyePointOffset||center,TrackCenter:center,TrackOffset:d.TrackOffset||[0,-0.25,1.5],TiltValue:d.TiltValue??5}},ThirdPersonFOV:d.ThirdPersonFOV??48,FirstPersonFOV:d.FirstPersonFOV??48});
    this.remoteDefs.set(d,id);if(this.displayOwner)this.displayOwner.remoteDefinition(droid);return id;
  }
  // Cold projection/death commands and prediction replay have no display dt.
  // Clear every mode flag before their record reuses the authority buffer.
  // These helpers are not calls from the displayed per-frame camera path.
  // A projection restore must not accidentally replay a queued display event.
  base(camera,dt) {
    this.collision.rayBodies();this.refresh();const a=this.input;
    a[F.VERSION]=VERSION;a[F.DT]=dt;a[F.AUTHORITY]=1;a[F.FROZEN]=0;a[F.COMMANDS]=0;a[F.PROJECT_ONLY]=0;a[F.WORLD_START]=0;
    a[F.SOLDIER_RESET]=0;a[F.REMOTE_RESET]=0;a[F.DEATH_ACTIVE]=0;a[F.REMOTE_ACTIVE]=0;a[F.VEHICLE_ACTIVE]=0;a[F.TURRET_ACTIVE]=0;a[F.HAS_SHOT]=0;a[F.OWN_EYES]=0;
    vector(a,F.CAMERA_POSITION,camera.position);quaternion(a,F.CAMERA_QUATERNION,camera.quaternion);vector(a,F.CAMERA_UP,camera.up);
    a[F.CAMERA_FOV]=camera.fov;a[F.CAMERA_ASPECT]=camera.aspect;a[F.CAMERA_NEAR]=camera.near;a[F.CAMERA_FAR]=camera.far;
    a[F.WORLD]=this.collision.rayWorld;a[F.IGNORED_COUNT]=0;
    for(let k=0;k<this.collision.bodies.length;k++){const body=this.collision.bodies[k],id=body.raySlot;if(id>=0&&id<L.BODIES){const enabled=body.enabled?1:0;if(this.flags[id]!==enabled)this.flags[id]=enabled;}}
    return a;
  }
  projectRemote(context) { this.projectLens(context.simulationView.fov, context.aimCamera); }
  // A network recon restore supplies a complete pose and lens checkpoint.
  // Composing that restored pose in the native projection command keeps its
  // aim matrix valid even when there are no subsequent replay ticks.
  projectLens(fov,camera) {
    const a=this.base(camera,0);
    a[F.PROJECT_ONLY]=1;a[F.REMOTE_FOV]=fov;
    this.x.camera_frame(this.handle); this.refresh();const o=this.output;
    camera.fov=o[O.FOV];camera.projectionMatrix.fromArray(o,O.PROJECTION);camera.projectionMatrixInverse.fromArray(o,O.PROJECTION_INVERSE);
    sceneTransform(camera.matrix.fromArray(o,O.MATRIX));sceneTransform(camera.matrixWorld.fromArray(o,O.MATRIX));camera.matrixWorldInverse.fromArray(o,O.INVERSE);
  }
  // RemoteContext.updateCamera also runs during private prediction replay.
  // Continuation scalars belong to that context, not the shown recon lease.
  // Restore them explicitly before each replayed tick and publish them once.
  // Reusing this authority owner avoids an unbounded store of remote cameras.
  // The rendered page still chooses recon through its ordinary coarse phase.
  remoteFrame(context,dt,first) {
    const d=context.piloted;if(!d)return;
    const saved=context.simulationView,camera=context.aimCamera,id=this.remoteDefinition(d),a=this.base(camera,dt),zoom=context.zoom,own=first||zoom>1;
    a[F.REMOTE_ACTIVE]=1;a[F.REMOTE_DEF]=id;a[F.REMOTE_RESET]=0;a[F.REMOTE_OVERRIDE]=1;
    a[F.REMOTE_FOV]=saved.fov;a[F.REMOTE_REACH]=saved.reach;a[F.REMOTE_FRESH]=saved.fresh?1:0;
    vector(a,F.REMOTE_FEET,d.position);a[F.REMOTE_YAW]=d.yaw;a[F.REMOTE_PITCH]=d.pitch;a[F.REMOTE_ZOOM]=zoom;a[F.REMOTE_FIRST]=own?1:0;a[F.REMOTE_HAS_EYE]=own?1:0;
    vector(a,F.REMOTE_EYE,d.eye);vector(a,F.REMOTE_AIM,d.aimDir);a[F.REMOTE_BACK]=zoom>1?0.5:0.3;
    this.x.camera_frame(this.handle); this.refresh();const o=this.output;
    sceneTransform(camera.position.fromArray(o,O.POSITION));publishTurn(camera,o,O.QUATERNION,O.EULER);camera.up.fromArray(o,O.UP);camera.fov=o[O.FOV];camera.near=o[O.NEAR];camera.far=o[O.FAR];
    sceneTransform(camera.matrix.fromArray(o,O.MATRIX));sceneTransform(camera.matrixWorld.fromArray(o,O.MATRIX));camera.matrixWorldInverse.fromArray(o,O.INVERSE);camera.projectionMatrix.fromArray(o,O.PROJECTION);camera.projectionMatrixInverse.fromArray(o,O.PROJECTION_INVERSE);
    saved.fov=o[O.REMOTE_FOV];saved.reach=o[O.REMOTE_REACH];saved.fresh=!!o[O.REMOTE_FRESH];context.firstPerson=first;
  }
  // DeathCamera.start is a semantic lifecycle edge, independent of drawing.
  // Native code derives the anchor and post-hit lens before identity capture.
  // The host retains only those scalar fields needed by packets and killer
  // lookup; the next authority phase owns all time and pose advancement.
  beginDeath(death,from,feet,killed,hfov,deadDt) {
    const a=this.base(from,0);a[F.DEATH_ACTIVE]=1;a[F.WORLD_START]=1;vector(a,F.FEET,feet);a[F.WORLD_DEATH_KILLED]=killed?1:0;a[F.DERIVE_DEATH_FOV]=hfov===null?1:0;a[F.DEATH_HFOV]=hfov??0;a[F.START_DEAD_DT]=deadDt;
    this.x.camera_frame(this.handle); this.refresh();const o=this.output;
    death.time=0;death.hfov=o[O.WORLD_DEATH_HFOV];sceneTransform(death.position.fromArray(o,O.WORLD_DEATH_POSITION));sceneTransform(death.quaternion.fromArray(o,O.WORLD_DEATH_QUATERNION));death.previousPosition.copy(death.position);death.previousQuaternion.copy(death.quaternion);death.body.fromArray(o,O.WORLD_DEATH_BODY);death.anchor.fromArray(o,O.WORLD_DEATH_ANCHOR);
  }
  // VehicleDriver.updateCamera formerly queried seatFrame and seatForward.
  // Canonical F2 seat records are already native and must retain their tick.
  // Displayed hulls instead provide their interpolated root and raw angles.
  // Rust reconstructs the heading-only repulsor frame and turret-seat pivot.
  // The rider head is a node produced by the preceding body-pose phase.
  // First person can use that head without reposing its skeleton here.
  // A flyer keeps its displayed frame separately from the flight controller:
  // flight banking and look angles must not be folded into the wrong clock.
  // Preview angles are input facts, not a second camera integrator.
  // A seat's authored section is immutable for the life of its class.
  // The resulting hull visibility is published only if this owner wins.
  vehicle(a, driver) {
    const v = driver.vehicle, index = v.seats[v.seatIndex] ? v.seatIndex : 0, seat = v.seats[index], view = seat.view;
    a[F.VEHICLE_ID] = v.root.id; a[F.VEHICLE_RESET] = (this.authority ? driver.aimView.cameraSet : driver.cameraSet) ? 0 : 1; a[F.VEHICLE_FLYING] = v.flying ? 1 : 0; a[F.VEHICLE_WALKING] = v.walking ? 1 : 0; a[F.VEHICLE_SEAT] = index; a[F.VEHICLE_EYE_HULL] = index === 0 || !view.turret ? 1 : 0;
    a[F.CANONICAL_SEAT] = this.authority && v.system.f2 ? 1 : 0;
    if (a[F.CANONICAL_SEAT]) a.set(v.f2Pose.seats[index].frame.elements, F.VEHICLE_FRAME);
    vector(a, F.SEAT_POSITION, this.authority ? v.position : v.flying ? v.root.position : v.visual);
    vector(a, F.SEAT_UP, view.up); vector(a, F.SEAT_PIVOT, view.pivot);
    a[F.SEAT_YAW] = seat.aimYaw; a[F.SEAT_PITCH] = seat.pitch;
    a.set(v.root.matrixWorld.elements, F.VEHICLE_HULL);
    vector(a, F.VEHICLE_POSITION, this.authority ? v.position : v.flying ? v.root.position : v.visual);
    quaternion(a, F.VEHICLE_DRAWN, v.root.quaternion); a[F.VEHICLE_BANK] = v.bank || 0;
    quaternion(a, F.VEHICLE_FLIGHT_FRAME, this.authority ? v.quaternion : v.flightFrameQ || v.root.quaternion);
    const head = !this.authority && view.rider && driver.actor?.root.visible ? driver.actor.byName?.bone_head : null;
    a[F.VEHICLE_HAS_EYE] = head ? 1 : 0; if (head) { this.p.setFromMatrixPosition(head.matrixWorld); vector(a, F.VEHICLE_EYE, this.p); }
    a[F.CANONICAL_DIRECTION] = v.system.f2 ? 1 : 0;
    if (v.system.f2) vector(a, F.VEHICLE_DIRECTION, v.f2Pose.seats[index].forward);
    const preview = !this.authority && driver.previewVehicle === v && driver.previewSeat === index;
    a[F.VEHICLE_HAS_PREVIEW] = preview ? 1 : 0; if (preview) { a[F.VEHICLE_PREVIEW_YAW] = driver.previewYaw; a[F.VEHICLE_PREVIEW_PITCH] = driver.previewPitch; }
    vector(a, F.VEHICLE_VELOCITY, v.velocity); vector(a, F.VEHICLE_CENTER, v.center); vector(a, F.VEHICLE_SIZE, v.size);
    vector(a, F.VEHICLE_BOUNDS, v.bounds.min); vector(a, F.VEHICLE_BOUNDS + 3, v.bounds.max);
    a[F.VEHICLE_YAW] = this.authority ? v.yaw : v.root.rotation.y; a[F.VEHICLE_TILT] = this.authority ? v.tilt : -v.root.rotation.x; a[F.VEHICLE_ROLL] = this.authority ? v.roll : v.root.rotation.z; a[F.VEHICLE_SPIN] = v.spin || 0;
    a[F.VEHICLE_MAX_SPEED] = v.speed; a[F.VEHICLE_HUMAN] = v.driver ? 1 : 0;
    a[F.VEHICLE_LOOK_YAW] = v.seats[0].lookYaw || 0; a[F.VEHICLE_HAS_LOOK_YAW] = v.seats[0].lookYaw === null ? 0 : 1; a[F.VEHICLE_LOOK_PITCH] = v.seats[0].lookPitch || 0;
    a[F.VEHICLE_BOOSTING] = v.boosting ? 1 : 0; a[F.VEHICLE_AGE] = v.age; a[F.VEHICLE_HEALTH] = v.health;
    a[F.VEHICLE_BLEND_DURATION] = v.flightConfig?.cameraBlendTime || 0; a[F.VEHICLE_BIKE] = v.name.includes("speederbike") ? 1 : 0;
    this.seat(a, view); this.ignore(a, v.body);
  }
  // Gunner.updateCamera formerly rebuilt the emplacement's aiming frame.
  // Its placed root, drum pivot, up and raw preview angles now cross as facts.
  // F2 supplies canonical aim and eye when the turret has a native seat.
  // The phase computes both the lens and the eye-inside-box decision.
  // Hiding the rig remains a retained-scene operation on the host.
  // It happens after selection, so a stale lease cannot hide a death shot.
  // The native prop phase has published the drum and gun for this frame.
  gunner(a, gunner) {
    const t = gunner.turret;
    a[F.TURRET_ID] = this.turretId;
    const root = t.rig.root;
    a.set(root.matrixWorld.elements, F.TURRET_ROOT);
    vector(a, F.TURRET_PIVOT, t.pivot); vector(a, F.TURRET_UP, t.up);
    a[F.TURRET_FACING] = t.facing; a[F.TURRET_YAW] = this.authority ? t.yaw : t.yaw + gunner.previewYaw;
    a[F.TURRET_PITCH] = this.authority ? t.pitch : gunner.shownPitch;
    this.seat(a, t.camera, true);
    a[F.TURRET_CANONICAL_DIRECTION] = a[F.TURRET_CANONICAL_EYE] = t.f2Seats ? 1 : 0;
    if (t.f2Seats) { vector(a, F.TURRET_DIRECTION, t.f2Pose.direction); vector(a, F.TURRET_EYE, t.f2Pose.eye); }
    const box = t.looks?.built?.model?.box; a[F.TURRET_HAS_BOUNDS] = box ? 1 : 0;
    if (box) { a.set(box[0], F.TURRET_BOUNDS); a.set(box[1], F.TURRET_BOUNDS + 3); }
    this.ignore(a, t.bodies?.built || null);
  }
}
