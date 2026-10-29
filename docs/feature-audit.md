# Feature audit: the retail game against the engine

What the 2004 game's data gives each map and class, and what this engine
does with it. The raw inventory comes from `tools/audit-features.py`
(rerun it after any extractor or engine change; see "Rerunning" at the end).
The reading below is ranked by how soon a player notices the gap. Status
words: **missing** (nothing in the engine or the prepared data),
**partial** (prepared or implemented in part), **implemented**.

Evidence conventions: archive paths are under
`GameData/Data/_LVL_PC`; "class" means an `entc`/`wpnc`/`ordc` chunk; an
instance is a `wrld` `inst` record; property names are the FNV-1a
resolutions the tools know (unknown keys stay as hex).

## The top 20 gaps

| # | Gap | Where it lives in the retail data | Maps | Engine status | What it would take |
| --- | --- | --- | --- | --- | --- |
| 1 | **Bespin Platforms' eight anti-aircraft turrets on the landing platforms** | `BES/bes1.lvl`: class `bes_bldg_AAA_turret` (base `armedbuilding`, geometry `kam_prop_antiaircraft`); 8 unnamed instances of class `bes_bldg_aaa_turret` (lower case), Team 0 | bes1 | **implemented**: `tools/extract.py` `extract_map` gives every placed record and instance its class's own spelling before anything looks it up (the original matches without regard to case), so `write_props`, `command_posts`, the foliage and the bake all resolve, and bes1 prepares the 8 `bes_bldg_AAA_turret` instances with `kam_prop_antiaircraft`. The cannon's two fire points (`hp_fire1`, `hp_fire2`) take turns (`Turret.muzzle`) and its flak bursts at the end of its flight (`ExplosionExpire`, src/ordnance.js). Its retail `LockOnRange` is 0.0 (only `LockTime 0.5` and `AutoAimSize 3`), so there is no lock to show | Nothing left for the turret; the other maps' case-different props resolve to their classes too |
| 2 | **The Hoth snowspeeder's harpoon and tow cable** | `SIDE/all.lvl`: seat `TURRET` of `all_fly_snowspeeder` (TurretNodeName `towcablegun`, YawLimits `80 280`, weapon 1 `all_weap_fly_snowspeeder_reargun` base `towcableweapon`, 20 rounds, ShotDelay 3, LockOnRange 70, LockTime 0.4, TargetVehicle only; weapon 2 the cannon); ordnance `all_weap_fly_snowspeeder_reargun_ord` base `towcable`, Velocity 250, LifeSpan 2.5, CableLength 60 (key `d9b746b2`), attach sound `all_weap_fly_snowspeeder_towcable_attach`, release sound on the weapon | hot1 | **implemented**: docs/vehicles.md "The snowspeeder's harpoon and tow cable". The shot leaves `towcablegun`; `CableLength` is exported; no launcher lock diamond is drawn for it (docs/weapon-targeting.md); only a leg hit holds (a bot's harpoon is re-aimed at a leg); the cable is plain state (the world's tow records) that the page draws, wrapping the legs a point per quarter circuit; the attach and release sounds play; `tools/tow-cable-probe.mjs` runs headless Hoth battles and reports each harpoon's hold and each cable's end. The bots fly the original's ellipse (pattern 15) and the gun holds its fire while the cable holds (docs/ai-vehicles.md "Pattern 15: the tow-cable run"); bot crews trip AT-ATs | Nothing left; the "Hoth" section below records the retail data |
| 3 | **The Clone Wars era of every two-era map** (and the GCW era of none) | `mission.lvl`: 44 scripts, two or three per map (`bes1a`/`bes1r`, `kas1c`/`kas1i`, ...); the pads' `ClassRepATK`/`ClassCISDEF` lists; `SIDE/rep.lvl`, `cis.lvl` vehicle classes | bes1, bes2, kas1, kas2, nab1, nab2, rhn1, rhn2, tat1, tat2, yav1, yav2 | **implemented**: `tools/extract.py` `extract_map` exports every battle fought in the map's world (`map_missions`: each era's and the historical `_h` versions, marked) into `map.json` `missions`, each with its own teams, spawn screen and vehicle pack (`vehicles/<mission>/`), its sound sub-level indexed by name and its script in `ai-mission.json`; `src/mission.js` picks the battle from `mission=`, `era=` or `side=` (Instant Action's era picker, `?map=&side=` links, server rotations, the desktop host) in the page, the world worker and on servers alike, and the network fingerprint covers the picked battle. The campaign's Theed and Naboo Plains are fought in worlds of their own (`NAB/nab2_h.lvl`, `nab1_h.lvl`), recorded but not playable until those worlds are extracted | The historical versions wait for a campaign mode |
| 4 | **Local sides**: Tusken Raiders and Jawas, Gungans, Wookiees, Ewoks, Geonosians, Gamorreans | `SIDE/des.lvl` (`tat_inf_tuskenraider`, `tat_inf_tuskenhunter`, `tat_inf_jawa`), `gun.lvl`, `wok.lvl`, `ewk.lvl`, `geo.lvl`, the add-on's `gam.lvl`; missions set team 3 (`tat1i`: 5 raiders and 2 hunters, SetUnitCount 3 = 14, `SetTeamAsEnemy`/`Friend`); `tat_bldg_tuskanspawn` posts ts1/ts4/ts5 (Team 3); `AllyPath`s on Kashyyyk, Naboo, Endor, Geonosis and Mos Eisley posts | tat1, tat2, tat3, nab1, end1, kas1, kas2, geo1 | **implemented**: docs/ai-population.md "Locals". `tools/extract.py` takes a side's units by the classes its archive defines; `src/game.js` loads every mission team's classes and gives the registry the mission's relations, factions and capture teams; `src/battle.js` keeps the locals' reinforcements; `src/commandposts.js` counts them for their capture team, hides posts without a `MapTexture` and holograms without a `HoloOdf`; the HUD draws the Locals faction yellow | Nothing left for the locals; Hoth's probe droid (team 4) never deploys in the original either |
| 5 | **Destructible objective posts**: Hoth's shield generator, Geonosis' Techno Union ships and the Republic forward command center, Endor's bunker control panel | `hoth_bldg_shieldgenerator` (commandpost, MaxHealth 50000, wreck `_dst`, instance `shield` with `KillRegion CP2_Death`, damage-smoke stages at 85/70/55/40/25 %); `geo_bldg_technounion_cp` (MaxHealth 20000, `AnimationName` idle/destroy, instances cp3/cp5/cp7); `rep_bldg_forwardcenter` (MaxHealth 30000, instance cp2, class in `SIDE/rep.lvl`); `end_bldg_bunker_controlpanel` (MaxHealth 4000, wreck `_dest`, instance cp1) | hot1, geo1, end1 | **implemented**: docs/objectives.md. The four classes are live buildings (`tools/extract.py` `destructible_post`, `objective_post`; `src/props.js` `ObjectivePost`) paired with their posts (`src/commandposts.js` `attachBuildings`): any team's fire damages them, they never regenerate, destroyed they become their wreck (the Techno Union ship plays its `destroy` clip with its random and final explosions, leaving its static hulk), and the post goes inactive as in the original (lost, no spawns or bleed, a post point for the destroyer, the kill region's 15 blasts and kills). Their map icon is `hud_dest_icon`, bots and the reticle target them (`src/objectives.js`), and the Info voice-over ("destroy the shield generator") plays every 90 s. Geonosis cp2's class comes from `SIDE/rep.lvl`, which the side lookup reads (`rep_bldg_forwardcenter`) | Nothing left; when `AmbientSound` starts and what else reads `HealthTexture` are not known |
| 6 | **The Geonosian LR1K sonic cannon turrets' round** | `GEO/geo1.lvl`: `geo_bldg_geoturret` (10 instances, RespawnTime 30, MaxHealth 1250), weapon `geo_weap_bldg_geoturret` base `weapon` with ordnance base `fatray` | geo1 | **implemented**: `src/ordnance.js` `fatray`: a thick (1.5 m) slow ball at Velocity for LifeSpan (100 m/s, 0.45 s: 45 m) that strikes every soldier it passes once, stops at the first wall or hull, and whose `TrailEffect` (`geo_blunderbuss_blast`) is its look; the retail class sets no explosion | Nothing left |
| 7 | **Ewok log traps** | `END/end1.lvl`: `end_weap_treesmash` base `trap` (2 instances, Team 0): TriggerAnimation `trigger`, BuildAnimation `reset`, BuildPoint `hp_active`, BuildModelOdf `com_prop_buildzone`, UnbuiltHoloOdf `com_holo_trap`, TriggerTeam `imp`, ResetTeam `all`, MinEnemyRadius 4, DeathTime 0.666, DeathEffect `explosion` at DeathEffectOffset `0 7.5 0`, BuiltCollision 1, MaxHealth 1000 (key names matched to their hashes) | end1 | **implemented**: docs/live-objects.md "Ewok log traps". `tools/extract.py` and `tools/zero_trap.py` make `trap` live; `src/traps.js` runs it as the original's trap does: it starts unbuilt (pad and hologram out) and belongs to the Alliance; a fusion cutter's BuildingBuild builds it (Rebel pilots and the Ewoks' repair tool, bots included); built, it is hauled up, then a vehicle of the Empire larger than 4 m within 3 m (per axis, sphere to box) of its middle sets it off, as does shooting it to 0; 0.666 s into the swing every vehicle that large in that box, of any side, dies uncredited and is gone at once, the `explosion` effect where the logs meet; once the swing has played it is unbuilt again. Its phase travels as world schema 10. `tools/trap-probe.mjs`, `tests/traps.test.js`, `tests/log-trap-browser.mjs`. No other map, stock or add-on, places a `trap` | No collision on the logs (the class's capsules hang from the swinging bones); its sphere for blasts is the pad's, an inference; the pad's own collision box is not built; no sound (the class names none) |
| 8 | **Command vehicles as mobile spawn points and bleed sources** | `imp_walk_atat`, `rep_walk_atte`, `cis_hover_mtt`, `all_hover_hovernaut` (bases `commandwalker`/`commandhover`): `SpawnPointCount 6`, six spawn offsets (key `a067d145`), `ValueBleed 10`, the AT-AT's `FinalExplosion` and `ExplosionCritical` | hot1, rhn1, geo1 (and the Clone Wars eras) | **implemented**: a living command vehicle is a post (`src/mobile-posts.js`, `CommandPosts.all()`; docs/human-spawn.md, "Command vehicles"). Its team spawns at its class spawn points as it stands (the spawn screen, every human's spawn on the server, the Classic spawn manager), its `ValueBleed` counts in `src/battle.js`, its weights make it an objective, and its death loses the post. The staged death is implemented for every walker: its death animation with its `DeathAnimationExplosion` effects, then `FinalExplosion` at `FinalExplosionOffset` (docs/vehicles.md, "Dying") | Nothing left |
| 9 | **Vehicle damage smoke and sparks by health** | Every vehicle class: `DamageStartPercent`/`DamageStopPercent`/`DamageEffect`/`DamageAttachPoint` stages (`vehiclespark` 75 to 30 %, `vehiclesmoke`, `vehicleflame` under 25 %), also on the shield generator and Techno Union ships | all | **implemented** for vehicles and placed turrets: `tools/zero_vehicle.py` `vehicle_fx` names the keys and exports every class's records as `fx.damage` (a turret's through `tools/extract.py` `write_props`); `src/vehicle-effects.js` burns each at its node while the health is inside its band, at its scale and inherited velocity, on the page in every mode (docs/vehicles.md, "Effects and sounds"; docs/live-objects.md, "Turrets"). The shield generator's damage stages burn as its health falls (gap 5); the Techno Union ship's later stages are written with misspelled keys the original never reads | Nothing left |
| 10 | **Ambient birds and fish** | `SetBirdType`, `SetNumBirdTypes`, `SetBirdFlockMinHeight` (17 scripts), `SetFishType`/`SetNumFishTypes` (Kamino, Naboo, Kashyyyk: 6 scripts) | 17 maps' scripts | **missing**: no reader in `src/` or `tools/` | Flocks that scatter from fire; fish under the water surface |
| 11 | **Doors, elevators and animated props** | `bes2_carbon_forklift` (base `animatedprop`, `basepose`/`activate` animations, activation hard point `hp_activate`); `geo_bldg_technounion_cp` idle animation; the Hoth hangar lift (`HOTH_prop_hangar_lift_platform` is a static prop in the data) | bes2, geo1, tat1, tat3 | **implemented**: `animatedprop` and `door` are live bases (`tools/extract.py` `LIVE_BASES`, `src/props.js` `AnimatedProp`; docs/live-objects.md "Animated props and the sarlacc"): the sarlacc's tentacles, Jabba, the rancor, the torture droid, the trapdoor and the twelve cell doors. The forklift's trigger only fires for a body at the bottom of its pit, which is Cloud City's death region; bodies stay in death regions, so it runs. The Hoth hangar lift is a static prop in the retail data too | Nothing left |
| 12 | **Vehicle cockpits in first person** | `FirstPerson` on every hull and turret seat (`all_1st_cockpit_snowspeeder`, `imp_1st_cockpit_atat`, `com_1st_cockpit_viewfinder` on the AAA turret, `com_1st_scope_universal` on the Geonosian and Yavin turrets), `CockpitTension` | all | **missing**: `docs/vehicles.md` records that the cockpit model is not imported; the hull is hidden instead | Import the `_1st_cockpit_*` models from the side archives and draw them over the view |
| 13 | **Walker and vehicle water damage** | `WaterDamageInterval`/`WaterDamageAmount` (1 s, 10) on every walker (AT-AT, AT-TE, spider walker, the AT-STs) and the droideka (`UprightWaterDamageHeight` 1.5, `BallWaterDamageHeight` 0.5); no hover or flyer has them | walkers: kas2, yav1 (the maps with water and walkers); droidekas: kam1, kas1, kas2, nab2, yav1, tat3 | **implemented**: `src/vehicle-water.js`, from the walker and droideka updates; walkers wade on the ground under the water and deep water wears them down at the amount per update (docs/vehicles.md "Water") | Nothing left |
| 14 | **Godray clusters, dust and rumble volumes** | `end_prop_godraycluster` (135 instances, base `godray`), `yav_prop_godraycluster`; `dusteffect` props (hot1 67, tat1 66, geo1 23, kam1 22, rhn2 13, yav1 23, yav2 9); `hot_rum_caves` (base `rumbleeffect`, 3) | end1, yav1, hot1, tat1, geo1, kam1, rhn2, yav2 | **partial**, all cosmetic: dust is **implemented** (`tools/zero_atmosphere.py`, `src/atmosphere.js`); the godrays and the cave rumble (camera shake and `hoth_rumble`; no damage key) are **missing** | Light shafts as billboards, dust as drifting particle boxes (`MinSize`/`MaxSize`/`NumParticles`/`MaxDistance`/`Texture`), the cave rumble as camera shake and sound |
| 15 | **The Wookiee smuggler's time bomb** | `all_weap_inf_remote_charge_dispenser` (dispenser, MaxItems 3) throwing `all_weap_inf_remote_charge_dispenser_ord` (base `mine`, LifeSpan 5, timer sound `all_weap_inf_timebomb_timer`); on `all_inf_smuggler` and `all_inf_smugglersnow` slot 3 | all GCW maps | **implemented**: the bomb's `LifeSpan` ending sets off its `ExplosionExpire` with the thrower's credit (`src/items.js` `expire`), shown to every client as any explosion; it ticks (`TickSound`, falling in pitch) in single player and online (`src/audio/scene.js`); `tests/time-bomb.test.js`, `tests/audio-charge-ticks.test.js` | none |
| 16 | **Hero projectile deflection and the block pose** | `CanDeflect` on the lightsaber classes (`all_weap_lightsaber`, `_blue`, `imp_`, `rep_`, `cis_weap_lightsaber`) | every map with heroes | **partial**: `docs/ai-heroes.md`: melee block only; bolt reflection and the block pose and sounds are not implemented | As documented there |
| 17 | **Prop lights and attached effects** | `AttachOdf`/`AttachToHardPoint` lights on `building` props (`kam_bldg_bunker1`, `yav_bldg_greattemple`, `HOTH_prop_lights_hanging`), `AttachEffect` steam and fireflies (`bes1_bldg_tunnel`, `Kas1_fireflies`, `kas_prop_mist`, `kas1_bldg_bonfire`) | kam1, yav1, hot1, bes1, kas1 | **partial**: the attached effects are exported by `tools/zero_environment.py` (an effect no archive defines, like `kas1_fireflies`, attaches nothing) and `AttachOdf` geometry by `tools/extract.py`; the `AttachOdf` light classes are not exported as lights | Export the light classes to `src/local-lights.js` |
| 18 | **Ambient sound emitters** | `com_snd_amb_static` (base `soundambiencestatic`: kas1 66, kas2 68, nab2 36, yav1 20, tat1 1) and `com_snd_amb_streaming` (yav1) | kas1, kas2, nab2, yav1, tat1 | **implemented**: `tools/zero_sound.py` exports every placed object with a `Sound` or `SoundStream` property and the map's sound regions, and `src/audio/ambience.js` plays them as looping positioned sounds and region streams | Nothing left |
| 19 | **Low-reinforcement voice-over** | `SetLowReinforcementsVoiceOver` (23 scripts) | 23 scripts | **missing**: no reader (`SetBleedingVoiceOver` and `SetOutOfBoundsVoiceOver` are read) | Trigger the line at the threshold |
| 20 | **Walker pool sizes** | `AddWalkerType`/`ClearWalkers` (42 scripts) | all | **missing** as a reader; pads spawn walkers anyway | Only matters for mods that rely on the pool; low |

Also seen, lower on the list: the K-wing (`all_fly_kwing`, four turret seats) and the Lambda shuttle (`imp_fly_trooptrans`) exist as classes but no pad places them, so nothing is lost; `SetStayInTurrets`, `SetDefenderSnipeRange`, `SetAllowBlindJetJumps`, `SetAIVehicleNotifyRadius`, difficulty, bleed thresholds, death regions, fly heights, bonuses, heroes, spawn delay and reinforcement counts are all read (`src/ai/classic/goals.js`, `spawning.js`, `difficulty.js`, `src/battle.js`).

## Bespin Platforms: the anti-aircraft turrets

The turrets stand on the landing platforms beside the parked ships (the class
reuses Kamino's `kam_prop_antiaircraft` model and HUD icons: `HealthTexture
HUD_kam_turret_icon`).

Retail data (`BES/bes1.lvl`):

- Class `bes_bldg_AAA_turret`, base `armedbuilding`. `BODY`: MaxHealth 500,
  `DestroyedGeometryName antiaircraft_chunk1`, three `CHUNKSECTION`s, explosion
  `bes_bldg_AAA_turret_exp` (as `ExplosionDestruct` and `ExplosionCritical`,
  which the building reader does not read: it doesn't explode, its pieces
  fly), map icon `turret_icon`. `TURRET1`:
  `TurretNodeName aimer_y`, PitchLimits `-30 20`, YawLimits `-180 180`,
  `PitchTurnFactor 0`, MaxTurnSpeed and MaxPitchSpeed 3.8, EyePointOffset
  `0 6.2 -8.3`, TrackCenter `0 8 5`, TrackOffset `0 -3 10`, ThirdPersonFOV 65,
  `FirstPerson com_1st_cockpit_viewfinder`, `PilotPosition hp_main_gunner`,
  pilot animation `drive` (an enclosed seat), turret whir sounds. Weapon
  `kam_weap_bldg_AAA_turret_cannon` (base `launcher`, ordnance a `bolt`) on
  aimer `aimer_x` (AimerPitchLimits `-30 20`, AimerYawLimits `0 0`) with two
  recoiling barrels `barrel1`/`barrel2` firing from `hp_fire1`/`hp_fire2`.
- Eight instances, all unnamed, `Team 0`, `Layer 1`, class written
  `bes_bldg_aaa_turret` (lower case).
- `kam_prop_antiaircraft` and its chunks are in `bes1.lvl` (five references), so
  nothing has to come from Kamino.

In the engine, the eight turrets are prepared and manned like Kamino's
`com_weap_gunturret` (`src/props.js` builds them, `src/gunner.js` and
`src/seats.js` man them). The instances spell the class in lower case while
the class chunk does not, and `extract_map` spells every placed record by its
class's own spelling before any lookup, as the original's case-insensitive
lookup does. The same mismatch touches other maps' props (28 Hoth classes
(`HOTH_bldg_*`, `HOTH_prop_*`), 53 on Rhen Var Harbor, 94 on the Citadel, 5 on
Theed), which are static props the bake resolves by model name. The cannon
has no lock (retail `LockOnRange` 0.0), so it fires straight bolts.

Other maps' turret classes all resolve (same case in class and instance):
`hoth_bldg_hoth_turret` 12 and `hoth_bldg_dish_turret` 12, `geo_bldg_geoturret`
10, `yav_bldg_tower_turret` 7, `tat_bldg_barge_turret` 3, `com_weap_gunturret`
on eleven maps. `HOTH_prop_trench_turrets` (4) is a plain prop with vehicle
collision only: decorative in the original too.

## Hoth: the harpoon and tow cable

Retail data (`SIDE/all.lvl`, `HOT/hot1.lvl` mission `hot1i`):

- Seat `TURRET` of `all_fly_snowspeeder`: `PilotPosition hp_gunner`, animation
  `drivesnowspeedergunner` (an exposed rider, unshootable by key `2b60e217 0`),
  YawLimits `80 280` (he faces aft), PitchLimits `-15 25`, TurnRate 1.3,
  `TurretNodeName towcablegun`, `FirstPerson all_1st_cockpit_securitycam`,
  EyePointOffset `0 2 4`, FirstPersonFOV 50. Weapon section 1:
  `all_weap_fly_snowspeeder_reargun`, 20 rounds; section 2: the cannon.
- `all_weap_fly_snowspeeder_reargun`: base `towcableweapon`, Label "Turret
  Gunner", reticle `reticule_sniper`, RoundsPerClip 20, ShotDelay 3, ReloadTime
  3, LockOnRange 70, LockTime 0.4, AutoAimSize 1, TargetEnemy 1, TargetVehicle 1
  (nothing else), MinRange 0, OptimalRange 200, MaxRange 300, fire sound
  `all_weap_fly_snowspeeder_reargun_fire`, release sound
  `all_weap_fly_snowspeeder_towcable_release`.
- `all_weap_fly_snowspeeder_reargun_ord`: base `towcable`, LaserLength 2.5,
  LaserWidth 0.2, red bolt sprite, Velocity 250, LifeSpan 2.5, Gravity 1,
  Rebound 0, MaxDamage 50, `CableLength 60`, attach sound
  `all_weap_fly_snowspeeder_towcable_attach`.
- The model `all_fly_snowspeeder` has hard points `hp_pilot`, `hp_gunner`,
  `towcablegun`, `Lcannon`, `Rcannon`, `hp_damage_1..3` and no `hp_fire`.
- `imp_walk_atat` (base `commandwalker`): two `WalkerLegPair` groups
  (`front_legs`, `back_legs`), leg collision shapes `p_front_left_thigh`,
  `_shin`, feet, SoldierCollision and OrdnanceCollision on each leg piece.

In the engine (`src/vehicles.js`; docs/vehicles.md "The snowspeeder's harpoon
and tow cable"):

- The shot leaves the seat's `towcablegun` turret node, aimed aft, since the
  model has no `hp_fire`; `CableLength` is among the exported ordnance keys.
- `launchTow` holds only a leg of an enemy command walker (the AT-AT's leg
  shapes are in the data, and the original attaches to the legs); a hit on
  the body glances off, and a bot's harpoon is re-aimed at a leg.
- The cable is plain state on the speeder, which the world sends and the page
  draws; three complete circuits trip the walker.
- The attach and release sounds play (`emitSound`).
- The Classic AI flies the original's tow-cable run (pattern 15,
  docs/ai-vehicles.md "Pattern 15: the tow-cable run").
- `tests/vehicles.test.js` ("tow cable: ...") fires the harpoon from the rear
  seat of a synthetic speeder and checks that it holds a walker leg, not its
  body, with the attach sound.

## Inventory by area

### 1. Vehicle classes and seats

35 vehicle classes (`extract/audit/features.json` `classes.vehicles`): 13
flyers, 9 hovers, 6 walkers, 2 command walkers, 2 command hovers, 1 walker
droid, plus `bes_fly_cloudcar` (bes1), `tat_hover_skiff` (tat1),
`gun_walk_kaadu` (gun). Per class the inventory lists every section with its
weapons, aimers, barrels, fire points, turret node, pilot keys, limits and
camera. Engine coverage (`src/vehicles.js`, `src/seats.js`, `docs/formats.md`
"Vehicle seats"):

| Feature | Status | Note |
| --- | --- | --- |
| Hull and turret sections, per-seat aim, limits, cameras, 9-pose riders | implemented | `Vehicle.clampAim`, `poseRider`, `VehicleCamera` |
| Riders in the open struck and killed by rounds, their seat let go (hash `2b60e217`) | implemented | `src/rider-pose.js`, `Vehicle.riderShapes`, `src/boarding.js` `leaveKilled`; placed turrets' gunners too, blasts included (`Turret.riderShapes`, `SeatLease.blastable`) |
| Aimers, barrels taking turns, fire points, beams per barrel | implemented | `muzzleOf`, `aimSeats` |
| Flight model (take-off, landing, MinSpeed/MidSpeed, StrafeSpeed, ceilings) | implemented | `docs/vehicles.md` |
| Hover and walker movement, the droideka ball | implemented | `src/droideka.js`, walker code in `vehicles.js` |
| Tow cable | implemented | docs/vehicles.md |
| Cockpit models (`FirstPerson`) | missing | hull hidden instead |
| Damage smoke stages | implemented | #9 |
| Final explosion / critical explosion (AT-AT, AAT) | implemented (the staged death) | #8 |
| Water damage | implemented | #13 |
| Command vehicle spawn points | implemented | `src/mobile-posts.js` |
| Command vehicle bleed | implemented | `src/mobile-posts.js`, `src/battle.js` |
| Vehicle decay and respawn (`DecayTime`, `SpawnTime`) | implemented | `respawnTime` on pads |
| Passenger seats (gunship, skiff) | implemented | seats without weapons |
| Ejection | not in the 2004 data | no `Eject*` keys exist. E in flight bails out as the original's seat exit does (docs/vehicles.md "Boarding, leaving and seat controls") |
| Landing gear (flyers' `takeoff` clips) | implemented, past the original | the 2004 game poses a craft at its takeoff clip's first frame (gear down) for good; the port plays the clip by flight state: down landed, stowed in flight (docs/vehicles.md "Landing gear") |

### 2. Placed turrets, droids, posts and pads

Per map (`maps.<name>` in the inventory: `turrets`, `droids`, `posts`,
`pads`, `special`):

| Map | Turrets (retail / prepared) | Droids | Posts (retail / prepared) | Pads | Other placed behaviours |
| --- | --- | --- | --- | --- | --- |
| bes1 | 8 / 8 (`bes_bldg_AAA_turret`) | 30 / 30 | 7 / 7 | 28 | |
| bes2 | 5 / 5 | 14 / 14 | 6 / 6 | 0 | 1 animated prop (carbon forklift) |
| end1 | 3 / 3 | 19 / 19 | 6 / 6 (cp1 destructible) | 12 | 2 log traps, 135 godrays, 21 buildings |
| geo1 | 10 / 10 | 13 / 13 | 8 / 8 (cp2, cp3, cp5, cp7 destructible) | 10 | 23 dust |
| hot1 | 24 / 24 | 27 / 27 | 7 / 7 (shield destructible) | 16 | 67 dust, 3 rumble |
| kam1 | 16 / 16 | 25 / 25 | 7 / 7 | 0 | 22 dust (mist), 12 buildings with lights |
| kas1 | 0 | 25 / 25 | 6 / 6 | 4 | 66 sound emitters, fireflies, bonfire |
| kas2 | 16 / 16 | 24 / 24 | 7 / 7 | 11 | 68 sound emitters |
| nab1 | 21 / 21 | 15 / 15 | 5 / 5 | 21 | |
| nab2 | 9 / 9 | 23 / 23 | 6 / 6 | 4 | 36 sound emitters |
| rhn1 | 14 / 14 | 17 / 17 | 5 / 5 | 5 | |
| rhn2 | 10 / 10 | 28 / 28 | 7 / 7 | 0 | 13 dust |
| tat1 | 12 / 12 | 11 / 11 | 9 / 9 (3 Tusken spawns, team 3) | 16 | 66 dust |
| tat2 | 14 / 14 | 20 / 20 | 6 / 6 | 1 | |
| yav1 | 13 / 13 | 16 / 16 | 7 / 7 | 7 | godray, 23 dust, 21 sound emitters |
| yav2 | 6 / 6 | 18 / 18 | 5 / 5 | 8 | 1 placed health power-up |
| tat3 | 0 | 19 / 19 | 6 / 6 | 0 | 12 cell doors, the trapdoor, the torture droid; Jabba and the rancor attached |

Pads: each `com_item_vehicle_spawn` carries up to ten `Class<Faction><Role>`
choices; `tools/zero_vehicle.py` `vehicle_spawns` keeps each battle's own
factions, a pack per battle (#3). A historical version that calls
`SetHistorical` reads the pads' `ClassHisATK/DEF` for teams 1 and 2; the
Fambaa shield generator (Naboo Plains) and the Gian speeder (Theed) are
classes of the campaign's own worlds (`NAB/nab1_h.lvl`, `nab2_h.lvl`), which
no map extracts yet.

### 3. Weapons and ordnance

147 weapon classes (`weapons` in the inventory, with every special key):
cannon/bolt 46, vehicle `weapon`/bolt 16, grenade/sticky 14, launcher/missile
12, repair 9, cannon/beam 7, mine dispensers 6, cannon/missile 5, melee 5,
cannon/bullet 4, power-up dispensers 4, remote droids 4, destruct 4,
cannon/shell 3, and one each of towcable, shield, grenade/shell (the Ewok
rock), grenade/grenade (the Gungan haywire detonator), weapon/fatray (LR1K),
launcher/bolt (AAA), weapon/missile (gunship guided missile), arc caster
emitter. Engine coverage (`src/ordnance.js`, `src/combat.js`, `src/weapons.js`,
`src/items.js`, `src/remote.js`, `src/heroes.js`):

| Behaviour | Status |
| --- | --- |
| bolt, beam, bullet, shell, missile (homing at `TurnRate`), sticky, grenade (bounce, fuse), mine (proximity, expiry) | implemented |
| lock-on for launchers (`LockOnRange`/`LockOnAngle`/`LockTime`) with the HUD marker | implemented for soldiers, vehicle seats and turrets (docs/weapon-targeting.md); the marker needs `LockOffAngle` |
| charge levels (`NextCharge`), salvos, heat, zoom and sniper scope, spread | implemented |
| dispensers (mines, power-ups), repair, melee, shield, remote droid with destruct strike | implemented |
| arc caster emitter | implemented |
| fatray | implemented |
| towcable | implemented (docs/vehicles.md) |
| hero bolt deflection | missing (`docs/ai-heroes.md`) |

### 4. Mission scripts

44 scripts; every call and its arguments are in `maps.<name>.missions`. The
calls nothing in `src/` or `tools/` reads: `SetBirdType`, `SetNumBirdTypes`,
`SetBirdFlockMinHeight`, `SetFishType`, `SetNumFishTypes`,
`SetLowReinforcementsVoiceOver`, `AddWalkerType`, `ClearWalkers`. Everything
else (teams, unit classes and counts, heroes, attacker, bleed, death regions,
fly heights, spawn delay, difficulty, aggressiveness, friends and enemies,
bonuses, turrets, snipe range, blind jet jumps, notify radius, music, voice
over, camera shots, objectives, dense environment, historical flag) has a
reader.

### 5. Map features

| Feature | Retail data | Status |
| --- | --- | --- |
| Command post types: standard, major (`NeutralizeTime 36`, `CaptureTime 16`), invisible (Hoth's probe droid spawn), the locals' spawns | `com_bldg_major_controlzone` on 11 maps, `com_bldg_invisible_controlzone` hot1, `tat_bldg_tuskanspawn` x3 | implemented as posts (the locals' spawns with #4) |
| Destructible posts | #5 | implemented |
| Death regions | `AddDeathRegion` in 18 scripts, `KillRegion` on the shield generator | implemented (`map.json` `deathRegions`, `src/game.js`); the shield generator's kill region goes off once as it is destroyed (docs/objectives.md) |
| Water with damage | WATR, soldier drowning, walker and droideka water damage | implemented (#13) |
| Buildings with lights | `building` classes with `AttachOdf` lights | partial (#17) |
| Traps | #7 | implemented (Endor's log traps) |
| Animated props, doors | #11 | implemented |
| Placed power-ups | `com_item_powerup_health100` on yav2 | implemented |
| Godrays, dust, rumble | #14 | dust implemented; godrays and rumble missing (cosmetic) |
| Skydomes, fly ceilings (`SetMaxFlyHeight`, `SetMinFlyHeight`) | all scripts | implemented |
| Foliage (grass and leaf patches) | nab1, nab2, yav1, yav2 | implemented |
| Sound emitters | #18 | implemented |

### 6. Classic AI features the docs mark missing

From `docs/ai.md` and `docs/ai-*.md`: hero projectile deflection and the block
pose (`docs/ai-heroes.md`); the Enhanced mode's planned evidence, fear and
withdrawal systems (perception limited to what a bot could see or hear,
threat-aware routes, cover and flanking: docs/ai-enhanced.md "Not yet", by
design not Classic). Nothing else in the AI docs is marked planned.

## Interactive map objects, every map

Every placed class on the 16 stock maps and Jabba's Palace (`tat3`, the
add-on), with the classes hung on placed props by `AttachOdf`, by the base
at the root of its class chain (`tools/audit-features.py` `interactive`,
`cosmetic`). The bases on the maps: `prop` (4769 instances, static),
`powerupstation` 344, `dusteffect` 223, `soundambiencestatic` 191,
`armedbuilding` 181, `building` 173 (static: none sets a `MaxHealth`),
`vehiclespawn` 143, `godray` 136 (plus 537 attached), `commandpost` 110,
`grasspatch` 83, `leafpatch` 43, `door` 12, `animatedprop` 3 (plus 7
attached), `rumbleeffect` 3, `trap` 2, `hologram` 1,
`soundambiencestreaming` 1, `powerupitem` 1. Turrets, droids, posts and
pads are in section 2 above.

Gameplay objects (a player or a bot sets them off, uses them, damages them
or is hurt by them):

| Map | Class | Base | What it does | Status |
| --- | --- | --- | --- | --- |
| bes2 | `bes2_carbon_forklift` (1) | animatedprop | The carbon-freeze chamber: a body at the bottom of its pit (the pit is the map's `deathregion`) sets off `activate`; the claws come down and the casing rises (docs/live-objects.md) | **implemented**: bodies stay in death regions, so the pit sets it off |
| end1 | `end_weap_treesmash` (2) | trap | The Ewok log traps (#7) | **implemented** |
| end1, geo1, hot1 | `end_bldg_bunker_controlpanel`, `geo_bldg_technounion_cp` (3), `rep_bldg_forwardcenter`, `hoth_bldg_shieldgenerator` | commandpost with MaxHealth | Destructible objectives (#5) | **implemented** |
| tat1 | `tat1_bldg_sarlacctentacle` (5, attached to the pit) | animatedprop | Grabs and swallows; the pit `Sarlac01` kills | **implemented** |
| tat3 | `tat3_bldg_rancor`, `tat3_bldg_jabba` (attached), `tat3_bldg_torturedroid`, `tat3_prop_trapdoor` | animatedprop | The rancor grabs; Jabba's `grab` and the trapdoor's `open` play for a soldier at their hotspots (the trapdoor opens under him into the rancor's pit); the torture droid idles | **implemented** |
| tat3 | `tat3_prop_celldoor` (12) | door | Open while anyone is within 2 m | **implemented** |
| yav2 | `com_item_powerup_health100` (1) | powerupitem | 75 health to whoever takes it, for its 30 s `LifeSpan` from the battle's start | **implemented** (docs/live-objects.md "Placed power-ups") |
| kas2, yav1 (walkers); kam1, kas1, kas2, nab2, yav1, tat3 (droidekas) | every walker class, `cis_inf_droideka` | water damage keys | Deep water wears them down (#13) | **implemented** (docs/vehicles.md "Water") |
| all | `armedbuilding` (181), `powerupstation` (344) | | Turrets and droids | **implemented** |

Checked and not a gap:

- `end_bldg_bunker` (Endor) is a `prop` whose class sets `MaxHealth 4000`,
  an `ExplosionName` and damage stages, but the original's prop class reader
  has no `MaxHealth` case. The bunker is static in the original too.
  Endor's region `cp1_kill` is named by no instance (`cp1` sets no
  `KillRegion`), so nothing uses it.
- The Hoth hangar lift (`HOTH_prop_hangar_lift_platform`) is a plain prop.
- No map places a vehicle, a mine or any other weapon item; no map
  defines a damage or hazard region beyond the death regions (implemented),
  and the light, sound, reflection and rain-shadow regions are cosmetic.
- No mission script creates interactive gameplay beyond what is read: the
  calls nothing reads are the bird and fish flocks, the walker pool sizes
  and the low-reinforcement voice-over.

Cosmetic gaps (not implemented, listed for completeness): the godrays
(`end_prop_godraycluster` 135 with 471 attached `end_prop_godray`,
`yav_prop_godraycluster` with 64 attached, `geo_bldg_godray`,
`tat3_prop_godray`), Hoth's cave rumble (`hot_rum_caves`, 3: camera shake
and `hoth_rumble`, no damage key), the placed `com_holo_controlzone`
hologram on Hoth, attached lights (`AttachOdf` light classes on 8 maps,
#17), the birds and fish (#10), the walkers' `FootWaterSplashEffect`
(docs/vehicles.md).

## Rerunning

```
BFE_GAME_DIR=<install> BFE_CONTENT_DIR=<game data> \
python3 tools/audit-features.py            # writes extract/audit/features.json and prints the gaps
python3 tools/audit-features.py --maps bes1 hot1 --out features.json
python3 -m unittest tests.test_audit_features
```

`<install>` is the folder holding `GameData`; `<game data>` is the prepared
content folder (default `public/`).

The add-ons' maps (Jabba's Palace) are read with the stock ones, through
`tools/zero_addon.py`. `GAP interactive` lines name every gameplay class
(placed or attached) the prepared `props.json` lacks.

The JSON holds, per map, every instance of the live and special bases with
its class spelling, resolved class, position and properties; every class's
seats and weapons; every weapon's special keys and ordnance; every mission
script's calls; what the prepared folder has for the map; and the computed
gaps (`gaps`: `live`, `post`, `postMissing`, `pads` by era, `side`). The
report lines starting `GAP` are the comparisons alone; the ranking above is
the reading of them against `src/`.
