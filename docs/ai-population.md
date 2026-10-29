# Classic AI: population, spawning, difficulty and objectives

Population of the Classic AI (`docs/ai.md`). Behavior follows the 2004 game:
difficulty, spawning, class choice and player commands; the spawn release
equation and the per-post queue; objectives, mission controls and interrupt
priorities; objective reassignment events and the local recapture response;
command-post objective destinations taken from the CaptureRegion; hero class
ownership. Further rules found while implementing are listed under
"Findings".

| File | What |
| --- | --- |
| `tools/zero_ai_mission.py` | Writes `public/maps/<map>/ai-mission.json` (gitignored) |
| `src/ai/classic/difficulty.js` | `ClassicDifficulty`: tiers, skills, curves, mission setters, scheduler numbers, grenade danger delay |
| `src/ai/classic/spawning.js` | `ClassicSpawning`: the roster (Characters), spawn manager clocks, class choice, post queues, spawning |
| `src/ai/classic/goals.js` | Objective selection and publication, spawn-post selection, reassignment events, recapture, objective destinations, mission controls |
| `tests/ai-population.test.js` | `node --test tests/ai-population.test.js` |
| `tests/test_ai_mission_export.py` | `python3 -m unittest tests/test_ai_mission_export.py` |

The modules import only each other (and nothing from the game), so they run
in Node. Every random draw goes through the shared `ClassicRandom`
(`src/ai/random.js`) in the original game's order.

## `ai-mission.json`

`python3 tools/zero_ai_mission.py <map> [--side rep]` reads `mission.lvl`, the
map's world archive, `Common/ENG/common.lvl`, `core.lvl` and the side archives
the map's scripts load. It writes only `public/maps/<map>/ai-mission.json`.

```js
{
  schema: 1,
  generator: "zero_ai_mission.py 1",
  map: "nab2", world: "naboo2",          // world name from the wrld chunk
  mission: "nab2c",                      // the regular script for --side (rep); the
                                         //   runtime sets it to the battle the URL
                                         //   plays (src/mission.js selectAIMission)
  variants: ["nab2c_h"],                 // the mission's other scripts (historical)
  sources: { "mission.lvl": sha256, "NAB/nab2.lvl": sha256, ... },
  factions: { 0: "none", 1: "alliance", 2: "empire", 3: "republic", 4: "cis", 5: "locals" },
  defaultHeroes: { 1: "all_inf_lukeskywalker", 2: ..., 3: ..., 4: ... },
  scripts: {                             // every script for the map: nab2a, nab2c, nab2c_h
    nab2c_h: {
      variant: "historical" | "regular", historical: bool (SetHistorical called),
      sides: ["gar", "cis"],             // SIDE archives it reads
      callCount: 94,                     // all calls in the script
      calls: [                           // AI-relevant calls, source order
        { at: 5, name: "SetPlayerTeamDifficultyEasy", args: [2] },
        { at: 11, name: "SetTeamName", args: [2, "Naboo Guard"], faction: 5 },
        { at: 30, name: "ActivateBonus", args: [1, "HERO"], mask: 8 },
        ...
      ],
      teams: { "1": { name, faction, factionName, units, reinforcements, classes: [[class, count]],
                      hero, bonus, aggressiveness, bleed: [[threshold, rate]] (sorted), relations: {team: -1|0|1} } },
      spawnDelay: [A, B],                // the last SetSpawnDelay, else [15, 0.5]
      difficulty: [ ...the difficulty setter calls, in order ],
    },
  },
  posts: [{                              // placed command posts, world-list order
    name, order, class, base,            // base: native class at the root of the ODF chain
    team, position: [x, y, z], yaw,
    values: { atk: [6], def: [6], overrides: [[ "class" | "instance", property, value ]] },
                                         // index 0 = class default (100), 1 Alliance,
                                         //   2 Empire, 3 Republic, 4 CIS, 5 Locals
    valueBleed,                          // %d, class default 100
    bans, banProps: [[property, value]], // SoldierBan 1 HoverBan 2 SmallBan 4 MediumBan 8
                                         //   HugeBan 16 FlyerBan 32, OR-only
    spawnPath: { name, present, path, points: [[x, y, z, yaw]] } | null,
    allyPath:  { ... } | null,
    allyCount,                           // %i, default 0
    classSpawnPoints, classSpawnLocations: [[x, y, z, yawDeg]], classSpawnWorld: [[x, y, z, yaw]],
    captureRegion: { name, present, type, matrix, size, center } | null,
    controlRegion, killRegion,           // same shape
    radius,
    properties: [[property, value]],     // the instance's AI-relevant properties, in order
  }],
  commandClasses: { class: { base, values, valueBleed, spawnPoints } },
                                         // command vehicles/buildings with class spawn
                                         //   points, from the scripts' side archives;
                                         //   the command vehicles' posts read them
                                         //   (src/mobile-posts.js)
  names: { "0".."4": { first: [], last: [] }, local: "ROYAL GUARD" },
                                         // bot name lists by faction (English) and the
                                         //   level's local name
}
```

Parsing rules (as in the original game): Lua number arguments are read as
float32 and truncated. SetTeamName parses the name by its case-folded hash:
`all` / `alliance` 1, `imp` / `empire` 2, `rep` / `republic` 3, `cis` 4,
`neutral` (and one more name known only by its hash) 0, anything else 5
(Locals). ActivateBonus names map to the bonus mask (`Hero` 8, `Training`
0x80 = Advanced Training, others by hash). Weights and ValueBleed use `%d` (a
value that doesn't convert leaves the previous one); AllyCount `%i`; the bans
OR their bit only when `atoi(value) > 0`. The class parser reads the weights,
ValueBleed and the class spawn points (`SpawnPointCount`, then one
`SpawnPointLocation x y z yawDeg` each); bans, paths, regions and AllyCount
are instance properties. Paths and regions resolve by case-folded name; an
unresolved name means no path.

The team summaries do not append heroes: whether a team fields its hero
depends on the match's Heroes option (and ActivateBonus), which the runtime
decides.

## `difficulty.js`

```js
const difficulty = new ClassicDifficulty({ tier: 2, playerTeam, network: false });
difficulty.apply(aiMission.scripts[aiMission.mission].calls);   // setters, in order
difficulty.advancedTraining();      // the Yavin GC bonus, when it applies
difficulty.curves(team)             // { A, D, B, S }, float32
difficulty.skill(team)              // 1..20
difficulty.playerSkill, enemySkill, tier, profileTier
difficulty.cycle, losBudget, vehicleBucket   // 15/6/2 (Easy, Medium), 3/12/10 (Hard)
difficulty.dangerDelay(random)      // seconds after first contact, by the enemy skill
```

- Tiers: Easy 1, Medium 2, Hard 3 (any other value initializes as 4/14).
  Skills [player, enemy]: Easy 6/4, Medium 4/14, Hard 4/17, clamped 1..20.
- Curves: through skill 1, 14, 20 with anchors A 0.8/1.9/20, D 0.8/1.9/2.4,
  B 3/1.6/1, S 2/1/0; `lo + f32(t * f32(hi - lo))` with `t` = `(k-1)/13`
  or `(k-14)/6`, every operation rounded to float32. Bit-identical to the
  original game for all 20 skills.
- Setters: `SetPlayerTeamDifficulty(n)` / `SetEnemyTeamDifficulty(n)`
  replace; the `Easy/Medium/Hard` forms add only when the local profile's tier
  matches (`profileTier`, even in a network game). Every setter clamps and
  recomputes the curves. The override guard affects only `init`.
- Player team: the team of local player 1, offline. Network games and
  split-screen players on different teams use the enemy curves for everyone.
  Allied locals use the enemy curves.
- Danger delay: enemy skill below 14: [1.1, 2.1); 14..16: [0.6, 1.3); 17+:
  [0, 0.8), drawn `lo + U(hi - lo)` in float32, for every recipient.

## `spawning.js`

```js
const spawning = new ClassicSpawning({
  calls,                // aiMission.scripts[mission].calls (or callsFromMapMission(map.mission))
  posts,                // CommandPosts.posts with .ai attached (attachPostAI), or () => posts
  random: world.random,
  humans: [{ team: playerTeam }],
  heroes,               // Heroes On/Off (the match preset)
  names: aiMission.names,
  battle,               // { reinforcements: {team: n}, spend(team) }
  spawnUnit: world.spawnUnit,
  makeCharacter: (slot, team, human) => new Character(slot, team, human),
  hasClass: (name) => factory.sideOf(name) != null,
  forceKill: (character) => { /* kill the hero's unit */ },
  // forwarded to the default ClassicObjectives:
  crew, vehicleMask, units: world.units, travelMask, onPublish,
});
spawning.characters          // every Character, roster order (humans first)
spawning.team(t)             // a team's Characters, in team order
spawning.human               // the local human's Character
spawning.teams[t]            // { faction, unitCount, tickets, classes: [{name, target, hero}],
                             //   heroCount, relations, characters, living, ... }
spawning.relation(a, b)      // team a's relation to team b (mission SetTeamAs*)
spawning.faction(t)
spawning.objectives          // the ClassicObjectives (goals.js) it uses
spawning.update(dt)          // clock(dt) then updateCharacters(dt)
spawning.onDeath(character)
spawning.onHumanReady(character, { className, classIndex, post })
spawning.awaitingHuman(character)
```

Construction builds the roster and runs the match start (pass `start: false`
to do it later with `begin()`): humans join their teams first, bots fill each
team to its `SetUnitCount` (drawing names), then each team 0..7 pads its
class targets and activates its Characters in order (class 0, the final
hero slots get the hero classes, a bot draws its objective, then every
Character prepares).

Each Character gets these fields (on the object `makeCharacter` returns):

```js
character.slot, managerIndex, playerId (-1 for bots), name
character.classIndex          // index into teams[team].classes, null before activation
character.className, hero     // from the class
character.unit, objective     // the current body; the objective post
character.spawn = { timer, wave /* S */, ordinal /* J, -1 none */, post, ready }
```

### Driving it from the battle service

Per simulation step `h`, after units, ordnance and capture:

```js
spawning.clock(h);              // the spawn manager's wave/stagger clocks and the locals cap
battle.update(h);               // team bleed and the no-post countdown (Team updates 1..7)
spawning.updateCharacters(h);   // each Character: respawn timer, gates, spawn
```

(`spawning.update(h)` does the first and last together.)

Events the service sends:

| When | Call |
| --- | --- |
| A Character's unit dies (once per death, any cause) | `spawning.onDeath(character)` |
| The human confirms the spawn screen (only while `awaitingHuman`) | `spawning.onHumanReady(human, { className, post })` |
| A post changes hands (`captured`: 0 → by; `neutralized`: from → 0) | `spawning.objectives.onOwnerChanged(post, old, new, spawning.characters)` |
| A post is destroyed or deactivated (set `post.active = false`) | `spawning.objectives.onDeactivated(post, spawning.characters)` |
| A Character boards, leaves or changes vehicle | `spawning.objectives.onBoarded(character, oldVehicle, newVehicle)` |
| The Advanced Training bonus applies | `difficulty.advancedTraining()` |

`onDeath` spends the team's reinforcement (when it has any) and keeps the
team's living count; the service must not spend another for the same death.

`world.spawnUnit(character, className, post, [x, y, z], yaw)`: make a unit of
`className` at the point (a path point: settle it on the ground as
`Soldier.spawn` does), tie it to the Character (`character.unit`, team), and
return it; return `null` when the class can't be made (spawning then tries
class 0 once, and otherwise retries later without losing the queue place).
It must not touch reinforcements or living counts. For the human's Character
it places the player's own soldier. Attach the bot's controller inside
`spawnUnit` (so the objective republished right after spawning reaches it),
or call `spawning.objectives.republish(character)` when attaching it later.

Tickets come from `battle.reinforcements[team]` when the battle tracks the
team, else from the team's own count. Teams 3-7 start with 2147483647 unless
the mission sets them; the battle service must use the same default.

### What the update does

- Clock: wave countdown `-= dt`; at `<= 0` add A (demo: the first three
  waves last 1.0 s), wave + 1, stagger and releases 0. Then stagger `-= dt`;
  at `<= 0` releases + 1, add B. One step each per update, float32.
- Locals cap: the AllyCount of active posts whose owner is friendly to team 3,
  at most team 3's unit count. Bots of teams 3-7 spawn only while fewer of
  their team's bots are alive.
- Character: with a unit, only the hero check (a hero is killed when its
  team's reinforcements are at or below the team's living count and it has
  health). Without: the respawn timer counts down; when it crosses zero the
  Character prepares; a bot then needs reinforcements above the living count;
  it keeps or picks a post (`retainSpawnPost` / `humanSpawnPost`), moving its
  queue place if the post changes; with no post or not ready it is
  rescheduled for the next wave; a bot waits while
  `(S - W) * Q - R + J >= 0`; locals check the cap; then it spawns.
- Preparation: a bot (not a hero) picks its class by roster deficit and is
  ready; a human is not ready until `onHumanReady`. It joins its post's queue
  (ordinal J) and is scheduled for wave `W + 1` (`W + 2` if the wave clock is
  overdue).
- Spawn: a random point of the post's spawn path (ally path at an ally's
  post; class spawn points; else the post itself), one draw; `spawnUnit`;
  the class-0 retry; the living count (heroes excluded); a bot's objective is
  republished; the queue place is released.
- Death: the timer is set to 4 s; a bot draws a fresh objective and forgets
  its post (a human keeps his); living count and reinforcement as above.

## `goals.js`

```js
attachPostAI(posts, aiMission, map.spawns)   // post.ai from ai-mission.json (by name)
friendlySpawn(post, team, relation)          // can team spawn here?
postWeight(post, team, faction)              // 1: ATK[f], 2: DEF[f], 3: DEF[f] if f > 0, else 0
selectObjective({ posts, team, faction, random, relation, driver, vehicleMask, excludePrevious, previous })
selectSpawnPost / retainSpawnPost / humanSpawnPost
objectiveCenter(post) / objectiveOffset(post, random) / objectiveArrived(post, offset, actor, opts)
missionControls(calls)   // aggressiveness (stored only), dense, attackingTeam, voTeam, snipe
                         //   ranges (140/140), stayInTurrets, stationaryWait (10 s),
                         //   vehicleNotifyRadius (50), formation (0/3), blindJetJumps, flyHeight
new ClassicObjectives({ posts, random, relation, faction, crew, vehicleMask, units, travelMask, onPublish })
  .request(character, excludePrevious, previous)   // select + publish (+ driver propagation)
  .publish / republish / assign / reassign / recruit
  .onOwnerChanged / onDeactivated / onBoarded
  .stale(character) / revalidate(character)        // the patrol's 5 s check
classicBinding(ai)       // { requestObjective, friendlySpawnEligible } for index.js,
                         //   through world.objectives or world.spawning.objectives
```

`difficulty.js` also exports `curvesFor(difficulty, team)` and
`isHard(difficulty)`, which `index.js` binds by name with `world.difficulty`.

- Objective: candidates are the active posts the team can't spawn at, in
  world order; a vehicle driver also skips posts whose bans meet its
  vehicle's traversal mask (infantry are never ban-tested here). Weights add
  in float32; one draw in [0, total); the first post whose running total
  reaches it. No positive total: any active post uniformly (filters
  ignored), redrawing to avoid `previous` when asked; null with fewer than
  two. The weight is by team index, never by SetAttackingTeam.
- Publication: the Character's `objective`; when it has a body and a
  controller, `controller.publishObjective(post)` (or `onPublish`; falls back
  to `controller.onObjective`) so the Controller pushes source goal 3.
  Drivers hand the objective to AI co-occupants; a passenger copies an AI
  driver's.
- `crew(character)` returns null on foot or `{ vehicle, isDriver, driver,
  occupants }` (Characters); `vehicleMask(vehicle)` the vehicle class's
  traversal mask. `src/ai/index.js` provides both (`objectiveCrew`,
  `objectiveVehicleMask`).
- Ownership change: for each team 1..7 whose relation to the owner crosses
  between positive and not, its bots sent there draw again; a team whose
  friendly post went neutral also recruits: the registry's units within 60 m
  (cap 10, allied to the team, registry order), skipping humans, flyers and
  units whose traversal mask the post bans, up to five, sent there.
- Destination: the capture region's origin, else the post; offset z then x,
  uniform in the region's built size (±3 without a region), y 0, world axes:
  a cylinder's radius on both (docs/formats.md "Regions").
  Arrival: inside the region or within half the bot's forward speed;
  without a region within `max(10, radii + 5)` (+20 for a moving post).

## Locals

Teams 3 to 7 are the locals: a mission names one (`SetTeamName(3,
"locals")`, any name SetTeamName doesn't know gives the Locals faction),
gives it classes and a unit count, and sets how it stands with the armies
(`SetTeamAsFriend` / `SetTeamAsEnemy`; without a call two teams are
enemies). The battle fields them through the same modules in single player,
in a world worker, on a server and on a hosting desktop:

- **Who fights whom:** `missionTeams(calls)` / `teamTable(spawning.teams)`
  (`spawning.js`) give each team's relations, faction and capture team;
  `src/game.js` hands the table to the unit registry (`units.setTeams`), so
  combat, the AI's perception, kill credit, mines and the HUD all read the
  mission's relations (`units.relation(a, b)`, team a's to team b).
- **Fielding them:** every mission team's classes are loaded (a locals' class
  that won't load fields nobody); the spawn manager fills team 3 to its unit
  count and spawns its bots at their own posts' spawn paths or at the ally
  paths of posts their friends hold, up to the locals cap (above).
- **Keeping them:** the battle (`src/battle.js`) runs the bleed and the
  no-post count for the armies only. A local team keeps its 2147483647
  reinforcements all battle (each death still spends one), unless it is
  friendly with exactly one army and that army's reinforcements are down to
  its living soldiers: then it has none left.
- **Posts:** a local's soldier in a capture region counts for its team's
  capture team (`src/commandposts.js`): the Jawas, friends of both armies,
  for nobody; the Gungans, Ewoks, Wookiees and Geonosians for the army they
  fight beside; the Tusken Raiders and Gamorreans for themselves. Posts
  without a capture region never change hands; posts whose class has no
  `MapTexture` (or `MapScale` 0) are not on the maps, and without a
  `HoloOdf` have no hologram (Tatooine's Tusken campfires, Hoth's probe droid
  drop, Jabba's Palace's Gamorrean post).
- **Colours:** a team of the Locals faction is drawn in the locals' yellow
  (223 223 32; holograms 240 220 30), friend or foe, as the original game's
  team colour table has it; any other team by relation. The viewer's own team
  stays friendly. This makes the historical battles' local armies (Theed's
  Naboo Guard, the Gungans of `nab1c_h`, the Wookiees of `kas1c_h`) yellow
  to the CIS too. The reticle still goes by relation (allies green).
- **Names:** every bot of a locals team takes the level's local name
  (`names.local`: TUSKEN RAIDER, JAWA, GUNGAN, EWOK, WOOKIEE, GEONOSIAN,
  GAMORREAN). The scoreboard lists the armies only.

Every BF1 mission with locals (team, classes and count, relations, where
they deploy; `cap` is the locals cap, AllyCount of their posts and their
friends', at most the unit count):

| Map | Missions | Local team | Classes, units | Relations | Captures for | Deploys |
| --- | --- | --- | --- | --- | --- | --- |
| tat1 Dune Sea | tat1i, tat1i_h, tat1r | 3 "locals", Tusken Raiders | tat_inf_tuskenraider 5, tat_inf_tuskenhunter 2; 14 | enemy of both | itself | own posts cp1 TUSKEN CAMP, cp3 CISTERNS (capturable); the campfires ts1/ts4/ts5 never change hands; cap 14 |
| tat2 Mos Eisley | tat2i, tat2i_h, tat2r | 3 "locals", Jawas | tat_inf_jawa 14; 14 | friend of both | nobody | ally path `jawa_spawn` of cp7 HANGAR, whoever holds it; cap 14 |
| tat3 Jabba's Palace | tat3a, tat3c | 3 "locals", Gamorreans | gam_inf_gamorreanguard 3; 3 | enemy of both | itself | own post npccp1 (invisible, `NPC_PATH`); cap 3 |
| nab1 Naboo Plains | nab1c, nab1i | 3 "Locals", Gungans | gun_inf_soldier 5, gun_inf_rider 2; 7 | enemy of 1, friend of 2 | team 2 | ally paths of cp2, cp4 and cp11 once team 2 holds them (cp11 CENTER starts neutral); cap 7 |
| nab1 Naboo Plains | nab1c_h | 2 "Gungans" (an army) | gun_inf_soldier 17, gun_inf_rider 7, gun_inf_defender 8; 32 | | | an army: yellow, no locals |
| nab2 Theed | nab2c_h | 2 "Naboo Guard" (an army) | gar_inf_soldier 12, gar_inf_pilot 5, gar_inf_vanguard 5; 22 | | | an army: yellow, no locals |
| end1 Endor | end1a, end1a_h | 3 "locals", Ewoks | ewk_inf_trooper 4, ewk_inf_repair 6; 14 | friend of 1, enemy of 2 | team 1 | ally paths of the posts team 1 holds (cp2, cp5 at the start; cp6 once taken); cap 14 |
| kas1 Islands | kas1c, kas1i | 3 "locals", Wookiees | wok_inf_mechanic 2, wok_inf_rocketeer 2, wok_inf_warrior 3; 7 | enemy of 1, friend of 2 | team 2 | ally paths of the posts team 2 holds (cp1, cp4 at the start; cp2, cp3, cp5 once taken); cap 7 |
| kas1 Islands | kas1c_h | 2 "Wookie" (an army) | wok_inf_mechanic 5, wok_inf_rocketeer 10, wok_inf_warrior 10; 32 | | | an army: yellow, no locals |
| kas2 Docks | kas2c, kas2c_h, kas2i | 3 "locals", Wookiees | wok_inf_warrior 3, wok_inf_rocketeer 2, wok_inf_mechanic 2; 7 | enemy of 1, friend of 2 | team 2 | ally paths of the posts team 2 holds (cp3 at the start; cp2 once taken); cap 7 |
| geo1 Geonosis | geo1r, geo1r_h | 3 "locals", Geonosians | geo_inf_geonosian 7 (5 in geo1r_h); 7 | enemy of 1, friend of 2 | team 2 | ally paths of the CIS posts (AllyCount 2 to 6); cap 7 |
| hot1 Hoth | hot1i | 3 "Local" (no units), 4 unnamed | imp_droid_probe 1; 1 | 3: friend of 1, enemy of 2; 4: enemy of all | 3: team 1; 4: itself | team 4 never deploys: the locals cap is team 3's unit count, 0 (the probe droid is a droid, which the port doesn't build anyway) |

The other missions (bes1, bes2, hot1i_h, kam1, nab2a, nab2c, rhn1, rhn2,
yav1, yav2) field no locals. `SIDE/des.lvl` defines `tat_inf_*` classes, so
the side extraction takes a side's units by the classes its archive defines,
not by the `des_` prefix.

## Findings

- SetTeamName sets the team's faction from the name: the names listed above.
  "Naboo Guard" is therefore Locals (5), and a team the mission never names
  has faction 0 (the default).
- SetHeroClass only registers a hero class per team. Heroes are appended after
  ScriptInit by the planetary bonus manager (teams 1 and 2) when the team's
  Hero bonus (8) is set: offline from the instant-action hero option, or by
  the mission's `ActivateBonus(team, "Hero")`; in network games from the
  server option. Without a registered class the faction's default hero is
  used (Luke, Vader, Mace, Dooku).
- The spawn manager defaults to SetSpawnDelay(15, 0.5); teams 3-7 get
  2147483647 reinforcements; teams 1-7 hold 200 Characters and 12 classes;
  teams 1 and 2 hold four bleed thresholds, the others none. Thresholds are
  sorted ascending when the match starts.
- Bots' names are drawn when the roster is built: first name from the
  faction's localized list (two random letters without one), then last name
  (three random digits without one); Locals take `level.<world>.local`. With
  the English lists that is 2 draws per Alliance bot, 5 per Empire bot and 4
  per Republic or CIS bot. Team name lists are loaded per faction.
- Per update the spawn manager also sums the AllyCount of posts whose
  owner is friendly to team 3, capped by team 3's unit count; bots of teams
  above 2 spawn only while their living count is below it. AllyCount is a
  `%i` instance property.
- The team update runs the bleed and the no-post count only for teams 1 and
  2; its score test is the bleed score (a team holding only posts worth no
  ValueBleed, none neutral, counts down). For teams 3 to 7 it finds the one
  army the team is friendly with (friendly with both: none); when that army's
  reinforcements are no more than its living count, the team's reinforcements
  go to 0 (its starting count stays).
- The team setup after the mission script sets each team's capture team (its
  own index by default) for teams above 2: 1 or 2 when the team is friendly
  with that army alone, 0 when friendly with both. The command post update
  counts each soldier for its team's capture team (0 counts for nobody), and
  neutralizes an owned post at the sum of the rates of the capture teams
  present other than the owner.
- The same setup fills a colour per other team: the locals' yellow when that
  team's faction is Locals, else green, grey or red by relation. The map's
  posts, the radar and the holograms read that colour.
- The spawn point: a raw draw modulo the path's point count; class spawn
  points only at the team's own post; else the post's own transform. A path
  point is then settled with two ray casts (10 m up, then 20 m down); a class
  spawn point is brought back along a ray from the post's entity's collision
  centre to the first terrain, building or prop (mask 0x48). Its yaw is the
  post's plus the authored one (the class parser makes it a turn about the up
  axis, composed with the entity's transform), so the AT-AT's points (5 and
  355 degrees, 8 m out either side) face outward. Command vehicles are such
  posts while they live (`src/mobile-posts.js`, `CommandPosts.all()`, which
  the spawn manager walks; formats.md "Spawning and the spawn screen").
- The human's post: kept while eligible; else the eligible post nearest the
  old one; with none, the eligible post nearest any enemy-held post.
- Releasing a queue place uses the post's owner at that moment, so a post
  that changed hands while a Character waited decrements the other category.
- SetStayInTurrets, SetStationaryWaitTime, SetAllowBlindJetJumps,
  SetAIFormationRadius and the snipe ranges just store a global setting;
  defaults are listed under `missionControls`. SetUrbanEnvironment does the
  same as SetDenseEnvironment.

## Provisional

- `spawning.js`: the manager's clock runs before every Character's update,
  and Characters update in roster order. The original game orders them
  through its own priority list; not checked against a populated world.
- `difficulty.js`, `goals.js`, `spawning.js`: float range and integer draws
  round every operation to float32, as the original game's single-precision
  arithmetic does; under double precision a last-bit difference is possible.
- `goals.js`: a post's validity (an entity reference and a validity flag in
  the original game) is `post.active !== false`.
- `goals.js`: the recapture query uses the port's unit registry. The
  original game's registry also holds buildings and mines, which count toward
  its cap of ten.
- `goals.js`: arrival without a capture region needs the post object's
  collision radius and speed, which the extractor doesn't provide for the
  placed posts yet (default 0). A command vehicle's post gives its hull's
  nominal speed and, for the radius, the sphere round its collision bounds
  (`src/mobile-posts.js`), not the original game's own broadphase sphere.
- `spawning.js`: the network, secondary-world and network-occupancy branches
  (fifty peer slots, the 16 living-bot cap, team switching over capacity)
  are not modeled beyond the offline rules; attract mode's clocks are.
- `spawning.js`: a human class outside the mission's roster is spawned but
  not counted in the roster deficit.
- `goals.js`: more than 16 active posts would overrun the original game's
  arrays; the port selects normally and `validatePosts` reports it.

## Contract changes

- `Character` gains `slot`, `managerIndex`, `playerId`, `name`,
  `classIndex`, `hero` and `spawn: { timer, wave, ordinal, post, ready }`
  (set by `ClassicSpawning` on the object `makeCharacter` returns).
- The Controller receives objectives as `controller.publishObjective(post)`
  (or through an `onPublish` hook): source goal 3 for that post.
- `world.spawning` (the match's `ClassicSpawning`) or `world.objectives` (its
  `ClassicObjectives`) and `world.difficulty` (a `ClassicDifficulty`): what
  `index.js`'s population binding reads.
- `world.posts[]` carry `.ai` (`attachPostAI`) and `.active`.
- The battle service: teams 3-7 default to 2147483647 reinforcements; bleed
  debits at most one reinforcement per team update; the no-post countdown
  pauses (not resets) while a neutral post exists and resets to 20 s only
  when the team holds a post.
