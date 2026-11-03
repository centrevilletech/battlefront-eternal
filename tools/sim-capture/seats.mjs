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

// A hull as its seats read it: the store's hull, its place in the battle's
// list, and what its seats read beyond the store (its team, flight state,
// last attacker and stick; its class's base, health type, turret nodes and
// weapons).
function hullValue(cap, game, v) {
  return {
    index: game.vehicles.vehicles.indexOf(v),
    hull: cap.export('vehicle', v),
    side: {
      team: v.team, flightState: v.flightState ?? null, lastAttacker: v.lastAttacker ?? null,
      stick: v.flying ? [v.stickX, v.stickY] : null,
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
    },
  };
}

// The seats' state as a frame starts or ends: `named` leases, units and
// hulls the frame's arguments name (one being let go, one boarding).
function seatState(game, cap, { leases: namedLeases = [], units: namedUnits = [], hulls: namedHulls = [] } = {}) {
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
// Every unit, as a kill left them (the killed one holds no seat by then).
const everyUnit = (game, cap) => ({ units: game.units.all().filter((u) => u.combat).map((u) => unitValue(cap, u)) });

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
    ['seats.kill', w, 'kill', everyUnit],
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

export const seats = {
  frames: [
    hullFrame('latchCrews'),
    hullFrame('board', (args) => ({ units: [args[0]], hulls: [args[1]] })),
    hullFrame('vacate', (args) => ({ leases: [args[0]] })),
    hullFrame('destroyedCrew', (args) => ({ hulls: [args[0]] })),
    hullFrame('requestExit', (args) => ({ units: [args[0]] })),
  ],
};
