# Enhanced AI

Enhanced is the second battle AI, next to Classic. Classic (`src/ai/classic`,
[ai.md](ai.md)) is the 2004 game's AI, quirks included.
Enhanced keeps Classic's machinery (navigation, the sight scan, weapons,
vehicles, class jobs, squad commands, chatter) and replaces the decisions
players asked to change: weak allies, bots that never reach their command
post, and aim that differs by team. This is its first slice, not the whole
of the planned design (see "Not yet").

Choose it in Options, Game, BATTLE AI (OFF, CLASSIC, ENHANCED; it applies on
Restart battle), or with `?ai=enhanced`. DIFFICULTY's EASY, NORMAL and ELITE
are Enhanced's Easy, Normal and Hard.

Classic is unchanged. No file under `src/ai/classic` changed for Enhanced;
Enhanced plugs in through the Classic entry point's own extension options
(`createClassicAI` `agents` and `combat`, `ClassicSpawning` `objectives`,
the difficulty object). A Classic battle with a given seed plays exactly as
it did before Enhanced was added (checked by comparing whole headless battles).

## Files

| File | What |
| --- | --- |
| `src/ai/enhanced/index.js` | Exports, and `enhancedOptions(world, { tier, seed })` for `createClassicAI` |
| `src/ai/enhanced/difficulty.js` | `EnhancedDifficulty`: one skill for both teams; decision cycle by tier |
| `src/ai/enhanced/objectives.js` | `EnhancedObjectives` (the objective draw, defense) and `EnhancedSpawning` (spawn posts) |
| `src/ai/enhanced/agents.js` | Trooper, scout and repair agents that keep going for their post |
| `src/ai/enhanced/aim.js` | `EnhancedAimer`, the player's spread rules for bots, the combat overrides |
| `src/ai/index.js` | `createAI("enhanced", ...)` picks the Enhanced pieces |
| `tests/ai-enhanced.test.js` | `node --test tests/ai-enhanced.test.js` |
| `tools/ai-focus-probe.mjs` | Whole-battle measurements for Classic and Enhanced (below) |

## What Enhanced changes

### Equal teams

Classic gives the AI on the player's team one skill and every other team
another: Easy 6/4, Medium 4/14, Hard 4/17 (player's team/enemy), and mission
scripts can move them further apart. So in Classic the player's allies aim
worst on Medium and Hard. Enhanced gives both teams the tier's skill (Easy
8, Normal 14, Hard 17) and ignores the missions' difficulty calls and the
Advanced Training bonus. The skill still feeds the Classic pieces Enhanced
shares (the attack-pattern table and the grenade warning delay); aim has its
own profiles (below).

How often a bot decides, in updates: Easy 15 as Classic, Normal 10 (Classic
Medium 15), Hard 6 (Classic Hard 3, which costs the most on big battles);
line-of-sight tests per update 6, 9 and 12 (Classic 6, 6 and 12).

### Objectives

Classic draws a bot's post from every post its team cannot spawn at,
weighted only by the map's authored values, so a bot is sent to the nearest
one about one time in three, typically 160 to 240 m away when the nearest is
90 to 150 m. Enhanced keeps the same candidates and the authored values (a
post the map weighs 0 is still never chosen) and multiplies in:

| Factor | Value |
| --- | --- |
| Nearness | (nearest candidate's distance / this one's)^2; distances under 20 m count as 20 m. Measured from the bot when it is alive, else from the nearest post its team spawns at |
| Neutral post | x1.5 |
| Standing destructible enemy objective (Hoth's generator, Geonosis' ships, Endor's bunker) | x2 |
| Crowding | past 6 teammates already sent there, weight / (1 + (n - 6) / 6) |
| A friendly post whose capture meter runs against its owner, and for 8 s after | a candidate at x2 (authored value, or 100 when it has none) |

With nothing worth drawing it falls back to Classic's uniform pick. The
patrol's 5 s check ("is my post now a spawn point?") keeps a defender on a
post still under attack.

A post under attack also calls in up to 4 of its owner's (and friends') bots
within 80 m, nearest first: on foot, not human, not fighting, not seated, not
standing in another post's capture region. The same post recruits again
after 10 s. Classic only answers once the post has gone neutral (its 60 m,
up to 5 recapture response, which Enhanced keeps). The capture meter is what
the player sees on the HUD; no hidden enemy count is read.

### Spawning

A bot spawns at the post its team can spawn at that is nearest its
objective. Classic picks one at random. The player's own spawn choice is
unchanged.

### Objective focus

On the objective run (walking to the post, or the patrol taking a post that
is not yet the team's), the trooper, scout and repair agents:

- stop to fight a seen enemy only within 20 m (Classic: anyone the sight scan
  picks up, out to about 60 m). Farther ones they shoot at as they walk:
  Classic's objective agents already track, aim and fire on the move, so
  nothing new was needed for that;
- check a heard enemy only within 12 m (Classic hears moving and firing
  enemies through walls, out to its weapon's range, and turns to look);
- answer a shot or a hit only from within 25 m, and otherwise turn toward the
  shooter for 2.5 s so their own scan can find him;
- inside the capture region, stay: they fight from where they stand.

Off the run (defending a friendly post, sniping, laying mines) Classic's
policies apply, except that a heard enemy checked in the last 10 s is not
checked again (the Mos Eisley look-round). Grenades are always dodged.

### Aim

Classic filters each bot's gun through the side's skill curves: an
acceleration limit and damping that make low skills slow to settle, extra
spread per shot (2.7 times a player's at skill 4) and extra sniper error.
Enhanced replaces that filter with one model, the same for every bot:

- **Reaction:** a new target is not fired at until a reaction time has passed:
  log-normal around the tier's median, never under its floor. A target lost
  for under 1.5 s and found again needs no new reaction.
- **Error:** the gun points off the body's aim by a wandering angle,
  correlated over 0.25 s, of size `sigma25 x (0.7 + 0.3 x clamp(d / 25, 0, 4))
  x (1 + 0.5 x own speed fraction)`, halved through a zoomed sniper scope.
  At acquisition a bias of 2.5 sigma is added, fading over the convergence
  time.
- **Slew:** the gun turns no faster than the tier's rate.
- **Weapons:** the player's rules. No extra spread per shot (curve B) or
  sniper error (curve S), and the player's spread floor (a player's kick
  starts at, and recovers to, minus SpreadThreshold; a Classic bot's never
  goes below zero), so a bot's first shots are as tight as a player's.
- Each bot gets a stable +-10% skill variation for the match.

| Tier | Reaction median / floor | Convergence | sigma25 | Slew |
| --- | --- | --- | --- | --- |
| Easy | 450 / 300 ms | 650 ms | 3.0 deg | 120 deg/s |
| Normal | 330 / 220 ms | 480 ms | 1.6 deg | 160 deg/s |
| Hard | 250 / 170 ms | 350 ms | 1.0 deg | 200 deg/s |

Reaction, convergence and slew are the design's initial values. sigma25 was
tuned by measurement (next section).

## Tuning

The initial sigma25 (1.2, 0.8 and 0.5 deg for Easy, Normal and Hard) proved
too deadly once bots had the player's spread rules: Normal bots hit with 30%
of their rifle rounds, against 18% for Classic Medium's enemies, and killed
half again as often. The goal chosen instead: **Enhanced Normal's bots, on
both teams, shoot about as well
as Classic Medium's enemies**, so switching to Enhanced does not make the
enemy deadlier than the player knows it, it makes his allies as good. Hard is
a step past Classic Hard's enemies, Easy about Classic Medium's allies.
Sweeping the error scale over four maps (Theed, Mos Eisley, Hoth, Kashyyyk
Docks; 3 minutes each):

| Error scale (x initial sigma25) | Normal rifle hits/shot (allies / enemies) | Sniper |
| --- | --- | --- |
| 1 | 31% / 28% | 22% / 16% |
| 1.75 | 22% / 17% | 6% / 5% |
| 2.5 | 18% / 13% | 6% / 4% |

Scaling hurt snipers far more than rifles (their shots are long), so a zoomed
scope now halves the error, as a player's scope shrinks his. A second sweep
per tier at twice the initial error, over those maps plus Cloud City and Yavin
Temple, gave Normal 20% / 16% rifle hits, Hard 25% / 21% and Easy 18% / 13%;
Easy was widened a little more for the table above. These are starting
points for play-testing, all in `AIM_PROFILES`, `AIM_MODEL`,
`ENHANCED_OBJECTIVES` and `ENHANCED_FOCUS`.

## Measuring

```
BFE_CONTENT_DIR=<game-data> node tools/ai-focus-probe.mjs --ai enhanced --tier medium \
  --maps nab2,tat2,bes2,yav1,kas2,hot1,geo1,rhn1 --seeds 1,2 --minutes 5 --endless
```

Whole headless battles, the player idle at his post, every bot sampled once a
game second. It prints, per side: where bots spend their time, the lives that
ever stood in their objective's capture region, captures per minute, hits per
shot by weapon family (damage events on enemies per round, on-foot bots), and
interrupts per minute by cause. `tools/ai-stuck-probe.mjs --ai enhanced`
counts bots that stop moving.

Results over Theed, Mos Eisley, Cloud City, Yavin Temple, Kashyyyk Docks,
Hoth, Geonosis and Rhen Var Harbor, seeds 1 and 2, 5 minutes each (80
battle-minutes per row), with `--endless`. Each pair is the player's team /
the other team:

| AI, tier | Lives reaching their objective | Captures a minute | Neutralized a minute | Time on objective | Time in combat | Rifle hits/shot | Kills a minute |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Classic Easy | 2% / 4% | 0.11 / 0.11 | 0.11 | 44% / 48% | 32% / 28% | 13% / 11% | 17 / 15 |
| Enhanced Easy | 13% / 18% | 0.34 / 0.29 | 0.53 | 53% / 54% | 26% / 24% | 12% / 11% | 19 / 17 |
| Classic Medium | 4% / 4% | 0.15 / 0.12 | 0.19 | 45% / 48% | 29% / 26% | 11% / 18% | 15 / 17 |
| Enhanced Normal | 13% / 17% | 0.42 / 0.44 | 0.75 | 59% / 58% | 20% / 19% | 19% / 16% | 23 / 21 |
| Classic Hard | 2% / 7% | 0.07 / 0.20 | 0.15 | 45% / 50% | 32% / 27% | 7% / 20% | 13 / 22 |
| Enhanced Hard | 15% / 16% | 0.36 / 0.40 | 0.69 | 62% / 62% | 18% / 17% | 24% / 21% | 23 / 23 |

"Time on objective" includes sniping, mining and repair jobs; the rest is
vehicles, boarding and recovery. Posts change hands about three times as
often, and the player's allies shoot as well as the enemy. Bots also kill
30 to 40% more often at the same hit rate, because more of them meet at the
same posts, so tickets run down sooner: a point for play-testing.

Stuck bots (`tools/ai-stuck-probe.mjs`, seed 1, 5 minutes, Normal against
Classic Medium), stuck bot-seconds a minute: Theed 1.4 against 0, Mos Eisley
18.6 against 32.6, Cloud City 24.4 against 46, Hoth 6.4 against 15. More bots
walk, so stuck-recovery starts more often (7 to 12 a minute against 3 to 10),
but fewer stay stuck.

## Not yet

The planned Enhanced design goes further. Still to build:

- a team commander that allocates squads between attack, threatened posts
  and a reserve by travel time and ticket value, instead of each bot drawing
  its own post;
- perception limited to what a bot could see or hear (no hearing through
  walls, rain and fog), threat-aware routes, cover and flanking, and small
  fireteams;
- Enhanced vehicle and turret behavior, and Enhanced killable heroes;
- an Expert tier, and calibration against human players' win rates.
