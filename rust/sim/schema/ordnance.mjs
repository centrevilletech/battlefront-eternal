// Rounds in flight (src/ordnance.js): the bolts, beams, grenades and rounds
// (rockets, missiles, shells, fat rays) the battle's Ordnance holds, every
// field each record gets, from where it is made or from the modules that
// add to it later (vehicles.js), with the fields only a page's drawing or
// the firing code's own bookkeeping read listed apart (`skip`: those stay
// JavaScript's, on the mirrors). docs/sim-store.md says how a
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

    // What the rounds ask of the rest of the battle (src/ordnance-world.js
    // OrdnanceWorld, rust/sim/src/ordnance/world.rs), as the module's
    // imports: each call's arguments and answer. An answer that can be
    // "none" says so in `found`.
    NoArgs: {},
    NumberAnswer: { value: 'f64' },
    FlagAnswer: { value: 'bool' },
    PointAnswer: { found: 'bool', point: 'vec3' },
    // cast and castMoving: `behind` names the ordnance's record the cast
    // fills (its `behind`, else its `hit`).
    CastArgs: { origin: 'vec3', direction: 'vec3', far: 'f64', behind: 'bool', ignoreBody: 'ref:body|null|undefined', staticOnly: 'bool|undefined' },
    CastAnswer: { found: 'bool', distance: 'f64', point: 'vec3', normal: 'vec3', terrain: 'bool', body: 'ref:body|null' },
    UnitCastArgs: { origin: 'vec3', direction: 'vec3', far: 'f64', owner: 'union:OrdnanceOwner', historyTick: 'f64|undefined', ignoreBody: 'ref:body|null', mount: 'ref:vehicle|null' },
    UnitCastAnswer: { found: 'bool', unit: 'ref:unit|null', distance: 'f64', point: 'vec3', normal: 'vec3', part: 'enum:HitPart|null|undefined' },
    BodyArgs: { body: 'ref:body|null|undefined' },
    MountAnswer: { found: 'bool', vehicle: 'ref:vehicle|null', boundRadius: 'f64', body: 'ref:body|null|undefined' },
    NearArgs: { center: 'vec3', radius: 'f64', owner: 'union:OrdnanceOwner', struck: 'set:unit', ignoreBody: 'ref:body|null' },
    NearAnswer: { found: 'bool', units: 'list:unit' },
    UnitArgs: { unit: 'ref:unit' },
    SphereAnswer: { found: 'bool', center: 'vec3', radius: 'f64' },
    GoneArgs: { subject: 'union:BeamSource' },
    SourceArgs: { source: 'union:BeamSource', origin: 'vec3' },
    BeamPoseArgs: { source: 'union:BeamSource', localOrigin: 'vec3', position: 'vec3', direction: 'vec3' },
    BeamPoseAnswer: { position: 'vec3', direction: 'vec3' },
    ShotArgs: { owner: 'union:OrdnanceOwner', channel: 'f64' },
    TickAnswer: { value: 'f64|null|undefined' },
    ShooterAnswer: { fireId: 'f64', unit: 'f64', generation: 'f64' },
    TargetArgs: { targetId: 'f64', targetGeneration: 'f64' },
    SeekArgs: { position: 'vec3', velocity: 'vec3', flightClass: 'str|undefined', targetId: 'f64', targetGeneration: 'f64', dt: 'f64' },
    SeekAnswer: { velocity: 'vec3', targetId: 'f64' },
    SegmentArgs: { from: 'vec3', to: 'vec3' },
    // An explosion: its definition as the class's field holds it (`defTag`:
    // 0 undefined, 1 null, 2 false, 3 true, 4 a number, 5 a string, 6 an
    // explosion, `def` the number, the string's or the explosion's index).
    BlastArgs: { defTag: 'u32', def: 'f64', position: 'vec3', owner: 'union:OrdnanceOwner' },
    // Damage done: by the round's class, or (`beam` given; for a unit,
    // `beamDef`) by the beam's copy of it holding this step's share of the
    // Damage (`damage`). `at` says which vector
    // the unit's hit is handed (0 one of its own, 1 the beam's end, 2 the
    // ordnance's unit raycast record's point, 3 the ordnance's start: a
    // damage event keeps it).
    StrikeArgs: { body: 'ref:body', def: 'def:ordnance', beam: 'ref:beam|null', damage: 'f64|undefined', owner: 'union:OrdnanceOwner', direction: 'vec3' },
    UnitStrikeArgs: { unit: 'ref:unit', def: 'def:ordnance', beam: 'ref:beam|null', beamDef: 'bool', damage: 'f64|undefined', at: 'u32', point: 'vec3', owner: 'union:OrdnanceOwner', part: 'enum:HitPart|null|undefined', direction: 'vec3' },
    // A round handed to the capture's observers: its kind's code, then,
    // after these words, its row as the store exports it (slot, generation,
    // its mirrored fields), so its mirror is the round as it is now.
    // `type`: 0 a round, 1 a grenade, 2 a beam (projectile's).
    ProjectileArgs: { kind: 'u32', type: 'u32' },
    LaunchArgs: { kind: 'u32', tick: 'f64|null|undefined' },
    // An end: `struck` the bolt's impact record when it struck what it was
    // fired at, else the point and normal given.
    ImpactArgs: { kind: 'u32', point: 'vec3', normal: 'vec3', surface: 'f64', struck: 'rec:Impact|null' },
    ContactArgs: { kind: 'u32' },
    BounceArgs: { def: 'def:ordnance', position: 'vec3', speed: 'f64' },

    // What JavaScript asks of the rounds (src/ordnance-sim.js), written into
    // the call block for the module's exports (rust/sim/src/ordnance/
    // module.rs): a round fired (ordnance.js fireRound's arguments) and a
    // flyer's missile set up (flightMissile's).
    FireArgs: {
      def: 'def:ordnance',
      origin: 'vec3',
      direction: 'vec3',
      owner: 'union:OrdnanceOwner',
      visual: 'vec3|null',
      ignoreBody: 'ref:body|null',
      emitter: 'union:BeamSource',
      channel: 'f64',
      targetId: 'f64|undefined',
      targetGeneration: 'f64|undefined',
    },
    FlightMissileArgs: { round: 'ref:round', flightClass: 'str|undefined', life: 'f64', speed: 'f64' },
  },

  imports: {
    cast: { args: 'CastArgs', result: 'CastAnswer' },
    castMoving: { args: 'CastArgs', result: 'CastAnswer' },
    raycastUnits: { args: 'UnitCastArgs', result: 'UnitCastAnswer' },
    riderMount: { args: 'BodyArgs', result: 'MountAnswer' },
    fatQuery: { args: 'NearArgs', result: 'NearAnswer' },
    unitSphere: { args: 'UnitArgs', result: 'SphereAnswer' },
    isDroid: { args: 'UnitArgs', result: 'FlagAnswer' },
    gone: { args: 'GoneArgs', result: 'FlagAnswer' },
    beamLocalOrigin: { args: 'SourceArgs', result: 'PointAnswer' },
    beamPose: { args: 'BeamPoseArgs', result: 'BeamPoseAnswer' },
    shotTick: { args: 'ShotArgs', result: 'TickAnswer' },
    historyTick: { args: 'NoArgs', result: 'NumberAnswer' },
    shooter: { args: 'ShotArgs', result: 'ShooterAnswer' },
    missileTarget: { args: 'TargetArgs', result: 'PointAnswer' },
    navigateMissile: { args: 'SeekArgs', result: 'SeekAnswer' },
    waterCrossing: { args: 'SegmentArgs', result: 'PointAnswer' },
    waver: { args: 'NoArgs', result: 'NumberAnswer' },
    spin: { args: 'NoArgs', result: 'NumberAnswer' },
    explosion: { args: 'BlastArgs', result: null },
    hit: { args: 'StrikeArgs', result: null },
    hitUnit: { args: 'UnitStrikeArgs', result: null },
    projectile: { args: 'ProjectileArgs', result: null },
    launch: { args: 'LaunchArgs', result: null },
    impact: { args: 'ImpactArgs', result: null },
    grenadeContact: { args: 'ContactArgs', result: null },
    bounce: { args: 'BounceArgs', result: null },
  },

  kinds: {
    // The battle's Ordnance (ordnance.js:177): its four lists, in the order
    // they are stepped (and filtered, keeping order) each tick.
    ordnance: {
      singleton: true,
      fields: {
        bolts: 'list:bolt',
        beams: 'list:beam',
        grenades: 'list:grenade',
        rounds: 'list:round',
        // Where the last bolt step started, and where a fat ray struck
        // (sweepFat): a unit's damage event keeps this vector as where the
        // hit came from, and the world's digest reads it at the tick's end.
        start: 'vec3',
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
        seed: 'page drawing (its flicker)',
        weapon: 'the gun drawn firing it (combat.js bindRoundMuzzle), for the muzzle the page draws it from',
        muzzleGeneration: 'the same',
        muzzleIndex: 'the same',
        muzzleBarrel: 'the same',
        mount: 'the hull or turret drawn firing it (muzzles.js bindMountedRound), for the same',
        mountSeat: 'the same',
        mountWeapon: 'the same',
        mountBarrel: 'the same',
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
        // `damageDef` is the definition's copy whose Damage each step that
        // strikes sets (stepBeam): its Damage is the state (undefined until
        // then when the class names none).
        damage: { type: 'f64|undefined', js: 'damageDef.Damage', mirror: false },
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
        contactTime: 'when it first touched anything, written by the battle (game.js onGrenadeContact) for the AI\'s danger warnings',
        aiDanger: 'the AI\'s own record of warning soldiers of it (ai/classic/combat.js grenadeUpdate)',
      },
    },

    // A rocket, missile, shell or fat ray (ordnance.js round(), fatray(),
    // stepRound()), with what a flyer's missiles add (vehicles.js
    // fireSeat).
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
      },
      skip: {
        mesh: 'its model, drawn by the page',
        trail: 'its trail effect, drawn by the page',
        light: 'colour from the definition, for drawing',
        visualMuzzle: 'a page muzzle callback (combat.js bindRoundMuzzle)',
        muzzleOffset: 'page drawing',
        mountResolved: 'page drawing',
        weapon: 'the gun drawn firing it (combat.js bindRoundMuzzle), for the muzzle the page draws it from',
        muzzleGeneration: 'the same',
        muzzleIndex: 'the same',
        muzzleBarrel: 'the same',
        mount: 'the hull or turret drawn firing it (muzzles.js bindMountedRound), for the same',
        mountSeat: 'the same',
        mountWeapon: 'the same',
        mountBarrel: 'the same',
      },
    },

    // Kinds the ordnance refers to, owned elsewhere: a unit is known by its
    // id (src/registry.js), the others by identity alone. `class` names the
    // JavaScript classes a union tells them apart by (src/units.js Unit,
    // src/vehicles.js Vehicle); an emitter (a hull's or seat's beamSource)
    // and an owner snapshot (src/ordnance-owner.js) are plain objects, told
    // apart by the tests the battle gives.
    unit: { key: 'id', class: 'Unit' },
    vehicle: { class: 'Vehicle' },
    body: {},
    emitter: {},
    ownerSnapshot: {},
  },
};
