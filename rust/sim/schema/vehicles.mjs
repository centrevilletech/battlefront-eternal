// The hulls (src/vehicles.js Vehicles and Vehicle): what each hull's tick
// on the ground reads and writes (Vehicle.tick with src/vehicle-hover.js),
// the first part of the hulls the simulation's port moves. What a hull is
// for the battle (its speeds, its box, its probes) is imported only
// (mirror: false); what it does is mirrored. Its class's own keys (the
// hover's springs and rates, a walker's legs, its seats' rates and its
// explosion) come from the battle's content where the hull is made, and
// are not here. docs/sim-store.md says how a schema is written;
// tools/sim-schema.mjs makes the code from it.
//
// Still to declare, as their packages port them: a hull's pose (its rig,
// its seats' aim targets, turrets, aimers and barrels: aimTick and render),
// its flight (the flight package), its seats' leases and weapons (the
// seats package), its tow cable, dying and spawn slot. The completeness
// check names those fields until then.
const fixed = (type, js) => ({ type, mirror: false, ...(js ? { js } : {}) });

export default {
  records: {
    // A wheel (the Hailfire's): its place across the hull and its radius,
    // its turn and the turn a step ago.
    VehicleWheel: {
      x: fixed('f64'),
      radius: fixed('f64'),
      phase: 'f64',
      previous: 'f64',
      // Its turn at the start of the world tick (captureTick), for drawing
      // between ticks.
      tickFrom: 'f64|undefined',
    },

    // A seat's aim (the player's look, a bot's analogs, the hull's
    // steering) and its lease (a person's or a unit's: the seats package's).
    VehicleSeat: {
      aimYaw: 'f64',
      pitch: 'f64',
      lookYaw: 'f64|null',
      lookPitch: 'f64|null',
      lease: 'ref:seatLease|null',
    },

    // A seat's limits as seatView keeps them (pitch up positive, yaw from the
    // hull's heading, null: free; an arc round the back kept in 0..2pi).
    SeatView: {
      pitchLimits: 'f64x2',
      yawLimits: 'f64x2|null',
      wrapYaw: 'bool',
    },

    // What the tick reads of a seat's lease (Vehicle.seatLeases: the lease,
    // or for an empty seat nothing, every field undefined): whose it is (a
    // person's, a unit's; a network player's) and its frozen input's
    // analogs. Imported only: the lease is the seats package's.
    SeatLeaseView: {
      human: 'bool|undefined',
      unit: 'ref:unit|null|undefined',
      networkHuman: fixed('bool|undefined', 'unit.networkHuman'),
      forward: fixed('f64|undefined', 'input.forward'),
      right: fixed('f64|undefined', 'input.right'),
      turn: fixed('f64|undefined', 'input.turn'),
      pitch: fixed('f64|undefined', 'input.pitch'),
      brake: fixed('bool|undefined', 'input.brake'),
    },

    // A spawn pad (Vehicles.slots, in the battle's order): its clock to the
    // next spawn, the hull on it, and the hulls made for it by class (its
    // place, its team and classes are the battle's content).
    SpawnSlot: {
      remaining: 'f64',
      vehicle: 'ref:vehicle|null',
      variants: 'map:ref:vehicle',
    },

    // The hull phase's calls into the battle while JavaScript serves them
    // (src/vehicles-sim.js: rust/sim/src/vehicles/module.rs's world), one
    // record each way per VehicleWorld method. A body is its slot in the
    // bridge's identity of bodies (-1: none); a hull its place in the
    // battle's list.
    VehRay: { origin: 'vec3', direction: 'vec3', far: 'f64', ignore: 'f64' },
    VehHit: { found: 'bool', distance: 'f64', point: 'vec3', normal: 'vec3', terrain: 'bool', body: 'f64' },
    VehPush: { hull: 'f64', probes: 'list:vec3', radius: 'f64', q: 'quat', own: 'quat|null', previous: 'vec3', position: 'vec3', velocity: 'vec3', terrain: 'bool' },
    VehPushed: { undo: 'bool', position: 'vec3', velocity: 'vec3' },
    VehColumn: { x: 'f64', z: 'f64', top: 'f64' },
    VehGround: { found: 'bool', height: 'f64', normal: 'vec3' },
    VehSpring: { hull: 'f64', index: 'f64', center: 'vec3', radius: 'f64' },
    VehTriangle: { a: 'vec3', b: 'vec3', c: 'vec3' },
    VehPolygon: { id: 'f64', moving: 'bool', triangles: 'list:rec:VehTriangle' },
    VehPolygons: { polygons: 'list:rec:VehPolygon' },
    VehBody: { body: 'f64', matrix: 'mat4', enabled: 'bool' },
    VehPoint: { position: 'vec3' },
    VehFlag: { value: 'bool' },
    VehNumber: { value: 'f64' },
    VehNothing: {},
    VehHull: { hull: 'f64' },
    VehDamaged: { hull: 'f64', amount: 'f64', attacker: 'union:OrdnanceOwner' },
    VehBlast: { hull: 'f64', final: 'bool', position: 'vec3' },
    VehEffect: { name: 'str', position: 'vec3' },
    // Registry entries by their places in units.all(), and their aim points.
    VehMembers: { indices: 'list:f64' },
    VehPoints: { points: 'list:vec3' },

    // A walker's water damage (src/vehicle-water.js waterDamageRule).
    WaterRule: {
      interval: 'f64',
      amount: 'f64',
    },
  },

  kinds: {
    // The battle's hulls (Vehicles), in the order they were made: the order
    // every pass over them takes.
    vehicles: {
      singleton: true,
      fields: {
        vehicles: 'list:vehicle',
        slots: 'list:rec:SpawnSlot',
        // Where the page's player stands while alive (none while dead): no
        // hull spawns on him.
        spawnOccupant: 'vec3|null',
      },
    },

    // A hull (Vehicle), made for its spawn slot and class the first time
    // that class is wanted there, and spawned again for each life.
    vehicle: {
      home: 'vehicles.vehicles',
      class: 'Vehicle',
      fields: {
        // What it is.
        name: fixed('str'),
        flying: fixed('bool'),
        walking: fixed('bool'),
        creature: fixed('bool'),
        wheeled: fixed('bool'),
        repulsor: fixed('bool'),
        speed: fixed('f64'),
        reverseSpeed: fixed('f64'),
        strafeSpeed: fixed('f64'),
        acceleration: fixed('f64'),
        deceleration: fixed('f64'),
        turnRate: fixed('f64'),
        hover: fixed('f64'),
        boundsMin: fixed('vec3', 'bounds.min'),
        boundsMax: fixed('vec3', 'bounds.max'),
        center: fixed('vec3'),
        size: fixed('vec3'),
        boundRadius: fixed('f64'),
        probeRadius: fixed('f64'),
        probes: fixed('list:vec3'),
        supportProbes: fixed('list:vec3'),
        maxHealth: fixed('f64'),
        waterRule: fixed('rec:WaterRule|null'),
        waterDepth: fixed('f64'),
        body: fixed('ref:body'),
        // What it does.
        position: 'vec3',
        previous: 'vec3',
        velocity: 'vec3',
        yaw: 'f64',
        previousYaw: 'f64',
        tilt: 'f64',
        roll: 'f64',
        previousTilt: 'f64',
        previousRoll: 'f64',
        yawRate: 'f64',
        tiltRate: 'f64',
        rollRate: 'f64',
        support: 'f64',
        forwardInput: 'f64',
        rightInput: 'f64',
        alive: 'bool',
        sleeping: 'bool|undefined',
        health: 'f64',
        dying: 'f64',
        waterClock: 'f64',
        quaternion: 'quat',
        matrix: 'mat4',
        // Its item target's centre (itemTarget.center, kept in place through
        // Vehicle.prototype.itemCenter): what a power-up measures to.
        itemCenter: 'vec3',
        supportNormal: 'vec3',
        legSide: 'list:f64|undefined',
        seatIndex: 'f64',
        driver: 'ref:vehicleDriver|null',
        wheels: 'list:rec:VehicleWheel',
        seats: 'list:rec:VehicleSeat',
        // Each seat's limits and lease as the tick reads them (Vehicle.prototype
        // seatViews and seatLeases): the content's and the seats package's.
        seatViews: fixed('list:rec:SeatView'),
        seatLeases: fixed('list:rec:SeatLeaseView'),
        // Its pose at the start of the world tick (captureTick), for drawing
        // between ticks; whether it shows (a destroyed walker while it dies).
        tickFrom: 'vec3|undefined',
        tickFromYaw: 'f64|undefined',
        tickFromTilt: 'f64|undefined',
        tickFromRoll: 'f64|undefined',
        visible: { type: 'bool', js: 'root.visible', mirror: false },
        // Its tow cable (Vehicles.launchTow): the walker it holds (its place in
        // the battle's list, -1 none), the anchor in that walker's frame, the
        // leg's name, the angle round the walker at the last tick and how far
        // the cable has wound since it struck.
        towTarget: 'f64',
        towLocal: 'vec3',
        towLeg: 'str',
        towAngle: 'f64',
        towWinding: 'f64',
        // Who last damaged it, for kill credit (a round's owner, a tow's crew).
        lastAttacker: 'union:OrdnanceOwner',
        // Its spawn pad (its place in Vehicles.slots), and its entry in the
        // units registry (VehicleTarget): its number there and the life it
        // is on (each spawn starts one, so references to the last go stale).
        // The registry is JavaScript's: a spawn the module makes tells it
        // (VehicleWorld::new_life) rather than writing the generation back.
        slotIndex: fixed('f64'),
        targetId: fixed('f64', 'aiTarget.id'),
        targetGeneration: fixed('f64', 'aiTarget.generation'),
      },
    },

    // What the hulls name without the store holding them (yet): a seat's
    // lease (the seats package's), the page's player at the controls.
    seatLease: {},
    vehicleDriver: {},
  },

  // The hull phase on the module asks the battle these while it is
  // JavaScript's (src/vehicles-sim.js serves them with src/vehicles-world.js).
  // A hook that hands JavaScript a hull (damaged, destroyedCrew, destroyed,
  // explosion) sees every hull as the phase has left it, and what it does to
  // any hull comes back before the phase goes on.
  imports: {
    vehCast: { args: 'VehRay', result: 'VehHit' },
    vehPushWalls: { args: 'VehPush', result: 'VehPushed' },
    vehGround: { args: 'VehColumn', result: 'VehGround' },
    vehSpringPolygons: { args: 'VehSpring', result: 'VehPolygons' },
    vehMoveBody: { args: 'VehBody', result: null },
    vehEnableBody: { args: 'VehBody', result: null },
    vehBodyEnabled: { args: 'VehBody', result: 'VehFlag' },
    vehHasWater: { args: 'VehNothing', result: 'VehFlag' },
    vehWaterOver: { args: 'VehColumn', result: 'VehFlag' },
    vehWaterLevel: { args: 'VehNothing', result: 'VehNumber' },
    vehInDeathRegion: { args: 'VehPoint', result: 'VehFlag' },
    vehRaycastUnits: { args: 'VehRay', result: 'VehHit' },
    vehClock: { args: 'VehNothing', result: 'VehNumber' },
    vehDamaged: { args: 'VehDamaged', result: null },
    vehDestroyedCrew: { args: 'VehHull', result: null },
    vehDestroyed: { args: 'VehHull', result: null },
    vehRespawnClock: { args: 'VehHull', result: null },
    vehReleaseCrew: { args: 'VehHull', result: null },
    vehExplosion: { args: 'VehBlast', result: null },
    vehEffect: { args: 'VehEffect', result: null },
    // A unit's aim point (Unit.aimPoint: its hit shapes' middle, worked
    // out on the first ask in a tick): registry entry `hull` (its place in
    // units.all()), for flight's targeting.
    vehAimPoint: { args: 'VehHull', result: 'VehPoint' },
    // The aim points of several units at once (a targeting pass's), in order.
    vehAimPoints: { args: 'VehMembers', result: 'VehPoints' },
  },
};
