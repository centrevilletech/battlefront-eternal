// The battle (plan P12, flip F4; rust/sim/src/battle): the battle's rules
// and scores (src/battle.js, src/battle-stats.js) with its Characters, the
// command posts (src/commandposts.js) and the command vehicles' posts
// (src/mobile-posts.js), and the Classic spawn manager
// (src/ai/classic/spawning.js). What these objects hold that is content (a
// mission's team table, a post's rules and regions), the AI's (a post's AI
// record) or a page's (holograms, colours) is read and not mirrored.
export default {
  enums: {
    // A post changing hands (CommandPosts.events: captured, neutralized, or
    // lost with its building).
    PostChange: ['captured', 'neutralized', 'lost'],
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
    // A team as the mission's table gives it (map.json mission.teams).
    TeamDesc: { units: 'f64|undefined', reinforcements: 'f64|undefined', bleed: 'list:f64x2|undefined' },
    // The kills ring the sprees count in (battle-stats.js BattleStats.ring).
    KillRing: { killers: 'list:(ref:character|null)', times: 'Float64Array(100)', slots: 'Int32Array(100)', head: 'f64', tail: 'f64' },
    BattleStats: { time: 'f64', ring: 'rec:KillRing' },
    // A time table's entry (a class, weapon or vehicle and its seconds).
    TimeEntry: { key: 'str', label: 'str|null', time: 'f64' },
    // A Character's tallies for one team slot (battle-stats.js slotRecord).
    StatSlot: {
      teamKills: 'f64', vehiclesDestroyed: 'f64', spree: 'f64', survivalist: 'f64', lifeKills: 'f64', lifeKillsBest: 'f64', headshots: 'f64',
      deaths: 'f64', life: 'f64', lifeBest: 'f64', camp: 'f64', campBest: 'f64', escapes: 'f64',
      victims: 'map:ref:character:f64', classes: 'list:rec:TimeEntry', weapons: 'list:rec:TimeEntry', vehicles: 'list:rec:TimeEntry',
    },
    // Where a Character camps (a plain { x, y, z }).
    Anchor: { x: 'f64', y: 'f64', z: 'f64' },
    // A Character's end-of-match record (battle-stats.js recordOf).
    Record: { slots: 'list:rec:StatSlot', anchor: 'rec:Anchor|null', health: 'f64', low: 'bool' },
    // A Character's next life (the spawn manager's; a network player's from
    // the roster holds only its timer and whether it is ready).
    Spawn: { timer: 'f64', wave: 'f64|undefined', ordinal: 'f64|undefined', post: 'union:PostRef', ready: 'bool' },
    // A region as a world file gives it, and as buildRegion builds it
    // (src/regions.js).
    RegionDesc: { type: 'str', matrix: 'list:f64|undefined', center: 'list:f64|undefined', size: 'list:f64' },
    Built: { type: 'str', half: 'f64x3', radius: 'f64', center: 'vec3', inverse: 'mat4' },
    // The map's command post rules.
    PostRules: { neutralizeTime: 'f64', captureTime: 'f64' },
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
    battle: {
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
        rules: { type: 'rec:PostRules', mirror: false },
        position: 'vec3',
        capture: { type: 'rec:Built|null', mirror: false },
        valueBleed: { type: 'f64|undefined', js: 'desc.valueBleed', mirror: false },
        killRegion: { type: 'rec:RegionDesc|undefined', js: 'desc.killRegion', mirror: false },
        neutralize: 'f64',
        progress: 'f64',
        toward: 'f64',
        counts: 'map:num:f64',
        contested: 'bool',
        building: 'ref:building|null',
        active: 'bool',
        radius: 'f64|undefined',
        ai: { type: 'rec:PostAi|undefined', mirror: false },
      },
      skip: {
        desc: 'the world file\'s (its valueBleed and killRegion are read)',
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
