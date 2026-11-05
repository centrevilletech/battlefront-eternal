// The seats (src/seats.js, src/boarding.js, the hulls' seats in
// src/vehicles.js): every lease that holds a hull's seat or a turret, and
// the turret crews' system. A lease is the seat's reference for whoever
// sits there (unit.seat, a hull seat's `lease`, a turret's `lease`), made
// when he takes the seat and let go when he leaves it; the store names it
// by a handle as every other reference. docs/sim-store.md says how a
// schema is written; tools/sim-schema.mjs makes the code from it.
//
// Turret aim, rig and weapon state use the props kind and retained native
// bindings. SeatUnit still names the foreign AI control facade; its intent
// crosses once before latching rather than becoming seat authority.
export default {
  enums: {
    // What a seat is to whoever holds it (seats.js:31-33, seatRole).
    SeatRole: ['pilot', 'gunner', 'passenger'],
    // Why someone left a seat (Vehicles.vacate's `reason`).
    SeatReason: ['exit', 'stale', 'killed', 'destroyed', 'displaced'],
    // What the seat phase on the module leaves JavaScript to do once it is
    // over, in the order it happened: a body's combat
    // and drawing as a seat took or let it go, and the battle's hooks.
    SeatEffectKind: ['seated', 'unseated', 'placed', 'dropped', 'boarded', 'unboarded', 'moved'],
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

    // Seat solver records retain foreign facts and ordered combat/AI hooks.
    // The required F2 scheduler owns leases, latching and rider bodies; these
    // records also let native fixtures exercise the independent solvers.

    // A hull as its seats read it beyond the store's fields
    // (seats::hulls::HullSide): its place in the battle's list, who last
    // hurt it (attacker 0 nobody, 1 the player, 2 a unit by its id, 3 a
    // hull by its place: attackerId), its class's base and health type,
    // and each seat's turret node and number of weapons.
    SeatHullFacts: {
      vehicle: 'ref:vehicle',
      index: 'f64',
      attacker: 'f64',
      attackerId: 'f64',
      hover: 'bool',
      animal: 'bool',
      turretNodes: 'list:bool',
      weapons: 'list:f64',
    },
    // The seat's own intent as the AI drives it (seats.js blankSeatIntent
    // and what the AI writes there), as SeatInput.latch reads it.
    SeatAiIntent: {
      forward: 'f64', right: 'f64', turn: 'f64', pitch: 'f64',
      jump: 'bool', crouch: 'bool', prone: 'bool', use: 'bool', exit: 'bool',
      buttons: 'f64x2',
      reload: 'bool', nextPrimary: 'bool', previousPrimary: 'bool', nextSecondary: 'bool', zoom: 'bool',
      aim: 'vec3|null',
      brake: 'bool', up: 'f64', down: 'f64',
      trick: 'bool', immelmann: 'bool', boost: 'bool',
    },
    // A bot's seat control (SeatUnit): whether a controller drives it, and
    // its intent.
    SeatAiControl: { seatUnit: 'ref:seatUnit', driven: 'bool', intent: 'rec:SeatAiIntent' },
    // The phase's arguments: the vehicle updates' clock and input batches
    // so far, every hull in the battle's order, and the seat controls of
    // the leases.
    SeatPhaseArgs: { time: 'f64', ticks: 'f64', hulls: 'list:rec:SeatHullFacts', controls: 'list:rec:SeatAiControl' },
    // One thing for JavaScript to do once the phase is over: a body's
    // combat and drawing as it took a seat (seated), left one (unseated:
    // `yaw` its aim then), was set down (placed, facing `yaw`) or let go
    // where it rode (dropped, at `at`); a hook (boarded, unboarded with
    // its post and reason, moved). unit -1: none; hull: the hull's place.
    SeatEffect: {
      kind: 'enum:SeatEffectKind', unit: 'f64', hull: 'f64', seat: 'f64', yaw: 'f64', at: 'vec3',
      post: 'ref:post|null', reason: 'enum:SeatReason',
    },
    // The phase's answer: the input batches now, the seat controls whose
    // intents it latched (their edges are taken), and what is left to do.
    SeatPhaseAnswer: { ticks: 'f64', latched: 'list:seatUnit', effects: 'list:rec:SeatEffect' },

    // What the phase asks JavaScript (its imports): a cast and a sphere
    // test of the collision world (leaving a body out), an AI controller's
    // state, a new seat control for a bot's lease (its new seat and role
    // first) and a seat's weapons' spread rule.
    SeatRay: { origin: 'vec3', direction: 'vec3', far: 'f64', ignore: 'ref:body|null' },
    SeatHit: { found: 'bool', distance: 'f64', point: 'vec3', normal: 'vec3', terrain: 'bool', body: 'ref:body|null' },
    SeatSphere: { center: 'vec3', radius: 'f64', ignore: 'ref:body|null' },
    SeatFlag: { value: 'bool' },
    SeatWho: { unit: 'f64' },
    SeatNumber: { value: 'f64|undefined' },
    SeatLeaseArgs: { lease: 'ref:seatLease', index: 'f64', role: 'enum:SeatRole' },
    SeatMade: { seatUnit: 'ref:seatUnit' },
    SeatPolicyArgs: { hull: 'f64', seat: 'f64', player: 'bool', random: 'bool' },
  },

  commands: {
    // Vehicles.latchCrews: the hulls' input batch (seats::hulls::Hulls::latch_crews).
    seatLatch: { args: 'SeatPhaseArgs', result: 'SeatPhaseAnswer', rust: 'seats::module::latch' },
  },

  imports: {
    seatCast: { args: 'SeatRay', result: 'SeatHit' },
    seatOverlaps: { args: 'SeatSphere', result: 'SeatFlag' },
    seatControllerState: { args: 'SeatWho', result: 'SeatNumber' },
    seatControllable: { args: 'SeatLeaseArgs', result: 'SeatMade' },
    seatPolicy: { args: 'SeatPolicyArgs', result: null },
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
