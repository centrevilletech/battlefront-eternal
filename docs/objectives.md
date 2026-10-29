# Destructible objectives

Some command posts are objectives you destroy rather than capture. In the
Steam BF1 data four classes do this (base `commandpost` with a `MaxHealth`),
on three maps:

| Map | Post | Class | Owner | MaxHealth | Wreck | Death | Kill region |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Hoth (hot1) | `shield` (CP2) | `hoth_bldg_shieldgenerator` | Alliance | 50000 | `hoth_bldg_shieldgenerator_dst` | `imp_walk_atst_exp` | `CP2_Death` |
| Geonosis (geo1) | `cp3`, `cp5`, `cp7` | `geo_bldg_technounion_cp` (the Techno Union ships) | CIS | 20000 | none; the static first chunk is the hulk | `geo_bldg_technounion_exp`, then the `destroy` clip | none |
| Geonosis (geo1) | `cp2` | `rep_bldg_forwardcenter` (class in `SIDE/rep.lvl`) | Republic | 30000 | `rep_bldg_forwardcenter_dest` | none (it names only `ExplosionDestruct`) | none |
| Endor (end1) | `cp1` | `end_bldg_bunker_controlpanel` | Empire | 4000 | `end_bldg_bunker_controlpanel_dest` | `imp_walk_atst_exp` | none |

No other map, the add-on maps included, places one (`tools/audit-features.py`
lists every post class with its `maxHealth`).

What follows is how the original game treats them, and where the port does
the same.

## The post object

The `commandpost` class is an animated building carrying a command post (kept
in the global post list). Its class keys go through the building readers and
the post's own; its instance keys include `KillRegion` (76ed2f81),
`CaptureRegion` and `ControlRegion`. Key name hashes: `IdleAnimation`
297bc6eb, `DeathAnimation` 5fac58b5, `DeathEffect` f138015c
(`"<effect> <hardpoint>"`, up to 5), `AmbientSound` 65a920ba, and a flag
3e4e9ba0 (`DeathAnimationExplosions` in the port) that sets off random
explosions while the death clip plays. Keys no reader of this class takes, so
the game ignores them: the Techno Union's hulk key 0403d917,
`ExplosionOffset`, and the forward center's `ExplosionDestruct`. The Techno
Union's later damage stages are written with misspelled keys (0e615de7 and so
on) and have no effect either.

The port: `tools/extract.py` treats such a class as a live object
(`destructible_post`, `live_class`), so it is not baked into the world or
its collision; `write_props` writes its building (`base: "commandpost"`,
`post: <instance name>`, its wreck, explosion, damage effects and chunks,
and `objective_post`'s animation and `fx.dying`), and `command_posts` gives
its post `maxHealth` and `killRegion`. `src/props.js` `ObjectivePost` is
the building, `src/commandposts.js` the post (`attachBuildings` pairs them
by name).

## Damage

- Any team's rounds and blasts damage it: the owner's own by the friendly
  fire factor, a neutral post fully. Capture and ownership don't gate it. It
  takes damage as a building (the weapons' building scales).
- Its damage stages burn between their start and stop shares of MaxHealth,
  as a hull's do (`src/vehicle-effects.js`): the shield generator sparks
  below 85 %, smokes below 70 % and burns below 55 %.
- It never comes back by itself. The building's rebuild would reactivate the
  post, but nothing in the stock game calls it, so the port lets no repair
  weapon work on the wreck (`ObjectivePost.healthType`); a standing one takes
  `BuildingHealth` like any building.

## Destroyed

1. The building's death. Without a `DeathAnimation` its
   `ExplosionName` goes off and it becomes its wreck at once. With one (the
   Techno Union ship) the explosion goes off at once, the clip plays (226
   frames at 30 a second, 7.5 s), with the flag set an `explosion`,
   `med_explosion` or `small_explosion` effect every 0.4 s at a node picked
   at random, and as it ends each `DeathEffect`
   (`super_explosion` at `hp_explode1` to `5`) goes off and the ship is
   gone, its static first chunk (`geo_bldg_technounion_hulk`)
   left in its place for good and the rest flying. The port draws the
   dying from the moment a host sees the building destroyed
   (`ObjectivePost.show`, `present`; the walker dying rules in
   `src/vehicle-effects.js`), so the state that travels stays the plain
   building state and a late joiner sees the wreck.
2. The post goes: out of the post update, so no capture logic and
   no more Info line; the bots sent there choose again; "<team>
   lost <post>" is announced, which also plays the owner's
   `VO_<listener>_<owner>Lost` line and the class's `LostSound`; whoever
   damaged it last earns a command post point unless he is the owner's
   friend; its hologram goes off; and its `KillRegion` goes off. Its team
   is left as it was.
3. The kill region, once: 15 effects at points drawn
   uniformly through the region's box, each `explosion`, `med_explosion`
   or `small_explosion` at random; then up to 64 soldiers and vehicles whose
   origin is inside are killed outright, no one credited. Single player or
   the host only. The port: `src/objectives.js` `killRegion`, from
   `src/game.js` `objectiveDestroyed`.
4. From then on the post iterators skip it: it is no
   spawn point, no AI objective, adds no `ValueBleed` and counts neither as
   held nor as neutral, so a team left with nothing else starts its 20 s
   no-post countdown. The port marks the post `active: false`, which every
   iterator already honoured for the command vehicles' posts, and the maps
   no longer draw it.

## The maps

A post is drawn with its class's `MapTexture` at its `MapScale`: the
objectives' `hud_dest_icon` at 3.0 against the capture posts' flag at 2.5
(`src/hud.js` `postIcon`).

## Voice-over

A post's instance names its lines `VO_<listener>_<actor><event>`, with the
events Capture, Lost, InDispute, Saved and Info (each stored by listener
side, actor side and event). The objectives set Lost and Info:

| Post | Info (listener's side) | Lost |
| --- | --- | --- |
| Hoth `shield` | Rebels `all_alldefend_shieldgenerator`, Empire `imp_impdestroy_shieldgenerator` | `all_alllost_shieldgenerator`, `imp_impdestroyed_shieldgenerator` |
| Geonosis `cp3`, `cp5`, `cp7` | Republic `rep_repdestroy_technounion`, CIS `cis_cisdefend_technounion` | `rep_cislost_technounion`, `cis_repdestroyed_technounion` |
| Endor `cp1` | Rebels `all_alldestroy_shieldbunker` | `all_implost_shieldbunker` |

The Info line: every frame, while the post stands, its update asks for the
owner's Info line for the local player's side, not while the
game is over. The line's stream properties pace it: these say
`PlayInterval 90`, so it is heard as the battle starts and every 90 s
after. The port does the same in `src/audio/vo.js` (`objectives`), once the
player has a body; `tools/zero_sound.py` exports the Info keys
(`CP_EVENTS`). No line or message is tied to the post taking damage: the
original game has no "under attack" report.

## The AI and the reticle

The original's target check takes any living object with a finite
MaxHealth whose owner the viewer is hostile to, and the post answers yes.
The port registers each objective's building in the units
registry as a building (classifier 9, category 4: `ObjectiveTarget`), so
bots see and shoot it with the weapons that may hit buildings, the
player's reticle turns red on it and the enemy health bar shows its health
(with its `HealthTexture`, the forward center's `HUD_rep_forward_comm_icon`).
The bots go there as they go to any post, by its `Value_ATK`/`Value_DEF`
weights; there is no separate "destroy" goal in the original. The post
object is the building, so a bot has arrived within the building's
collision radius plus its own and 5 m (`Post.radius`), not at its middle.

Left at the original's rules, the bots almost never brought one down:
headless 15-minute battles on Hoth and Geonosis ended with the generator and
the ships at full health. These changes make the bots play for the
objectives (the draw bias, the shot priority and the repair rule are the
port's own; the other two fix port bugs):

- **Drawn more often, on the front:** a standing enemy objective weighs 3
  times its authored value in the objective draw (`OBJECTIVE_ATTACK_BIAS`,
  `src/ai/classic/goals.js`) while it is on the team's front: the
  candidate nearest one of the posts the team spawns at (a command
  walker's post included). Further back it keeps its authored value, so
  the posts in front of it are drawn as the original draws them.
- **Shot first:** to a bot whose objective it is, the building ranks as
  its top match-up (priority 20, an armed building's, rather than a
  building's 1) in the sight scan and the target tracker
  (`targetPriority`, `src/ai/classic/perception.js`), so it fires on the
  generator rather than on every soldier along the way.
- **Hoth's probe droid drop weighs nothing:** the post records are joined
  to the runtime posts by the runtime name, and the unnamed post (the probe
  droid drop, `#5` to the network) never found its record. It took the
  default weights, 100 for every side where it says 0, and most of the
  Empire's draws, its AT-AT drivers' included, sent them to a ravine they
  could not reach. It now takes the record at its place
  (`attachPostAI`, by the post's world name).
- **An AT-AT keeps its pilot:** after a gunner left (an idle gunner leaves
  after 10 s on Hoth), the seat promotion moved the pilot into the gun seat
  and the walker stood driverless. Only a bot in a later seat moves up
  (`Vehicles.promote`).
- **Not repaired by bots:** no bot takes an objective as a repair job
  (`src/ai/services.js`). Four pilots' fusion cutters (60 health a tick)
  restored the generator to full faster than the Empire's infantry hurt
  it. A player's fusion cutter still repairs one.

With all of them, seeded 15-minute Hoth battles (`SEED=1..3
tools/objective-probe.mjs hot1 54000`) destroy the generator in some runs,
mostly by infantry: the AT-ATs close to 200 to 300 m, but the ridge in
front of the base, the head guns' narrow arc and the chin guns' short AI
range still keep them from shooting it much.

Endor needs the front rule. The bunker's control panel stands inside the
bunker, and only the door corridor sees it; the Empire spawns at cp1 in
front of that door and holds cp6 and cp4 on the way there, 310 m and 200 m
out. Drawn 3 times over everywhere, the bunker took 60 % of the Rebels'
draws, so most of them ran past cp6 and cp4, which then were rarely
taken; the line stalled there, 250 to 400 m short of the bunker, and the
Rebels bled tickets. With the front rule the bunker is drawn at its
authored weight until cp4 falls. In headless 15-minute battles that turned
Rebel losses on Endor into wins, and left Hoth (where the AT-ATs carry the
Empire's spawns toward the generator) and Geonosis unchanged within the
noise.

The bots' speeder bikes aim correctly (docs/ai-vehicles.md, "The hover's own
aim"; they had ridden aimed 5 degrees down and shot the ground). Both sides
ride them, and most of Endor's bikes stand at the Empire's posts.

**The live assault on Endor.** In the headless battles a Rebel rarely
reaches the bunker while the Empire still has reinforcements. The Rebels sent
to the bunker spend almost all their time more than 300 m from it, most of
the rest 220 to 300 m out, and none comes within 99 m; they die there, around
cp6 and short of cp4, mostly to the Empire's infantry, the AT-STs and its
bikes. The Empire spawns about a third of its troops at each of cp6, cp4 and
cp1, uniformly among the posts it may spawn at, as the original does. So
cp1's own defenders at the door are never reached: the line holds at cp6 and
cp4. The pieces on the way are the original's: the objective draw by the
authored weights (the port's front-rule bias does not apply until cp4
falls); the bikes' stand-and-shoot pattern 2 under `SetDenseEnvironment`;
and, for a Rebel who does arrive, the patrol's check 5 s on (the post's
CaptureRegion is null, so it asks for another objective). The retail world
gives cp1 no `CaptureRegion` and `Radius 0.0`, so the original makes it no
region either. Inside, only the soldier's crack grenade (3 of them, 400
each, 8 to 24 m) may target a building among the Rebels' weapons. Nothing
here is a port divergence; the bunker falls in these battles only once the
Empire's spawn at cp1 has stopped.

## Checks

- `tests/objectives.test.js`: damage, no regeneration or rebuild, the death
  animation's span, the post's loss (once, inactive, team kept, no bleed,
  no spawns, the no-post countdown), the kill region, the target and the
  Info line.
- `tools/objective-probe.mjs <map> [ticks] [--destroy]`: a headless battle
  on the prepared data, with the objectives' health every 30 s, how many
  bots have one as their sight candidate, how many are sent to it and how
  near they are, and who damaged them; `--destroy` also puts three bots in
  the kill region first.
- `tests/objectives-browser.mjs`: the page over the world worker (Geonosis
  ship standing, dying, gone; the map; Hoth's wreck), with screenshots.

Not yet determined: when `AmbientSound` (the shield generator's
`hoth_shield_amb`, the control panel's `control_panel_b`) starts, and what
the HUD does with `HealthTexture` beyond the enemy health bar.
