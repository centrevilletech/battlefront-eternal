// A craft's flight (src/flight/model.js, src/flight/pilotless.js,
// src/flight/targeting.js, Vehicle.fly): what a flyer's tick reads and
// writes besides what every hull's does (rust/sim/schema/vehicles.mjs, the
// same kind: the generator joins the two). What every hull has (its flight
// state, null on the ground, and the numbers the constructor gives every
// hull) is plain; what initFlight gives a craft as it spawns can be
// undefined (a ground hull has none of it). What a craft is for the battle
// (its class, its speeds, how far below its origin its gear stands, its
// size) is imported only (mirror: false). Its class's tuning (flightConfig:
// the class definition's speeds and handling under the live FLIGHT_CLASSES
// entry) is content and the battle's settings, not the hull's state.
//
// The texts a craft shows (its warning, its landing prompt and message, its
// trick and lock) are strings for now: each has '' among its values, which
// an enum cannot name yet.
const fixed = (type, js) => ({ type, mirror: false, ...(js ? { js } : {}) });

export default {
  enums: {
    // A craft's flight state (null for a ground hull).
    FlightState: ['Landed', 'TakingOff', 'Flying', 'Landing', 'Destroyed'],
    // Its class of craft (src/flight/config.js flightClass), for its tuning.
    FlightClass: ['fighter', 'scout', 'bomber', 'transport'],
  },

  kinds: {
    vehicle: {
      fields: {
        // Every hull's (the constructor's, and spawn's).
        flightState: 'enum:FlightState|null',
        grounded: 'bool',
        altitude: 'f64',
        throttle: 'f64',
        airspeed: 'f64',
        stateTime: 'f64',
        floorY: 'f64',
        bank: 'f64',
        gearFoot: fixed('f64|null'),
        radius: fixed('f64'),
        team: 'f64|undefined',
        // A craft's (initFlight, at each spawn).
        flightClass: fixed('enum:FlightClass|undefined'),
        minSpeed: fixed('f64|undefined'),
        midSpeed: fixed('f64|undefined'),
        boostSpeed: fixed('f64|undefined'),
        power: 'f64|undefined',
        stickX: 'f64|undefined',
        stickY: 'f64|undefined',
        flightRollRate: 'f64|undefined',
        rollLock: 'bool|undefined',
        boosting: 'bool|undefined',
        trick: 'str|undefined',
        trickTime: 'f64|undefined',
        trickCooldown: 'f64|undefined',
        trickPending: 'f64|undefined',
        trickDirection: 'f64|undefined',
        gear: 'f64|undefined',
        landingMessage: 'str|undefined',
        landingMessageTime: 'f64|undefined',
        flightTrickRight: 'vec3|undefined',
        flightClock: 'f64|undefined',
        boostExhausted: 'bool|undefined',
        landingReason: 'str|undefined',
        flightWarning: 'str|undefined',
        terrainWarning: 'bool|undefined',
        flightAssistId: 'f64|undefined',
        flightLockId: 'f64|undefined',
        flightLockGeneration: 'f64|undefined',
        flightLockTimer: 'f64|undefined',
        flightLostLOS: 'f64|undefined',
        flightLockState: 'str|undefined',
        flightStart: 'vec3|undefined',
        flightEnd: 'vec3|undefined',
        flightStartQ: 'quat|undefined',
        flightTrickLoopQ: 'quat|undefined',
        flightTrickEndQ: 'quat|undefined',
        flightPreviousQ: 'quat|undefined',
        flightTickQ: 'quat|undefined',
        // The keys held at its last step (undefined before its first).
        flightActionHeld: 'bool|undefined',
        trickHeld: 'bool|undefined',
        immelmannHeld: 'bool|undefined',
        // The frame it flies by as its pose last drew it (drawFlyer).
        flightFrameQ: 'quat|undefined',
      },
      skip: {
        flightConfig: 'its class\'s tuning (src/flight/config.js flightConfig): the class definition\'s speeds and handling under the live FLIGHT_CLASSES entry',
        flightLeanQ: 'drawFlyer\'s scratch, set before every read',
      },
    },
  },
};
