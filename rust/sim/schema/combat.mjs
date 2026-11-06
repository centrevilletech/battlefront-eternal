// A soldier's side of the fight (src/combat.js PlayerCombat, src/weapons.js
// Weapon and Arsenal, src/weapon-targeting.js): his health and death, his
// aim, his weapons and their timers, the aim's kick, a repair tool's lock
// and the rounds a droideka's guns still owe this tick. One combat a unit,
// the same object for the battle (the factory pools the unit and its
// combat together); a new life makes a new arsenal (PlayerCombat.reset).
//
// What only draws (the weapon in the hand, its muzzle flash and light, a
// charge's glow, the first-person view) is the page's and not here; nor are
// the battle's systems a combat is handed (the collision world, the rounds,
// the items, the live props, the recon droids) or its hooks into the battle
// (onDamaged, onDeath, onBlowApart, onFired, onPresentation, arcTargets):
// those are its boundary (src/combat-world.js, rust/sim/src/combat/world.rs).
// docs/sim-store.md says how a schema is written; tools/sim-schema.mjs
// makes the code from it.
//
// A unit's combat is its `combat` field (rust/sim/schema/units.mjs). What
// the JavaScript keeps otherwise than as numbers and vectors is read
// through getters made for the store (src/combat.js, src/weapons.js): a
// weapon's class is the first of its levels and its level is a number, its
// generators and its authority are whether it has them (a capture's draw
// log names the stream), a press's seeded draws are their seed and how many
// have been taken, `seen` is aligned with the arsenal's weapons and the
// rounds a droideka's guns owe are plain records.
export default {
  enums: {
    F3CombatField:['health','zoomStep','dead','deathTime','blowApart','mounted','gunUnderwater','team','player','deferMuzzles','random','remotes'],
    F3WeaponField:['reserve','kick','clip','heat','floor','random','spreadScale','yawSpreadOverride','clearYawSpread'],
    CbShotKind:['round','repair','meleeStart'],
    F3OptionalState: ['absent', 'null', 'value'],
    F3PresentKind: ['weaponAction', 'endWeaponAction', 'lightning'],
    F3WeaponAction: ['reload', 'shoot', 'shoot2', 'shoot3'],
    F3Effect: ['headshot', 'meleeHit', 'meleeDeflect'],
    F3BlastStage: ['present', 'event', 'finished'],
    F3BodyEvent: ['deathBlast', 'blowApart', 'respawn'],
    F3HullEvent: ['damaged', 'explosionEffect'],
  },
  unions: {
    // Who hurt him last (PlayerCombat.lastAttacker): nobody yet
    // (undefined), nothing (null: a fall, the water, a death region), or
    // what a round or blast is credited to (src/ordnance-owner.js: the
    // player, a unit, a hull, a snapshot of a unit whose life has ended),
    // or a turret.
    Attacker: ['undefined', 'null', 'lit:player', 'ref:unit', 'ref:vehicle', 'ref:ownerSnapshot', 'ref:turret'],
    // What a repair tool's lock is on (src/props.js lockOn): one of the live
    // props' buildings (a turret among them), or a hull (live.vehicleTargets
    // is the battle's hulls).
    // Turret extends Building in JS. Classify the subclass before its base,
    // so native repair and crew services resolve the same stationary gun.
    RepairTarget: ['ref:turret', 'ref:building', 'ref:vehicle'],
  },

  records: {
    // A bound ClassicRandom is shared with the AI at phase/aimer boundaries.
    // Lazy named streams carry their first native use order without inserting
    // them into the JavaScript world's observed stream map during preparation.
    F3RandomEpoch: { $layout:true, revision:'u32' },
    F3RandomState: { state: 'u32', present: 'bool', sequence: 'u32' },
    F3RandomGlobals: { gameplay: 'u32', spin: 'u32', drop: 'u32', seat: 'u32' },
    F3Presentation: { key: 'rec:F3UnitKey', kind: 'enum:F3PresentKind', action: 'enum:F3WeaponAction|null',
      bank: 'str', posture: 'enum:Posture|null', prepare: 'bool', name: 'str', from: 'vec3', to: 'vec3', charge: 'f64', channel: 'f64|undefined' },
    F3ShieldNotice: { key: 'rec:F3UnitKey', taken: 'f64', left: 'f64', sourcePresent: 'bool', source: 'vec3', explosion: 'bool' },
    F3EffectNotice: { key: 'rec:F3UnitKey', effect: 'enum:F3Effect', position: 'vec3' },
    F3LifeRef: { id: 'f64', generation: 'f64' },
    // Character tallies remain a narrow synchronous foreign operation. The
    // native hit chooses the previous attacker's Character before damage.
    F3HeadCredit: { character: 'ref:character' },
    F3HullCredit: { character: 'ref:character' },
    // Hull geometry and class policy load once. Physics and health continue
    // to live in the fleet's generated vehicle row, including pooled lives.
    F3HullSetup: { hull:'ref:vehicle', member:'union:Member|null', health:'u32', category:'u32', kind:'f64', key:'str',
      blastCenter:'vec3', blastRadius:'f64', explosion:'u32|null', finalExplosion:'u32|null' },
    // Vehicles.variant binds a cold facade only when its original spawn is reached.
    F3HullBirth: { hull:'ref:vehicle', memberId:'u32', index:'u32', bodySerial:'u32' },
    F3HullNotice: { hull:'ref:vehicle', event:'enum:F3HullEvent', life:'f64', amount:'f64', attacker:'union:OrdnanceOwner',
      position:'vec3', team:'f64', attackerRef:'rec:F3LifeRef', hasAttackerRef:'bool', attackerTeam:'f64' },
    // Game.onDeath keeps Character population and credit foreign. Return the
    // controller pointer after AI detachment instead of entering a setter.
    F3DeathCredit: { key:'rec:F3UnitKey', unit:'ref:unit', killer:'ref:character|null', relation:'f64' },
    F3DeathAnswer: { controller:'ref:controller|null' },
    F3DeathNotice: { key:'rec:F3UnitKey', attacker:'union:Attacker', attackerRef:'rec:F3LifeRef', hasAttackerRef:'bool',
      attackerTeam:'f64', position:'vec3', team:'f64' },
    F3BlastNotice: { stage:'enum:F3BlastStage', explosion:'u32', position:'vec3', owner:'union:OrdnanceOwner',
      source:'rec:F3LifeRef', hasSource:'bool', team:'f64', radius:'f64', wet:'bool' },
    F3BlastCall: { explosion:'u32', position:'vec3', owner:'union:OrdnanceOwner' },
    F3BodyNotice: { key:'rec:F3UnitKey', event:'enum:F3BodyEvent', health:'f64' },
    F3DamageInfo: { explosion: 'bool', amount: 'f64|undefined', typed: 'f64|undefined', generic: 'f64|undefined', known: 'bool|undefined', pointState: 'enum:F3OptionalState', point: 'vec3', sourcePresent: 'bool', source: 'vec3' },
    F3ShotNotice: { key: 'rec:F3UnitKey', shot: 'rec:CbShot', position: 'vec3', direction: 'vec3', team: 'f64' },
    F3DamageNotice: { key: 'rec:F3UnitKey', amount: 'f64', attacker: 'union:Attacker', attackerRef: 'rec:F3LifeRef', hasAttackerRef: 'bool', attackerTeam: 'f64', info: 'rec:F3DamageInfo', position: 'vec3', team: 'f64' },
    // F1's class table carries only flight fields. Hit/blast policy is
    // prepared separately from the full authored damage definition.
    F3HitPolicy: { ordnance: 'def:ordnance', damage: 'f64', person: 'f64', animal: 'f64', droid: 'f64', vehicle: 'f64', building: 'f64', shield: 'f64' },
    F3BlastBinding: { explosion: 'u32', combat: 'u32' },
    F3ImpactRules: { friendlyFire: 'bool' },
    // CombatWorld.fireRound and dispense use numeric F1 handles prepared once
    // for each weapon level, including ordnance merged by charge levels.
    F3LockRule: { range: 'f64', onAngle: 'f64', offAngle: 'f64', relationMask: 'u32', categoryMask: 'u32' },
    F3WeaponService: { ordnance: 'def:ordnance|null', item: 'def:item|null', channel: 'f64', lock: 'rec:F3LockRule',
      bank: 'str', lightning: 'str', hasLightning: 'bool' },
    // Commands carry the unit handle and its gameplay life, plus an arsenal
    // index. They cannot target a weapon of a replacement life.
    F3WeaponSettings: {
      key: 'rec:F3UnitKey', weapon: 'u32', stream: 'u32', spreadScale: 'f64', sniperScale: 'f64',
      yawSpreadOverride: 'f64|null', floor: 'f64',
    },
    F3Selection: { key: 'rec:F3UnitKey', weapon: 'u32' },
    // Weapon.captureState: most clocks are unchanged on an inactive weapon.
    // The retained reader visits only changed weapons and changed fields.
    F3WeaponView: { $layout:true,
      $delta:['thrown','repair','level','admitShot','deployed','firing','timer','bottomless','perShot','clip','reserve',
        'heat','overheated','floor','kick','spreadScale','sniperScale','yawSpreadOverride','wait','reloadLeft','throwTime',
        'released','hidden','charging','held','charge','limit','pattern','salvosLeft','salvoWait','salvoSeed','salvoCount',
        'lockTargetId','lockGeneration','lockElapsed','lockComplete','melee'],
      thrown:'bool', repair:'bool',
      level: 'f64', admitShot: {type: 'bool', js: 'admits'}, deployed: 'bool', firing: 'bool', timer: 'f64',
      bottomless: 'bool', perShot: 'f64', clip: 'f64', reserve: 'f64', heat: 'f64', overheated: 'bool', floor: 'f64', kick: 'f64',
      spreadScale: 'f64', sniperScale: 'f64', yawSpreadOverride: 'f64|null',
      wait: 'f64', reloadLeft: 'f64', throwTime: 'f64', released: 'bool', hidden: 'bool',
      charging: 'bool', held: 'f64', charge: 'f64', limit: 'f64', pattern: 'f64', salvosLeft: 'f64', salvoWait: 'f64',
      salvoSeed: 'f64|null', salvoCount: 'f64', lockTargetId: 'f64', lockGeneration: 'f64', lockElapsed: 'f64', lockComplete: 'bool',
      melee: 'rec:Melee|null',
    },
    // PlayerCombat.update owns its notification latches and deferred muzzle
    // queue natively. Only fields read by foreign gameplay cross this view.
    F3CombatView: { $layout:true, $parts:['weapons','origin','target','aimDir','shotDir','left','up','pivot'],  $delta:['health','dead','deathTime','lastAttacker','repairTarget','repairLock','blowApart','zoomStep','kick',
      'mounted','gunUnderwater','origin','target','aimDir','shotDir','left','up','pivot','selected','weapons'],
      health: 'f64', dead: 'bool', deathTime: 'f64', lastAttacker: 'union:Attacker',
      repairTarget: 'union:RepairTarget|null', repairLock: 'str', blowApart: 'bool', zoomStep: 'f64', kick: 'rec:Kick',
      mounted: 'bool|undefined', gunUnderwater: 'bool',
      origin: 'vec3', target: 'vec3', aimDir: 'vec3', shotDir: 'vec3', left: 'vec3', up: 'vec3', pivot: 'vec3',
      selected: {type: 'f64x2', js: 'arsenal.selected', mirror: false}, weapons: {type: 'list:rec:F3WeaponView', js: 'arsenal.weapons', mirror: false},
    },

    // A melee weapon's swing (src/heroes.js MeleeSwing, captureState): the
    // time into it, which of its swings, and whether one is under way.
    Melee: { time: 'f64', index: 'f64', active: 'bool' },

    // A weapon (src/weapons.js Weapon): its class's level now, its clip,
    // reserve, heat and kick, its trigger's timers, its salvo and throw,
    // its charge, and a launcher's lock.
    Weapon: { $layout: true,
      // client-prediction.js publishes only replay-owned fields; keep class,
      // collision scratch and drawn objects on their existing JavaScript owner.
      $publish: ['deployed', 'firing', 'timer', 'bottomless', 'perShot', 'clip', 'reserve', 'heat', 'overheated', 'floor', 'kick', 'spreadScale', 'sniperScale', 'yawSpreadOverride', 'wait', 'reloadLeft', 'throwTime', 'released', 'hidden', 'charging', 'held', 'charge', 'limit', 'pattern', 'salvosLeft', 'salvoWait', 'lockTargetId', 'lockGeneration', 'lockElapsed', 'lockComplete'],
      // Its class (the first of its levels; `def` is the level it is at).
      def: { type: 'def:weapon', js: 'levels.0', mirror: false },
      // Which of its levels (the class, then each charge level) it is at.
      level: 'f64',
      // Whether an authority admits each shot first (a recon droid's
      // launch, src/remote.js).
      admitShot: { type: 'bool', js: 'admits' },
      deployed: 'bool',
      firing: 'bool',
      timer: 'f64',
      bottomless: 'bool',
      perShot: 'f64',
      clip: 'f64',
      reserve: 'f64',
      heat: 'f64',
      overheated: 'bool',
      floor: 'f64',
      kick: 'f64',
      // Whether its spread draws from a generator (the AI's, or a world
      // stream), or from the legacy stream (Math.random).
      random: { type: 'bool', js: 'hasRandom' },
      spreadScale: 'f64',
      sniperScale: 'f64',
      yawSpreadOverride: 'f64|null',
      wait: 'f64',
      reloadLeft: 'f64',
      throwTime: 'f64',
      released: 'bool',
      hidden: 'bool',
      charging: 'bool',
      held: 'f64',
      charge: 'f64',
      limit: 'f64',
      pattern: 'f64',
      salvosLeft: 'f64',
      salvoWait: 'f64',
      // A press's seeded draws (weapons.js seededRandom): its seed (null
      // when the fire cycle has none) and how many numbers it has given.
      salvoSeed: 'f64|null',
      salvoCount: 'f64',
      lockTargetId: 'f64',
      lockGeneration: 'f64',
      lockElapsed: 'f64',
      lockComplete: 'bool',
      // A melee weapon's swing (prepareMelee: its update is the swing's).
      melee: 'rec:Melee|null',
    },

    // What PlayerCombat.update last saw of a weapon (`seen`): whether it
    // was reloading and throwing, for the clips it starts and ends.
    Seen: { known: 'bool', reloading: 'bool', throwing: 'bool' },

    // The aim's kick (PlayerCombat.kick).
    Kick: { rate: 'f64', hold: 'f64', offset: 'f64' },

    // A shot a droideka's gun still owes this tick (pendingMuzzles): fired
    // after the tick samples its gun rig (flushMuzzles).
    PendingShot: {
      // Its weapon's place in the arsenal.
      weapon: 'f64',
      yaw: 'f64',
      pitch: 'f64',
      charge: 'f64',
      targetId: 'f64',
      targetGeneration: 'f64',
      selectedIndex: 'f64',
      selectedBarrel: 'f64',
      target: 'vec3',
      forward: 'vec3',
      kick: 'f64',
    },

    // A press a network player's trigger edge carries (unit.networkShot,
    // unit.networkShots: src/runtime/network-shot-aim.js): its button, its
    // id, how late it reached the authority, and the aim it saw.
    NetworkShot: { button: 'f64', id: 'f64', late: 'f64', aimYaw: 'f64', aimPitch: 'f64' },

    // A soldier's combat (src/combat.js PlayerCombat).
    Combat: {
      owner: 'ref:unit',
      player: 'bool',
      lastAttacker: 'union:Attacker',
      aimOverride: 'vec3|null|undefined',
      maxHealth: 'f64',
      health: 'f64',
      dead: 'bool',
      deathTime: 'f64',
      team: 'f64',
      repairTarget: 'union:RepairTarget|null',
      repairLock: 'str',
      blowApart: 'bool',
      chunks: 'f64',
      zoomStep: 'f64',
      kick: 'rec:Kick',
      // Its class's health scale's name (HEALTH_SCALES).
      scale: 'str',
      firstPerson: 'bool',
      mounted: 'bool|undefined',
      gunUnderwater: 'bool',
      deferMuzzles: 'bool',
      pendingMuzzles: { type: 'list:rec:PendingShot', js: 'pendingShots' },
      // Whether its arc rolls and blow-apart rolls draw from a generator
      // (the battle's gameplay stream) or the legacy stream.
      random: { type: 'bool', js: 'hasRandom' },
      // Whether it has recon droids to fly (a network player's).
      hasRemotes: 'bool',
      // Its arsenal (Arsenal): the weapons, each channel's selection.
      weapons: { type: 'list:rec:Weapon', js: 'arsenal.weapons', mirror: false },
      selected: { type: 'f64x2', js: 'arsenal.selected', mirror: false },
      seen: { type: 'list:rec:Seen', js: 'seenList' },
      // The aim as aim() last left it, which the battle reads between
      // updates (the pivot and gun origin, where shots go).
      origin: 'vec3',
      target: 'vec3',
      aimDir: 'vec3',
      shotDir: 'vec3',
      left: 'vec3',
      up: 'vec3',
      pivot: 'vec3',
    },

    // ---- Combat on the simulation's module (src/f3-combat.js, rust/sim/
    // src/combat/module.rs). A unit is named by its id, a weapon by its
    // place in his arsenal.
    // A trigger edge a network player's press carries (unit.networkShot,
    // unit.networkShots), its numbers possibly absent.
    CbEdge: { button: 'f64', id: 'f64|undefined', late: 'f64|undefined', aimYaw: 'f64|undefined', aimPitch: 'f64|undefined' },
    // A shot (weapons.js): kind 0 a round, 1 a repair tick, 2 a swing's start.
    CbShot: { unit: 'f64', weapon: 'f64', kind: 'enum:CbShotKind', yaw: 'f64', pitch: 'f64', charge: 'f64', targetId: 'f64', targetGeneration: 'f64', admission: 'f64|undefined', swing: 'f64' },
  },

  events: {
    f3Shot: 'F3ShotNotice',
    f3Damage: 'F3DamageNotice',
    f3Presentation: 'F3Presentation',
    f3Shield: 'F3ShieldNotice',
    f3Effect: 'F3EffectNotice',
    f3Death: 'F3DeathNotice',
    f3Blast: 'F3BlastNotice',
    f3Body: 'F3BodyNotice',
    f3Hull: 'F3HullNotice',
    f3HullBirth: 'F3HullBirth',
  },

  imports: {
    f3HeadCredit: { args: 'F3HeadCredit', result: null },
    f3HullCredit: { args: 'F3HullCredit', result: null },
    f3DeathCredit: { args:'F3DeathCredit', result:'F3DeathAnswer' },
  },

  kinds: {
    // Kinds a combat refers to, owned elsewhere.
    building: { class: 'Building' },
    turret: { class: 'Turret' },
  },
};
