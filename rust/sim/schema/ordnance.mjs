// Rounds in flight (src/ordnance.js): the bolts, beams, grenades and rounds
// (rockets, missiles, shells, fat rays) the battle's Ordnance holds, every
// field each record gets, from where it is made or from the modules that
// add to it later (combat.js, muzzles.js, vehicles.js), with the fields only
// a page's drawing reads listed apart (`skip`). docs/sim-store.md says how a
// schema is written; tools/sim-schema.mjs makes the code from it.
export default {
  enums: {
    // A round's base (ordnance.js:351 round(), 240 fatray()).
    RoundKind: ['bullet', 'missile', 'shell', 'fatray'],
    // Where a round struck a soldier (src/registry.js raycast).
    HitPart: ['head', 'body'],
  },

  unions: {
    // Whose a round is (ordnance.js `owner`): a unit, a vehicle firing with
    // nobody in its seat, the page's own player, nothing, or what
    // src/ordnance-owner.js froze of a unit when it threw something that
    // goes off later.
    OrdnanceOwner: ['null', 'lit:player', 'ref:unit', 'ref:vehicle', 'ref:ownerSnapshot'],
    // What a beam follows (ordnance.js:285): the seat's or hull's emitter,
    // else its owner.
    BeamSource: ['null', 'lit:player', 'ref:emitter', 'ref:unit', 'ref:vehicle', 'ref:ownerSnapshot'],
    // What a vehicle's or turret's round is drawn from (src/muzzles.js
    // bindMountedRound).
    Mount: ['ref:vehicle', 'ref:turret'],
  },

  records: {
    // Where a bolt or rocket will strike, found when it was fired
    // (ordnance.js:369, 418) or as it flew (712, 716).
    Impact: {
      point: 'vec3',
      normal: 'vec3',
      terrain: 'bool',
      body: 'ref:body|null|undefined',
      unit: 'ref:unit|undefined',
      part: 'enum:HitPart|null|undefined',
    },
  },

  kinds: {
    // The battle's Ordnance (ordnance.js:165): its four lists, in the order
    // they are stepped (and filtered, keeping order) each tick.
    ordnance: {
      singleton: true,
      fields: {
        bolts: 'list:bolt',
        beams: 'list:beam',
        grenades: 'list:grenade',
        rounds: 'list:round',
      },
    },

    // A blaster bolt (ordnance.js:408 bolt(), 684 stepBolt()).
    bolt: {
      home: 'ordnance.bolts',
      fields: {
        def: 'def:ordnance',
        owner: 'union:OrdnanceOwner',
        networkFireId: 'f64',
        networkFireChannel: 'f64',
        networkFireUnit: 'f64',
        networkFireGeneration: 'f64',
        ignoreBody: 'ref:body|null',
        position: 'vec3',
        direction: 'vec3',
        speed: 'f64',
        life: 'f64',
        drawn: 'f64',
        flown: 'f64',
        offset: 'vec3|null',
        merge: 'f64',
        impact: 'rec:Impact|null',
        // The last step's (685), for drawing between ticks.
        previousFlown: 'f64|undefined',
        previousDrawn: 'f64|undefined',
      },
      skip: {
        glow: 'colour from the definition, for drawing',
        light: 'colour from the definition, for drawing',
        visualMuzzle: 'a page muzzle callback (combat.js)',
        mountResolved: 'page drawing',
        presented: 'page drawing',
      },
    },

    // A beam (ordnance.js:282 beam(), 292 stepBeam()).
    beam: {
      home: 'ordnance.beams',
      fields: {
        networkFireChannel: 'f64',
        historyTick: 'f64|null',
        def: 'def:ordnance',
        owner: 'union:OrdnanceOwner',
        source: 'union:BeamSource',
        ignoreBody: 'ref:body|null',
        life: 'f64',
        duration: 'f64',
        effectTime: 'f64',
        position: 'vec3',
        end: 'vec3',
        direction: 'vec3',
        localOrigin: 'vec3|null',
        // `damageDef` is the definition's copy whose Damage each step sets
        // (306): its Damage is the state.
        damage: { type: 'f64', js: 'damageDef.Damage', mirror: false },
      },
      skip: {
        damageDef: 'a copy of the definition; its Damage is `damage`',
        axis: 'scratch for drawing',
        color: 'colour from the definition, for drawing',
      },
    },

    // A thrown grenade (ordnance.js:456 throw(), 878 stepGrenade()).
    grenade: {
      home: 'ordnance.grenades',
      fields: {
        networkFireChannel: 'f64',
        def: 'def:ordnance',
        owner: 'union:OrdnanceOwner',
        position: 'vec3',
        previous: 'vec3',
        launchPosition: 'vec3',
        velocity: 'vec3',
        gravity: 'f64',
        slow: 'f64',
        resting: 'bool',
        flight: 'f64',
        fuse: 'f64|null',
        spin: 'vec3',
        quaternion: 'quat',
        sinking: 'bool|undefined',
        touched: 'bool|undefined',
        gone: 'bool|undefined',
      },
      skip: {
        mesh: 'its model, drawn by the page',
        trail: 'its trail effect, drawn by the page',
        light: 'colour from the definition, for drawing',
      },
    },

    // A rocket, missile, shell or fat ray (ordnance.js:350 round(), 240
    // fatray(), 558 stepRound()), with what combat.js (784) and
    // muzzles.js (bindMountedRound) and a flyer's missiles (vehicles.js
    // 1933) add.
    round: {
      home: 'ordnance.rounds',
      fields: {
        kind: 'enum:RoundKind',
        networkFireChannel: 'f64',
        ignoreBody: 'ref:body|null',
        def: 'def:ordnance',
        owner: 'union:OrdnanceOwner',
        position: 'vec3',
        previous: 'vec3',
        launchPosition: 'vec3',
        velocity: 'vec3',
        life: 'f64',
        impact: 'rec:Impact|null',
        dead: 'bool',
        gravity: 'f64',
        ramp: 'f64',
        desired: 'vec3',
        targetId: 'f64|undefined',
        targetGeneration: 'f64|undefined',
        phase: 'f64x2',
        quaternion: 'quat',
        struck: 'set:unit|undefined',
        gone: 'bool|undefined',
        flightMissile: 'bool|undefined',
        flightClass: 'str|undefined',
        weapon: 'ref:weapon|undefined',
        muzzleGeneration: 'f64|null|undefined',
        muzzleIndex: 'f64|undefined',
        muzzleBarrel: 'f64|undefined',
        mount: 'union:Mount|undefined',
        mountSeat: 'f64|undefined',
        mountWeapon: 'f64|undefined',
        mountBarrel: 'f64|undefined',
      },
      skip: {
        mesh: 'its model, drawn by the page',
        trail: 'its trail effect, drawn by the page',
        light: 'colour from the definition, for drawing',
        visualMuzzle: 'a page muzzle callback (combat.js 788)',
        muzzleOffset: 'page drawing',
        mountResolved: 'page drawing',
      },
    },

    // Kinds the ordnance refers to, owned elsewhere: a unit is known by its
    // id (src/registry.js), the others by identity alone.
    unit: { key: 'id' },
    vehicle: {},
    body: {},
    emitter: {},
    ownerSnapshot: {},
    weapon: {},
    turret: {},
  },
};
