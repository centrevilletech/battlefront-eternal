// Classic population and spawning, as the 2004 game has it
// (docs/ai-population.md).
//
// Each team has a fixed roster of Characters, SetUnitCount of them, the local
// human included: Theed's 22 a side is the player and 21 bots on his team.
// A Character outlives its bodies. Each life it picks a class by the team's
// roster deficit (the mission's AddUnitClass targets minus the classes its
// Characters currently hold, the requester's own included), a post it can
// spawn at, and a place in that post's queue; then it waits for its wave.
//
// The spawn manager keeps two clocks: a wave every A seconds and a release
// every B seconds within the wave (SetSpawnDelay(A, B); Q = trunc(A / B)).
// Each update steps each clock at most once: no catching up after a long
// frame. A bot scheduled for wave S with queue ordinal J waits while
// (S - W) * Q - R + J >= 0, W being the wave number and R the releases so far
// in this wave. Offline, the human skips that wait.
//
// A post's queue is four counters (AI or human, waiting at its own team's
// post or an ally's). Joining returns the count before joining, plus, for a
// bot, the humans waiting in the same category: bots queue behind humans.
// Leaving decrements; nothing renumbers the ordinals others already hold.
//
// Every random draw goes through the shared ClassicRandom in the order the
// 2004 game makes it: quota padding and each Character's objective, class
// and post at match start, the spawn point and class at each respawn. Bot
// names come from each team's own generator (src/bot-names.js), so they
// take nothing from it.

import { ClassicObjectives, activePosts, humanSpawnPost, ownerOf, postAI, retainSpawnPost, factionOf } from "./goals.js";
import { botNames, humanName } from "../../bot-names.js";

const f32 = Math.fround;

export const TEAM_COUNT = 8;
export const TEAM_CAPACITY = 200; // Characters per team, offline
export const CLASS_CAPACITY = 12; // classes per team
export const RESPAWN_DELAY = 4; // seconds from death to the next preparation
export const DEFAULT_SPAWN_DELAY = [15, 0.5]; // without SetSpawnDelay
export const LOCALS_TICKETS = 0x7fffffff; // teams 3-7 without SetReinforcementCount
export const HERO_BONUS = 0x08;
export const TRAINING_BONUS = 0x80;
// The hero a team fields when its mission named none, by faction.
export const DEFAULT_HEROES = {
  1: "all_inf_lukeskywalker",
  2: "imp_inf_darthvader",
  3: "rep_inf_macewindu",
  4: "cis_inf_countdooku",
};
// ActivateBonus names the port can read (ai-mission.json carries the rest as
// raw names; their masks are decoded by hash in tools/zero_ai_mission.py).
const BONUS_NAMES = { sabotage: 0x20, hero: HERO_BONUS, training: TRAINING_BONUS };

export function bonusMask(name) {
  return BONUS_NAMES[String(name || "").toLowerCase()] || 0;
}

function luaInt(value) {
  return Math.trunc(f32(Number(value) || 0));
}

function argsOf(call) {
  return (Array.isArray(call) ? call[1] : call.args) || [];
}

function nameOf(call) {
  return Array.isArray(call) ? call[0] : call.name;
}

// The eight teams as the manager constructs them: team 0 holds nobody; each
// team is its own friend and every other team's enemy, neutral to team 0;
// teams 3-7 have effectively unlimited reinforcements.
export function makeTeams(capacity = TEAM_CAPACITY) {
  return Array.from({ length: TEAM_COUNT }, (_, index) => {
    const relations = new Int8Array(TEAM_COUNT);
    if (index) for (let other = 1; other < TEAM_COUNT; other++) relations[other] = other === index ? 1 : -1;
    return {
      index,
      name: null,
      faction: 0,
      unitCount: 0,
      tickets: index <= 2 ? 0 : LOCALS_TICKETS,
      classes: [], // [{ name, target, hero }]
      heroCount: 0,
      heroClass: null, // SetHeroClass registration
      bonus: 0,
      aggressiveness: 1, // stored, never read
      relations,
      characters: [],
      living: 0, // living ordinary (non-hero) Characters, the human included
      capacity: index ? capacity : 0,
    };
  });
}

// Applies a mission's calls in source order to the teams; returns the spawn
// delay the last SetSpawnDelay left. Difficulty setters are ClassicDifficulty's.
export function applyMission(teams, calls, { hasClass = () => true } = {}) {
  let spawnDelay = DEFAULT_SPAWN_DELAY;
  const team = (n) => {
    const t = luaInt(n);
    return t >= 0 && t < TEAM_COUNT ? teams[t] : null;
  };
  for (const call of calls || []) {
    const args = argsOf(call);
    const t = team(args[0]);
    switch (nameOf(call)) {
      case "SetTeamName":
        if (t) {
          t.name = typeof args[1] === "string" ? args[1] : null;
          t.faction = call.faction ?? factionOf(t.name);
        }
        break;
      case "AddUnitClass":
        if (t && typeof args[1] === "string" && hasClass(args[1]) && t.classes.length < CLASS_CAPACITY) {
          t.classes.push({ name: args[1], target: typeof args[2] === "number" ? luaInt(args[2]) : 0, hero: false });
        }
        break;
      case "SetUnitCount":
        if (t) t.unitCount = luaInt(args[1]);
        break;
      case "SetReinforcementCount":
        if (t) t.tickets = luaInt(args[1]);
        break;
      case "SetHeroClass":
        if (t && typeof args[1] === "string") t.heroClass = args[1];
        break;
      case "ActivateBonus":
        if (t) t.bonus |= call.mask ?? bonusMask(args[1]);
        break;
      case "SetTeamAggressiveness":
        if (t) t.aggressiveness = f32(Number(args[1]));
        break;
      case "SetTeamAsFriend":
      case "SetTeamAsEnemy":
      case "SetTeamAsNeutral": {
        const other = luaInt(args[1]);
        if (t && other >= 0 && other < TEAM_COUNT) {
          t.relations[other] = { SetTeamAsFriend: 1, SetTeamAsEnemy: -1, SetTeamAsNeutral: 0 }[nameOf(call)];
        }
        break;
      }
      case "SetSpawnDelay":
        if (args.length >= 2) spawnDelay = [f32(Number(args[0])), f32(Number(args[1]))];
        break;
      default:
        break;
    }
  }
  return spawnDelay;
}

// Whom a team's soldiers count for in a command post's capture region, as
// the team setup after the mission script decides it: an army counts for
// itself; a team above 2 friendly to exactly one army counts for that army
// (Kashyyyk's Wookiees help the Republic take a post), friendly to both for
// nobody (Mos Eisley's Jawas), friendly to neither for itself (the Tusken
// Raiders take posts of their own).
export function captureTeamOf(team) {
  if (team.index <= 2) return team.index;
  const first = team.relations[1] > 0, second = team.relations[2] > 0;
  return first ? (second ? 0 : 1) : second ? 2 : team.index;
}

// What the rest of the battle needs of the teams (src/registry.js): each
// team's relations (team a's to team b), faction and capture team.
export function teamTable(teams) {
  return {
    relations: teams.map((team) => Array.from(team.relations)),
    factions: teams.map((team) => team.faction),
    capture: teams.map(captureTeamOf),
  };
}

// The table for a mission's calls, without a roster (a battle the Classic
// AI doesn't run, or a client of one).
export function missionTeams(calls) {
  const teams = makeTeams();
  applyMission(teams, calls);
  return teamTable(teams);
}

// After the mission script: teams 1 and 2 field their hero when heroes are on
// (or the mission's ActivateBonus asked for one). The hero class is appended
// with a target of 1 and the first class's target drops by one.
export function appendHeroes(teams, { heroes = false, hasClass = () => true } = {}) {
  for (const team of [teams[1], teams[2]]) {
    if (!heroes && !(team.bonus & HERO_BONUS)) continue;
    const name = team.heroClass || DEFAULT_HEROES[team.faction];
    if (!name || !hasClass(name) || team.classes.length >= CLASS_CAPACITY) continue;
    team.classes.push({ name, target: 1, hero: true });
    team.heroCount++;
    team.classes[0].target -= 1;
  }
}

// map.json's mission block as calls, for maps without ai-mission.json. It
// lacks relations, bonuses and the original order.
export function callsFromMapMission(mission) {
  const calls = [];
  if (!mission) return calls;
  for (const [team, t] of Object.entries(mission.teams || {})) {
    const n = Number(team);
    if (t.name) calls.push({ name: "SetTeamName", args: [n, t.name] });
    for (const [className, count] of t.classes || []) calls.push({ name: "AddUnitClass", args: [n, className, count] });
    if (t.hero) calls.push({ name: "SetHeroClass", args: [n, t.hero] });
    if (t.units != null) calls.push({ name: "SetUnitCount", args: [n, t.units] });
    if (t.reinforcements != null) calls.push({ name: "SetReinforcementCount", args: [n, t.reinforcements] });
  }
  if (mission.spawnDelay) calls.push({ name: "SetSpawnDelay", args: mission.spawnDelay });
  if (mission.attacker != null) calls.push({ name: "SetAttackingTeam", args: [mission.attacker] });
  return calls;
}

export class ClassicSpawning {
  // calls: the mission's AI calls in source order (ai-mission.json
  //   scripts[mission].calls, or callsFromMapMission(map.mission)).
  // posts: the command posts (CommandPosts.posts or a function returning
  //   them), with AI properties attached (goals.attachPostAI).
  // random: the shared ClassicRandom.
  // humans: [{ team, id, name }] local humans; they join their teams first.
  // heroes: Heroes On/Off. demo: attract mode (no humans, quick first waves).
  // names: ai-mission.json `names` (bot name lists by faction).
  // heroName(className): a hero class's display name, which a Character
  //   taking it is called from then on.
  // battle: reinforcements ({ reinforcements: {team: n}, spend(team) });
  //   without one the teams keep their own counts.
  // spawnUnit(character, className, post, [x, y, z], yaw): a unit or null.
  // makeCharacter(slot, team, human): the Character object (src/battle.js).
  // hasClass(name): whether a class can be spawned (AddUnitClass resolves it).
  // forceKill(character): kill a hero whose team has run short of tickets.
  // objectives: a ClassicObjectives; by default one is made from posts,
  //   random, this roster's relations and factions, and these extras:
  //   crew, vehicleMask, units, travelMask, onPublish (see goals.js).
  // start: build the roster and run the match-start setup now.
  // unitScale: the port's single-player battle size, scaling every team's
  //   SetUnitCount (1, the default, is the mission's own). Classes,
  //   reinforcements and the AI are unchanged.
  constructor({
    unitScale = 1,
    calls = [],
    posts = [],
    random,
    humans = [],
    heroes = false,
    demo = false,
    names = null,
    heroName = null,
    battle = null,
    spawnUnit = null,
    makeCharacter = null,
    hasClass = () => true,
    forceKill = null,
    health = (unit) => unit?.combat?.health ?? 0,
    relation = null,
    capacity = TEAM_CAPACITY,
    objectives,
    crew = null,
    vehicleMask = null,
    units = null,
    travelMask = null,
    onPublish = null,
    start = true,
  }) {
    this.postList = posts;
    this.random = random;
    this.humanSpecs = humans;
    this.heroes = heroes;
    this.demo = demo;
    this.names = names;
    this.heroName = heroName;
    this.battle = battle;
    this.spawnUnit = spawnUnit;
    this.makeCharacter = makeCharacter;
    this.hasClass = hasClass;
    this.forceKill = forceKill;
    this.health = health;
    this.relationOverride = relation;

    this.teams = makeTeams(capacity);
    const [A, B] = applyMission(this.teams, calls, { hasClass });
    if (unitScale !== 1) {
      for (const team of this.teams) if (team.unitCount > 0) team.unitCount = Math.max(1, Math.round(team.unitCount * unitScale));
    }
    appendHeroes(this.teams, { heroes, hasClass });
    this.A = f32(A);
    this.B = f32(B);
    this.Q = Math.trunc(f32(this.A / this.B));

    // Spawn manager state (initialized when the roster is built).
    this.waveCountdown = this.A;
    this.wave = 0;
    this.stagger = 0;
    this.released = 0;
    this.localsCap = 0;

    this.characters = [];
    this.queues = new Map();
    this.objectives =
      objectives !== undefined
        ? objectives
        : new ClassicObjectives({
            posts: () => this.posts,
            random,
            relation: (a, b) => this.relation(a, b),
            faction: (team) => this.faction(team),
            crew,
            vehicleMask,
            units,
            travelMask,
            onPublish,
          });
    this.started = false;
    if (start) this.begin();
  }

  get posts() {
    return typeof this.postList === "function" ? this.postList() : this.postList;
  }

  // Team a's relation to team b: 1 friend, 0 neutral, -1 enemy.
  relation(a, b) {
    if (this.relationOverride) return this.relationOverride(a, b);
    return this.teams[a]?.relations[b] ?? 0;
  }

  faction(team) {
    return this.teams[team]?.faction ?? 0;
  }

  team(team) {
    return this.teams[team]?.characters || [];
  }

  get human() {
    return this.characters.find((c) => c.human) || null;
  }

  // ------------------------------------------------------------ the roster

  // Builds the Characters and runs the match start: humans join their teams,
  // bots fill each team to its unit count (named by their team), then each
  // team pads its class targets and activates its Characters in order.
  begin() {
    if (this.started) return this;
    this.started = true;
    this.waveCountdown = this.demo ? f32(0.1) : this.A;
    this.wave = 0;
    this.stagger = 0;
    this.released = 0;
    const registered = new Array(TEAM_COUNT).fill(0);
    if (!this.demo) {
      this.humanSpecs.forEach((spec, i) => {
        const team = spec.team | 0;
        if (team > 0 && team < TEAM_COUNT) {
          this.insert(team, true, spec.id ?? i, spec.name ?? humanName(i));
          registered[team]++;
        }
      });
    }
    for (let t = 1; t < TEAM_COUNT; t++) {
      const team = this.teams[t];
      for (let i = registered[t]; i < team.unitCount; i++) this.insert(t, false, -1, this.botName(team));
    }
    for (const team of this.teams) this.setupTeam(team);
    return this;
  }

  insert(t, human, playerId, name) {
    const team = this.teams[t];
    if (team.characters.length >= team.capacity) return null;
    const slot = team.characters.length;
    const character = this.makeCharacter
      ? this.makeCharacter(slot, t, human)
      : { index: slot, team: t, human, className: null, unit: null, controller: null, objective: null };
    character.team = t;
    character.human = human;
    character.slot = slot;
    character.managerIndex = this.characters.length;
    character.playerId = playerId;
    character.name = name;
    character.classIndex = null; // no class assigned yet
    character.hero = false;
    character.unit ??= null;
    character.objective ??= null;
    character.spawn = { timer: 0, wave: 0, ordinal: -1, post: null, ready: true };
    team.characters.push(character);
    this.characters.push(character);
    return character;
  }

  // A bot's name, from the team's own generator, seeded with its side type
  // when its first bot is made (src/bot-names.js). Locals take the level's
  // local name.
  botName(team) {
    team.botNames ??= botNames(this.names, team.faction);
    return team.botNames();
  }

  setupTeam(team) {
    this.padQuotas(team);
    for (const character of [...team.characters]) this.activate(character);
  }

  // When the class targets add up to fewer than the team's Characters, add
  // one target at a time to class max(RNG % 4n - 3n, 0) (n ordinary classes):
  // the first class gets most of them.
  padQuotas(team) {
    const n = team.classes.length - team.heroCount;
    if (n <= 0) return; // the 2004 game would divide by zero here
    let sum = team.classes.reduce((s, c) => s + c.target, 0);
    while (sum < team.characters.length) {
      const index = Math.max((this.random.next() % (4 * n)) - 3 * n, 0);
      team.classes[index].target++;
      sum++;
    }
  }

  // A Character taking a hero class takes the hero's name too, for the rest
  // of the battle (the 2004 game's SetClass).
  setClass(character, index) {
    const cls = this.teams[character.team].classes[index];
    character.classIndex = cls ? index : null;
    if (!cls) return;
    character.className = cls.name;
    character.hero = cls.hero;
    if (cls.hero) character.name = this.heroName?.(cls.name) || character.name;
  }

  // Match start for one Character: class 0, the final heroCount slots take
  // the hero classes, a bot draws its objective, then it prepares.
  activate(character) {
    const team = this.teams[character.team];
    this.setClass(character, 0);
    if (!character.human) {
      for (let k = team.heroCount; k > 0; k--) {
        if (team.characters[team.characters.length - k] === character) {
          this.setClass(character, team.classes.length - k);
          break;
        }
      }
      this.objectives?.request(character);
    }
    this.prepare(character);
  }

  // The roster-deficit class pick. A Character holding a hero class keeps it.
  // Counting every Character's assigned class (its own included), each
  // ordinary class's deficit is max(target - count, 0); a draw of RNG % total
  // picks the first class whose running deficit exceeds it. No deficit
  // anywhere: keep the current class, without drawing.
  pickClass(character) {
    const team = this.teams[character.team];
    const current = team.classes[character.classIndex];
    if (character.classIndex != null && current?.hero) return character.classIndex;
    const counts = new Array(team.classes.length).fill(0);
    for (const other of team.characters) {
      if (other.classIndex != null && team.classes[other.classIndex]) counts[other.classIndex]++;
    }
    const n = team.classes.length - team.heroCount;
    const deficits = [];
    let total = 0;
    for (let i = 0; i < n; i++) {
      const d = Math.max(team.classes[i].target - counts[i], 0);
      deficits.push(d);
      total += d;
    }
    if (total > 0) {
      const r = this.random.next() % total;
      let sum = 0;
      for (let i = 0; i < n; i++) {
        sum += deficits[i];
        if (r < sum) return i;
      }
      return -1;
    }
    return character.classIndex ?? 0;
  }

  // Readies a Character for its next life: a bot (unless a hero) picks its
  // class and is ready at once, a human waits to be ready; it keeps or picks
  // a post, joins that post's queue, and is scheduled for the next wave (two
  // ahead if the wave clock is overdue).
  prepare(character) {
    if (!character.human) {
      if (!character.hero) this.setClass(character, this.pickClass(character));
      character.spawn.ready = true;
    } else {
      character.spawn.ready = false;
    }
    this.changePost(character, this.choosePost(character));
    const post = character.spawn.post;
    if (post) character.spawn.ordinal = this.reserve(post, character.human, character.team === ownerOf(post));
    character.spawn.wave = this.wave + 1 + (this.waveCountdown < 0 ? 1 : 0);
  }

  choosePost(character) {
    const options = { posts: this.posts, team: character.team, random: this.random, relation: (a, b) => this.relation(a, b) };
    return character.human ? humanSpawnPost(character.spawn.post, options) : retainSpawnPost(character.spawn.post, options);
  }

  // ------------------------------------------------------------ post queues

  queueOf(post) {
    let queue = this.queues.get(post);
    if (!queue) {
      queue = [0, 0, 0, 0]; // AI at an ally's post, humans there, AI at own post, humans there
      this.queues.set(post, queue);
    }
    return queue;
  }

  reserve(post, human, own) {
    const queue = this.queueOf(post);
    const i = (human ? 1 : 0) + (own ? 2 : 0);
    const before = queue[i]++;
    return human ? before : before + queue[1 + (own ? 2 : 0)];
  }

  release(post, human, own) {
    const queue = this.queueOf(post);
    const i = (human ? 1 : 0) + (own ? 2 : 0);
    queue[i]--;
    return human ? queue[i] : queue[i] + queue[1 + (own ? 2 : 0)];
  }

  // Changing posts moves a held reservation: leave the old post's queue (in
  // the category its current owner gives), join the new one's.
  changePost(character, post) {
    const spawn = character.spawn;
    if (spawn.ordinal >= 0 && spawn.post !== post) {
      if (spawn.post) this.release(spawn.post, character.human, character.team === ownerOf(spawn.post));
      spawn.ordinal = -1;
      if (post) spawn.ordinal = this.reserve(post, character.human, character.team === ownerOf(post));
    }
    spawn.post = post;
  }

  // ------------------------------------------------------------ tickets

  tickets(team) {
    const r = this.battle?.reinforcements;
    return r && typeof r[team] === "number" ? r[team] : this.teams[team].tickets;
  }

  spendTicket(team) {
    const r = this.battle?.reinforcements;
    if (r && typeof r[team] === "number") {
      if (this.battle.spend) this.battle.spend(team);
      else r[team] = Math.max(r[team] - 1, 0);
    } else {
      this.teams[team].tickets--;
    }
  }

  // ------------------------------------------------------------ the update

  // The manager's clock, then every Character in roster order. A battle
  // service that runs the teams' bleed between the two (as the 2004 game's
  // manager does) calls clock() and updateCharacters() itself.
  update(dt) {
    this.clock(dt);
    this.updateCharacters(dt);
  }

  updateCharacters(dt) {
    for (const character of this.characters) this.updateCharacter(character, dt);
  }

  // The human has been prepared for his next life and waits for the spawn
  // screen's choice (onHumanReady).
  awaitingHuman(character) {
    return character.human && !character.unit && !character.spawn.ready && character.spawn.timer <= 0;
  }

  // The spawn manager's clocks, one conditional step each. Also the locals'
  // living cap: the AllyCount of posts held by teams friendly to team 3,
  // at most team 3's unit count.
  clock(dt) {
    dt = f32(dt);
    let cap = 0;
    for (const post of activePosts(this.posts)) {
      if (this.relation(ownerOf(post), 3) > 0) cap += postAI(post).allyCount | 0;
    }
    this.localsCap = Math.min(cap, this.teams[3].unitCount);
    const countdown = f32(this.waveCountdown - dt);
    this.waveCountdown = countdown;
    let stagger = this.stagger;
    if (!(countdown > 0)) {
      this.waveCountdown = f32(this.A + countdown);
      if (this.demo && this.wave < 3) this.waveCountdown = 1;
      this.wave++;
      stagger = 0;
      this.released = 0;
    }
    stagger = f32(stagger - dt);
    this.stagger = stagger;
    if (stagger <= 0) {
      this.released++;
      this.stagger = f32(this.B + stagger);
    }
  }

  // Whether the wave/stagger gate holds this Character back (strict).
  waiting(character) {
    return (character.spawn.wave - this.wave) * this.Q - this.released + character.spawn.ordinal >= 0;
  }

  updateCharacter(character, dt) {
    dt = f32(dt);
    let team = this.teams[character.team];
    if (character.unit) {
      // A hero dies when its team's tickets fall to its living count.
      if (character.hero && this.tickets(character.team) <= team.living && this.health(character.unit) > 0) {
        this.forceKill?.(character);
      }
      return;
    }
    const spawn = character.spawn;
    const timer = f32(spawn.timer - dt);
    spawn.timer = timer;
    if (timer > 0) return;
    if (timer > -dt) this.prepare(character);
    if (!character.human && this.tickets(character.team) <= team.living) return;
    if (!character.human && team.characters.length > team.capacity) {
      this.changePost(character, null);
      this.switchTeam(character, 3 - character.team);
      this.objectives?.request(character);
      team = this.teams[character.team];
    }
    this.changePost(character, this.choosePost(character));
    if (!spawn.ready || !spawn.post) {
      spawn.wave = this.wave + 1;
      return;
    }
    if (!character.human) {
      if (this.waiting(character)) return;
      if (!character.hero && character.team > 2) {
        let living = 0;
        for (const other of team.characters) if (other.unit && !other.human) living++;
        if (this.localsCap <= living) return;
      }
    }
    this.spawn(character);
  }

  // Where at a post a Character appears: a random point of the post's spawn
  // path (its ally path when the post is an ally's), else a random class
  // spawn point (own posts only), else the post itself. [x, y, z, yaw].
  spawnPoint(post, allied) {
    const ai = postAI(post);
    const path = allied ? ai.allyPath : ai.spawnPath;
    const points = path?.present ? path.points : null;
    if (points?.length) return points[this.random.next() % points.length];
    if (!allied && ai.classSpawnPoints > 0 && ai.classSpawnWorld?.length) {
      return ai.classSpawnWorld[this.random.next() % ai.classSpawnPoints] || ai.classSpawnWorld[0];
    }
    return [post.position.x, post.position.y, post.position.z, ai.yaw || 0];
  }

  // Into the world. If the class can't be made, class 0 is tried once; if
  // that fails too, the Character keeps its place and tries again later.
  spawn(character) {
    const post = character.spawn.post;
    const team = this.teams[character.team];
    const point = this.spawnPoint(post, character.team !== ownerOf(post));
    const at = [point[0], point[1], point[2]];
    let unit = this.spawnUnit ? this.spawnUnit(character, character.className, post, at, point[3]) : null;
    if (!unit) {
      if (!character.classIndex) return null;
      this.setClass(character, 0);
      unit = this.spawnUnit ? this.spawnUnit(character, character.className, post, at, point[3]) : null;
      if (!unit) return null;
    }
    character.unit = unit;
    const cls = team.classes[character.classIndex];
    character.hero = !!cls?.hero;
    if (!character.hero) team.living++;
    if (!character.human) this.objectives?.republish(character);
    if (post && character.spawn.ordinal >= 0) {
      this.release(post, character.human, character.team === ownerOf(post));
      character.spawn.ordinal = -1;
    }
    return unit;
  }

  // ------------------------------------------------------------ events

  // A Character's unit died: it prepares again in 4 s; a bot draws a fresh
  // objective and forgets its post; the team's living count drops (heroes
  // aren't counted) and it spends a reinforcement if it has any.
  onDeath(character) {
    const team = this.teams[character.team];
    character.spawn.timer = RESPAWN_DELAY;
    if (!character.human) {
      this.objectives?.request(character);
      character.spawn.post = null;
    }
    character.unit = null;
    if (!character.hero) team.living--;
    if (this.tickets(character.team) > 0) this.spendTicket(character.team);
  }

  // The human chose (on the spawn screen): a class from the team's roster (by
  // name or index) and optionally a post; he spawns on the next update.
  onHumanReady(character, { className = null, classIndex = null, post } = {}) {
    const team = this.teams[character.team];
    if (className != null) {
      const index = team.classes.findIndex((c) => c.name === className);
      if (index >= 0) this.setClass(character, index);
      else character.className = className; // outside the roster: not counted
    } else if (classIndex != null) {
      this.setClass(character, Math.min(Math.max(classIndex | 0, 0), team.classes.length - 1));
    }
    if (post !== undefined) this.changePost(character, post);
    character.spawn.ready = true;
  }

  // Moves a non-hero Character to another team when it has room; a bot picks
  // a class there. (Offline the over-capacity branch that calls this cannot
  // trigger: teams hold 200.)
  switchTeam(character, to) {
    if (character.hero || character.team === to || to < 1 || to >= TEAM_COUNT) return false;
    const from = this.teams[character.team];
    const target = this.teams[to];
    if (target.characters.length >= target.capacity) return false;
    // A place held in a post's queue is given up while the old team still
    // decides its category (its own post or an ally's), as the over-capacity
    // branch does before calling this, and taken again at the post chosen
    // for the new team.
    const held = character.spawn.ordinal >= 0;
    this.changePost(character, null);
    from.characters.splice(from.characters.indexOf(character), 1);
    target.characters.push(character);
    character.team = to;
    if (!character.human) this.setClass(character, this.pickClass(character));
    else this.setClass(character, Math.min(Math.max(character.classIndex ?? 0, 0), target.classes.length - 1));
    const post = this.choosePost(character);
    this.changePost(character, post);
    if (held && post) character.spawn.ordinal = this.reserve(post, character.human, to === ownerOf(post));
    return true;
  }
}
