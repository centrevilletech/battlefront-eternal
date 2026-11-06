// predictVehicle's page half imports facts and publishes one private hull.
// Rust owns eligibility, correction restore, every pending tick and history.
// This object retains presentation storage; it has no simulation tick methods.
import * as THREE from 'three';
import { ClientHullMirror, createClientHullView, putClientHullState, createClientHullStateView, syncClientHullStateViewMath, putClientHullSeat,
  createClientHullSeatView, ClientHullMemberMirror, putClientHullMember, ClientHullRefMirror,
  putClientHullRef, CLIENT_HULL_REF_WORDS, CLIENT_HULL_WORDS, CLIENT_HULL_OFFSETS, CLIENT_HULL_STATE_WORDS, CLIENT_HULL_SEAT_WORDS,
  CLIENT_HULL_MEMBER_WORDS } from '../sim/gen/client.js';
import { putVehicleWheel, syncVehicleWheel, VehicleWheelMirror, VEHICLE_WHEEL_WORDS } from '../sim/gen/vehicles.js';
import { bridgeFor } from '../sim/runtime.js';
import { KIND_BODY } from '../sim/gen/index.js';
const EMPTY = Object.freeze([]);
// Construction copies only mutable math objects. Static definitions and rig
// resources keep their loaded identities; no pending tick visits this helper.
const copy = value => value?.isVector3 || value?.isQuaternion || value?.isMatrix4 || value?.isEuler ? value.clone() : value;

// Renderer/correction fixtures may also need an independent pose facade.
// Its old physics entry points are gone; it can only consume native results.
export function presentationHull(source, bridge = null) {
  const view = createClientHullStateView(bridge ?? source.system.f2?.bridge ?? source.system.predictionContent?.bridge ?? bridgeFor(source.system.collision.fixtureInstance), source);
  // Solver scalars stay in generated typed storage. Renderer-only scratch
  // remains independently owned, while immutable resources retain identity.
  for (const key of ['visual', 'q', 'from', 'legEuler', 'beamFrom', 'aimQ', 'aimQ2', 'recoilAxis', 'recoilPoint', 'flightFrameQ', 'flightLeanQ']) if (source[key]) view[key] = copy(source[key]);
  // Gait endpoints remain in the generated hull header. The renderer reads
  // these cells while native replay retains the actual continuation state.
  const gait = new Float64Array(CLIENT_HULL_WORDS);
  gait[CLIENT_HULL_OFFSETS.walkPhase] = source.walkPhase;
  gait[CLIENT_HULL_OFFSETS.walkBlend] = source.walkBlend;
  gait[CLIENT_HULL_OFFSETS.walkFromPhase] = source.walkFromPhase ?? 0;
  gait[CLIENT_HULL_OFFSETS.walkFromBlend] = source.walkFromBlend ?? 0;
  Object.defineProperties(view, {
    predictionHullWords: { value: gait },
    walkPhase: { configurable: true, get() { return gait[CLIENT_HULL_OFFSETS.walkPhase]; }, set(v) { gait[CLIENT_HULL_OFFSETS.walkPhase] = v; } },
    walkBlend: { configurable: true, get() { return gait[CLIENT_HULL_OFFSETS.walkBlend]; }, set(v) { gait[CLIENT_HULL_OFFSETS.walkBlend] = v; } },
    walkFromPhase: { configurable: true, get() { return gait[CLIENT_HULL_OFFSETS.walkFromPhase]; }, set(v) { gait[CLIENT_HULL_OFFSETS.walkFromPhase] = v; } },
    walkFromBlend: { configurable: true, get() { return gait[CLIENT_HULL_OFFSETS.walkFromBlend]; }, set(v) { gait[CLIENT_HULL_OFFSETS.walkFromBlend] = v; } },
  });
  for (const key of ['hit', 'aimHit', 'unitHit', 'frameScratch']) if (source[key]) {
    const row = view[key] = {}; for (const name of Object.keys(source[key])) row[name] = copy(source[key][name]);
  }
  // Walker support belongs to the private facade too. Sharing this small
  // array would let native publication alter an authoritative contact sample.
  view.legSide = source.legSide?.slice();
  view.wheels = source.wheels.map(wheel => { const row = new VehicleWheelMirror(); for (const key of Object.keys(wheel)) row[key] = copy(wheel[key]); return row; });
  view.seats = source.seats.map(seat => {
    const row = createClientHullSeatView(null, seat);
    row.lease = null;
    return row;
  });
  view.itemTarget = { ...source.itemTarget, center: source.itemTarget.center.clone() };
  view.itemCenter.copy(view.itemTarget.center);
  view.itemTarget.center = view.itemCenter;
  view.driver = {}; view.seatIndex = 0; view.predictionSource = source;
  // Vehicle.present reads this lease state and age in the source's place.
  // Its collider and the authority owner never receive predicted transforms.
  view.held = i => source.held(i); view.age = source.age;
  return view;
}

export class ClientHull {
  constructor(bridge, collision) {
    this.bridge = bridge; this.collision = collision;
    this.source = null; this.view = null; this.generation = 0;
    // Generated mirrors own their vector storage. Reusing a page frame must
    // not leave an output vector borrowed from a detached WASM memory view.
    this.frame = createClientHullView(null, new ClientHullMirror());
    this.reference = new ClientHullRefMirror(); this.setup = new Float64Array(CLIENT_HULL_REF_WORDS);
    this.member = new ClientHullMemberMirror(); this.point = new THREE.Vector3();
    this.member.point = this.point;
    this.oldPosition = new THREE.Vector3(); this.legs = []; this.teams = []; this.teamCount = 0;
    this.members = EMPTY; this.hadPrevious = false; this.content = null;
    this.outputBuffer = null; this.stateSource = null; this.headerSource = null;
    this.seatSources = []; this.legSource = null;
  }
  // predictionVehicle copied renderer scratch as well as motion. Do that only
  // on a source/life transition, preserving the existing drawn rig identities.
  configure(source) {
    if (this.source === source && this.generation === source.aiTarget.generation) return false;
    this.content = source.system.predictionContent ?? source.system.f2;
    if (source.system.predictionContent) this.content.adopt(source);
    this.content.bodiesIn();
    this.source = source; this.generation = source.aiTarget.generation;
    const view = presentationHull(source, this.bridge);
    this.view = view;
    this.legs = new Float64Array(source.def.fx?.legs?.pairs?.length ?? source.legSide?.length ?? 0);
    if (source.legSide) this.legs.set(source.legSide);
    this.outputBuffer = null;
    this.reference.slot = source.simSlot; this.reference.generation = source.simGen;
    putClientHullRef(null, this.reference, this.setup, 0, null);
    this.hadPrevious = false;
    return true;
  }
  // Preserve Units.relation insertion order, including its nullable team.
  // An active count avoids repeatedly discarding and rebuilding array storage.
  invalidate() { this.source = null; this.outputBuffer = null; }
  addTeam(team) {
    for (let i = 0; i < this.teamCount; i++) if (Object.is(this.teams[i], team) || this.teams[i] === team) return;
    this.teams[this.teamCount++] = team;
  }
  // World/collision and remote target facts are sampled once per frame. Aim
  // points are eagerly supplied here so targeting cannot call JS in replay.
  prepare() {
    const source = this.source;
    this.collision.rayBodies();
    if (this.content.bodiesVersion !== this.collision.bodiesVersion) this.content.bodiesIn();
    this.members = source.flying ? source.system.units?.all() ?? EMPTY : EMPTY;
    this.teamCount = 0; this.addTeam(source.team ?? null);
    for (let i = 0; i < this.members.length; i++) this.addTeam(this.members[i].team ?? null);
    // Correction smoothing compares the previous prediction at its old tick,
    // not the newest authority pose. Save that point before publishing again.
    this.oldPosition.copy(this.view.position);
    this.frame.checkTick = this.view.predictionTick ?? 0;
    return CLIENT_HULL_WORDS + CLIENT_HULL_STATE_WORDS + source.seats.length * CLIENT_HULL_SEAT_WORDS
      + source.wheels.length * VEHICLE_WHEEL_WORDS + (source.legSide?.length ?? 0)
      + this.members.length * CLIENT_HULL_MEMBER_WORDS + this.teamCount + this.teamCount * this.teamCount;
  }
  pack(input, at) {
    // Offsets are frame-relative generated spans, so memory growth cannot
    // turn a command or target row into a retained JavaScript reference.
    const start = at, h = this.frame.predictionWords, source = this.source, b = this.bridge;
    at += CLIENT_HULL_WORDS;
    h[CLIENT_HULL_OFFSETS.slot] = source.simSlot; h[CLIENT_HULL_OFFSETS.generation] = source.simGen;
    h[CLIENT_HULL_OFFSETS.stateAt] = at; putClientHullState(b, source, input, at, null); at += CLIENT_HULL_STATE_WORDS;
    h[CLIENT_HULL_OFFSETS.seatsAt] = at; h[CLIENT_HULL_OFFSETS.seatsCount] = source.seats.length;
    for (let i = 0; i < source.seats.length; i++, at += CLIENT_HULL_SEAT_WORDS) putClientHullSeat(null, source.seats[i], input, at, null);
    h[CLIENT_HULL_OFFSETS.wheelsAt] = at; h[CLIENT_HULL_OFFSETS.wheelsCount] = source.wheels.length;
    for (let i = 0; i < source.wheels.length; i++, at += VEHICLE_WHEEL_WORDS) putVehicleWheel(null, source.wheels[i], input, at, null);
    h[CLIENT_HULL_OFFSETS.legsAt] = at; h[CLIENT_HULL_OFFSETS.legsCount] = source.legSide?.length ?? 0; h[CLIENT_HULL_OFFSETS.hasLegs] = Number(source.legSide !== undefined);
    for (let i = 0; i < h[CLIENT_HULL_OFFSETS.legsCount]; i++) input[at++] = source.legSide[i];
    h[CLIENT_HULL_OFFSETS.walkPhase] = source.walkPhase; h[CLIENT_HULL_OFFSETS.walkBlend] = source.walkBlend;
    h[CLIENT_HULL_OFFSETS.walkFromPhase] = source.walkFromPhase ?? 0; h[CLIENT_HULL_OFFSETS.walkFromBlend] = source.walkFromBlend ?? 0;
    h[CLIENT_HULL_OFFSETS.level] = source.system.water?.level ?? 0; h[CLIENT_HULL_OFFSETS.assists] = Number(Boolean(source.system.flightAssists));
    h[CLIENT_HULL_OFFSETS.membersAt] = at; h[CLIENT_HULL_OFFSETS.membersCount] = this.members.length;
    for (let i = 0; i < this.members.length; i++, at += CLIENT_HULL_MEMBER_WORDS) {
      const t = this.members[i], m = this.member;
      m.id = t.id; m.generation = t.generation; m.alive = Boolean(t.alive); m.team = t.team ?? undefined;
      m.own = t === source.aiTarget; m.unit = Boolean(t.soldier); m.body = t.vehicle?.body ? b.refs[KIND_BODY].slotOf(t.vehicle.body) : -1;
      // inCone never reads a dead, friendly or own target's pose. Avoid
      // waking rider/hit-shape work for entries whose point cannot be used.
      const relation = m.alive && !m.own ? source.system.units?.relation?.(source.team, t.team) ?? (t.team === source.team ? 1 : -1) : 0;
      if (relation < 0) { if (t.aimPoint) t.aimPoint(this.point); else this.point.copy(t.position); }
      else this.point.set(0, 0, 0);
      putClientHullMember(null, m, input, at, null);
    }
    // Relations are foreign battle facts until roster ownership moves.
    // Sampling the table once keeps native target selection callback-free.
    h[CLIENT_HULL_OFFSETS.teamsAt] = at; h[CLIENT_HULL_OFFSETS.teamsCount] = this.teamCount;
    for (let i = 0; i < this.teamCount; i++) input[at++] = this.teams[i] ?? NaN;
    h[CLIENT_HULL_OFFSETS.relationsAt] = at;
    for (let i = 0; i < this.teamCount; i++) for (let j = 0; j < this.teamCount; j++) {
      input[at++] = source.system.units?.relation?.(this.teams[i], this.teams[j]) ?? (this.teams[i] === this.teams[j] ? 1 : -1);
    }
    input.set(h, start);
  }
  // Generated sync writes vectors and quaternions in place. Wheel, seat and
  // leg ranges retain identity, including the walker's interpolation phase.
  publish(output, result) {
    const v = this.view, h = v.predictionHullWords;
    // Exact subranges are cached across frames. Native bulk copies avoid
    // boxing float words through a generic JavaScript element-store loop.
    if (this.outputBuffer !== output.buffer) {
      this.outputBuffer = output.buffer;
      this.headerSource = new Float64Array(output.buffer, output.byteOffset + result.hullAt * Float64Array.BYTES_PER_ELEMENT, CLIENT_HULL_WORDS);
      h.set(this.headerSource);
      this.stateSource = new Float64Array(output.buffer, output.byteOffset + h[CLIENT_HULL_OFFSETS.stateAt] * Float64Array.BYTES_PER_ELEMENT, CLIENT_HULL_STATE_WORDS);
      for (let i = 0; i < this.source.seats.length; i++) this.seatSources[i] = new Float64Array(output.buffer,
        output.byteOffset + (h[CLIENT_HULL_OFFSETS.seatsAt] + i * CLIENT_HULL_SEAT_WORDS) * Float64Array.BYTES_PER_ELEMENT, CLIENT_HULL_SEAT_WORDS);
      this.legSource = new Float64Array(output.buffer, output.byteOffset + h[CLIENT_HULL_OFFSETS.legsAt] * Float64Array.BYTES_PER_ELEMENT, this.legs.length);
    }
    h.set(this.headerSource); v.predictionWords.set(this.stateSource);
    syncClientHullStateViewMath(this.bridge, v);
    for (let i = 0; i < this.source.seats.length; i++) v.seats[i].predictionWords.set(this.seatSources[i]);
    for (let i = 0; i < h[CLIENT_HULL_OFFSETS.wheelsCount]; i++) syncVehicleWheel(null, output, h[CLIENT_HULL_OFFSETS.wheelsAt] + i * VEHICLE_WHEEL_WORDS, 0, v.wheels[i]);
    // A correction may omit support until first contact. Keep the backing
    // array alive across that absence so the next contact does not allocate.
    if (h[CLIENT_HULL_OFFSETS.hasLegs]) {
      v.legSide = this.legs; this.legs.set(this.legSource);
    } else v.legSide = undefined;
    v.predictionTick = result.tick; this.hadPrevious = true;
    return v;
  }
}
