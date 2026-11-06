// BattleHud's scalar facts. Asset/text handles are prepared once at load;
// frame rows contain no strings, object pointers or geometry from the page.
// This boundary has no simulation-store mirror or runtime JS marshaller.
export const layoutOnly = true;
export default { enums: { HudArt: ['none', 'hud1', 'hud2', 'hud_health', 'hud_vehiclehealth', 'hud_flag_timer', 'hud_flag_icon', 'player_icon', 'map_mask', 'ambush_icon', 'hud_jetpack_icon', 'troop_icon', 'hit_indicator', 'stats_kills', 'stats_deaths', 'stats_cpp', 'solid'], HudCommand: ['none', 'wake', 'health', 'second', 'hull', 'supply', 'enemy', 'stats', 'message', 'banner', 'damage', 'hit', 'shot', 'squad', 'toggleMap', 'kill'] }, records: {
  HudGlyph: { $layout: true, code: 'u32', page: 'u32', advance: 'f64', x0: 'f64', x1: 'f64', y0: 'f64', y1: 'f64', u0: 'f64', v0: 'f64', u1: 'f64', v1: 'f64' },
  HudUniform: { slot: 'f64', kind: 'f64', data: 'mat4' },
  HudRenderFrame: { $layout: true, groups: 'u32', program: 'u32', camera: 'rec:HudUniform', cameraOther: 'rec:HudUniform', slots: 'f64x3', upload: 'f64x2', viewport: 'f64x2' },
  HudDraw: { $layout: true, first: 'u32', count: 'u32', texture: 'i32', mask: 'i32', masked: 'bool' },
  HudFrame: { $layout: true,
    dt: 'f64', height: 'f64', covered: 'bool', layer: 'bool', dead: 'bool', arsenal: 'bool', vehicle: 'bool', mount: 'bool',
    health: 'f64', healthIcon: 'u32', secondIcon: 'u32', secondFill: 'f64', secondLook: 'u32', hullIcon: 'u32', hullFill: 'f64',
    supplyHealth: 'bool', supplyAmmo: 'bool', scope: 'u32', leaving: 'f64',
    player: 'bool', playerX: 'f64', playerZ: 'f64', playerYaw: 'f64', speed: 'f64',
    map: 'bool', mapX: 'f64', mapZ: 'f64', mapHalf: 'f64', mapScale: 'f64', mapImage: 'u32',
    battle: 'bool', own: 'f64', ownStart: 'f64', ownBleed: 'f64', foe: 'f64', foeStart: 'f64', foeBleed: 'f64', countdown: 'f64', losing: 'bool', gameOver: 'bool',
    viewTeams: 'bool', hideTeams: 'bool', flip: 'f64', spawnMap: 'bool', reticle: 'bool', reticleX: 'f64', reticleY: 'f64',
    targetRelation: 'u32', secondRelation: 'u32', marker: 'bool', markerX: 'f64', markerY: 'f64', markerProgress: 'f64', lockChannel: 'u32',
    fov: 'f64', aspect: 'f64', seatName: 'u32', seats: 'f64x5', capturePost: 'i32', playerTeam: 'f64',
    states: 'u32', posts: 'u32', units: 'u32', easy: 'bool', viewer: 'u32', viewerTeam: 'f64', controllable: 'u32', hasViewer: 'bool', hasControl: 'bool', controlX: 'f64', controlY: 'f64', controlZ: 'f64', controlScan: 'f64',
    flightPilot: 'bool', flightWarning: 'u32', flightState: 'u32', flightCanLand: 'bool', promptTexts: 'f64x7', victory: 'u32', defeat: 'u32', leavingText: 'u32', killTemplates: 'f64x4',
  },
  HudWeapon: { $layout: true, id: 'u32', icon: 'u32', label: 'u32', reticle: 'u32', lock: 'bool', lockTarget: 'f64', lockElapsed: 'f64', lockTime: 'f64', charging: 'bool', held: 'f64', maxPressed: 'f64', reloading: 'bool', reloadLeft: 'f64', reloadTime: 'f64', bottomless: 'bool', heatPerShot: 'f64', heat: 'f64', overheated: 'bool', loaded: 'f64', spare: 'f64', reserve: 'f64', roundsPerClip: 'f64', wait: 'f64', shotDelay: 'f64', lockOffAngle: 'f64', kick: 'f64', stillSpread: 'f64', yawSpread: 'f64', pitchSpread: 'f64' },
  HudPostCount: { $layout: true, team: 'f64', count: 'f64', relation: 'u32' },
  HudPost: { $layout: true, x: 'f64', z: 'f64', mapped: 'bool', active: 'bool', team: 'f64', relation: 'u32', neutralize: 'f64', progress: 'f64', toward: 'f64', towardRelation: 'u32', contested: 'bool', counts: 'list:rec:HudPostCount', texture: 'u32', scale: 'f64', label: 'u32' },
  HudUnit: { $layout: true, id: 'u32', generation: 'f64', x: 'f64', y: 'f64', z: 'f64', visualX: 'f64', visualZ: 'f64', yaw: 'f64', forwardX: 'f64', forwardZ: 'f64', forward: 'bool', hasQuaternion: 'bool', quaternion: 'quat', removed: 'bool', seated: 'bool', dead: 'bool', hasPosition: 'bool', kind: 'f64', soldier: 'bool', hero: 'bool', alive: 'bool', team: 'f64', texture: 'u32', hasMapTexture: 'bool', scale: 'f64', scan: 'f64', transmit: 'f64' },
  HudState: { $layout: true, values: 'f64x4' },
  HudTeam: { $layout: true, relation: 'f64', locals: 'bool', tickets: 'f64', living: 'f64' },
  HudMapDraw: { $layout: true, matrix: 'mat4', size: 'f64', labels: 'bool', spawn: 'bool', player: 'bool', playerX: 'f64', playerZ: 'f64', playerTurn: 'f64', mask: 'u32', rings: 'u32' },
  HudRing: { $layout: true, x: 'f64', z: 'f64', size: 'f64', alpha: 'f64' },
} };
