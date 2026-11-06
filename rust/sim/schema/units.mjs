// The battle's units (src/units.js, src/registry.js, src/soldier.js,
// src/droideka.js): the registry and its members in its order, each unit's
// life, intent, controls and hit shapes, its body, a soldier's or a
// droideka's, and its combat (the combat package's record). The fields of
// other packages' objects a unit's code reads (its recon droids, its hold
// by a creature) are declared here through their paths, unmirrored, until
// those packages declare their kinds.
// docs/sim-store.md says how a schema is written; tools/sim-schema.mjs
// makes the code from it.
export default {
  enums: {
    F3AllocationStage: ['input', 'movement', 'aimer', 'combat', 'publication', 'other', 'hit', 'death', 'drop', 'fire'],
    F3SensorMode: ['shapes', 'raycast', 'query', 'bounds'],
    F3PoseAction: ['seize', 'carry'],
    F3Action: ['team', 'look', 'lookRaw', 'spawn', 'remove', 'health', 'combatField', 'weaponField', 'select', 'next', 'reset', 'damage', 'explode', 'controller', 'letGo', 'unspawn', 'bodyAir', 'randomBinding', 'viewpoint', 'metadata', 'bodySpawn', 'deadTime', 'poolRelease', 'consumeContact', 'mountedRandom'],
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
    F3CharacterRelease: { unit: 'ref:unit', character: 'ref:character' },
    F3RemovedNotice: { key: 'rec:F3UnitKey' },
    F3Allocation: { stage: 'enum:F3AllocationStage', bytes: 'u32', reallocation: 'bool' },
    // IntentControls.consume always publishes one-shot edges. Its held input
    // and latch mirrors need stores only when the canonical words changed.
    F3ControlView: { $layout:true, $delta:['was','pressed0','pressed1','input'],
      jump: {type: 'bool', js: 'intent.jump', mirror: false}, crouch: {type: 'bool', js: 'intent.crouch', mirror: false}, prone: {type: 'bool', js: 'intent.prone', mirror: false},
      use: {type: 'bool', js: 'intent.use', mirror: false}, exit: {type:'bool', js:'intent.exit', mirror:false}, reload: {type: 'bool', js: 'intent.reload', mirror: false},
      nextPrimary: {type: 'bool', js: 'intent.nextPrimary', mirror: false}, previousPrimary: {type: 'bool', js: 'intent.previousPrimary', mirror: false},
      nextSecondary: {type: 'bool', js: 'intent.nextSecondary', mirror: false}, zoom: {type: 'bool', js: 'intent.zoom', mirror: false},
      was: {type: 'f64x2', js: 'controls.was', mirror: false},
      pressed0: {type: 'bool', js: 'controls.pressed.0', mirror: false}, pressed1: {type: 'bool', js: 'controls.pressed.1', mirror: false},
      input: 'rec:MoveInput',
    },
    F3Publication: { $layout:true, $delta:['body','combat'], $parts:['body','controls'], body: 'rec:F3UnitMirror', arsenal: 'u32', combat: 'rec:F3CombatView', controls: 'rec:F3ControlView|null' },
    F3Acknowledgement: { $layout:true, version:'u32', eventEpoch:'u32', eventOffset:'u32' },
    F3Boundary: { $layout:true,
      $refresh: ['version', 'controlsConsumed', 'output', 'outputRows', 'outputHeap', 'outputWords',
        'events', 'eventWords', 'eventEpoch', 'boundsCache', 'boundsCapacity', 'accepted', 'allocations', 'reallocations', 'allocationBytes'],
      version: 'u32', controlsConsumed:'bool', acknowledgement:'u32', descriptorWords: 'u32', allocationWords: 'u32', relationWords: 'u32', hitPolicyWords: 'u32', blastBindingWords: 'u32', impactRulesWords: 'u32', serviceWords: 'u32', missileClassWords: 'u32', poseWords: 'u32', sensorWords: 'u32', foreignWords: 'u32', bodyNameWords: 'u32', input: 'u32', inputCapacity: 'u32', inputStride: 'u32', networkWords: 'u32',
      command: 'u32', commandWords: 'u32', adoptWords: 'u32', output: 'u32', outputRows: 'u32',
      outputStride: 'u32', outputHeap: 'u32', outputWords: 'u32',
      events: 'u32', eventWords: 'u32', eventEpoch: 'u32',
      boundsCache:'u32', boundsCapacity:'u32', randomEpoch:'u32', randomStates: 'u32', randomCapacity: 'u32', randomWords: 'u32', randomGlobals: 'u32', randomGlobalWords: 'u32',
      aimInput: 'u32', aimWords: 'u32', aimCapacity: 'u32', aimHeader: 'u32', aimHeaderWords: 'u32',
      foreignHeader: 'u32', foreignHeaderWords: 'u32', buildingSetupWords: 'u32', buildingStateWords: 'u32', hullSetupWords:'u32',
      separation: 'u32', separationWords: 'u32',
      itemClassWords: 'u32', itemTargetWords:'u32', dispenserWords: 'u32', unitItemsWords: 'u32', blastWords:'u32',
      reconClassWords: 'u32', reconLevelWords: 'u32', reconStrikeWords: 'u32', reconBindingWords: 'u32', reconWeaponWords: 'u32',
      accepted: 'bool', allocations: 'u32', reallocations: 'u32', allocationBytes: 'u32',
    },
    F3Metadata: { character: 'ref:character|null', controller: 'ref:controller|null', team: 'f64', life: 'f64', human: 'bool', networkHuman: 'bool|undefined', hero: 'bool' },
    // The post-movement aimer returns scalar settings in this retained block.
    // It never calls a mutating export while the owned phase is on the stack.
    F3AimHeader: { count: 'u32' },
    F3ForeignHeader: { count: 'u32', buildings: 'u32', network: 'u32' },
    // separateSoldiers scopes the authority to its simulation camera, while
    // multiplayer worlds visit every pair. The player handle stays native.
    F3Separation: { global: 'bool',  camera: 'vec3' },
    F3MissileClass: { name: 'str', class: 'enum:FlightClass' },
    F3AimChange: { key: 'rec:F3UnitKey', action: 'enum:F3Action', field: 'u32', index: 'u32', value: 'f64' },
    F3Change: { key: 'rec:F3UnitKey', action: 'enum:F3Action', field: 'u32', index: 'u32', value: 'f64', vector: 'vec3', flag: 'bool',
      onShield: 'f64', source: 'vec3|null', damageInfo: 'rec:F3DamageInfo|null', attacker: 'union:Attacker', controller: 'ref:controller|null', metadata: 'rec:F3Metadata|null', lease:'ref:seatLease|null' },
    F3Adoption: { key: 'rec:F3UnitKey', passive: 'bool', category: 'u32', hasActor:'bool', noEnter:'f64', droidekaWater: 'rec:DroidekaWater|null' },
    F3NetworkView: { view: 'f64|undefined', lead: 'f64|undefined', playout: 'f64|undefined', fire0: 'f64', fire1: 'f64', view0: 'f64|undefined', view1: 'f64|undefined' },
    // UnitFactory cold rows are inactive until a native acquisition assigns
    // their mixed-registry id. Structural births bind mirrors before readers.
    F3ColdStage: { $layout:true, cookie:'u32', adoption:'rec:F3Adoption' },
    F3UnitBirth: { key:'rec:F3UnitKey', cookie:'u32', replacement:'bool', order:'u32', retiredCookie:'u32|null' },
    F3LifeStreams: { key:'rec:F3UnitKey', combat:'u32', weapon:'u32', metadata:'rec:F3Metadata' },
    F3PlayerRealization: { $layout:true, className:'str', previous:'rec:F3UnitKey|null', local:'bool',
      metadata:'rec:F3Metadata', combatRandom:'u32', weaponRandom:'u32', reconKey:'u32|null', reconSeed:'u32' },
    // Admission resolves callbacks outside the native lifecycle. Later class
    // changes reuse these numeric streams without a JavaScript factory hook.
    F3PlayerBinding: { $layout:true, character:'ref:character', body:'ref:unit|null', local:'bool',
      combatRandom:'u32', weaponRandom:'u32', reconKey:'u32|null', reconSeed:'u32' },
    F3PlayerSpawn: { $layout:true, character:'ref:character', previous:'ref:unit|null',
      className:'str', position:'vec3', yaw:'f64', local:'bool' },
    // Unit.tick consumes short intent rows for every body. Only network actors
    // append rewind and trigger records; bots do not send null edge padding.
    F3PhaseInput: { key: 'rec:F3UnitKey', botTarget: 'union:Member|null', botTargetSet: 'bool', intent: 'rec:Intent', remoteFirstPerson: 'bool', hasNetwork: 'bool', networkIndex: 'u32' },
    F3PhaseNetwork: { network: 'rec:F3NetworkView', edgeList: 'bool', edge0: 'rec:CbEdge|null', edge1: 'rec:CbEdge|null', edge: 'rec:CbEdge|null' },
    // Ownership boundary. Slab generation names the pooled object; life names
    // its current spawn. Neither can substitute for the other.
    F3UnitKey: { $layout:true, slot: 'u32', generation: 'u32', life: 'f64' },
    F3IntentCommand: { key: 'rec:F3UnitKey', intent: 'rec:Intent' },
    F3LookCommand: { key: 'rec:F3UnitKey', dx: 'f64', dy: 'f64', sensitivity: 'f64', invert: 'bool', raw: 'bool' },
    F3SpawnCommand: { key: 'rec:F3UnitKey', position: 'vec3', yaw: 'f64' },
    // Published body values exclude class constants, clips, scratch storage,
    // shapes and full rig matrices. JS mutates its existing vectors in place.
    // These small motion records publish together. A straight reader keeps
    // their numeric stores simple while the enclosing body mask skips an
    // entirely unchanged pose and preserves the existing public storage.
    F3SoldierMotion: { $layout:true,
      normal: 'vec3', aimYaw: 'f64', aimPitch: 'f64', posture: 'enum:Posture', air: 'enum:Air|null',
      dive: 'f64', diveFrom: 'enum:Posture', stateTime: 'f64', lock: 'f64', grounded: 'bool',
      lookX: 'f64', lookY: 'f64', tickLength: 'f64', groundAdjustment: 'f64', fuel: 'f64',
      passThrough: 'ref:body|null', dead: 'bool|undefined',
    },
    F3DroidekaMotion: { $layout:true,
      aimYaw: 'f64', aimPitch: 'f64', stateTime: 'f64', grounded: 'bool', lookX: 'f64', lookY: 'f64',
      mode: 'f64', time: 'f64', reverse: 'bool', legs: 'f64', spin: 'f64', roll: 'f64', lean: 'f64',
      dead: 'bool', blastClock: 'f64', gun: 'f64', lastGun: 'rec:LastGun|undefined',
      shield: 'rec:F3ShieldView|null', waterClock: 'f64|undefined',
    },
    F3ShieldView: { $delta:['value','on'], value: 'f64', on: 'bool' },
    F3UnitMirror: { $layout:true,
      $delta: ['key','pooled','team','alive','dead','deadTime','removed','character','controller','seat','held','visible',
        'targetable','tickCount','remoteActive','position','previous','settled','velocity','avoidance',
        'soldier','droideka','waterContact'],
      key: 'rec:F3UnitKey', pooled:'bool', team: 'f64', alive: 'bool', dead: 'bool', deadTime: 'f64', removed: 'bool',
      character: 'ref:character|null', controller: 'ref:controller|null', seat: 'ref:seatLease|null', held: 'bool|null|undefined', visible: 'bool',
      targetable: 'bool|undefined', tickCount: 'f64', remoteActive: 'bool|undefined',
      position: 'vec3', previous: 'vec3', settled: 'vec3', velocity: 'vec3', avoidance: 'vec3',
      soldier: 'rec:F3SoldierMotion|null', droideka: 'rec:F3DroidekaMotion|null',
      waterContact: 'rec:WaterContact|undefined',
    },
    // Shape reads are explicit foreign AI sensor requests. Publication must
    // not force this cache early: death and blast do not invalidate it.
    F3DroidekaClass: {$layout:true,maxPitchSpeed:'f64|null',ballTurnSpeed:'f64|null',ballMaxLean:'f64|null',
      ballAcceleration:'f64|null',ballRollingFriction:'f64|null',clips:'f64x14'},
    F3BodyName: { body: 'ref:body', kernel: 'u32' },
    F3BodyBone: { name: 'str', parent: 'i32', position: 'vec3', quaternion: 'quat', scale: 'vec3' },
    F3BodySkeleton: { key: 'rec:F3UnitKey', bones: 'list:rec:F3BodyBone' },
    // Props own the grab node until their phase moves. Rust samples the
    // soldier's root and limbs; this command never contains posed body shapes.
    F3PoseCommand: { key: 'rec:F3UnitKey', action: 'enum:F3PoseAction', prop: 'ref:prop',
      clip: 'u32|null', grab: 'u32', since: 'f64', frame: 'mat4', time: 'f64' },
    F3HeldNotice: { key: 'rec:F3UnitKey', prop: 'ref:prop', grab: 'u32', since: 'f64',
      placed: 'bool', frame: 'mat4', time: 'f64|undefined' },
    F3SensorShape: { type: 'enum:ShapeType', part: 'enum:ShapePart', center: 'vec3', radius: 'f64', half: 'f64', halfIsSize: 'bool', halfSize: 'vec3', yaw: 'f64' },
    F3BoundsView: { $layout:true, $refresh:['bounds'], key:'rec:F3UnitKey', tick:'f64', bounds:'rec:Bounds' },
    F3ShapeView: { key: 'rec:F3UnitKey', tick: 'f64', bounds: 'rec:Bounds', shapes: 'list:rec:F3SensorShape' },
    F3Sensor: { $layout:true, $parts:['mode','key','origin','direction','far','radius','riders','masked','exclude0','exclude1'], mode: 'enum:F3SensorMode', key: 'rec:F3UnitKey', origin: 'vec3', direction: 'vec3', far: 'f64', radius: 'f64', riders: 'bool', masked: 'bool', exclude0:'ref:unit|null', exclude1:'ref:unit|null' },
    F3SensorResult: { hit: 'rec:UnitCastAnswer', members: 'list:union:Member' },
    // The foreign block declares inputs from still-unported props. F2 hulls,
    // units and canonical registry shapes must be read directly from stores.
    F3PropFact: { id: 'u32', alive: 'bool', dead: 'bool', team: 'f64', bounds: 'rec:Bounds', health: 'f64', category: 'u32', kind: 'f64', position: 'vec3', velocity: 'vec3', rollRate: 'f64', aim: 'vec3', life: 'f64', body: 'ref:body|null', box: 'rec:F3TargetBox|null' },
    F3TargetBox: { min: 'vec3', max: 'vec3', matrix: 'mat4' },
    F3RelationFact: { a: 'f64', b: 'f64', value: 'f64' },
    F3Foreign: { waterLevel: 'f64', props: 'list:rec:F3PropFact' },
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
    Soldier: { $layout: true,
      // client-prediction.js publishes only replay-owned fields; keep class,
      // collision scratch and drawn objects on their existing JavaScript owner.
      $publish: ['position', 'previous', 'settled', 'velocity', 'normal', 'avoidance', 'fuel', 'collisionDamage', 'aimYaw', 'aimPitch', 'dive', 'stateTime', 'lock', 'lookX', 'lookY', 'tickLength', 'groundAdjustment', 'posture', 'air', 'diveFrom', 'grounded'],
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
    // ---- Canonical unit phase inputs (Unit.tick,
    // rust/sim/src/units/module.rs): a world's units ticked in Rust,
    // written into the store before it and synced back after. A unit is
    // named by its id.
    // A unit the phase ticks, in the battle's order (the factory's built
    // list), and what the battle keeps of him outside the store: whether a
    // network player's recon droid steps with him (remoteStep), his trigger
    // edges (unit.networkShots, networkShot) and, for a droideka, its class's
    // water rule (game.js droidekaWater: src/vehicle-water.js).
    UnitsPhaseUnit: {
      unit: 'f64', remote: 'bool', edges: 'list:rec:CbEdge|null', edge: 'rec:CbEdge|null',
      droidekaWater: 'rec:DroidekaWater|null',
    },
    // waterDamageRule's interval and amount, and droidekaWaterDepth's depth
    // upright and rolled.
    DroidekaWater: { interval: 'f64', amount: 'f64', upright: 'f64', ball: 'f64' },
    // The AI's aimer (unit.onMoved: ai.aim) with his body's aim after it
    // moved; it answers with his intent's aim (the aimer's direction).
    // Classic updateAim and Enhanced updateAim read only these movement facts.
    // The ordinary publication retains the remaining motion/correction state.
    UnitAimArgs: { unit: 'f64', dt: 'f64', aimYaw: 'f64', aimPitch: 'f64',
      position: 'vec3', velocity: 'vec3', posture: 'enum:Posture|undefined', tickCount: 'f64' },
    UnitAimAnswer: { aim: 'vec3|null' },
    // The splash of a fall into the water (game.js presentEffect).
    UnitSplash: { at: 'vec3' },

    // A droideka's body (src/droideka.js): its class's numbers, its modes,
    // walk and ball, its shield and guns.
    Droideka: { $layout: true,
      // client-prediction.js publishes only replay-owned fields; keep class,
      // collision scratch and drawn objects on their existing JavaScript owner.
      $publish: ['position', 'previous', 'settled', 'velocity', 'avoidance', 'collisionDamage', 'aimYaw', 'aimPitch', 'stateTime', 'lookX', 'lookY', 'mode', 'time', 'legs', 'spin', 'roll', 'lean', 'blastClock', 'grounded', 'reverse', 'gun'],
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

  imports: {
    // Character pointers remain foreign until battle ownership moves.
    f3CharacterRelease: { args: 'F3CharacterRelease', result: null },
    // The AI's aimer between moving and firing.
    unitAim: { args: 'UnitAimArgs', result: 'UnitAimAnswer' },
  },

  events: { f3LifeStreams:'F3LifeStreams', f3UnitBirth:'F3UnitBirth', f3Removed: 'F3RemovedNotice', f3Held: 'F3HeldNotice', f3Splash: 'UnitSplash' },

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
        // PlayerCombat belongs to this canonical Unit. Construction imports
        // it once through the combat content facade; body adoption must not
        // overwrite it with an incomplete or older JavaScript mirror.
        combat: { type: 'rec:Combat', mirror: false, import: false },
        // Whether he flies a recon droid now (the droids' controller,
        // src/remote.js, which his combat holds).
        remotesActive: { type: 'bool|undefined', js: 'combat.remotes.active', mirror: false },
      },
      skip: {
        hitSet: 'which of its arrays the hit shapes are: riderHits exactly when shapeCount is over 2 (a rider set has 6 or 10, its own 1 or 2)',
        actor: 'its model and skeleton (content, and the page\'s drawing)',
        animator: 'the page\'s pose (a droideka\'s hidden gun rig is its body\'s)',
        presentation: 'the page\'s smoothing (a world\'s reads nothing back)',
        jetFlame: 'the page\'s effect', bubble: 'the page\'s shield', poseClock: 'the page\'s posing', gone: 'the page\'s dissolve',
        ownMaterials: 'the page\'s materials', crowdSlot: 'the page\'s crowd shadows', shadowNear: 'the page\'s shadows',
        flameAt: 'the page\'s effect', flameTurn: 'the page\'s effect', flameScale: 'the page\'s effect',
        onRemoved: 'the factory\'s hook', onMoved: 'the AI\'s hook (ai.aim)',
        enhancedDeath: 'the page\'s setting', supply: 'getters over its body and combat', itemTarget: 'getters over its body, and hooks',
        steer: 'the vehicle AI\'s', remoteStep: 'the network world\'s hook (a network player\'s recon droid)',
        networkShot: 'the network capture\'s', networkShots: 'the network capture\'s', networkViewTick: 'the network capture\'s',
        networkInputLeadTicks: 'the network mailbox\'s', networkPlayoutMaxTicks: 'the network mailbox\'s',
      },
    },

    // Kinds the units refer to, owned elsewhere.
    vehicleTarget: { key: 'id', class: 'VehicleTarget' },
    turretTarget: { key: 'id', class: 'TurretTarget' },
    objectiveTarget: { key: 'id', class: 'ObjectiveTarget' },
    character: { class: 'Character' },
    controller: {},
    seatLease: { class: 'SeatLease' },
    prop: {},
  },
};
