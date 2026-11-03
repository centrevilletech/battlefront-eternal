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

    // A seat as the tick reads it: its aim (the player's look, a bot's
    // analogs, the hull's steering), its limits (seatView), and its lease
    // (a person's or a unit's, a network player's or a bot's, its frozen
    // input's analogs: the seats package's state, read here).
    VehicleSeat: {
      aimYaw: 'f64',
      pitch: 'f64',
      lookYaw: 'f64|null',
      lookPitch: 'f64|null',
      lease: 'ref:seatLease|null',
      leaseHuman: fixed('bool|undefined', 'lease.human'),
      leaseUnit: fixed('ref:unit|null|undefined', 'lease.unit'),
      leaseNetworkHuman: fixed('bool|undefined', 'lease.unit.networkHuman'),
      leaseTurn: fixed('f64|undefined', 'lease.input.turn'),
      leasePitch: fixed('f64|undefined', 'lease.input.pitch'),
      pitchLimits: fixed('f64x2', 'view.pitchLimits'),
      yawLimits: fixed('f64x2|null', 'view.yawLimits'),
      wrapYaw: fixed('bool', 'view.wrapYaw'),
    },

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
        itemCenter: fixed('vec3', 'itemTarget.center'),
        supportNormal: 'vec3',
        legSide: 'list:f64|undefined',
        seatIndex: 'f64',
        driver: 'ref:vehicleDriver|null',
        wheels: 'list:rec:VehicleWheel',
        seats: 'list:rec:VehicleSeat',
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
      },
    },

    // What the hulls name without the store holding them (yet): a seat's
    // lease (the seats package's), the page's player at the controls.
    seatLease: {},
    vehicleDriver: {},
  },
};
