// The battle's units (src/units.js, src/registry.js, src/soldier.js,
// src/droideka.js): the registry and its members in its order, each unit's
// life, intent, controls and hit shapes, and its body, a soldier's or a
// droideka's. The fields of other packages' objects a unit's code reads
// (its combat, its hold by a creature) are declared here through their
// paths, unmirrored, until those packages declare their kinds.
// docs/sim-store.md says how a schema is written; tools/sim-schema.mjs
// makes the code from it.
export default {
  enums: {
    // A soldier's posture (soldier.js `posture`, `diveFrom`).
    Posture: ['stand', 'crouch', 'prone'],
    // In the air (soldier.js `air`; null on the ground).
    Air: ['jump', 'fall', 'thrown', 'jet'],
    // What a hit shape is and which part of the body (units.js, rider-pose.js).
    ShapeType: ['sphere', 'cylinder', 'box', 'capsule'],
    ShapePart: ['body', 'head'],
  },

  unions: {
    // The registry's members (registry.js `list`): the units, and the
    // targets hulls, turrets and buildings add (src/seats.js VehicleTarget,
    // TurretTarget; src/objectives.js ObjectiveTarget).
    Member: ['ref:unit', 'ref:vehicleTarget', 'ref:turretTarget', 'ref:objectiveTarget'],
    // A hit shape's `half`: a cylinder's half height (0 on a sphere), or a
    // box's or capsule's half size, the shape's own `halfSize`.
    Half: ['f64', 'vec3'],
    // A unit's body (`soldier`): a soldier, or a droideka.
    UnitBody: ['rec:Soldier', 'rec:Droideka'],
  },

  records: {
    // What a unit's controller asks each tick (units.js blankIntent), and
    // what a seat's AI adds once it drives (brake, up, down).
    Intent: {
      forward: 'f64', right: 'f64', turn: 'f64', pitch: 'f64',
      jump: 'bool', crouch: 'bool', prone: 'bool', use: 'bool',
      buttons: 'f64x2',
      reload: 'bool', nextPrimary: 'bool', previousPrimary: 'bool', nextSecondary: 'bool', zoom: 'bool',
      aim: 'vec3|null', jetDirection: 'vec3|null', exit: 'bool',
      brake: 'bool|undefined', up: 'f64|undefined', down: 'f64|undefined',
    },
    // What the unit's tick hands its body (Unit `input`).
    MoveInput: {
      forward: 'f64', right: 'f64', turn: 'f64', pitch: 'f64',
      jump: 'bool', crouch: 'bool', prone: 'bool', use: 'bool', jetDirection: 'vec3|null',
    },
    // IntentControls (units.js:130): the buttons latched once a tick.
    Controls: { was: 'f64x2', pressed: 'list:bool' },
    // The broadphase sphere about the hit shapes (`bounds`).
    Bounds: { center: 'vec3', radius: 'f64' },
    // A hit shape (units.js:212, rider-pose.js).
    HitShape: { type: 'enum:ShapeType', part: 'enum:ShapePart', center: 'vec3', radius: 'f64', half: 'union:Half', yaw: 'f64', halfSize: 'vec3' },
    // A creature's hold on him (props.js seize): its prop, since when, and
    // where it holds him now.
    Held: { prop: 'ref:prop', since: 'f64', frame: 'mat4|null|undefined', time: 'f64|undefined' },
    // Under water (game.js's step, water-state.js soldierStep).
    WaterContact: { gunUnder: 'bool|undefined', under: 'f64|undefined', wakeClock: 'f64|undefined', splashClock: 'f64|undefined' },
    // A class's collision damage rule (collision-damage.js collisionRule).
    CollisionRule: { threshold: 'f64x3', scale: 'f64x3' },
    // A soldier's jetpack numbers (Soldier `jet`).
    Jet: { jump: 'f64', push: 'f64', recharge: 'f64', cost: 'f64', initial: 'f64', minBorder: 'f64', hover: 'bool' },

    // A soldier's body (src/soldier.js): its class's numbers and its
    // movement's state. Scratch vectors and the collision world are not
    // state (each tick sets them before it reads them).
    Soldier: {
      $class: 'Soldier',
      maxSpeed: 'f64', maxStrafe: 'f64', maxJump: 'f64', maxTurn: 'f64', maxPitch: 'f64', accel: 'f64', pitchLimits: 'f64x2',
      jet: 'rec:Jet', fuel: 'f64', collisionRule: 'rec:CollisionRule', collisionDamage: 'f64',
      maxFlyHeight: 'f64', getupTime: 'f64', diveTime: 'f64',
      position: 'vec3', previous: 'vec3', settled: 'vec3', velocity: 'vec3', normal: 'vec3',
      aimYaw: 'f64', aimPitch: 'f64', posture: 'enum:Posture', air: 'enum:Air|null', dive: 'f64', diveFrom: 'enum:Posture',
      stateTime: 'f64', lock: 'f64', grounded: 'bool', lookX: 'f64', lookY: 'f64', tickLength: 'f64',
      groundAdjustment: 'f64', avoidance: 'vec3', passThrough: 'ref:body|null', mover: 'f64',
      // Set by the first spawn (Unit.spawn).
      dead: 'bool|undefined',
    },

    // A droideka's shield (droideka.js arm).
    Shield: { def: 'def:weapon', max: 'f64', value: 'f64', on: 'bool', drain: 'f64', refill: 'f64' },
    // The gun and barrel the last shot left from (nextGun).
    LastGun: { index: 'f64', barrel: 'f64' },
    // A pose the unfold's blend starts from (DroidekaAnimator `from`, a bone's).
    BonePose: { q: 'quat', p: 'vec3' },
    // The hidden gun rig's animator (DroidekaAnimator, as droideka.js arm
    // makes `muzzleAnimator`): what persists from one tick's posing to the
    // next. The rig's bones are posed afresh from these each tick.
    RigAnimator: {
      facing: 'f64', legs: 'f64', mode: 'f64', wentUpRolled: 'bool', rollOrigin: 'f64', transition: 'f64',
      from: 'list:rec:BonePose', pivot: 'vec3', rollCenter: 'vec3',
    },
    // A droideka's body (src/droideka.js): its class's numbers, its modes,
    // walk and ball, its shield and guns.
    Droideka: {
      $class: 'Droideka',
      def: 'def:unitClass', maxTurn: 'f64', pitchLimits: 'f64x2', speed: 'f64', turnThreshold: 'f64', ballRadius: 'f64',
      maxSpin: 'f64', maxBallSlope: 'f64', eyeHeight: 'f64', rootIndex: 'f64', maxFlyHeight: 'f64',
      collisionRule: 'rec:CollisionRule', collisionDamage: 'f64',
      position: 'vec3', previous: 'vec3', settled: 'vec3', velocity: 'vec3', avoidance: 'vec3',
      aimYaw: 'f64', aimPitch: 'f64', stateTime: 'f64', grounded: 'bool', lookX: 'f64', lookY: 'f64',
      mode: 'f64', time: 'f64', reverse: 'bool', legs: 'f64', spin: 'f64', roll: 'f64', lean: 'f64', dead: 'bool',
      blastClock: 'f64', gun: 'f64', lastGun: 'rec:LastGun|undefined', shield: 'rec:Shield|null',
      // (Its water rule is its class's, waterDamageRule(def), made once.)
      waterClock: 'f64|undefined',
      muzzleAnimator: 'rec:RigAnimator|undefined',
    },
  },

  kinds: {
    // The registry (src/registry.js Units): its members in the order they
    // were added (raycasts' ties go to the earlier), the next id and the
    // relations set by hand.
    units: {
      singleton: true,
      fields: {
        list: 'list:union:Member',
        nextId: 'f64',
        alliances: 'map:f64',
      },
      skip: {
        byIdMap: 'the list by id',
        teams: 'the mission\'s table (spawning.js teamTable: each team\'s relations, faction and capture team), set once as the battle starts, from its content',
      },
    },

    // A unit (src/units.js Unit), the same object for the battle (pooled
    // by class, its generation bumped each life).
    unit: {
      key: 'id',
      class: 'Unit',
      fields: {
        generation: 'f64',
        side: 'str',
        className: 'str',
        classDef: 'def:unitClass',
        walker: 'bool',
        human: 'bool',
        kind: 'f64',
        hero: 'bool',
        team: 'f64',
        character: 'ref:character|null',
        controller: 'ref:controller|null',
        seat: 'ref:seatLease|null',
        alive: 'bool',
        dead: 'bool',
        deadTime: 'f64',
        removed: 'bool',
        intent: 'rec:Intent',
        controls: 'rec:Controls',
        input: 'rec:MoveInput',
        bounds: 'rec:Bounds',
        rootScale: 'f64',
        shapes: 'list:rec:HitShape',
        shapeCount: 'f64',
        spheresTick: 'f64',
        tickCount: 'f64',
        navRadius: 'f64|undefined',
        targetable: 'bool|undefined',
        waterContact: 'rec:WaterContact|undefined',
        riderHits: 'list:rec:HitShape|undefined',
        held: 'rec:Held|null|undefined',
        networkHuman: 'bool|undefined',
        soldier: 'union:UnitBody',
        // What the unit's code reads of its combat (src/combat.js, the
        // combat package's) and writes into it.
        combatDead: { type: 'bool', js: 'combat.dead', mirror: false },
        aimOverride: { type: 'vec3|null|undefined', js: 'combat.aimOverride', mirror: false },
        mounted: { type: 'bool|undefined', js: 'combat.mounted', mirror: false },
        remotesActive: { type: 'bool|undefined', js: 'combat.remotes.active', mirror: false },
        pivot: { type: 'vec3', js: 'combat.pivot', mirror: false },
        gunUnderwater: { type: 'bool|undefined', js: 'combat.gunUnderwater', mirror: false },
      },
      skip: {
        hitSet: 'which of its arrays the hit shapes are: riderHits exactly when shapeCount is over 2 (a rider set has 6 or 10, its own 1 or 2)',
        actor: 'its model and skeleton (content, and the page\'s drawing)',
        animator: 'the page\'s pose (a droideka\'s hidden gun rig is its body\'s)',
        combat: 'the combat package\'s PlayerCombat (the fields read here are declared through paths)',
        presentation: 'the page\'s smoothing (a world\'s reads nothing back)',
        jetFlame: 'the page\'s effect', bubble: 'the page\'s shield', poseClock: 'the page\'s posing', gone: 'the page\'s dissolve',
        ownMaterials: 'the page\'s materials', crowdSlot: 'the page\'s crowd shadows', shadowNear: 'the page\'s shadows',
        flameAt: 'the page\'s effect', flameTurn: 'the page\'s effect', flameScale: 'the page\'s effect',
        onDeath: 'the battle\'s hook', onRemoved: 'the factory\'s hook', onMoved: 'the AI\'s hook (ai.aim)',
        enhancedDeath: 'the page\'s setting', supply: 'getters over its body and combat', itemTarget: 'getters over its body, and hooks',
        steer: 'the vehicle AI\'s', remoteStep: 'the network world\'s hook (a network player\'s recon droid)',
        networkShot: 'the network capture\'s', networkShots: 'the network capture\'s', networkViewTick: 'the network capture\'s',
        networkInputLeadTicks: 'the network mailbox\'s', networkPlayoutMaxTicks: 'the network mailbox\'s',
      },
    },

    // Kinds the units refer to, owned elsewhere.
    vehicleTarget: { class: 'VehicleTarget' },
    turretTarget: { class: 'TurretTarget' },
    objectiveTarget: { class: 'ObjectiveTarget' },
    character: { class: 'Character' },
    controller: {},
    seatLease: { class: 'SeatLease' },
    prop: {},
  },
};
