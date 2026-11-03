// The seats package's frames (tools/sim-capture/specs.mjs `seats`): the hulls'
// seats (src/vehicles.js Vehicles: the batch latchCrews takes, boarding,
// getting out, a destroyed hull's crew, an exit asked for) and the turret
// crews (src/seats.js TurretSeats). rust/sim/tests/replay_seats.rs reads
// them back.
//
// A frame's state is the seats as their code reads and writes them: every
// lease that holds a seat (the store's `seats` list, made here: JavaScript
// keeps none), the hulls with someone aboard (the store's hulls, and what
// their seats read beyond them), the units in those seats (the store's
// units, and the combat flags a seat sets), and the seats the AI drives
// (whether a controller drives each, and its intent). Calls across the
// seats' boundary (src/seats-world.js) are recorded with their results; a
// kill and a rider posed come back with the units as they left them.

import * as THREE from 'three';
import { hasMissileLock } from '../../src/weapon-targeting.js';
import { networkVehicle } from '../../src/runtime/network-vehicle.js';
import { networkSeat } from '../../src/runtime/network-seat.js';
import { boardingReach } from '../../src/boarding.js';

// The leases that hold a seat, hulls first (hull by hull, seat by seat),
// then the turrets' (in the order they were manned), then any the frame
// names that holds none (a lease being let go).
function leasesOf(game, named = []) {
  const out = [];
  for (const v of game.vehicles?.vehicles ?? []) for (const seat of v.seats) if (seat.lease) out.push(seat.lease);
  for (const lease of game.turretSeats?.leases ?? []) if (!out.includes(lease)) out.push(lease);
  for (const lease of named) if (lease && !out.includes(lease)) out.push(lease);
  return out;
}

// The store holds values where the JavaScript shares objects (combat's
// aimOverride is the intent's aim): a capture writes each such vector as
// a copy of its own (as tools/sim-capture/units.mjs does).
function dealias(v, seen = new Set()) {
  if (!v || typeof v !== 'object') return v;
  if (v.isVector3 || v.isQuaternion || v.isMatrix4) {
    if (seen.has(v)) return v.clone();
    seen.add(v);
    return v;
  }
  if (Array.isArray(v)) return v.map((x) => dealias(x, seen));
  if (Object.getPrototypeOf(v) === Object.prototype) for (const k of Object.keys(v)) v[k] = dealias(v[k], seen);
  return v;
}

// A unit as the seats see it: the store's unit, the combat flags a seat
// reads and sets, and its class's NoEnterVehicles as canBoard reads it.
function unitValue(cap, u) {
  const c = u.combat;
  return {
    unit: dealias(cap.export('unit', u)),
    combat: { zoomStep: c?.zoomStep ?? 0, zoom: c?.zoom ?? 1, firstPerson: Boolean(c?.firstPerson), blowApart: Boolean(c?.blowApart) },
    noEnter: Number.parseFloat(u.classDef?.NoEnterVehicles ?? 0),
  };
}

// A hull as its seats read it: the store's hull (its team, flight state and
// stick among the flight package's fields), its place in the battle's list,
// and what its seats read beyond the store (its last attacker; its class's
// base, health type, turret nodes and weapons).
function hullValue(cap, game, v) {
  return {
    index: game.vehicles.vehicles.indexOf(v),
    hull: cap.export('vehicle', v),
    side: {
      lastAttacker: v.lastAttacker ?? null,
      hover: v.base === 'hover', animal: v.healthType === 'animal',
      turretNodes: v.seats.map((s) => Boolean(s.settings?.TurretNodeName)),
      weapons: v.seats.map((s) => s.weapons.length),
    },
  };
}

// The seat as the AI drives it, for each lease that has one: whether a
// controller drives it, and its intent (src/seats.js blankSeatIntent and
// what the AI writes there, as SeatInput.latch reads it).
function controlValue(cap, lease) {
  const c = lease.controllable, i = c.intent;
  return {
    lease: cap.export('seatLease', lease).$id,
    driven: Boolean(c.controller),
    intent: {
      forward: Number(i.forward), right: Number(i.right), turn: Number(i.turn), pitch: Number(i.pitch),
      jump: Boolean(i.jump), crouch: Boolean(i.crouch), prone: Boolean(i.prone), use: Boolean(i.use), exit: Boolean(i.exit),
      buttons: [Number(i.buttons?.[0] ?? 0), Number(i.buttons?.[1] ?? 0)],
      reload: Boolean(i.reload), nextPrimary: Boolean(i.nextPrimary), previousPrimary: Boolean(i.previousPrimary),
      nextSecondary: Boolean(i.nextSecondary), zoom: Boolean(i.zoom),
      aim: i.aim ? i.aim.clone() : null, brake: Boolean(i.brake), up: Number(i.up), down: Number(i.down),
      trick: Boolean(i.trick), immelmann: Boolean(i.immelmann), boost: Boolean(i.boost),
    },
  };
}

// The seats' state as a frame starts or ends: `named` leases, units and
// hulls the frame's arguments name (one being let go, one boarding).
function seatState(game, cap, { leases: namedLeases = [], units: namedUnits = [], hulls: namedHulls = [] } = {}) {
  lastNamed = { leases: namedLeases, hulls: namedHulls };
  const leases = leasesOf(game, namedLeases);
  const units = [], hulls = [];
  for (const lease of leases) {
    if (lease.unit && !units.includes(lease.unit)) units.push(lease.unit);
    if (lease.vehicle && !hulls.includes(lease.vehicle)) hulls.push(lease.vehicle);
  }
  for (const u of namedUnits) if (u && !units.includes(u)) units.push(u);
  for (const v of namedHulls) if (v && !hulls.includes(v)) hulls.push(v);
  const vs = game.vehicles;
  return {
    system: vs ? { time: vs.time, ticks: vs.ticks } : null,
    seats: cap.export('seats', { leases }),
    hulls: hulls.map((v) => hullValue(cap, game, v)),
    units: units.map((u) => unitValue(cap, u)),
    controls: leases.filter((l) => l.controllable).map((l) => controlValue(cap, l)),
  };
}

// The units seated now, as a call left them (a rider posed, a crew killed).
const unitsAfter = (game, cap) => ({ units: leasesOf(game).flatMap((l) => (l.unit ? [unitValue(cap, l.unit)] : [])) });
// The seats as a kill left them: a death's hooks run inside it and may
// come back into the seats (another rider killed by the same blast leaves
// his seat, a bot mans another turret). The state is the innermost open
// frame's (what it names, as it named it), with every unit of the
// registry (the killed one holds no seat by then; not the hulls', turrets'
// and buildings' targets in its list).
let lastNamed = {};
const everyUnit = (game, cap) => game.units.all().filter((u) => u.soldier && u.combat);
const seatsAfterKill = (game, cap) => ({ ...seatState(game, cap, { ...lastNamed, units: [...(lastNamed.units ?? []), ...everyUnit(game)] }) });
const crewsAfterKill = (game, cap) => ({ ...crewState(game, cap, { ...lastNamed, units: [...(lastNamed.units ?? []), ...everyUnit(game)] }) });

// The hulls' seats' boundary: their SeatsWorld's calls, a body stood up
// (Soldier.spawn's cast: the port runs the units' spawn), and a rider posed
// (the hulls' package's).
const hullSeatBoundaries = (game) => {
  const w = game.vehicles?.seatsWorld ?? null;
  return [
    ['seats.cast', w, 'cast'],
    ['seats.overlapsSphere', w, 'overlapsSphere'],
    ['seats.raycast', w, 'raycast'],
    ['seats.relation', w, 'relation'],
    ['collision.cast', game.collision, 'cast'],
    ['seats.kill', w, 'kill', seatsAfterKill],
    ['seats.riderDrop', w, 'riderDrop'],
    ['seats.controllerState', w, 'controllerState'],
    ['seats.controllable', w, 'controllable'],
    ['vehicles.syncRider', game.vehicles ?? null, 'syncRider', unitsAfter],
    ['seats.boarded', w, 'boarded'],
    ['seats.unboarded', w, 'unboarded'],
    ['seats.moved', w, 'moved'],
  ];
};

const hullFrame = (method, named = () => ({})) => ({
  method: `vehicles.${method}`,
  target: (game) => game.vehicles ?? null,
  state: (game, args, cap) => seatState(game, cap, named(args)),
  boundaries: hullSeatBoundaries,
});

// ---- The turret crews (src/seats.js TurretSeats).

// A turret as its crew reads and writes it: its place among the battle's
// turrets, whether it stands built, its lease and the unit whose body mans
// it, its aim and gun's angles and barrel, who last hurt it, its target in
// the registry (a launcher's lock leaves it out), its bounding sphere as it
// stands, its weapon, what it is for the battle (desc: as its constructor
// worked it out) and its rig's bones (each one's parent among them, -1 for
// the root, its local position, turn and scale, and its world matrix as
// the last update left it).
function turretValue(cap, game, t) {
  const rig = t.rig, bones = rig.bones;
  const at = (node) => (node ? bones.indexOf(node) : null);
  const gunner = !t.gunner ? null : t.lease?.unit?.soldier === t.gunner ? t.lease.unit : game.units.all().find((u) => u.soldier === t.gunner) ?? 'unknown';
  const center = new THREE.Vector3(), radius = t.sphere(center);
  return {
    index: game.live.turrets.indexOf(t), turret: t, alive: t.alive,
    lease: t.lease ? cap.export('seatLease', t.lease).$id : null, gunner,
    yaw: t.yaw, pitch: t.pitch, yawVel: t.yawVel, pitchVel: t.pitchVel, gunYaw: t.gunYaw, gunPitch: t.gunPitch, barrel: t.barrel,
    lastAttacker: t.lastAttacker ?? null, aiTarget: t.aiTarget ?? null,
    sphere: { center, radius },
    weapon: t.weapon ? { ordnance: t.weapon.def.ordnance ?? null, missileLock: hasMissileLock(t.weapon.def) } : null,
    desc: {
      facing: t.facing, position: t.position.clone(), body: t.bodies?.built || null,
      pitchLimits: [...t.pitchLimits], yawLimits: t.yawLimits ? [...t.yawLimits] : null, wrapYaw: t.wrapYaw,
      gunPitchLimits: [...t.gunPitchLimits], gunYawLimits: [...t.gunYawLimits],
      turnRate: t.turnRate, filter: t.filter, gunPitchRate: t.gunPitchRate, gunYawRate: t.gunYawRate, yawFactor: t.yawFactor, pitchFactor: t.pitchFactor,
      camera: { eye: t.camera.eye.clone(), center: t.camera.center.clone(), offset: t.camera.offset.clone(), tilt: t.camera.tilt },
      pivot: t.pivot.clone(), up: t.up.clone(),
      drumParentTurn: t.drumParentTurn ? t.drumParentTurn.clone() : null, gunBind: t.gunBind ? t.gunBind.clone() : null,
      rider: Boolean(t.rider), shootable: Boolean(t.rider?.shootable), matrix: [...t.matrix.elements],
      drum: at(t.drum), gun: at(t.gun), seat: at(t.seat), firePoints: (t.firePoints ?? []).map(at),
    },
    rig: {
      root: [...rig.root.matrix.elements],
      bones: bones.map((b) => ({ parent: bones.indexOf(b.parent), position: b.position.clone(), quaternion: b.quaternion.clone(), scale: b.scale.clone() })),
      world: bones.map((b) => [...b.matrixWorld.elements]),
    },
  };
}

// The crews' state as a frame starts or ends: their leases (and any the
// frame names that holds none), the turrets they man (and any named), the
// units in those seats (and any named) and the seats the AI drives.
function crewState(game, cap, { leases: namedLeases = [], units: namedUnits = [], turrets: namedTurrets = [] } = {}) {
  lastNamed = { leases: namedLeases, turrets: namedTurrets };
  const ts = game.turretSeats;
  const leases = [...ts.leases];
  for (const lease of namedLeases) if (lease && !leases.includes(lease)) leases.push(lease);
  const units = [], turrets = [];
  for (const lease of leases) {
    if (lease.unit && !units.includes(lease.unit)) units.push(lease.unit);
    if (lease.turret && !turrets.includes(lease.turret)) turrets.push(lease.turret);
  }
  for (const u of namedUnits) if (u && !units.includes(u)) units.push(u);
  for (const t of namedTurrets) if (t && !turrets.includes(t)) turrets.push(t);
  return {
    turretSeats: cap.export('turretSeats', ts),
    collide: Boolean(ts.collision),
    seats: cap.export('seats', { leases }),
    turrets: turrets.map((t) => turretValue(cap, game, t)),
    units: units.map((u) => unitValue(cap, u)),
    controls: leases.filter((l) => l.controllable).map((l) => controlValue(cap, l)),
  };
}

// The crews' boundary: their SeatsWorld's calls (the turret's weapon and
// rounds among them) and a body stood up (Soldier.spawn's cast).
const crewBoundaries = (game) => {
  const w = game.turretSeats?.world ?? null;
  return [
    ['crews.cast', w, 'cast'],
    ['crews.raycast', w, 'raycast'],
    ['collision.cast', game.collision, 'cast'],
    ['crews.kill', w, 'kill', crewsAfterKill],
    ['crews.riderDrop', w, 'riderDrop'],
    ['crews.controllable', w, 'controllable'],
    ['crews.updateLock', w, 'updateLock'],
    ['crews.weaponUpdate', w, 'weaponUpdate'],
    ['crews.fire', w, 'fire'],
    ['crews.bindRound', w, 'bindRound'],
    ['crews.sound', w, 'sound'],
    ['crews.boarded', w, 'boarded'],
    ['crews.unboarded', w, 'unboarded'],
    ['crews.fired', w, 'fired'],
  ];
};

const crewFrame = (method, named = () => ({})) => ({
  method: `turretSeats.${method}`,
  target: (game) => (game.turretSeats?.turrets.length ? game.turretSeats : null),
  state: (game, args, cap) => crewState(game, cap, named(args)),
  boundaries: crewBoundaries,
});

// ---- A network human's mouse and keys in a hull's seat
// (src/runtime/network-vehicle.js): the hull's seats as the mouse turns
// them (a craft's pilot's virtual stick, the hull's), and the driving keys
// he holds.
const networkSeatFrame = {
  method: 'networkVehicle.input',
  target: () => networkVehicle,
  state: (game, args, cap) => {
    const [v, , controls] = args;
    return {
      index: game.vehicles.vehicles.indexOf(v), hull: cap.export('vehicle', v),
      keys: { forward: controls.axis('forward'), right: controls.axis('right'), turn: controls.axis('turn'), space: controls.down.has('Space') },
    };
  },
  boundaries: () => [],
};

export const seats = {
  frames: [
    hullFrame('latchCrews'),
    hullFrame('board', (args) => ({ units: [args[0]], hulls: [args[1]] })),
    hullFrame('vacate', (args) => ({ leases: [args[0]] })),
    hullFrame('destroyedCrew', (args) => ({ hulls: [args[0]] })),
    hullFrame('requestExit', (args) => ({ units: [args[0]] })),
    crewFrame('update'),
    crewFrame('man', (args) => ({ units: [args[0]], turrets: [args[1]] })),
    crewFrame('vacate', (args) => ({ leases: [args[0]] })),
    crewFrame('destroyed', (args) => ({ turrets: [args[0]] })),
    crewFrame('requestExit', (args) => ({ units: [args[0]] })),
    crewFrame('leave', (args) => ({ units: [args[0]] })),
    networkSeatFrame,
    // A network human's seat switch (src/runtime/network-seat.js).
    { ...hullFrame('cycle', (args) => ({ units: [args[1]] })), method: 'networkSeat.cycle', target: () => networkSeat },
    // What a soldier's use key gets him into (src/boarding.js mountInReach):
    // every hull and every turret, in the battle's order, as the reach
    // queries walk them.
    {
      method: 'boardingReach.mount',
      target: () => boardingReach,
      state: (game, args, cap) => ({
        ...seatState(game, cap, { units: [args[1]], hulls: game.vehicles?.vehicles ?? [] }),
        turrets: (game.live?.turrets ?? []).map((t) => turretValue(cap, game, t)),
      }),
      boundaries: (game) => [
        ['vehicles.relation', game.vehicles ?? null, 'relation'],
        ['collision.cast', game.collision, 'cast'],
      ],
    },
  ],
};
