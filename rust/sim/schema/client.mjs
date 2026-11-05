// Page prediction's flat frame blocks. Offsets are Float64 words relative to
// the frame. Correction blocks pack four little-endian bytes per word.
// A retained command never contains a JavaScript reference.
export default {
  enums: {
    // client-prediction.js run: setup commands and the single frame operation.
    ClientOp: ['configureActor', 'configureWeapon', 'predict', 'reset', 'configureMuzzles'],
    // Actor replay and mounted weapon timers share the owner, not its body.
    ClientMode: ['actor', 'mounted'],
    // Existing actor/config and recursive weapon-definition discriminants.
    ClientActorKind: ['soldier', 'droideka'],
    ClientValueKind: ['undefined', 'null', 'false', 'true', 'number', 'string', 'array', 'object'],
    ClientHeld: ['forward', 'backward', 'right', 'left', 'turnRight', 'turnLeft', 'primary', 'secondary'],
    ClientAction: ['jump', 'crouch', 'prone', 'use', 'firePrimary', 'fireSecondary', 'reload', 'nextPrimary', 'previousPrimary', 'nextSecondary', 'zoom'] },
  records: {
    // prediction/weapons.rs Session.write: variable Weapon rows have a count,
    // then this fixed selection/zoom tail and optional mounted barrel results.
    ClientWeapons: { $layout: true, count: 'u32' },
    ClientArsenalTail: { $layout: true, primary: 'f64', secondary: 'f64', zoomStep: 'f64', barrelsCount: 'u32' },
    ClientBarrelResult: { $layout: true, barrel: 'f64', written: 'bool' },
    ClientMount: { $layout: true, advance: 'bool', tow: 'bool' },
    ClientMuzzles: { $layout: true, barrels: 'list:u32' },
    ClientFrame: { $layout: true,
      epoch: 'f64', identity: 'f64', generation: 'f64', source: 'f64', authorityTick: 'f64',
      mode: 'enum:ClientMode', mountsAt: 'u32', mountsCount: 'u32', world: 'u32', ignored: 'f64', movement: 'bool', dead: 'bool', underwater: 'bool', shield: 'bool',
      actorAt: 'u32', actorWords: 'u32', correctionAt: 'u32', correctionBytes: 'u32',
      weaponsAt: 'u32', weaponsWords: 'u32', flagsAt: 'u32', flagsCount: 'u32',
      commandsAt: 'u32', commandsCount: 'u32', crowdAt: 'u32', crowdCount: 'u32',
      held: 'u32', presentedThrough: 'f64', cosmeticSlots: 'u32', mouseScale: 'f64', invert: 'bool', kick: 'f64x3', checkTick: 'f64',
    },
    ClientCommand: { $layout: true,
      tick: 'f64', generation: 'f64', sequence: 'f64', sent: 'bool', held: 'u32', actions: 'u32',
      lookAt: 'u32', lookCount: 'u32', mouseScale: 'f64|undefined', invert: 'bool|undefined',
      primaryCount: 'u32', primaryId: 'f64', secondaryCount: 'u32', secondaryId: 'f64',
    },
    ClientLook: { $layout: true, x: 'f64', y: 'f64' },
    ClientCrowd: { $layout: true, position: 'vec3', radius: 'f64', height: 'f64', center: 'f64' },
    ClientResult: { $layout: true,
      tick: 'f64', continued: 'bool', sequence: 'f64', held: 'u32', ticks: 'u32',
      actorAt: 'u32', actorWords: 'u32', correctionAt: 'u32', correctionBytes: 'u32',
      weaponsAt: 'u32', weaponsWords: 'u32', shotsAt: 'u32', shotsCount: 'u32',
      beforeAt: 'u32', beforeShot: 'u32', lookYaw: 'f64', checkFound: 'bool', checkPosition: 'vec3', kick: 'f64x3',
    },
    ClientShot: { $layout: true,
      tick: 'f64', sequence: 'f64', press: 'f64', held: 'bool', button: 'u32', weapon: 'u32', level: 'f64',
      kind: 'u32', yaw: 'f64', pitch: 'f64', charge: 'f64', target: 'f64', targetGeneration: 'f64', swing: 'f64',
      actorAt: 'u32', visualGun: 'rec:LastGun|undefined',
    },
  },
};
