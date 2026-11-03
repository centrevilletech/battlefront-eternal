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
// Until the flip that moves units and combat into the store together, a
// combat's state crosses to the replay harness as these records' values
// (tools/sim-capture/combat.mjs builds them), not as a field of the unit
// kind: a weapon's level is its place among its levels, its generators are
// whether it has one (the capture's draw log names the stream) and a
// press's seed with the draws taken from it, and `seen` is aligned with the
// arsenal's weapons.
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
    // A press's seeded draws (weapons.js seededRandom): its seed and how
    // many numbers its fire cycle has taken.
    SeededDraws: { seed: 'f64', count: 'f64' },

    // A melee weapon's swing (src/heroes.js MeleeSwing, captureState): the
    // time into it, which of its swings, and whether one is under way.
    Melee: { time: 'f64', index: 'f64', active: 'bool' },

    // A weapon (src/weapons.js Weapon): its class's level now, its clip,
    // reserve, heat and kick, its trigger's timers, its salvo and throw,
    // its charge, and a launcher's lock.
    Weapon: {
      def: 'def:weapon',
      // Which of its levels (the class, then each charge level) it is at.
      level: 'f64',
      // Whether an authority admits each shot first (a recon droid's
      // launch, src/remote.js).
      admitShot: 'bool',
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
      random: 'bool',
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
      salvoRandom: 'rec:SeededDraws|null',
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
      pendingMuzzles: 'list:rec:PendingShot',
      // Whether its arc rolls and blow-apart rolls draw from a generator
      // (the battle's gameplay stream) or the legacy stream.
      random: 'bool',
      // Whether it has recon droids to fly (a network player's).
      hasRemotes: 'bool',
      // Its arsenal (Arsenal): the weapons, each channel's selection.
      weapons: 'list:rec:Weapon',
      selected: 'f64x2',
      seen: 'list:rec:Seen',
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
  },

  kinds: {
    // Kinds a combat refers to, owned elsewhere.
    building: { class: 'Building' },
    turret: { class: 'Turret' },
  },
};
