// Phase-level facts and diagnostics. Entity references include both storage
// and gameplay generations so a recycled facade cannot redirect a command.
export default {
  unions: {
    AiClassScalar: ['undefined', 'null', 'bool', 'f64', 'str'],
    AiActor: ['ref:unit', 'ref:seatUnit'],
  },
  enums: { AiJournalKind: ['shot', 'damage'] },
  records: {
    // Cold service presence is distinct from an empty native table. The
    // live host loads this once; no per-controller options cross a phase.
    AiCapabilities: { navigation: 'bool', combat: 'bool', combatAgent: 'bool', population: 'bool', vehicles: 'bool', classes: 'bool', randomB: 'bool', mineGround: 'bool', vehicleAdapter: 'bool' },
    AiCapacity: { registry: 'u32', characters: 'u32', posts: 'u32', hulls: 'u32', buildings: 'u32', controls: 'u32', grenades: 'u32', scopes: 'u32', cameras: 'u32', supportKeys: 'u32', repairJobs: 'u32' },
    AiMissionCall: { name: 'str', args: 'list:f64', strings: 'list:str' },
    AiStreamState: { a: 'u32', b: 'u32', c: 'u32' },
    AiColdSettings: {
      seed: 'u32', streamA: 'u32', streamB: 'u32', streams: 'rec:AiStreamState|null', enhanced: 'bool', tier: 'f64', profileTier: 'f64', playerTeam: 'f64|null', secondPlayerTeam: 'f64|null',
      network: 'bool', skillOverride: 'bool', playerSkill: 'f64', enemySkill: 'f64',
      sensorRange: 'f64', followNear: 'f64|null', followFar: 'f64|null', hard: 'bool|null', lodDisabled: 'bool',
      dense: 'bool', missionStayInTurrets: 'bool', noJetObstacleTest: 'bool', mission: 'list:rec:AiMissionCall',
      capabilities: 'rec:AiCapabilities', capacity: 'rec:AiCapacity',
    },
    AiWorldSetup: { $layout: true, settings: 'rec:AiColdSettings', navigation: 'rec:AiNavSetup' },
    // Spawning keeps mixed arguments in authored positions; the control
    // mission projection separately holds numeric and string observations.
    AiRosterCall: { name: 'str', args: 'list:union:AiClassScalar', faction: 'f64|null', mask: 'f64|null' },
    // The page needs team metadata even when it owns no bot controllers.
    // Use the native mission parser without constructing a second roster.
    AiMissionTeamsInput: { $layout: true, calls: 'list:rec:AiRosterCall', names: 'list:str' },
    AiMissionTeam: { relations: 'f64x8', faction: 'f64', capture: 'f64' },
    AiMissionTeams: { $layout: true, teams: 'list:rec:AiMissionTeam', names: 'list:f64' },
    AiRosterSideNames: { first: 'list:str', last: 'list:str' },
    AiRosterHuman: { team: 'f64', id: 'f64|null', name: 'str|null' },
    AiRosterSetup: { $layout: true, calls: 'list:rec:AiRosterCall', names: 'list:rec:AiRosterSideNames', local: 'str|null', humans: 'list:rec:AiRosterHuman',
      unitScale: 'f64', heroes: 'bool', demo: 'bool', capacity: 'f64', prepared: 'list:str', pads: 'list:rec:F4PadSetup', heroNames: 'list:rec:F4HeroSetup' },
    AiCharacterBirth: { $layout: true, slot: 'u32', generation: 'u32' },
    AiWeaponData: { json: 'list:u8' },
    // Authored capture geometry uses arrays, rather than Three vectors.
    // A vector writer would turn their coordinates into NaNs at load.
    AiCapture: { $layout: true, center: 'f64x3', half: 'f64x3' },
    AiRawCapture: { $layout: true, center: 'f64x3|null', matrix: 'list:f64|null', size: 'f64x3|null', half: 'f64x3|null' },
    AiItemDefinition: { def: 'def:item', base: 'str|undefined', className: 'str|undefined',
      serviceHealth: 'list:union:AiClassScalar|null', serviceAmmo: 'list:union:AiClassScalar|null',
      health: 'list:union:AiClassScalar', ammo: 'list:union:AiClassScalar', bare: 'list:union:AiClassScalar' },
    AiPostDefinition: { policy: 'rec:AiPostPolicy', name: 'str|undefined', order: 'f64|null', valueBleed: 'f64', spawnPresent: 'bool|null', allyPresent: 'bool|null', allyCount: 'f64', capture: 'rec:AiCapture|null', radius: 'f64', yaw: 'f64', rawCapture: 'rec:AiRawCapture|null', spawnPath: 'bool', spawnPoints: 'f64', allyPath: 'bool' },
    // Registry identity and authored policy enter at construction. Mutable
    // holder, crew, health and life fields stay in the canonical owners.
    AiSeatDefinition: { policy: 'rec:AiUnitClass', kind: 'f64', weapons: 'u32', yaw: 'str|undefined', pitch: 'str|undefined' },
    AiHullDefinition: { hull: 'ref:vehicle', member: 'union:Member|null', kind: 'f64', category: 'f64', policy: 'rec:AiUnitClass', traversalSize: 'union:AiClassScalar', maxSpeed: 'f64', forwardSpeed: 'f64', slotPost: 'bool', decayTime: 'f64|null', seats: 'list:rec:AiSeatDefinition' },
    AiPropDefinition: { target: 'union:RepairTarget', owner: 'rec:F4Handle', member: 'union:Member|null', policy: 'rec:AiUnitClass', targetPolicy: 'rec:AiUnitClass|null', team: 'f64', armed: 'bool', commandPost: 'bool', post: 'union:SpawnPost|null', yaw: 'str|undefined', pitch: 'str|undefined' },
    AiWorldContent: { $layout: true, classes: 'list:rec:AiUnitClass', weapons: 'list:def:weapon', weaponData: 'list:rec:AiWeaponData', items: 'list:rec:AiItemDefinition', posts: 'list:rec:AiPostDefinition', hulls: 'list:rec:AiHullDefinition', props: 'list:rec:AiPropDefinition' },
    AiPlayerCommand: { $layout: true, character: 'rec:F4Handle', kind: 'f64', time: 'f64' },
    AiControlBirth: { slot: 'u32', generation: 'u32' },
    AiControlView: { $layout: true, control: 'ref:seatUnit', lease: 'ref:seatLease', controller: 'ref:controller|null' },
    AiControllerBirth: { slot: 'u32', generation: 'u32', index: 'u32' },
    AiNavPools: { paths: 'f64', pathNodes: 'f64', requests: 'f64', searchNodes: 'f64|null', heap: 'f64|null' },
    AiNavPost: { post: 'union:SpawnPost', name: 'str', position: 'vec3' },
    AiNavSetup: { json: 'list:u8', bin: 'list:u8', posts: 'list:rec:AiNavPost', pools: 'rec:AiNavPools', hintCapacity: 'f64|null' },
    // Immutable class facts cross once, with scalar presence preserved.
    AiUnitClass: {
      def: 'def:unitClass', name: 'str', label: 'str|undefined', noEnterVehicles: 'union:AiClassScalar', bounds: 'f64x6|undefined',
      role: 'union:AiClassScalar', aiRole: 'union:AiClassScalar', unitType: 'union:AiClassScalar',
      sizeType: 'union:AiClassScalar', healthType: 'union:AiClassScalar', base: 'union:AiClassScalar',
      maxSpeed: 'union:AiClassScalar', maxStrafeSpeed: 'union:AiClassScalar', strafeSpeed: 'union:AiClassScalar',
      maxTurnSpeed: 'union:AiClassScalar', maxPitchSpeed: 'union:AiClassScalar',
      forwardSpeed: 'union:AiClassScalar', turnRate: 'union:AiClassScalar', pitchRate: 'union:AiClassScalar',
      movingTurnOnly: 'union:AiClassScalar', minSpeed: 'union:AiClassScalar', maxYawSpeed: 'union:AiClassScalar', vehicleType: 'union:AiClassScalar',
      landingTime: 'union:AiClassScalar', landingSpeed: 'union:AiClassScalar', landedHeight: 'union:AiClassScalar',
      maxHealth: 'union:AiClassScalar', jetJump: 'union:AiClassScalar',
      jetFuelMinBorder: 'union:AiClassScalar', jetFuelRechargeRate: 'union:AiClassScalar',
    },
    AiPostPolicy: { post: 'union:SpawnPost', attack: 'f64x6', defend: 'f64x6', bans: 'f64', spawn: 'bool', ally: 'bool', classSpawnPoints: 'f64' },
    AiLifeHandle: { kind: 'u32', slot: 'u32', generation: 'u32', life: 'f64' },
    // Each pointer names retained storage reserved at load. Counts are
    // validated before decoding, without allocating generated list records.
    AiPhaseHeader: { $layout: true, facts: 'rec:AiPhaseFacts', time: 'f64', paused: 'bool', networkSecondary: 'bool', unitPhaseFollows: 'bool', cameras: 'u32', events: 'u32', sequence: 'f64' },
    AiPhaseBoundary: { $layout: true, header: 'u32', cameras: 'u32', cameraCapacity: 'u32', journal: 'u32', journalCapacity: 'u32', counters: 'u32', controls: 'u32', controlCount: 'u32', controlVersion: 'u32', controlAck: 'u32' },
    AiPhaseCamera: { $layout: true, elements: 'mat4', fov: 'f64', aspect: 'f64', near: 'f64', far: 'f64', zoom: 'f64' },
    // The human reticle is a boundary input, qualified independently from
    // its driver's pooled hull life and the selected registry member's life.
    AiPhaseReticle: { pilot: 'ref:vehicle|null', pilotLife: 'f64', target: 'union:Member|null', targetLife: 'f64' },
    AiPhaseFacts: { dt: 'f64', tick: 'u32', networkViews: 'bool', lodDisabled: 'bool', reticle: 'rec:AiPhaseReticle' },
    // Journal input contains event facts and qualified identities only.
    // Other event kinds consume sequence positions without copying payloads.
    AiJournalRef: { member: 'union:Member|null', generation: 'f64|null', direct: 'bool' },
    AiJournalEvent: { $layout: true, sequence: 'f64', kind: 'enum:AiJournalKind', source: 'rec:AiJournalRef', target: 'rec:AiJournalRef', attacker: 'rec:AiJournalRef', attackerCharacter: 'ref:character|null', amount: 'f64|null', generic: 'f64|null', typed: 'f64|null', known: 'bool', point: 'vec3|null' },
    AiJournalInput: { sequence: 'f64', records: 'list:rec:AiJournalEvent' },
    AiVoiceNotice: { actor: 'union:AiActor', generation: 'f64', time: 'f64', team: 'f64', position: 'vec3', human: 'bool', event: 'str', property: 'str', command: 'f64|null', context: 'f64|null', positional: 'bool|null', notice: 'bool|null' },
    AiItemOwnerNotice: { actor: 'union:AiActor' },
    AiNotice: { hook: 'rec:F4AIHook' },
    AiPhaseCounters: { $layout: true,
      liveControllers: 'u32', targets: 'u32', combatTargets: 'u32', fireRequests: 'u32', candidates: 'u32', trackers: 'u32', scanned: 'f64', considered: 'f64', serviced: 'f64', skipped: 'f64', schedulerTicks: 'f64', stepDt: 'f64', time: 'f64',
      phases: 'u32', dangerPhases: 'u32', commands: 'u32', inputWords: 'u32', outputWords: 'u32',
      events: 'u32', allocations: 'u32', reallocations: 'u32', allocationBytes: 'u32', staleCommands: 'u32', drawsA: 'u32', drawsB: 'u32', drawsC: 'u32',
    },
  },
  events: { aiControlBirth: 'AiControlBirth', aiControllerBirth: 'AiControllerBirth', aiNotice: 'AiNotice', aiVoice: 'AiVoiceNotice', aiItemOwner: 'AiItemOwnerNotice' },
};
