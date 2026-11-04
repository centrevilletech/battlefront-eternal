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
    RepairTarget: ['ref:building', 'ref:turret', 'ref:vehicle'],
  },

  records: {
    // A melee weapon's swing (src/heroes.js MeleeSwing, captureState): the
    // time into it, which of its swings, and whether one is under way.
    Melee: { time: 'f64', index: 'f64', active: 'bool' },

    // A weapon (src/weapons.js Weapon): its class's level now, its clip,
    // reserve, heat and kick, its trigger's timers, its salvo and throw,
    // its charge, and a launcher's lock.
    Weapon: { $layout: true,
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

    // ---- Combat on the simulation's module (src/combat-sim.js, rust/sim/
    // src/combat/module.rs). A unit is named by its id, a weapon by its
    // place in his arsenal.
    // A trigger edge a network player's press carries (unit.networkShot,
    // unit.networkShots), its numbers possibly absent.
    CbEdge: { button: 'f64', id: 'f64|undefined', late: 'f64|undefined', aimYaw: 'f64|undefined', aimPitch: 'f64|undefined' },
    // What the battle's JavaScript keeps of a weapon that its update reads:
    // whether it has a generator, and the AI's spread settings.
    CbWeaponIn: { random: 'bool', spreadScale: 'f64', yawSpreadOverride: 'f64|null' },
    // What a combat command reads of the unit while units are the
    // JavaScript's (his life, intent, controls, body and trigger edges), and
    // the combat's fields the battle's JavaScript keeps (what it sets on the
    // combat: whether he mans a gun or flies a droid, his team, his aimer's
    // direction, whether his gun is under water, the generators).
    CbIn: {
      unit: 'f64',
      generation: 'f64',
      classDef: 'def:unitClass',
      networkHuman: 'bool|undefined',
      seated: 'bool',
      remotesActive: 'bool|undefined',
      intent: 'rec:Intent',
      controls: 'rec:Controls',
      soldier: 'union:UnitBody',
      edges: 'list:rec:CbEdge|null',
      edge: 'rec:CbEdge|null',
      aimOverride: 'vec3|null|undefined',
      mounted: 'bool|undefined',
      gunUnderwater: 'bool',
      team: 'f64',
      player: 'bool',
      deferMuzzles: 'bool',
      random: 'bool',
      hasRemotes: 'bool',
      weapons: 'list:rec:CbWeaponIn',
    },
    // A hurt and a blast as their commands take them (PlayerCombat.damage,
    // explode): an explosion is its index in src/combat-sim.js's table.
    CbDamageIn: { in: 'rec:CbIn', amount: 'f64', onShield: 'f64', source: 'vec3|null', attacker: 'union:Attacker', explosion: 'bool' },
    CbExplodeIn: { in: 'rec:CbIn', explosion: 'f64', position: 'vec3', owner: 'union:Attacker', scale: 'f64' },
    // What a command wrote into the unit: the presses and intent flags it
    // took, his aim's pitch (the kick), a blast's push on a soldier and a
    // droideka's shield.
    CbOut: {
      pressed0: 'bool', pressed1: 'bool', reload: 'bool', nextPrimary: 'bool', previousPrimary: 'bool', nextSecondary: 'bool', zoom: 'bool',
      aimPitch: 'f64', velocity: 'vec3', air: 'enum:Air|null', stateTime: 'f64', grounded: 'bool', shieldOn: 'bool', shieldValue: 'f64',
    },

    // The calls combat makes into the battle (rust/sim/src/combat/world.rs
    // CombatWorld), one record each way, answered by src/combat-sim.js with
    // the same objects src/combat-world.js hands the battle.
    CbUnit: { unit: 'f64' },
    CbNumber: { value: 'f64' },
    CbFlag: { value: 'bool' },
    CbText: { value: 'str' },
    CbRay: { unit: 'f64', origin: 'vec3', direction: 'vec3', far: 'f64', staticOnly: 'bool' },
    CbHit: { found: 'bool', distance: 'f64', point: 'vec3', normal: 'vec3' },
    CbWeapon: { unit: 'f64', weapon: 'f64' },
    // A draw: weapon -1 for the combat's own generator; legacy: Math.random's
    // stream for a weapon or combat without one.
    CbDraw: { unit: 'f64', weapon: 'f64', legacy: 'bool' },
    // weapon.admitShot's answer: 0 yes, 1 no, 2 a token.
    CbAdmit: { kind: 'f64', token: 'f64' },
    CbDroid: { found: 'bool', droid: 'f64' },
    CbDroidCall: { unit: 'f64', droid: 'f64' },
    CbLaunch: { unit: 'f64', weapon: 'f64', origin: 'vec3', velocity: 'vec3', yaw: 'f64', team: 'f64', admission: 'f64|undefined' },
    // (A weapon's level and lock as they are now, which its mirror may not
    // have yet.)
    CbLockOn: { unit: 'f64', eye: 'vec3', dir: 'vec3', weapon: 'f64', level: 'f64', current: 'union:RepairTarget|null', held: 'bool', team: 'f64' },
    CbTarget: { target: 'union:RepairTarget|null' },
    CbLockKey: { unit: 'f64', target: 'union:RepairTarget|null' },
    CbRepair: { unit: 'f64', weapon: 'f64', target: 'union:RepairTarget|null', team: 'f64' },
    CbLockPick: { unit: 'f64', weapon: 'f64', level: 'f64', lockComplete: 'bool', lockTargetId: 'f64', lockGeneration: 'f64', origin: 'vec3', dir: 'vec3' },
    // What a launcher's lock picks: 0 nothing to look with, 1 nothing in
    // reach, 2 a unit.
    CbPick: { pick: 'f64', id: 'f64', generation: 'f64', alive: 'bool' },
    // A shot (weapons.js): kind 0 a round, 1 a repair tick, 2 a swing's start.
    CbShot: { unit: 'f64', weapon: 'f64', kind: 'f64', yaw: 'f64', pitch: 'f64', charge: 'f64', targetId: 'f64', targetGeneration: 'f64', admission: 'f64|undefined', swing: 'f64' },
    // fromOrigin: the round leaves his gun origin (combat.origin), not a gun of
    // a droideka's.
    CbFire: { shot: 'rec:CbShot', from: 'vec3', fromOrigin: 'bool', dir: 'vec3', bind: 'bool', selected: 'bool', selectedIndex: 'f64', selectedBarrel: 'f64' },
    CbDispense: { unit: 'f64', weapon: 'f64', most: 'f64', origin: 'vec3', velocity: 'vec3', yaw: 'f64', team: 'f64' },
    CbArcTarget: { origin: 'vec3', center: 'vec3|null', radius: 'f64|null' },
    CbArcTargets: { targets: 'list:rec:CbArcTarget' },
    CbHurtArc: { unit: 'f64', index: 'f64', amount: 'f64', from: 'vec3' },
    // A hurt's hook (onDamaged): what combat made of the hurt (explosion:
    // its own info, a blast's) or the caller's info, kept by src/combat-sim.js.
    CbDamaged: { unit: 'f64', amount: 'f64', attacker: 'union:Attacker', explosion: 'bool', infoAmount: 'f64', typed: 'f64', generic: 'f64' },
    CbHealth: { unit: 'f64', health: 'f64' },
    // kind 0 weaponAction, 1 endWeaponAction, 2 lightning.
    CbPresent: { unit: 'f64', kind: 'f64', action: 'str|null', bank: 'str', posture: 'str|null', prepare: 'bool', name: 'str', from: 'vec3', to: 'vec3', charge: 'f64', channel: 'f64|null' },
    // A droideka's next gun (nextGun): its muzzle, and the counter and the
    // gun it moved on to (its body's own state).
    CbGun: { found: 'bool', point: 'vec3', gun: 'f64', hasLast: 'bool', lastIndex: 'f64', lastBarrel: 'f64' },
    CbGunAt: { unit: 'f64', index: 'f64', barrel: 'f64' },
    CbPoint: { found: 'bool', point: 'vec3' },
    // A droideka's shield took a hit: its value after it, which its hook
    // finds there (the shield goes down after the hook).
    CbShield: { unit: 'f64', taken: 'f64', left: 'f64', explosion: 'bool' },
  },

  imports: {
    combatCast: { args: 'CbRay', result: 'CbHit' },
    combatDraw: { args: 'CbDraw', result: 'CbNumber' },
    combatAdmit: { args: 'CbWeapon', result: 'CbAdmit' },
    combatRemoteOf: { args: 'CbWeapon', result: 'CbDroid' },
    combatRemoteEnter: { args: 'CbDroidCall', result: null },
    combatRemoteKill: { args: 'CbDroidCall', result: null },
    combatRemoteLaunch: { args: 'CbLaunch', result: null },
    combatLockOn: { args: 'CbLockOn', result: 'CbTarget' },
    combatLockKey: { args: 'CbLockKey', result: 'CbText' },
    combatRepair: { args: 'CbRepair', result: 'CbFlag' },
    combatLockPick: { args: 'CbLockPick', result: 'CbPick' },
    combatFireRound: { args: 'CbFire', result: null },
    combatDispense: { args: 'CbDispense', result: 'CbFlag' },
    combatMeleeStrike: { args: 'CbUnit', result: null },
    combatArcTargets: { args: 'CbUnit', result: 'CbArcTargets' },
    combatHurtArc: { args: 'CbHurtArc', result: null },
    combatLightning: { args: 'CbText', result: 'CbFlag' },
    combatFired: { args: 'CbShot', result: null },
    combatDamaged: { args: 'CbDamaged', result: null },
    combatDied: { args: 'CbUnit', result: null },
    combatBlownApart: { args: 'CbHealth', result: null },
    combatRespawn: { args: 'CbUnit', result: null },
    combatPresent: { args: 'CbPresent', result: null },
    combatNextGun: { args: 'CbUnit', result: 'CbGun' },
    combatGunAt: { args: 'CbGunAt', result: 'CbPoint' },
    combatShieldHit: { args: 'CbShield', result: null },
  },

  kinds: {
    // Kinds a combat refers to, owned elsewhere.
    building: { class: 'Building' },
    turret: { class: 'Turret' },
  },
};
