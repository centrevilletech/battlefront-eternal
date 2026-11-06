// The battle (plan P12, flip F4; rust/sim/src/battle): the battle's rules
// and scores (src/battle.js, src/battle-stats.js) with its Characters, the
// command posts (src/commandposts.js) and the command vehicles' posts
// (src/mobile-posts.js), and the Classic spawn manager
// (src/ai/classic/spawning.js). What these objects hold that is content (a
// mission's team table, a post's rules and regions), the AI's (a post's AI
// record) or a page's (holograms, colours) is read and not mirrored.
const humanFields = {
  auto: 'rec:HumanAuto|null', className: 'str', post: 'str|null',
  phase: 'enum:HumanPhase', canSwitchTeams: 'bool', spawnedOnce: 'bool',
  deadTime: 'f64', pending: 'rec:HumanPick|null', ended: 'bool',
};

export default {
  enums: {
    // A post changing hands (CommandPosts.events: captured, neutralized, or
    // lost with its building).
    PostChange: ['captured', 'neutralized', 'lost'],
    // A region's TYPE (the world files name no other).
    RegionType: ['sphere', 'box', 'cylinder'],
    HumanPhase: ['unspawned', 'alive', 'dying', 'awaiting'],
    HsOperation: ['update', 'apply', 'phase', 'realize', 'died', 'spawned', 'teamChanged', 'state', 'authority'],
    HsQueryOp: ['team', 'dead', 'over', 'roster', 'classic', 'characterSpawn', 'canSpawn'],
    HsActionOp: ['setTeam', 'spawn', 'ready', 'kill', 'cancel', 'rejected'],
    HsEventKind: ['awaitingSpawn', 'spawned', 'teamChanged', 'playerDied'],
    HsCommandKind: ['team', 'spawn', 'suicide', 'cancel'],
    BrOperation: ['update', 'spend', 'credit', 'headshot', 'vehicleDestroyed', 'point', 'postDestroyed', 'score', 'updateTeam', 'updateLocals'],
    BrQueryOp: ['relation', 'unitAlive', 'observe', 'living', 'owned', 'statsActive'],
    PsOperation: ['update', 'all', 'owned', 'at', 'mobileEvents', 'refreshAll', 'refresh', 'postUpdate', 'destroy', 'contains', 'remaining'],
    PsQueryOp: ['captureTeam', 'buildingAlive', 'pad', 'vehiclePosition', 'classSpawnWorld'],
    PsMobileEventKind: ['lost', 'owner'],
    SpOperation: ['update', 'clock', 'updateCharacters', 'updateCharacter', 'awaitingHuman', 'setClass', 'activate', 'pickClass', 'prepare', 'choosePost', 'reserve', 'release', 'changePost', 'tickets', 'spendTicket', 'waiting', 'spawn', 'onDeath', 'onHumanReady', 'switchTeam', 'padQuotas', 'queueOf'],
    SpQueryOp: ['draw', 'health', 'heroName', 'spawnGoal'],
    SpActionOp: ['spawnUnit', 'forceKill', 'requestObjective', 'republish'],
  },
  unions: {
    // A post the AI sends a Character to, or one he spawns at: a placed
    // post or a command vehicle's.
    PostRef: ['null', 'undefined', 'ref:post', 'ref:mobilePost'],
    // A post as a key of the spawn manager's queues.
    SpawnPost: ['ref:post', 'ref:mobilePost'],
    // A player's id: a number, a network player's string, none for a bot
    // the manager did not make.
    PlayerId: ['undefined', 'f64', 'str'],
  },
  records: {
    HumanPick: { className: 'str', post: 'str|null' },
    HumanAuto: { team: 'f64', className: 'str', post: 'str|null' },
    // The live bridge hands owned values across callbacks. Neither the
    // store nor the string table stays borrowed while JavaScript runs.
    HsOwned: humanFields,
    HsState: { phase: 'enum:HumanPhase', team: 'f64', className: 'str', classIndex: 'f64', post: 'str|null', canSwitchTeams: 'bool', over: 'bool' },
    HsEvent: { kind: 'enum:HsEventKind', state: 'rec:HsState|null', team: 'f64', previous: 'f64', className: 'str|null', post: 'str|null' },
    HsCommand: { $layout: true, kind: 'enum:HsCommandKind', team: 'f64', className: 'str|null', post: 'str|null' },
    HsAuthority: { phase: 'enum:HumanPhase', className: 'str|null', post: 'str|null', spawnedOnce: 'bool', ended: 'bool', deadTime: 'f64', pending: 'rec:HumanPick|null', auto: 'rec:HumanAuto|null' },
    HsInput: { op: 'enum:HsOperation', dt: 'f64', commands: 'list:rec:HsCommand', className: 'str|null', post: 'str|null', team: 'f64', previous: 'f64', authority: 'rec:HsAuthority|null' },
    HsResult: { events: 'list:rec:HsEvent', view: 'rec:HsState|null' },
    HsQueryArgs: { op: 'enum:HsQueryOp', team: 'f64', className: 'str|null' },
    HsQueryResult: { number: 'f64', flag: 'bool', roster: 'list:str', present: 'bool', hasUnit: 'bool', timer: 'f64' },
    HsActionArgs: { op: 'enum:HsActionOp', state: 'rec:HsOwned', events: 'list:rec:HsEvent', team: 'f64', className: 'str|null', post: 'str|null' },
    HsActionResult: { state: 'rec:HsOwned', accepted: 'bool' },
    BrInput: { op: 'enum:BrOperation', dt: 'f64', team: 'f64', count: 'f64', victim: 'ref:character|null', killer: 'ref:character|null', relation: 'f64' },
    BrNumber: { value: 'f64' },
    BrFavourite: { key: 'str|null', label: 'str|null' },
    BrSeen: { position: 'f64x3|null', health: 'f64|null', weapon: 'rec:BrFavourite|null', vehicle: 'rec:BrFavourite|null', classLabel: 'str|null' },
    BrQueryArgs: { op: 'enum:BrQueryOp', a: 'f64', b: 'f64', unit: 'ref:unit|null', character: 'ref:character|null' },
    BrQueryResult: { number: 'f64', optionalNumber: 'f64|null', flag: 'bool', values: 'list:(f64|undefined)', seen: 'rec:BrSeen|null' },
    PsCapturer: { team: 'f64', position: 'f64x3' },
    PsInput: { op: 'enum:PsOperation', dt: 'f64', team: 'f64', post: 'ref:post|null', mobile: 'ref:mobilePost|null', position: 'f64x3', soldiers: 'list:rec:PsCapturer' },
    PsChange: { kind: 'enum:PostChange', from: 'f64|undefined', by: 'f64|undefined', destroyed: 'bool|undefined' },
    PsMobileEvent: { kind: 'enum:PsMobileEventKind', post: 'ref:mobilePost', vehicle: 'ref:vehicle|null', from: 'f64', to: 'f64|undefined' },
    PsResult: { list: 'list:union:SpawnPost', post: 'ref:post|null', mobileEvents: 'list:rec:PsMobileEvent', change: 'rec:PsChange|null', number: 'f64', flag: 'bool' },
    PsClass: { atk: 'list:f64|null', def: 'list:f64|null', valueBleed: 'f64|null', points: 'list:f64x4' },
    PsHull: { vehicle: 'ref:vehicle', alive: 'bool', command: 'bool', generation: 'f64', team: 'f64', controllingTeam: 'f64', seated: 'bool', yaw: 'f64', name: 'str', label: 'str', radius: 'f64', speed: 'f64', valueBleed: 'f64', record: 'rec:PsClass|null' },
    PsPad: { variants: 'list:rec:PsHull', vehicle: 'rec:PsHull|null' },
    PsQueryArgs: { op: 'enum:PsQueryOp', number: 'f64', building: 'ref:building|null', vehicle: 'ref:vehicle|null', post: 'ref:mobilePost|null', points: 'list:f64x4' },
    PsQueryResult: { number: 'f64', flag: 'bool', position: 'f64x3', pad: 'rec:PsPad|null', points: 'list:f64x4' },
    SpInput: { op: 'enum:SpOperation', dt: 'f64', team: 'f64', character: 'ref:character|null', classIndex: 'f64|null', className: 'str|null', post: 'union:PostRef', hasPost: 'bool', human: 'bool', own: 'bool' },
    SpResult: { number: 'f64', flag: 'bool', post: 'union:PostRef', unit: 'ref:unit|null' },
    SpQueryArgs: { op: 'enum:SpQueryOp', unit: 'ref:unit|null', className: 'str|null', character: 'ref:character|null' },
    SpQueryResult: { number: 'f64', name: 'str|null', goal: 'f64x3|null' },
    SpActionArgs: { op: 'enum:SpActionOp', character: 'ref:character', unit: 'ref:unit|null', className: 'str|null', post: 'union:PostRef', position: 'f64x3', yaw: 'f64|undefined' },
    SpActionResult: { unit: 'ref:unit|null' },
    // A team as the mission's table gives it (map.json mission.teams).
    TeamDesc: { units: 'f64|undefined', reinforcements: 'f64|undefined', bleed: 'list:f64x2|undefined' },
    // The kills ring the sprees count in (battle-stats.js BattleStats.ring).
    KillRing: { $delta: ['killers', 'times', 'slots', 'head', 'tail'], killers: 'list:(ref:character|null)', times: 'Float64Array(100)', slots: 'Int32Array(100)', head: 'f64', tail: 'f64' },
    BattleStats: { $delta: ['time', 'ring'], time: 'f64', ring: 'rec:KillRing' },
    // A time table's entry (a class, weapon or vehicle and its seconds).
    TimeEntry: { $observe: ['key','label','time'], $delta: ['key', 'label', 'time'], key: 'str', label: 'str|null', time: 'f64' },
    // A Character's tallies for one team slot (battle-stats.js slotRecord).
    StatSlot: { $observe: ['teamKills','vehiclesDestroyed','spree','survivalist','lifeKills','lifeKillsBest','headshots','deaths','life','lifeBest','camp','campBest','escapes'], $delta: ['teamKills', 'vehiclesDestroyed', 'spree', 'survivalist', 'lifeKills', 'lifeKillsBest', 'headshots', 'deaths', 'life', 'lifeBest', 'camp', 'campBest', 'escapes', 'victims', 'classes', 'weapons', 'vehicles'],
      teamKills: 'f64', vehiclesDestroyed: 'f64', spree: 'f64', survivalist: 'f64', lifeKills: 'f64', lifeKillsBest: 'f64', headshots: 'f64',
      deaths: 'f64', life: 'f64', lifeBest: 'f64', camp: 'f64', campBest: 'f64', escapes: 'f64',
      victims: 'map:ref:character:f64', classes: 'list:rec:TimeEntry', weapons: 'list:rec:TimeEntry', vehicles: 'list:rec:TimeEntry',
    },
    // Where a Character camps (a plain { x, y, z }).
    Anchor: { $observe: ['x','y','z'], $delta: ['x', 'y', 'z'], x: 'f64', y: 'f64', z: 'f64' },
    // A Character's end-of-match record (battle-stats.js recordOf).
    Record: { $observe: ['health','low'], $delta: ['slots', 'anchor', 'health', 'low'], slots: 'list:rec:StatSlot', anchor: 'rec:Anchor|null', health: 'f64', low: 'bool' },
    // A Character's next life (the spawn manager's; a network player's from
    // the roster holds only its timer and whether it is ready).
    Spawn: { $delta: ['timer', 'wave', 'ordinal', 'post', 'ready'], timer: 'f64', wave: 'f64|undefined', ordinal: 'f64|undefined', post: 'union:PostRef', ready: 'bool' },
    // A region as a world file gives it, and as buildRegion builds it
    // (src/regions.js).
    RegionDesc: { type: 'enum:RegionType', matrix: 'list:f64|undefined', center: 'list:f64|undefined', size: 'list:f64' },
    Built: { type: 'enum:RegionType', half: 'f64x3', radius: 'f64', center: 'vec3', inverse: 'mat4' },
    // A spawn path (its points x, y, z and yaw) and what the spawn manager
    // reads of a placed post's AI record (src/ai/classic/goals.js postAI).
    SpawnPath: { present: 'bool', points: 'list:f64x4' },
    PostAi: { spawnPath: 'rec:SpawnPath|null', allyPath: 'rec:SpawnPath|null', allyCount: 'f64', classSpawnPoints: 'f64', classSpawnWorld: 'list:f64x4', yaw: 'f64' },
    // A post changing hands, as CommandPosts.update queues it.
    PostEvent: { post: 'ref:post', kind: 'enum:PostChange', from: 'f64|undefined', by: 'f64|undefined', destroyed: 'bool|undefined' },
    // A command vehicle's post: its class's post data, and what it last
    // reported to the battle.
    MobileDesc: { name: 'str', displayName: 'str', valueBleed: 'f64', mobile: 'bool' },
    MobileAi: { atk: 'list:f64', def: 'list:f64', valueBleed: 'f64', points: 'list:f64x4', classSpawnPoints: 'f64' },
    Reported: { active: 'bool', team: 'f64', vehicle: 'ref:vehicle|null', life: 'f64|null' },
    // A team of the spawn manager (spawning.js makeTeams).
    SpawnClass: { name: 'str', target: 'f64', hero: 'bool' },
    SpawnTeam: {
      index: 'f64', name: 'str|null', faction: 'f64', unitCount: 'f64', tickets: 'f64', classes: 'list:rec:SpawnClass', heroCount: 'f64',
      heroClass: 'str|null', bonus: 'f64', aggressiveness: 'f64', relations: 'Int8Array(8)', characters: 'list:character', living: 'f64', capacity: 'f64',
    },
  },
  kinds: {
    humanSpawn: {
      singleton: true,
      class: 'HumanSpawn',
      fields: humanFields,
      skip: {
        world: 'the world callbacks (battle/human.rs World)',
        listeners: 'the screen and HUD observers',
        queue: 'events dispatched after the tick (battle/human.rs Event)',
      },
    },
    battle: {
      delta: true,
      singleton: true,
      class: 'Battle',
      fields: {
        own: 'f64',
        foe: 'f64',
        teams: { type: 'obj:rec:TeamDesc', mirror: false },
        reinforcements: 'obj:f64',
        start: 'obj:f64',
        teamIds: 'list:f64',
        noPost: 'obj:f64',
        bleed: 'obj:f64',
        victoryClock: 'f64|null|undefined',
        countdown: 'f64|null|undefined',
        losing: 'bool',
        over: 'bool',
        loser: 'f64|null',
        overTime: 'f64',
        gameOver: 'bool',
        finished: 'bool',
        stats: 'rec:BattleStats',
        bleedRules: 'obj:list:f64x2',
        bleedClock: 'obj:f64',
        characters: 'list:character',
      },
      skip: {
        posts: 'the command posts (their own kind)',
        names: 'the HUD\'s team names, from the mission',
        onOver: 'the world\'s handler',
        relation: 'the registry\'s relation (read through the world)',
        living: 'the spawn manager\'s count (read through the world)',
        statsActive: 'the world\'s rule for whose tallies run (read through the world)',
        local: 'a page\'s own player',
      },
    },
    character: {
      delta: true,
      defer: ['record'],
      home: 'battle.characters',
      class: 'Character',
      fields: {
        index: 'f64',
        team: 'f64',
        human: 'bool',
        name: 'str|null',
        className: 'str|null',
        unit: 'ref:unit|null',
        controller: 'ref:controller|null',
        objective: 'union:PostRef',
        kills: 'f64',
        teamKills: 'f64',
        deaths: 'f64',
        captures: 'f64',
        outOfBounds: 'f64',
        record: 'rec:Record|null|undefined',
        // The spawn manager's (spawning.js insert) and the roster's.
        slot: 'f64|undefined',
        managerIndex: 'f64|undefined',
        playerId: 'union:PlayerId',
        classIndex: 'f64|null|undefined',
        hero: 'bool|undefined',
        spawn: 'rec:Spawn|undefined',
        networkDormant: 'bool|undefined',
        rosterSlot: 'f64|undefined',
        diedAt: 'f64|undefined',
      },
    },
    commandPosts: {
      singleton: true,
      class: 'CommandPosts',
      fields: {
        posts: 'list:post',
        time: 'f64',
        events: 'list:rec:PostEvent',
      },
      skip: {
        playerTeam: 'the viewer\'s team, for colours',
        teams: 'the registry\'s team table (read through the world)',
        teamNames: 'the holograms\' logos, from the mission',
        holograms: 'drawn by a page',
        textures: 'drawn by a page',
        scratch: 'a scratch vector',
        mobile: 'the command vehicles\' posts (their own kind)',
        listed: 'the placed posts, then the command vehicles\'',
      },
    },
    post: {
      home: 'commandPosts.posts',
      class: 'Post',
      fields: {
        name: 'str',
        label: 'str|undefined',
        team: 'f64',
        // The map's command post rules (one object every post shares: read
        // as numbers, so a capture holds no alias the store cannot).
        neutralizeTime: { type: 'f64', js: 'rules.neutralizeTime', mirror: false },
        captureTime: { type: 'f64', js: 'rules.captureTime', mirror: false },
        position: 'vec3',
        capture: { type: 'rec:Built|null', mirror: false },
        valueBleed: { type: 'f64|undefined', js: 'desc.valueBleed', mirror: false },
        killRegion: { type: 'rec:RegionDesc|null|undefined', js: 'desc.killRegion', mirror: false },
        neutralize: 'f64',
        progress: 'f64',
        toward: 'f64',
        counts: 'map:num:f64',
        contested: 'bool',
        building: 'ref:building|null',
        active: 'bool',
        radius: 'f64|undefined',
        ai: 'rec:PostAi|undefined',
      },
      skip: {
        desc: 'the world file\'s (its valueBleed and killRegion are read)',
        rules: 'the map\'s command post rules (its times are read)',
        matrix: 'where it stands (its position is read)',
        anchor: 'the hologram\'s',
        mapped: 'the maps\'',
      },
    },
    mobilePosts: {
      singleton: true,
      class: 'MobilePosts',
      fields: { posts: 'list:mobilePost' },
      skip: { system: 'the vehicles (read through the world)' },
    },
    mobilePost: {
      home: 'mobilePosts.posts',
      class: 'MobilePost',
      fields: {
        index: 'f64',
        name: 'str',
        label: 'str',
        team: 'f64',
        active: 'bool',
        mobile: 'bool',
        vehicle: 'ref:vehicle|null',
        life: 'f64|null',
        yaw: 'f64',
        radius: 'f64',
        speed: 'f64',
        lastPosition: 'vec3',
        desc: 'rec:MobileDesc',
        ai: 'rec:MobileAi',
        reported: 'rec:Reported',
      },
      skip: {
        system: 'the vehicles (read through the world)',
        slot: 'its pad (its hulls read through the world)',
        classes: 'the command classes\' post data (read through the world)',
      },
    },
    spawning: {
      singleton: true,
      class: 'ClassicSpawning',
      fields: {
        heroes: 'bool',
        demo: 'bool',
        teams: 'list:rec:SpawnTeam',
        A: 'f64',
        B: 'f64',
        Q: 'f64',
        waveCountdown: 'f64',
        wave: 'f64',
        stagger: 'f64',
        released: 'f64',
        localsCap: 'f64',
        characters: 'list:character',
        queues: 'map:ref:SpawnPost:f64x4',
        started: 'bool',
      },
      skip: {
        postList: 'the command posts\' all()',
        random: 'the AI\'s generator A (its draws are the world\'s)',
        humanSpecs: 'the players at the match start',
        names: 'the bot names\' lists, from the content',
        heroName: 'a lookup in the content',
        battle: 'the battle (its own kind)',
        spawnUnit: 'the world\'s',
        makeCharacter: 'the world\'s',
        hasClass: 'a lookup in the content',
        forceKill: 'the world\'s',
        health: 'the world\'s',
        relationOverride: 'a test\'s relation table (null in a battle)',
        objectives: 'the AI\'s objectives',
      },
    },
  },

};
