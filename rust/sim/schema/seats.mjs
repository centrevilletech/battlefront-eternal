// The seats (src/seats.js, src/boarding.js, the hulls' seats in
// src/vehicles.js): every lease that holds a hull's seat or a turret, and
// the turret crews' system. A lease is the seat's reference for whoever
// sits there (unit.seat, a hull seat's `lease`, a turret's `lease`), made
// when he takes the seat and let go when he leaves it; the store names it
// by a handle as every other reference. docs/sim-store.md says how a
// schema is written; tools/sim-schema.mjs makes the code from it.
//
// Still to declare, as their packages port them: a turret's aim and its
// rig (the props package's kind; the seat frames' captures write them by
// hand until then), and the seat as the AI drives it (SeatUnit, the AI's
// object, which the store only names).
export default {
  enums: {
    // What a seat is to whoever holds it (seats.js:31-33, seatRole).
    SeatRole: ['pilot', 'gunner', 'passenger'],
  },

  records: {
    // A bot's intent as its seat froze it for this update (seats.js
    // SeatInput: latch, take, takeReload): its held analogs and buttons,
    // and each edge once. trick, immelmann and boost are set by the first
    // latch.
    SeatInput: {
      forward: 'f64',
      right: 'f64',
      turn: 'f64',
      pitch: 'f64',
      brake: 'bool',
      up: 'f64',
      down: 'f64',
      aim: 'vec3',
      aiming: 'bool',
      held: 'list:bool',
      pressed: 'list:bool',
      reload: 'bool',
      exit: 'bool',
      trick: 'bool|undefined',
      immelmann: 'bool|undefined',
      boost: 'bool|undefined',
    },

    // An exit asked for, taken at the next batch (TurretSeats.requestExit,
    // Vehicles.requestExit): the command post the AI sends him to once out,
    // whether the vacated seat is handed on (a hull's), and whether he is
    // set down beside it.
    ExitRequest: {
      post: 'ref:post|null',
      promote: 'bool|undefined',
      placement: 'bool',
    },
  },

  kinds: {
    // The battle's leases that hold a seat now: the hulls', hull by hull
    // and seat by seat, then the turrets', in the order they were manned.
    // JavaScript keeps no such list (each lease is held by its seat, its
    // turret and its unit); a capture lists them in this order, and the
    // module, once it owns the seats, keeps it.
    seats: {
      singleton: true,
      fields: {
        leases: 'list:seatLease',
      },
    },

    // A seat's lease (seats.js SeatLease): whose (a unit, or the page's own
    // player: human) and which life of him, his team, the hull or turret
    // and the seat, his role there, when it was granted, the buttons seen
    // at the last batch (for press edges), an exit asked for, whether it
    // was let go, this update's frozen input, and the seat as the AI drives
    // it (a bot's).
    seatLease: {
      home: 'seats.leases',
      class: 'SeatLease',
      fields: {
        unit: 'ref:unit|null',
        generation: 'f64',
        human: 'bool',
        team: 'f64',
        vehicle: 'ref:vehicle|null',
        turret: 'ref:turret|null',
        index: 'f64',
        role: 'enum:SeatRole',
        since: 'f64',
        tick: 'f64',
        was: 'f64x2',
        exitRequest: 'rec:ExitRequest|null',
        released: 'bool',
        input: 'rec:SeatInput',
        controllable: 'ref:seatUnit|null',
      },
      skip: {
        lean: 'a shown rider\'s sway, the page\'s (Vehicles.presentRiders)',
        emitter: 'getters over the lease and its hull or turret: what a beam fired from the seat follows (the ordnance\'s)',
      },
    },

    // The turret crews (seats.js TurretSeats): their leases in the order
    // they manned their turrets (the order update() takes them), and the
    // updates and time so far.
    turretSeats: {
      singleton: true,
      fields: {
        leases: 'list:seatLease',
        time: 'f64',
        ticks: 'f64',
      },
      skip: {
        live: 'the battle\'s live objects (its turrets)',
        ordnance: 'the battle\'s rounds', collision: 'the collision world', units: 'the registry',
        onFired: 'the battle\'s hook', onBoard: 'the battle\'s hook', onUnboard: 'the battle\'s hook',
        world: 'the boundary object (src/seats-world.js)',
        seatRandom: 'the world\'s seat stream, handed to the weapons of a seat taken',
        exit: 'scratch (classicSeatExit\'s out)', seatPoint: 'scratch', dropPoint: 'scratch', riderPose: 'scratch',
        seatTurn: 'scratch', view: 'scratch', muzzle: 'scratch', gunDir: 'scratch', target: 'scratch', hit: 'scratch',
      },
    },

    // Kinds the seats refer to, owned elsewhere: a turret (src/props.js,
    // the props package's), the seat as the AI drives it (src/seats.js
    // SeatUnit, the AI's object) and a command post.
    turret: { class: 'Turret' },
    seatUnit: { class: 'SeatUnit' },
    post: {},
  },
};
