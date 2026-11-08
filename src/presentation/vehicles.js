import {TreePublication} from "../rust-render/pose-streams.js";
import {PoseTreeKind} from "../rust-render/pose-layout.js";
import {sceneProperty,sceneTransform} from "../rust-render/scene-changes.js";
// The page's hulls in one native phase (rust/bfe-presentation vehicles.rs),
// which replaces Vehicle.present's pose (the root between ticks or at its
// record's sample, poseCrew, the gear, seats, barrels, wheels and a dying
// walker). Vehicle.present queues a hull; flush() runs the batch: the roots,
// then the block of rays (walkers' feet, a lifting craft's ground) this
// page's collision answers in one call, then every pose. Each
// hull's tree is the phase's from its first frame: its nodes' matrices (and
// its meshes' bind inverses) are copied from records, three never walks it.
// Back come only the fields other JS reads (root, age, walk, legs, aims).
import * as THREE from "three";
import { gearFollow } from "../flight/gear.js";
import { VERSION, VehicleFrame as F, VehicleEntry as E, VehicleSeatInput as S, VehicleWheelInput as WI, VehicleEntryOutput as O,
  VehicleAimerOutput as AO, VehicleNode as N, VehicleHeader as H, VehicleMode as M, VehicleRoot as R, VehicleFlightState as FS } from "./layout.js";

// The walk pose's gear root; the clips by the roles the kernel takes them
// in (vehicles.rs IDLE..TAKEOFF).
const DUMMY_ROOT = /^dummyroot$/i;
const clipNames = ["idle", "walk_leftfoot_rightfoot", "walk_rightfoot_leftfoot", "leftfoot_up", "rightfoot_up", "death01", "takeoff"];
const clipKeys = ["idle", "left", "right", "liftLeft", "liftRight", "death", "takeoff"];
// A craft's flight state as its gear clock reads it.
const FLIGHT_STATES = Object.freeze({ Landed: FS.LANDED, TakingOff: FS.TAKING_OFF, Flying: FS.FLYING, Landing: FS.LANDING });
const xyz = (v) => [v.x, v.y, v.z];
const xyzw = (q) => [q.x, q.y, q.z, q.w];
// A published tree's root: the scene's update stops here (the phase wrote it).
function published() {}
// JSON with -0 kept (JSON.stringify writes it as 0; the kernel reads -0.0).
const NEGATIVE_ZERO = "~negative zero~";
const exact = (value) => JSON.stringify(value, (_, x) => (Object.is(x, -0) ? NEGATIVE_ZERO : x)).replaceAll(`"${NEGATIVE_ZERO}"`, "-0.0");

export class PageVehicles {
  // loaded: the bfe-presentation module and its memory (loadPresentation).
  constructor(loaded) {
    this.loaded = loaded;
    this.kernel = new loaded.module.PageVehicles();
    // Rigs by their hull's root (a predicted view shares its source's),
    // clip handles by clip.
    this.rigs = new WeakMap();
    this.handles = new Map();
    // The hulls queued this frame, in order, and their rigs and slots.
    this.capacity = this.kernel.entries_capacity();
    this.queue = new Array(this.capacity * 3);
    this.count = 0;
    this.mismatched = 0;
    this.reported = false;
    // The correction his own predicted hull is drawn with (game.js sets it
    // around that hull's batch): x, y, z added to its root.
    this.offset = null;
    this.refresh();
  }
  // The record views, made again when the memory grew (any phase's
  // registration) or this one's records moved (force: a rig registered).
  refresh(force = false) {
    const k = this.kernel,
      b = this.loaded.memory.buffer;
    if (this.buffer === b && !force) return;
    this.buffer = b;
    this.input = new Float64Array(b, k.input_ptr(), k.input_len());
    this.networkStates=new Float64Array(b,k.network_states_ptr(),k.network_states_len());
    this.streamIn = new Float64Array(b, k.stream_in_ptr(), k.stream_in_len());
    this.streamOut = new Float64Array(b, k.stream_out_ptr(), k.stream_out_len());
    this.header = new Float64Array(b, k.header_ptr(), H.STRIDE);
    this.rays = new Float64Array(b, k.rays_ptr(), k.rays_len());
  }
  // A clip's handle and duration, loading it once (null without one).
  clip(clip) {
    if (!clip) return null;
    let handle = this.handles.get(clip);
    if (handle === undefined) {
      handle = this.kernel.load_clip(clip.frames, clip.fps, clip.rootMotion, Int32Array.from(clip.tracks), Float32Array.from(clip.data));
      this.handles.set(clip, handle);
    }
    return { handle, duration: clip.duration };
  }
  // A hull's rig, registered at its first frame from the hull itself (a
  // predicted view draws its source's rig).
  rig(hull) {
    const v = hull.predictionSource || hull;
    let rig = this.rigs.get(v.root);
    if (rig) return rig;
    // A node's bone index (-1: none, or not a bone).
    const index = (node) => (node ? v.bones.indexOf(node) : -1);
    const legs = v.def.fx?.legs;
    // Each seat as poseSeat reads it, its weapons' aimers, and the muzzles
    // recoilBarrels kicks (those with a barrel and a rest).
    const seats = v.seats.map((seat) => ({
      turret: index(seat.view.turret), up: xyz(seat.view.up), yawFactor: seat.view.yawFactor, pitchFactor: seat.view.pitchFactor,
      weapons: seat.weapons.map((w) => w.aimers.map((a) => ({
        bone: index(a.bone), owner: a.owner, yawRate: a.yawRate, pitchRate: a.pitchRate,
        yawLimits: a.yawLimits, pitchLimits: a.pitchLimits, wrapYaw: a.wrapYaw, yaw: a.yaw, pitch: a.pitch,
      }))),
      muzzles: seat.weapons.flatMap((w) => w.muzzles.filter((m) => m.barrel && m.rest)
        .map((m) => ({ barrel: index(m.barrel), rest: xyz(m.rest), recoil: Number(m.recoil) || 0 }))),
    }));
    // Its tree: the root, the bones, then the rest in traverse order.
    const nodes = [v.root, ...v.bones];
    v.root.traverse((o) => { if (o !== v.root && !v.bones.includes(o)) nodes.push(o); });
    const skinned = nodes.filter((o) => o.isSkinnedMesh);
    // The bones as they stand now, their bind, and what the frame poses.
    for(let i=0;i<seats.length;i++){const view=v.seats[i].view;seats[i].rider={node:nodes.indexOf(view.rider?.node),turnRate:view.turnRate,pitchLimits:view.pitchLimits,yawLimits:view.yawLimits||null,wrap:Boolean(view.wrapYaw)};}
    const descriptor = {
      sourceHeight:v.size.y,sourceAnimal:v.healthType==='animal',
      bones: v.bones.map((bone, i) => ({
        parent: index(bone.parent), position: xyz(bone.position), quaternion: xyzw(bone.quaternion), scale: xyz(bone.scale),
        bind: [...xyzw(v.bindQuaternions[i]), ...xyz(v.bindPositions[i])],
      })),
      yxz: v.root.rotation.order === "YXZ", hasPose: Boolean(v.pose), poseRoot: v.pose ? v.pose.root : -1,
      flying: Boolean(v.flying), creature: Boolean(v.creature),
      gearRoot: v.flying ? v.bones.findIndex((bone) => DUMMY_ROOT.test(bone.name)) : -1,
      // The rules its drawn gear follows a replicated flight state by.
      flight: v.flying && v.flightConfig ? { takeoffTime: v.flightConfig.takeoffTime, landingTime: v.flightConfig.landingTime,
        takeoffHeight: v.flightConfig.takeoffHeight, landedHeight: v.flightConfig.landedHeight, gearFoot: v.gearFoot ?? null } : null,
      // liftLegs' legBones: the bones under each pair's two leg bones.
      legs: legs ? {
        rayLength: legs.rayLength, at: legs.pairs.map((pair) => [pair.left.at, pair.right.at]),
        bones: legs.pairs.map((pair) => {
          const under = new Set();
          for (const which of ["left", "right"]) v.byName[pair[which].leg]?.traverse((b) => under.add(b));
          return v.bones.map((b, i) => (under.has(b) ? i : -1)).filter((i) => i >= 0);
        }),
      } : null,
      seats, wheels: v.wheels.map((w) => [index(w.bone), xyz(w.axis)]).filter(([i]) => i >= 0),
      nodes: nodes.slice(1 + v.bones.length).map((o) => ({ parent: nodes.indexOf(o.parent), position: xyz(o.position), quaternion: xyzw(o.quaternion), scale: xyz(o.scale), skinned: Boolean(o.isSkinnedMesh) })),
      deathSpan: v.def.fx?.dying?.span || 0,
    };
    clipNames.forEach((name, i) => (descriptor[clipKeys[i]] = this.clip(v.clips?.get(name))));
    const id = this.kernel.add_rig(exact(descriptor));
    // Capacity grows only at rig birth. Cache it here, never by a WASM
    // query for each hull on the frame path.
    this.capacity = this.kernel.entries_capacity();
    this.queue.length = this.capacity * 3;
    this.refresh(true);
    // What its records were sized to (a hull whose seats or wheels no
    // longer match is not drawn); its aimers and kicking muzzles, shared by
    // the hull and its predicted view; its nodes, now the phase's.
    const pairs = legs ? legs.pairs.length : 0;
    const aimers = v.seats.flatMap((seat) => seat.weapons.flatMap((w) => w.aimers));
    const muzzles = v.seats.flatMap((seat) => seat.weapons.flatMap((w) => w.muzzles.filter((m) => m.barrel && m.rest)));
    for (const o of nodes) sceneProperty(o,"matrixAutoUpdate",sceneProperty(o,"matrixWorldAutoUpdate",false));
    for(const object of nodes)object.updateMatrixWorld=object.updateWorldMatrix=published;
    rig = { id, pairs, aimers, muzzles, nodes, skinned, seats: seats.length, wheels: descriptor.wheels.length,
      frames: [new THREE.Quaternion(), new THREE.Quaternion()], sides: [new Array(pairs).fill(0), new Array(pairs).fill(0)],
      slots: [0, 1].map((slot) => [this.kernel.slot_input(id, slot), this.kernel.slot_output(id, slot)]) };
    // Record offsets within a slot (vehicles.rs Rig::in_* and out_*).
    rig.inSeats = pairs * 3; rig.inKicks = rig.inSeats + rig.seats * S.STRIDE; rig.inWheels = rig.inKicks + muzzles.length;
    rig.outSides = O.STRIDE; rig.outAimers = O.STRIDE + pairs;
    rig.outNodes = rig.outAimers + aimers.length * AO.STRIDE; rig.outSkins = rig.outNodes + nodes.length * N.STRIDE;
    rig.publication=new TreePublication(this,nodes,PoseTreeKind.VEHICLE);
    this.rigs.set(v.root, rig);
    v.nativeDisplay={owner:this,rig};
    return rig;
  }
  // An entry's record, or -1 past the records (degrades: not drawn this
  // frame, counted once).
  entry(hull, mode) {
    const rig = this.rig(hull);
    // Another phase may have grown the memory since: views made again.
    this.refresh();
    const at = this.count;
    if (at >= this.capacity) {
      this.mismatched++;
      return -1;
    }
    // Slot 0: the hull as the world draws it; 1: his predicted view of it.
    const slot = hull.predictionSource ? 1 : 0;
    const q = this.count++ * 3;
    this.queue[q] = hull; this.queue[q + 1] = rig; this.queue[q + 2] = slot;
    // The entry names its rig and its slot's own record words; the kernel
    // poses nothing for any other words.
    const e = F.STRIDE + at * E.STRIDE, input = this.input;
    input[e + E.RIG] = rig.id;
    input[e + E.MODE] = mode;
    input[e + E.INPUT] = rig.slots[slot][0];
    input[e + E.OUTPUT] = rig.slots[slot][1];
    return e;
  }
  // Vehicle.present for a hull alive: its state as this frame draws it.
  alive(v, alpha, dt) {
    return this.write(v, M.ALIVE, alpha, dt, false);
  }
  // A spawn's pose (Vehicle.render(1, 0) at the tick: no time, every seat
  // it holds at rest), drawn at once: spawns arrive between frames.
  spawned(v) {
    const batch = this.count > 0;
    const drawn = this.write(v, M.RENDER, 1, 0, true);
    if (!batch) this.flush();
    return drawn;
  }
  write(v, mode, alpha, dt, render) {
    const e = this.entry(v, mode);
    if (e < 0) return false;
    const x = this.input, rig = this.queue[(this.count - 1) * 3 + 1], slot = this.queue[(this.count - 1) * 3 + 2];
    // The native network link replaces these current source facts only when
    // a historical hull sample exists. Predicted/render slots stay local.
    x[e+E.VELOCITY]=v.velocity.x;x[e+E.VELOCITY+1]=v.velocity.y;x[e+E.VELOCITY+2]=v.velocity.z;
    x[e+E.SPEED]=v.speed;x[e+E.SOURCE_YAW_RATE]=v.yawRate;
    x[e+E.ALPHA]=alpha;x[e+E.DT]=dt;x[e+E.FRAME_DT]=dt;x[e+E.NETWORK]=0;x[e+E.LOCAL_PRESENT]=v.tickFrom&&!v.sleeping?1:0;x[e+E.DEFERRED]=0;
    x[e+E.POSITION]=v.position.x;x[e+E.POSITION+1]=v.position.y;x[e+E.POSITION+2]=v.position.z;
    const r=v.quaternion;x[e+E.QUATERNION]=r.x;x[e+E.QUATERNION+1]=r.y;x[e+E.QUATERNION+2]=r.z;x[e+E.QUATERNION+3]=r.w;
    // Where its last two ticks began (render(): the previous tick's state;
    // a record's sample has none: zeros, unread). A craft's turn there is
    // `flightTickQ || quaternion`, its missing roll its own.
    const from = render ? v.previous : v.tickFrom, tq = (render ? v.flightPreviousQ : v.flightTickQ) || r;
    x[e + E.TICK_FROM] = from ? from.x : 0; x[e + E.TICK_FROM + 1] = from ? from.y : 0; x[e + E.TICK_FROM + 2] = from ? from.z : 0;
    x[e + E.TICK_QUATERNION] = tq.x; x[e + E.TICK_QUATERNION + 1] = tq.y; x[e + E.TICK_QUATERNION + 2] = tq.z; x[e + E.TICK_QUATERNION + 3] = tq.w;
    const tickRoll = render ? v.previousRoll : v.tickFromRoll;
    x[e + E.TICK_YAW] = render ? +v.previousYaw : +v.tickFromYaw; x[e + E.YAW] = v.yaw;
    x[e + E.TICK_TILT] = render ? +v.previousTilt : +v.tickFromTilt; x[e + E.TILT] = v.tilt;
    x[e + E.TICK_ROLL] = v.flying ? tickRoll ?? v.roll : +tickRoll; x[e + E.ROLL] = v.roll;
    // A repulsor's bob, the hull's own clock, the walk cycle and where its
    // last tick began (his own hull), the clock barrels kick back on, and
    // his own hull's correction still being smoothed away.
    x[e + E.HOVER] = v.repulsor && !v.flying ? 1 : 0; x[e + E.HOVER_PHASE] = v.hoverPhase; x[e + E.AGE] = v.age;
    x[e + E.WALK_PHASE] = v.walkPhase; x[e + E.WALK_BLEND] = v.walkBlend;
    x[e + E.WALK_FROM_PHASE] = render || v.walkFromPhase === undefined ? NaN : +v.walkFromPhase; x[e + E.WALK_FROM_BLEND] = +v.walkFromBlend;
    // A craft's gear: its own, whose records it follows, and the flight
    // state its gear clock reads. Where it is (the ground cast under a
    // lifting craft starts there) and its own body, which casts leave out.
    x[e + E.GEAR] = v.gear ?? NaN; x[e + E.GEAR_FOLLOW] = v.flying ? gearFollow(v) : -1; x[e + E.GEAR_GENERATION] = v.aiTarget?.generation ?? 0;
    x[e + E.FLIGHT_STATE] = FLIGHT_STATES[v.flightState] ?? FS.OTHER;
    x[e + E.GROUND_FROM] = v.position.x; x[e + E.GROUND_FROM + 1] = v.position.y; x[e + E.GROUND_FROM + 2] = v.position.z;
    x[e + E.BODY] = v.body?.raySlot ?? -1;
    x[e + E.LEG_SIDES] = v.legSide ? 1 : 0; x[e + E.NOW] = v.system.world.clock();
    const offset = slot === 1 ? this.offset : null;
    x[e + E.OFFSET] = offset ? offset.x : 0; x[e + E.OFFSET + 1] = offset ? offset.y : 0; x[e + E.OFFSET + 2] = offset ? offset.z : 0;
    // The slot's records: leg sides, seats, kicks and wheels.
    const s = this.streamIn, at = rig.slots[slot][0];
    if (v.legSide) for (let i = 0; i < rig.pairs; i++) s[at + i] = v.legSide[i];
    for (let i = 0; i < rig.seats && i < v.seats.length; i++) {
      const seat = v.seats[i], o = at + rig.inSeats + i * S.STRIDE, t = seat.target, held = v.held(i);
      // present() poses the seats someone holds; render() the player's seat
      // and each one a bot crews.
      s[o + S.HELD] = held ? 1 : 0; s[o + S.POSE] = (render ? i === v.seatIndex || v.crewed(i) : held) ? 1 : 0;
      s[o+S.YAW_RATE]=seat.yawRate||0;
      s[o + S.AIM_YAW] = seat.aimYaw; s[o + S.PITCH] = seat.pitch; s[o + S.TARGET] = t ? 1 : 0;
      if (t) { s[o + S.TARGET_POINT] = t.x; s[o + S.TARGET_POINT + 1] = t.y; s[o + S.TARGET_POINT + 2] = t.z; }
    }
    // When each barrel last fired (undefined: never); each wheel's turn.
    for (let i = 0; i < rig.muzzles.length; i++) s[at + rig.inKicks + i] = rig.muzzles[i].kickAt === undefined ? NaN : +rig.muzzles[i].kickAt;
    for (let i = 0; i < rig.wheels && i < v.wheels.length; i++) {
      const wheel = v.wheels[i], o = at + rig.inWheels + i * WI.STRIDE;
      s[o + WI.PHASE] = wheel.phase; s[o + WI.TICK_FROM] = render ? wheel.previous : wheel.tickFrom ?? wheel.phase;
    }
    // A hull whose seats or wheels no longer match its rig poses nothing
    // (never a throw); counted and reported once.
    if (v.seats.length !== rig.seats || v.wheels.length < rig.wheels) {
      x[e + E.MODE] = M.SKIP;
      this.mismatched++;
      return false;
    }
    return true;
  }
  // presentDying: where it was destroyed, in its death clip.
  dying(v) {
    const e = this.entry(v, M.DYING);
    if (e < 0) return false;
    // presentDying shows the root again; the rest is the kernel's.
    sceneProperty(v.root,"visible",true);
    const x = this.input;
    x[e + E.POSITION] = v.position.x; x[e + E.POSITION + 1] = v.position.y; x[e + E.POSITION + 2] = v.position.z;
    x[e + E.TILT] = v.tilt; x[e + E.YAW] = v.yaw; x[e + E.ROLL] = v.roll; x[e + E.DYING] = v.dying;
    x[e + E.OFFSET] = x[e + E.OFFSET + 1] = x[e + E.OFFSET + 2] = 0;
    return true;
  }
  // The batch: roots, the frame's rays (each hull's own body out of them,
  // as feelLegs and heightAbove cast), the poses, then each hull's records
  // written back.
  flush() {
    // Entries in the queue: hull, rig and slot each.
    const n = this.count;
    if (!n) return;
    const k = this.kernel;
    this.input[F.VERSION] = VERSION;
    this.input[F.ENTRIES] = n;
    k.place_hulls();
    this.refresh();
    // A build whose layout disagrees draws no hull (never a throw).
    if (this.header[H.STATUS] !== 0) {
      this.count = 0;
      return;
    }
    // Every ray the kernel asked for, cast in one call on this page's
    // collision; the kernel reads the hits itself.
    if (this.header[H.RAYS] > 0) this.queue[0].system.world.castRays(this.rays, this.header[H.RAYS]);
    k.pose_hulls();
    for (let q = 0; q < n; q++) {
      const v = this.queue[q * 3], rig = this.queue[q * 3 + 1], o = rig.slots[this.queue[q * 3 + 2]][1];
      const mode = this.input[F.STRIDE + q * E.STRIDE + E.MODE];
      // A hull skipped for a mismatch keeps last frame's pose.
      if (mode !== M.SKIP && !this.input[F.STRIDE+q*E.STRIDE+E.DEFERRED]) this.writeBack(v, rig, o, mode);
    }
    this.count = 0;
    // Hulls past the records or with a mismatched rig, reported once.
    if (this.mismatched && !this.reported) {
      this.reported = true;
      console.warn(`vehicle presentation: ${this.mismatched} hull frames not drawn (records or rig mismatch)`);
    }
  }
  // An entry's records into three and the hull: the root's own transform
  // (a ground hull's Euler angles as present() set them; a craft's turn and
  // the frame its chase camera follows), every node's matrices and each
  // mesh's bind inverse; alive, its age, where it is drawn, the walk cycle
  // as drawn (on the hull itself: its feet stomp by it), its leg sides and
  // what each posed seat's aimers shoot by (shotDirection).
  writeBack(v, rig, o, mode) {
    const out = this.streamOut, root = v.root;
    sceneTransform(root.position.set(out[o + O.POSITION], out[o + O.POSITION + 1], out[o + O.POSITION + 2]));
    if (out[o + O.ROOT_MODE] === R.EULER) sceneTransform(root.rotation.set(out[o + O.ROTATION], out[o + O.ROTATION + 1], out[o + O.ROTATION + 2], "YXZ"));
    else if (out[o + O.ROOT_MODE] === R.QUATERNION) {
      sceneTransform(root.quaternion.set(out[o + O.QUATERNION], out[o + O.QUATERNION + 1], out[o + O.QUATERNION + 2], out[o + O.QUATERNION + 3]));
      (v.flightFrameQ ||= rig.frames[v.predictionSource ? 1 : 0]).set(out[o + O.FRAME], out[o + O.FRAME + 1], out[o + O.FRAME + 2], out[o + O.FRAME + 3]);
    }
    rig.publication.publish(out,o+rig.outNodes);
    if (mode === M.DYING) return;
    v.age = out[o + O.AGE];
    v.visual.set(out[o + O.VISUAL], out[o + O.VISUAL + 1], out[o + O.VISUAL + 2]);
    if (mode === M.ALIVE) { const shown = v.predictionSource || v; shown.shownWalkPhase = out[o + O.SHOWN_PHASE]; shown.shownWalkBlend = out[o + O.SHOWN_BLEND]; }
    if (out[o + O.LEG_SIDES]) {
      const sides = (v.legSide ||= rig.sides[v.predictionSource ? 1 : 0]);
      for (let i = 0; i < rig.pairs; i++) sides[i] = out[o + rig.outSides + i];
    }
    for (let i = 0, at = o + rig.outAimers; i < rig.aimers.length; i++, at += AO.STRIDE) {
      if (!out[at + AO.WRITTEN]) continue;
      const a = rig.aimers[i];
      a.onTarget = out[at + AO.ON_TARGET] === 1;
      if (!Number.isNaN(out[at + AO.FACING])) a.facing.set(out[at + AO.FACING], out[at + AO.FACING + 1], out[at + AO.FACING + 2]);
      if (out[at + AO.POSED] >= 0) a.posed = out[at + AO.POSED] === 1;
    }
  }
}
