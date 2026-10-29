# Format notes

Filled in from a local Steam install. This file does not contain retail data.

The Steam release is copied whole into `data/game/install/`: `installscript.vdf`, `Manuals/` (the manual PDF and a troubleshooting RTF), and `GameData/`. `GameData` holds `Battlefront.exe` with its DLLs, `SaveGames/` (the player's profile), `AddOn/Tat3/` (the Jabba's Palace map, with its own `core.lvl`, `mission.lvl`, side, and sound), and `Data/_LVL_PC/`:

| Path | Contents |
| --- | --- |
| `BES`, `END`, `GEO`, `HOT`, `KAM`, `KAS`, `NAB`, `RHN`, `TAT`, `YAV` | One folder per planet: each map's `<map>.lvl`, and for some a historical campaign version (`nab2_h.lvl`, `tat1h.lvl`) |
| `SIDE/<side>.lvl` | The sides' units, weapons, and vehicles: `all`, `cis`, `imp`, `rep`, and `des`, `ewk`, `gar`, `geo`, `gun`, `wok` for locals |
| `FPM/<side>/` | First-person models: arms rigs and cockpits |
| `Common/<language>/common.lvl` | Shared models, effects, animations, and scripts, one per language (`ENG`, `FRE`, `GER`, `ITA`, `SPA`) |
| `core.lvl`, `mission.lvl` | Shared HUD art, fonts, and localization; every battle's setup script |
| `Load/`, `Shell/<language>/`, `Sound/`, `Movies/` | Loading screens, the front end, sound banks, and the shell's movies |

The PC archives are `ucfb` chunk trees. A chunk is a 4-byte tag, a little-endian size, and a body aligned to 4 bytes. `lvl_` nodes are a hash and an inner size, then more chunks.

Bespin: Cloud City is `BES/bes2.lvl` (`bespin2`: carbon-freezing chamber, skyways, hotel). Bespin: Platforms is `bes1.lvl`. The clone trooper model is `rep_inf_trooper` in `SIDE/rep.lvl`, hard-skinned to the skeleton in that archive. The soldier animations are in `Common/common.lvl`.

`tools/extract.py <map>` writes a map, a side, and the shared soldier data under `public/`. A map's archive is `<PLANET>/<map>.lvl` (`bes2` is `BES/bes2.lvl`). Models and textures a map places but doesn't hold (command posts, recharge droids, turrets) come from `Common/common.lvl` and `core.lvl`; a placed class neither holds comes from the archive of a side the map's missions field (Geonosis places the Republic's `rep_bldg_forwardcenter`, in `SIDE/rep.lvl`). An effect a side's classes name that the shared archives lack comes from whichever map archive holds it (`hailfire_wake`, the hover vehicles' and droideka's wake, and `walkerstomp` and `dustwake` are in the maps they are used on). A class's `FirstPerson` names its FPM archive in either case (`all\alltroop` is `FPM/ALL/alltroop.lvl`), matched without regard to case. `tools/audit-import.py` runs these lookups for every map and side and lists what stays unresolved. `tools/audit-collision.py` checks an extract's collision buffers against the archives, map by map (docs/collision.md).

What the retail archives do not hold, so nothing imports it (the game drops these quietly too):

- `bes_bldg_aaa_turret` (Bespin: Platforms places eight), `rhn2_prop_cloud` (Rhen Var: Citadel, thirteen) and `rhn_bldg_coffin_shadowvolume` (one): no class or model in any archive.
- The texture `Rhn_bldg_snow_2` of `rhn_prop_snowdrift`: no texture of that name anywhere.
- The effects `deathdustcloud_brown` (the droideka's and spider walker's `DeathDustEffect`) and `ginormousdustcloud` (the AT-TE's): no `fx__` chunk anywhere.
- The cockpits `all_1st_cockpit_kwing` (`FPM/ALL/allkwing.lvl`) and `rep_1st_gunner` (`FPM/ALL/repgunnr.lvl`, the landspeeder's gunners), and `FPM/IMP/allxwing.lvl`: eight-byte archives with nothing in them. (Vehicle cockpits are not imported yet in any case.)

Placed classes the engine draws without a model of their own, which the extractor reads for what they are and otherwise leaves out: `godray` clusters (Endor's and Yavin's light shafts), `dusteffect` (ambient dust and mist), `rumbleeffect` regions, `vehiclespawn` markers (the vehicle pads), the `hologram` over a command post, and `SoundAmbienceStatic` and `SoundAmbienceStreaming` emitters (the audio's).

| File | Contents |
| --- | --- |
| `maps/<map>/map.json`, `geom.bin`, `index.bin`, `textures/` | World meshes (near and far scene), terrain, the sky dome, lights, distances, spawn paths, regions, command posts; and `missions`, every battle fought in the world by mission name: its `mission` (teams, sides, attacker, `era`, `historical`), `spawnScreen` and `vehicles` pack. The runtime plays one (`src/mission.js`) |
| `maps/<map>/vehicles/<mission>/vehicles.json`, `models.bin`, `extras.bin`, `animations.bin`, `textures/` | Each battle's vehicle pack: its pads with the classes its teams take, and those classes' rigs, collision, seats, weapons and effects |
| `maps/<map>/lumi.bin` | The terrain's 256 x 256 soldier light map |
| `maps/<map>/collision.bin`, `collision_index.bin`, `collision_poly.bin` | Building and prop collision in world space: triangles, and the polygon each triangle belongs to |
| `maps/<map>/terrain_collision.bin`, `terrain_collision_index.bin` | The terrain under the city, kept apart because soldiers test it as a height field |
| `sides/<side>/units.json` | Each playable unit's ODF properties (speeds, camera sections, FOVs, jetpack), its body and arms model names, and the side's textures |
| `sides/<side>/actors/<model>.json`, `.bin`, `_index.bin` | Each unit's skinned body (bind pose, one bone weight; rigid parts such as a jetpack bound whole to their node) |
| `sides/<side>/units.json` `weapons` | Each unit's loadout: each slot's weapon class, its ordnance (and charge levels), and their explosions; a dispenser's item, a remote weapon's droid with its own weapons |
| `sides/<side>/units.json` `items` | The mines and power-ups the side's dispensers throw and its soldiers drop |
| `sides/<side>/weapons.json`, `weapons.bin`, `weapons_index.bin` | Held weapon and flying ordnance models, droid chunks, items, and recon droids, and each weapon's fire point |
| `common/anims.json`, `common/anims.bin` | Decoded soldier clips, per bone `[qx qy qz qw tx ty tz]` per frame |
| `common/effects.json`, `fxmodels.bin`, `fxmodels_index.bin` | The particle effects the weapons use, decoded, and the meshes they and the muzzle flash draw |
| `maps/<map>/props.json`, `props.bin`, `props_index.bin` | Recharge droids and turrets: each class's rig (bones, parts in their bone's space, and the model's bounds), its build pad, wreck and hologram, and each placed one's class settings, explosion and weapon |
| `maps/<map>/props_collision.bin`, `_index.bin`, `_poly.bin` | Each live object's collision as built and as a wreck, as triangle ranges, so it can come and go |
| `sides/<side>/firstperson/<model>.json`, `.bin`, `_index.bin` | Each unit's first-person arms rig; the weapons' first-person models are in `weapons.bin` |
| `common/anims_fp.json`, `anims_fp.bin` | The first-person clips |
| `common/common.json`, `textures/` | Hologram logos, the HUD's fonts, art, and text, and the first-person camera motions |

## Name hashes

Animation banks key bones and clips by CRC-32 of the lowercased name: MSB first, polynomial `0x04C11DB7`, initial value and final xor `0xFFFFFFFF`. Every bone of the trooper skeleton matches its animation track this way.

Entity class properties, instance properties, and localization keys use a 32-bit FNV-1a over each byte OR 0x20. That folds letters to lower case, but it also turns `_` into 0x7f. So for a name with `_` (or `@[\]^`), such as `Value_ATK_CIS`, the hash is not FNV-1a of the lowercased name.

## Animation banks (`zaa_`, `zaf_`)

`common.lvl` has `zaa_` banks `human_animation_0` to `_4`, `_alert`, `_fp` (first person, own skeleton), and `_lowrez`. A `zaf_` with the same name holds that bank's animation skeleton (joint bind pose and a skin).

`zaa_` is `NAME` plus `BIN_`. Inside `BIN_` the chunk tags are byte-reversed:

- `SMNA`: a 24-byte header. Tag, a zero size, 8, the total bone record count, the curve data size, the clip count.
- `MINA`: 16 bytes per clip. Flags, an unused word, the name CRC, frame count (u16), bone count (u16). Flag bit 0 means `DummyRoot` carries the clip's ground travel.
- `TNJA`: 48 bytes per bone per clip. Bone CRC, four rotation curve offsets, three translation curve offsets, translation bias xyz, translation scale. A zero scale means the translation is the constant bias.
- `TADA`: the curves. Each starts with an int16 sample. Each following signed byte is a delta from the last sample, except -128 (hold for the next unsigned byte's count of frames) and -127 (an absolute int16 follows).

Rotations are quaternion xyzw over 2047. A few near-180-degree keys were clamped by the encoder and need renormalizing. Translations are bias + sample × scale. Clips run at 30 fps, and loops repeat their first frame as their last, so a loop lasts (frames − 1) / 30 seconds.

Clip names follow `human_<weapon>_<posture>_<action>`, with an `_alert` variant for the raised-weapon set (`human_animation_alert`), and posture-less clips such as `human_<weapon>_jump`, `_fall`, `_landsoft`, `_landhard`, `_diveforward`, and `_stand_getdown_prone`. The posture-less action names are: jump, jump_up, landsoft, landhard, fall, the thrown_* set, stand_getupfront and stand_getupback, diveforward, jetpack_hover, the four prone transitions, and the stand_death_* set. The extractor names clips by matching CRCs against that scheme.

Locomotion clips (flag bit 0) carry their travel on `DummyRoot`. The travel over one cycle divided by the cycle's duration is the speed the cycle was authored for. At 30 fps the standing run comes out within a few percent of the trooper's `MaxSpeed`. The runtime drops that travel and plays each cycle at ground speed over authored speed, so planted feet stay put.

The turn clips do not rotate the root. They are a foot shuffle that starts and ends in the idle stance, so the body yaw has to be turned in code while they play.

The banks key tracks by bone name, so one bank serves every body with those bones. Bodies don't all list them in the same order: the ARC trooper's skeleton has the clone trooper's 25 bones in a different order, so tracks are matched to bones by name.

## Entity classes (`entc`)

`TYPE`, `BASE` (the engine class, such as `soldier`), then `PROP` chunks: the FNV-1a of the property name and the value as a C string. The whole ODF inheritance chain is flattened into the class. Repeated keys keep their order; `CAMERASECTION` starts a group of `EyePointOffset`, `TrackCenter`, `TrackOffset`, and `TiltValue`.

Two more repeated groups: `CHUNKSECTION` starts one of the pieces a droid blows apart into (`ChunkGeometryName`, `ChunkNodeName`, `ChunkSpeed`, `ChunkOmega`, `ChunkTerrainCollisions`, `ChunkTerrainEffect`, `ChunkTrailEffect`, `ChunkPhysics`), and the drop list is `DropItemClass` (406d2b75) with its `DropItemProbability` (26996dae), `NextDropItem` (5ea04066, value `-`) moving to the next slot. Every Republic and CIS infantry class drops ammo 0.4, health100 0.3, dual 0.1.

`HealthType` (person, animal, droid, vehicle, building) picks which of an ordnance's or explosion's scales hurts a unit; unset is person. `OverrideTexture` re-skins a class's body: the assault droid's `cis_inf_battledroid_blue`, the assassin's `_red`, both over the B1's `cis_inf_battledroid`. The port swaps the texture most of the body uses (and `OverrideTexture2` the next).

The 2004 trooper classes set `MaxSpeed`, `MaxStrafeSpeed`, `MaxTurnSpeed`, `Acceleraton`, the camera sections, and `FirstPersonFOV` / `ThirdPersonFOV`. The acceleration property really is spelled `Acceleraton`. There is no `JumpHeight` in the 2004 engine; the jump is `MaxJumpSpeed`. Properties a class leaves unset take the soldier class defaults listed under engine behavior.

## Models (`modl`, `segm`)

A `modl` is `NAME`, `INFO` (counts, then the bounding box at bytes 12..35), and one `segm` per material. A segment holds `INFO` (the primitive type), `MTRL`, `RTYP` (the render type: `Normal`, `Bump`, `Scroll`, `Specmap`), `TNAM` (a slot number and a texture name), two or three `VBUF`s of the same vertices in different encodings, `IBUF`, and for skinned models `SKIN` and `BMAP`.

- **VBUF:** count, stride, flags, then the vertices. The fields come in this order, each only when its flag is set: position (0x2), bone weights (0x8), bone indices (0x4), normal (0x20), tangent and binormal (0x40), colour (0x80), baked light (0x100), and UV (0x200). Bits 0x1000/0x2000/0x4000/0x8000 select packed forms: positions as int16x4 within the model's box, weights as bytes, vectors as bytes (x, y, z in bytes 2, 1, 0, scaled to −1..1), UVs as int16 over 2048. The extractor reads the uncompressed buffer with the most fields. All encodings hold the same data.
- **Vertex colours** (0x80 and 0x100) are D3DCOLORs stored B, G, R, A. A 0x80 colour is a tint on the material; a 0x100 colour is baked light. They never appear together.
- **Tangents:** the tangent follows +u and the binormal +v of the raw UVs (v runs down the texture). About 1% of float tangents are NaN; the extractor builds a basis from the normal for those.
- **SKIN** is a vertex count, a type, then the vertices' bones as slots of `BMAP` (a list of skeleton indices). Type 1, a hard skin, is one byte per vertex, and the vertices are in their bone's space. Type 3, a soft skin, is five bytes per vertex: two weights out of 255 (the third weight is what remains), then three slots. The vertices of a soft skin are already in the model's space, and its `VBUF` carries the same weights and indices (flags 0x8 and 0x4; the index bytes are stored in reverse). Stock BF1 uses soft skins for Vader's and Dooku's capes, the sarlacc tentacle, Endor's log trap and the fambaa. Jabba's Palace uses them for the Gamorrean guard, Jabba, the rancor and the torture droid. The renderer blends by the `VBUF`'s float weights; `SKIN`'s bytes are the same rounded to 1/255. An animated prop keeps each vertex's three bones and weights (`props.json` `skinJoints`, `skinWeights`; docs/live-objects.md "Animated props and the sarlacc"); soldiers and vehicles give each soft-skinned vertex its heaviest bone and leave it where it is.
- **MTRL** is a u32 of flags, then two more u32s when bit 0x10 is set. Bits: 0x1 lit, 0x2 alpha test, 0x4 alpha blend, 0x10 two parameters follow, 0x20 specular, 0x40 baked lighting, 0x80 additive, 0x400 scrolling (only a marker). The parameters are a specular exponent and a 0x00RRGGBB colour when 0x20 is set, or two float scroll speeds in repeats per second.
- **TNAM:** slot 0 is the diffuse texture, slot 1 the bump map: a tangent-space normal map with green toward +v.
- **Levels of detail:** `gmod` lists a model's `LOD0` and, for most buildings, `LOWD`, a low-detail model named `<model>LOWD` with its own `_lowrez` textures.
- **Bounds:** a SWBF1 model `INFO` is 68 bytes: three counts, the vertex box, a second box, and two more words. `SPHR` is its bounding sphere (centre, radius). The engine locks onto and aims at a building by the second box's middle and half its diagonal (the gun turret: (0, 1.653, 0.323), radius 3.008), and onto a build pad by its sphere.

## Weapons (`wpnc`, `ordc`, `expc`)

Weapon, ordnance, and explosion classes are laid out like entity classes: `TYPE`, `BASE` (the engine class), and `PROP` chunks. The ODF chain is flattened, so a key can repeat and the last value wins.

- **Bases:** weapons `cannon` (rifles, pistols, the wrist blasters, the arc caster), `grenade` (grenades and the mortar), `launcher`, `dispenser` (mines and power-ups), `repair` (the fusion cutter), `remote` (the recon droid), `destruct` (the droid's orbital strike); ordnance `bolt`, `bullet` (rockets), `missile`, `shell`, `sticky` (grenades), `emitterordnance` (the arc caster's lightning); `explosion`.
- **Loadout:** the soldier class sets `WeaponName<N>`, `WeaponAmmo<N>` (clips counting the loaded one; 0 is unlimited), and `WeaponChannel<N>` (0 the primary trigger, 1 the secondary). The clone trooper carries the rifle (4 clips of 60) and pistol on the primary, and EMP (3) and concussion (2) grenades on the secondary.
- **The chain:** a weapon's `OrdnanceName` names its ordnance class, whose `ExplosionExpire` names the explosion. Classes the side archive doesn't hold come from `common.lvl`.
- **Models:** `GeometryName` is the model held in `hp_weapons`, with `hp_fire` (`FirePointName`) at the muzzle. Grenades fly as their ordnance's `GeometryName`.
- **Charge levels:** `NextCharge t` (8316f444) starts a copy of the weapon class as read so far, which the keys after it change, up to the next one. The mortar's base level fires `_ord1` (a shell) and its `NextCharge 0.9` level `_ord` (a sticky grenade); the arc caster's four levels only raise `HeatPerShot`.
- **Shot patterns:** `ShotPatternCount` and one `ShotPatternPitchYaw p y` per round, in degrees, positive pitch down. The super battle droid's shrapnel is (−0.1, −0.8), (0.8, 0), (−0.1, 0.8). `ShotPatternCount` starts a new list and a `NextCharge` level starts with none, so each charge level's entries are its own; the runtime takes the last `ShotPatternCount` of such a list.
- **Other keys:** `KickStrength` (degrees) and `KickBuildup` (the aim kick), `HideOnFire` (5578d44c), `ShotElevate` (ae3db60d), `SniperScope` (f15aaeb2). `KickSpread` and `SpreadRecover` are older names for `SpreadPerShot` and `SpreadRecoverRate`.
- **Keys the 2004 game ignores:** the retail weapon classes also set `Discharge` (8b83f9b7, `small_smoke_effect` on most guns), `ModeTexture` (54e9f4e5, a fire-mode icon: `HUD_weap_fullauto` on rifles, `HUD_weap_semiauto` on pistols, `HUD_weap_scatter` on the two shotguns, `HUD_weap_charge` on bowcasters) and `ChangeModeSound` (c77178c1). The 2004 game uses none of the three: it plays no discharge smoke, shows no fire-mode icon and has no fire modes to switch; a gun fires the one way its class describes. An ordnance's `MaxDamage` is the same field as `Damage`; the shotguns' bolts name only `MaxDamage` (55).
- **Dispensers:** `OrdnanceName` names an entity class (`entc`), not ordnance: `com_weap_inf_landmine` (base `mine`) or `com_item_powerup_dual` (base `powerupitem`). They add `Velocity`, `MinStrength`, `MaxStrength`, and `MaxItems`. Items read `LifeSpan`, `Gravity`, `Rebound`, `Friction`, `LightColor`, `LightRadius`; mines `ExplosionTrigger` (also `ExplosionName` or `Explosion`), `ExplosionDeath`, `TriggerRadius`, `SuppressRadius`; power-ups `PersonHealth` … `BuildingAmmo`, which `SoldierHealth` and `SoldierAmmo` write for persons and droids together.
- **Repair weapons:** amounts per tick for each health type (`PersonHealth` … `BuildingHealth`), `BuildingRebuild` (a wreck), and `BuildingBuild` (a pad), the same for ammo; `ShotDelay` defaults to 0.125 and `TriggerSingle` to 0.
- **Remote weapons:** `OrdnanceName` is the droid's class (base `droid`), with `SwitchImmediately` and `AllowDestruct`. The droid class lists its own weapons as `WeaponSection` groups of `WeaponName`, `WeaponAmmo`, and `WeaponChannel`. Its hover keys (`ForwardSpeed` and the rest) belong to a template the droid ignores; it reads `SetAltitude`, `GravityScale` ("1.0//2" reads as 1), `LiftSpring`, `LiftDamp`, and the soldier's `MaxSpeed`, `MaxStrafeSpeed`, `MaxTurnSpeed`, `Acceleraton` (defaults 9, 7, 2, 70). Its destruct weapon's `StrikeOrdnanceName` is the orbital strike, with `AimElevation`, `AimAzimuth`, `AimDistance`, and `ScatterDistance`.
- **Emitter ordnance:** `ConeLength`, `ConeAngle`, `FirstRadius`, `MaxTargets`, `MaxJumpDistance`, `DamageThreshold`, `LightningEffect`, `Push`, and `Damage` and `MaxDamage` as the damage at no charge and full charge (defaults: `ConeLength` 10, `ConeAngle` 60, `FirstRadius` 1.5, `MaxTargets` 5, `MaxJumpDistance` 10). fc0d5e74, whose name is unknown, is the damage lost per link (75). The class also accepts `Effect` (an effect started at the gun along the shot), `StunAnimation` and an unnamed number (8df1e8c9); the last two do nothing, and the arc caster's class sets none of the three. Its `PersonScale` … `ShieldScale` (`VehicleScale` 0, `ShieldScale` 4) go to the plain ordnance fields, which the emitter ignores too: its damage is the same for every kind of target.
- **Explosions:** `DamageRadius`, `ShakeRadius`, and `PushRadius` are older names for the outer radii (the inner stays 0). The turret's and the droids' explosions use them.

## Effects (`fx__`)

An `fx__` chunk is `NAME` (the FNV hash of the effect name) and the effect's source as a config tree (the same `DATA`/`SCOP` layout as `sky_`). Each `ParticleEmitter` block holds:

- emission: `MaxParticles`, `StartDelay`, `BurstDelay`, `BurstCount` (min, max ranges), `NoRegisterStep`;
- emitter-wide `Size` and colour (`Red`/`Green`/`Blue` or `Hue`/`Saturation`/`Value`, `Alpha`);
- a `Spawner`: a `Spread` box or `Circle` of direction ranges, an `Offset`, `PositionScale`, `VelocityScale`, `InheritVelocityFactor`, and size, colour, `StartRotation`, and `RotationVelocity` ranges (written with a leading selector, (0, min, max));
- a `Transformer`: the particle's `Lifetime`, then `Position`, `Size`, `Color`, and `Rotation` keys, each a `Lifetime` and an `Accelerate`, `Reach`, `Scale`, or `Move`, chained by `Next`;
- a `Geometry`: `Type` (`PARTICLE`, `ANIMATED`, `SPARK`, `STREAK`, `BILLBOARD`, `GEOMETRY`, `EMITTER`), `BlendMode`, `Texture` or `Model`, and flipbook (`TimePerFrame`, `FrameSize` cells per row, `TotalFrames`) or spark (`SparkLength`) settings. An `EMITTER` geometry holds the emitter each particle carries.

In the munged tree an effect's next emitter sits inside the previous emitter's block, after its `Geometry`.

A `LightningBolt` block (only `arccaster_lightning` has one) is a bolt between two points: `Texture`, `Width`, `FadeTime`, `SpreadFactor`, `InterpolationSpeed`, `BranchFactor`, `BranchSpreadFactor`, `BranchLength`, `MaxBranches`, `BreakDistance`, `TextureSize`, and `Child` versions of the shape keys, `NumChildren`, `Color`, and `ChildColor`.

## Collision (`coll`, `prim`)

`coll` is one model's collision mesh: `NAME`, `NODE`, `INFO` (vertex count, three counts, bounds), `POSI` (float3 per vertex), and `TREE`. `TREE` is an AABB tree written depth first. `NODE` children are a 6-byte quantized box. Each `LEAF` is one polygon: a vertex count, a 6-byte quantized box, then u16 vertex indices. The polygons are triangles and quads. The engine resolves contacts per polygon, so the extractor keeps each polygon's number with its triangles.

`prim` holds a model's primitive shapes: `INFO` (model name, shape count), then `NAME`, `PRNT`, `XFRM` (a 3x3 rotation and a translation, relative to the parent node), and `DATA` per shape. `DATA` is the type and three floats. Type 4 is a box with half extents. Type 2 is a cylinder along Y with radius and full height. The street lamp's post shows the height convention: it spans exactly the lamp's 0 to 4 m.

Every building placed in Cloud City has a `coll`, `prim` shapes, or both. Shapes parented to a bone belong to animated props.

## Terrain (`tern`)

`INFO` is the grid unit (8 m), height scale, min and max height, grid size (256 cells), and cells per patch (8). `PCHS` holds `COMN` (the triangle strip every plain patch shares) and one `PTCH` per patch, row by row. A patch's kind-290 `VBUF` (28-byte stride: position, normal, color) is in patch-local meters.

Patch (col, row) sits at x = col × 64 − 1024 and z = row × 64 − 1024 + 8: whole patches in x, and one grid unit along z. The hole edges in the cut patches land exactly on the `CUTS` volumes there, and the spawn points on the sunken plazas sit exactly on the terrain there. A grid laid out without that offset, or flipped, puts the plazas' floors in the wrong places.

`CUTS` holds `CUTR` volumes, each a list of planes (normal, d); terrain inside all of them (n·p + d < 0) is removed. The munger has already applied them: the patches under a cutter carry their own `VBUF` and `IBUF`, re-tessellated around the hole.

The patch colour is baked light at half strength: the renderer doubles it. Its alpha is the second texture layer's weight. `LTEX` names the layer textures, `SCAL` gives each layer's meters per repeat (UVs are x and z over it), and `DTLX` names the detail texture. `LUMI` is 256 x 256 bytes, indexed x-major (`x * 256 + z`, in grid units from the patch origin), that darkens soldiers standing on the terrain.

## Terrain water (`WATR`)

`OceanEnable` selects a textured ocean rather than the inland reflective-water
path. Intake also retains `FoamTexture`, `FoamTile`, `WindDirection`, and
`WindSpeed`, exporting both water and foam textures. The runtime uses a bounded
camera-following mesh with nearby wave displacement and an outer skirt in both
near and far scenes; the native WMAP mask still clips its footprint.
Enhanced ocean replaces the Classic surface with a 65,280-triangle ring mesh,
world-space Gerstner swells, crest compression foam and a 384-instance spray/mist
batch. Unresolved short waves fade with mesh spacing. A 768×384 hemisphere
panorama integrates a 3D cloud volume at most 10 times per second when stationary;
large camera translations invalidate it. Looking around reuses the panorama.
The sky and ocean share this cloud image. Lightning uses a fixed ribbon buffer
and short cloud/water/ambient light pulses; Classic restores native sky visibility
and map lighting. Ocean displacement is visual: drowning and water collision
continue to use the native mean water level.
`DomeInfo.TerrainEnable(0)` suppresses the editor height field in both visual
geometry and terrain collision. Kamino otherwise gains a gray sheet above the
ocean and a false floor beneath its suspended platforms.

`Effect("Precipitation")` is exported only when enabled, with its authored type,
range, particle count/size, streak length, velocity, color, alpha, and ground-effect
settings. Classic rain uses one instanced draw with GPU motion and a 16×16
toroidal cover cache. Enhanced uses up to 8,192 fine drops across several depth
layers (8,192 on Kamino), gusts, and soft depth intersections, plus two batches
for low mist and surface rings/spray. Its 32×32 cover cache shares roof queries
across drops; the impact pool performs at most four placement rays per update.
Mode switches reuse precipitation geometry and dispose the replaced cover map.

A map's `tern` can hold `WATR`; the water exists only if the world's fx config also has an `Effect("Water")` section.

- **INFO:** cells per side (64), cells per WMAP byte (2), cell size (32 m), x and z offsets (1024), and a scale that multiplies the look's tiling and divides its speeds. A point's cell is `floor((x + offset) / cell)`, clamped to the grid.
- **LAYR:** a name, a u32, the surface height (Theed: −54), then values only the editor reads. Only the first layer is used.
- **WMAP:** a nibble per cell (even cells in the high nibble): nonzero is water, bit 3 a cell drawn from its own patch.
- **CPAT:** one `LPTC` per patched cell, row by row: `INFO` bounds, an `IBUF` strip, and a `VBUF` (flags 0x222, stride 32: world position, normal, uv across the cell). Theed's six cut the water around the rotunda's island.
- **The look** (`Water` keys): `PatchDivisions` (grid quads per cell), `Tile` (normal-map repeats per cell), `Velocity` and `MainTexture` (the low-quality scrolling layer), `RefractionColor`, `ReflectionColor`, `UnderwaterColor`, `FresnelMinMax`, `NormalMapTextures`, `BumpMapTextures`, `SpecularMaskTextures` (each a name prefix, frame count, and frames per second; frames are `<prefix><n>` from 0), `SpecularMaskTile`, `SpecularMaskScrollSpeed`, and `OscillationEnable`. Normal maps are A4R4G4B4 with x in red, z in green, up in blue; bump maps are V8U8 du/dv offsets.

The low-quality runtime path uses `MainTexture`, `Velocity`, and animated `SpecularMaskTextures` without a reflection render. WATR's scale multiplies tile counts and divides scrolling speeds. High and Ultra retain their existing reflection/refraction paths. Rectangular `PatchDivisions` preserve both axes; custom shoreline patch geometry is kept. `DiffuseColor` and `RefractionColor` are aliases, with the last authored value winning. Gameplay keeps the original game's positive-Z WMAP lookup, edge-cell clamping, downward-only water crossings, and mean level plus the authored bob.

## Foliage (`grasspatch`, `leafpatch`, `prp_`, `FOLG`)

Foliage classes are entity classes. `grasspatch` (Theed's `nab_prop_flowers`) sets `MinSize`, `MaxSize`, `NumParticles`, `Texture` (a 2×2 atlas when `NumParts` is 4), `Alpha`, `MaxDistance`, `BoxSize` (or `RadiusFadeMin`/`Max`), `DarknessMin`/`Max`, and `YOffset`. `leafpatch` (Theed's `nab2_prop_vine1`/`2`) adds `Seed` (wrapped to 32 bits: vine1's 5232344514 is 937377218), `Radius`, `Height`, `HeightScale`, `ConeHeight`, `Offset`, `Vine` (two jitter gains: vine1 grows in the world's x-y plane, vine2 in z-y), `VineLength`, `VineSpread`, `WiggleSpeed`, and `WiggleAmount`.

A map's `prp_` config holds terrain foliage `Layer`s and `TreeLine` blocks. Its string values are FNV hashes. A `TreeLine`'s `Path` (by hash) gets a `BorderOdf` prop every `Distance` along its polyline, the walk not restarting at nodes; Theed's puts 19 `nab2_prop_treebill` along `jungle1` every 28 m. Its matrix uses the horizontal path tangent as right and `cross(tangent, up)` as forward, with Flip negating both axes. Treating the perpendicular as right mirrored the backdrops and turned them across Yavin's bridge exit. Theed has no terrain foliage layers.

Painted foliage: `FOLG` in `tern` is a u32 byte count followed by a nibble per cell (even columns in the high nibble). The PC game uses a **fixed 512×512 grid of 16 m cells**, independently of the terrain height-grid resolution. Columns run toward +X and rows toward **−Z**, centered on the world origin. Bit k enables layer k. Taking 4 m cells and +Z rows from the height map instead compresses and mirrors the vegetation onto unrelated ground, including Yavin Temple's bridge approach. Template paint outside the map's height field is ignored.

Only top-level `Layer(k)` sections in `prp_` are loaded; Geonosis's nested second layer is not promoted. `SpreadFactor` controls integer spacing or reciprocal placements per cell. Each `Mesh` is a weighted alternative selected by normalized `Frequency`, not an additional object planted at every cell. `grasspatch`/`leafpatch` name classes and take precedence over their editor `File`; standalone `File` entries name real meshes (including Yavin's shrubs). The 256-float placement table is read from the player's own executable at import time and checked by hash (`tools/zero_foliage.py`); no retail table is stored in source. Random streams restart per 32 m block and layer, and scatter within each enabled cell.

Painted cards use three terrain samples for their support height and up axis, with `Stiffness` controlling slope alignment. Imported tint uses terrain `LUMI` and `ColorVariation`. Painted grass draws to the near-scene end; painted leaves use twice the mesh entry's distance, capped at 1000 m. Placed patches retain their distinct distance rules. Painted model meshes only contribute collision when `UseCollision` is authored. FOLG does not suppress paint under WATR; water has its own independently oriented grid. Static painted model meshes currently share the normal world-mesh renderer rather than the original engine's streaming pool and per-mesh distance fade.

Attachments: an ODF's `AttachOdf` names the class that every `AttachToHardPoint` after it attaches, until the next `AttachOdf`. Yavin's `yav_prop_leaftree` hangs `yav_prop_leafpatch` on six branches, `yav_prop_leafpatch2` on three more, then godrays; reading only the first pair of each key left each tree one leaf cluster.

- **Grass:** `NumParts=1` uses the entire image with alternating U mirroring; `NumParts=4` uses the four atlas tiles. Radial edge cards clip their bottoms and UVs to the patch base plane. Each card is MinSize..MaxSize (0.9 × that tall, 1.1 × wide), one of the atlas's four plants in turn, a grey from the darkness range. Every frame its top leans toward the camera (up 70% toward it), its width turns by one of ten fixed amounts, and the top sways by 8% of the width at (1 + 0.2 (i mod 5)) × 0.8 rad/s. A placed bed draws to `MaxDistance`, sinking into the ground over the last 20 m (its height × 1 − (1 − a)²).
- **Leaves:** laid out once per class from `Seed` through the engine's random numbers (an LCG: two steps of s × 1664525 + 1013904223, the high half of the second and the high half of the first making 23 bits of a float; the seed first xored with 0xC71C2830 and one number dropped), so every vine of a class has the same shape. A vine walk emits `NumParticles` leaves, smaller toward its tips; each leaf's grey is darker on the side away from the sun. Leaves face the camera, pulled 0.1 m toward it, their widths turned by one of ten amounts, wiggling by `WiggleAmount` of their width at `WiggleSpeed`; odd leaves of a one-part texture are mirrored. Placed patches retain their up axis, discard yaw, and draw to the near scene's end, fading over its last 40%.
- **Drawing:** foliage is fullbright (texture × grey × a tint, no sun or ambient), cut out at alpha 0.25, double-sided, never casts shadows, and nothing collides with it.

## World (`wrld`, `PATH`)

`wrld` holds `NAME` (the world's name, such as `bespin2`), `inst` (placed objects), `regn` (regions: `TYPE` box, cylinder, or sphere, plus `NAME`, `XFRM`, `SIZE`), `Hint`, and `BARR`.

- **Instances:** an `inst` is an `INFO` (`TYPE` the class, `NAME` the instance name, `XFRM` the transform), then `PROP` chunks: the hash of the property name and the value as a C string. Command posts set `Team` (0 neutral, else the team number), `Label`, `CaptureRegion`, and `SpawnPath` this way.
- **Regions:** a region's runtime shape is built from `TYPE`, `XFRM` and `SIZE` (`src/regions.js`). A box's `SIZE` is half extents along its axes, scaled by the lengths of the `XFRM`'s axes. A cylinder's `SIZE` is scaled the same way, then its x and z both become sqrt(x^2 + z^2): that is its radius about its y axis, and y is its half height (Tatooine: Dune Sea's sarlacc pit `Sarlac01`, 2.14 x 6.06 x 8.16, is an 8.43 m cylinder; Kamino's light region `shadow0`, 0.1 x 10.15 x 93.16, is 93.2 m). A sphere's radius is the length of its unscaled `SIZE` (the world loader stores that length beside it, and the test compares against it); hand-sized spheres often stretch one axis and leave the others at 0.1: Dune Sea's `HOMESTEAD` capture region is 0.1 x 8.15 x 0.1, an 8.15 m sphere. Every reader takes the built shape: capture, kill, death and light regions, and the AI's objective offset, which spans the built x and z in world axes. A region a battle's script names with `AddDeathRegion` (by name without regard to case) kills a soldier inside it: Tatooine: Dune Sea's sarlacc pit (`Sarlac01`), Hoth's `Death`, Theed's `Water` and `Waterfall`, the Citadel's `FalltoDeath`, and the `deathregion*` regions of Cloud City, Endor and Kamino. The port also keeps every region named `deathregion*`, which Yavin 4: Arena's four are without a script naming them.

`PATH` holds `path` chunks: `NAME`, `INFO`, `PROP`, and `PNTS` (28 bytes per point: a position, then a rotation quaternion stored w, x, y, z). The `CP<n>Spawn` paths are each command post's spawn points.

## Sky and effects config (`sky_`, `fx__`)

These are trees of named values. A `DATA` chunk is the FNV hash of the name (u32), a value count (u8), that many 4-byte values, and a string table (u32 size, then the strings). A value is a float, or an offset to a string at values start + offset + 4. A `SCOP` after a `DATA` holds that entry's children.

- **SkyInfo:** `NearSceneRange` (fade start and end at the shortest and longest view distance), `FarSceneRange`, `AmbientColor`, `TopDirectionalAmbientColor`, `BottomDirectionalAmbientColor`, and `CharacterAmbientColor`. `FogColor` and `FogRange` are there but the PC game doesn't read them: it takes only those six, `VehicleAmbientColor`, `Enable`, and one colour whose name isn't known (hash 0x91e9c28b), the water reflection's fog (see "Fog" under Rendering).
- **SunInfo:** `Angle` (a0, a1 in degrees) and `Color`. The light travels along (−sin a0 cos a1, cos a0, −sin a0 sin a1).
- **DomeInfo:** each `DomeModel` names a sky model (`Geometry`). The engine draws them in order around the eye, without depth, before the far scene: the dome itself, then whatever stands on its horizon (Geonosis's arena, spire and rim; Hoth's mountains and ion cannon; Endor's satellite, tree line and Death Star; Kamino's city; Kashyyyk's mountains; Naboo Plains' waterfall and mountains; Rhen Var Citadel's two cloud layers; Tatooine's rocks; Yavin's gas giant; Bespin Platforms' Cloud City). A model with `Offset` or `MovementScale` s stands at height s × eye + (1 − s) × Offset instead of at the eye, so it sinks slightly as the eye climbs. The first model is opaque; the others keep their textures' alpha. `LowResTerrain` has the far scene's terrain texture and its scale, and its fog: `FogColor` (r, g, b, a), `FogNear`, `FogFar`. The fog's far end stretches by 255 / a, except at an a of 0 or 255, and the fog is only on when 0 ≤ `FogNear` < `FogFar` (see "Fog" under Rendering).
- **fx__** for the world holds `Blur` (soft focus: `ConstantBlend`, `DownSizeFactor`) and `SunFlare`, among others. The weather and haze sections:
  - `Effect("FogCloud")` (Enable 0 by default): `Texture` (fluffy), `Range` near and far (40, 90), `Color` (192, 192, 192, 160; three values leave the alpha at 255), `Velocity` x and z (0, 0), `Rotation` (0.1), `Height` (16), `ParticleSize` (28), `ParticleDensity` (30). The engine lays `ParticleDensity` points over every 256 m world cell (one draw of its random numbers from seed 1, so every cell holds the same pattern), scrolls the pattern by Velocity × time, and stands each point `Height` over the terrain height there. It keeps the points inside the bounding box of the view's slab between `Range` near and far (grown by ParticleSize × 0.707), drawing each as a camera-facing square `ParticleSize` across, turning at ±Rotation × π radians a second. Its alpha is the colour's × (1 − t²), t running −1..1 as the squared distance runs from near² to far². Geonosis (brownmist, 90 to 120 m), Endor, Rhen Var Citadel and both Yavins have it on; Bespin, Theed and Hoth carry it disabled.
  - `Effect("Precipitation")`: `Type` `Streaks` (rain) or `Quads` (Hoth's snow), `Texture`, `Range`, `Color`, `Velocity`, `VelocityRange`, `ParticleDensity`, `ParticleDensityRange`, `ParticleSize`, `StreakLength`, `AlphaMinMax`, `Rotation`, `RotationRange` (turns a second, default 2), `CameraCrossVelocityScale`, `CameraAxialVelocityScale`, and `GroundEffect`, `GroundEffectsPerSec`, `GroundEffectSpread`. ParticleDensity + ParticleDensityRange particles fill a 5 m box, and the box is repeated over the world's 5 m grid through the view out to Range, its alpha fading over the last third. Every half second each particle's wander (±VelocityRange per axis), alpha (one end of AlphaMinMax) and spin (Rotation ± RotationRange / 2) ease a fifth of the way to new random values. A quad is a camera-facing square reaching ParticleSize from its centre to each side. A streak runs back from its drop along its motion relative to the eye, StreakLength / 30 seconds of it. Regions named `rainshadow*` (Hoth, Kamino, Yavin Arena) keep it out of shelters.
  - `Effect("Wind")`: `Velocity` x and z, `VelocityRange`, `VelocityChangeRate`: the wind eases to a new target, the velocity give or take VelocityRange, every 1 / VelocityChangeRate seconds.
  - Also present, not yet drawn by the port: `Godray` (sun shafts: `Texture` fx_godray, `TextureScale` and ten unnamed values), `HeatShimmer` (Tatooine: a `shimmer_waves` distortion), `Lightning` and `LightningBolt` (Kamino, Yavin Arena: flashes, bolts and thunder sounds), `WorldShadowMap` (Endor: the `end_canopy` texture projected over the world), `ColorControl` (gamma, brightness, contrast).
- **sky_** also lists `SkyObject`s, the ships crossing the sky ("Sky flyers" below), and Rhen Var Citadel's DomeInfo has `Stars`, which the port doesn't draw yet.

### Sky flyers (`SkyObject`)

A `SkyObject` section of `sky_` is a class of ships crossing the sky: Geonosis's gunships, droid fighters and rising Techno Union core ships, and the capital ships over Bespin, Naboo Plains and Tatooine. Each section builds one class, which spawns, updates and draws its ships.

| Key | Default | Read as |
| --- | --- | --- |
| `Geometry` | none | the model, found by name among the models loaded when the level starts |
| `NumObjects` | 1 | ships always in the sky (a float, truncated) |
| `Height` | 70 100 | min and max height above the origin |
| `VelocityX`, `VelocityY`, `VelocityZ` | 0 0 | min and max speed along the ship's right, up and forward axes |
| `Acceleration` | 0 0 0 | added to that velocity every update, in the same axes |
| `Distance` | 300 | how far out across the ground they start |
| `InDirectionFactor` | 3 | how far inward they head (below) |
| `LifeTime` | 1000 | seconds (below) |

A range given as one value is that value; a key given twice keeps the last. The names are FNV hashes like every config key (`VelocityX` b6c8887e, `Acceleration` 2fa0ba9d, `LifeTime` 5212abee).

**Which battles draw them.** The `*_DOME` models live in the side archives, each in a sub-level of its own name (`SIDE/rep.lvl`'s `rep_fly_assault_dome` holds the model, its skeleton and its one texture). A battle has them only if its script reads that sub-level (`ReadDataFile("SIDE\\rep.lvl", "rep_fly_assault_dome", ...)`); a class whose model isn't loaded gets no ships (its count stays 0). So the sky config lists every era's ships and each battle draws its own:

| Map | SkyObjects | Battles that draw them |
| --- | --- | --- |
| Bespin: Platforms (bes1) | `rep_fly_assault_DOME`, `cis_fly_fedlander_DOME`, `all_fly_moncalamari_DOME`, `imp_fly_destroyer_DOME`: 2 each, Height 170 to 250, VelocityZ 20 to 50, Distance 1500, InDirectionFactor 2.5 | none: neither battle reads a dome sub-level |
| Bespin: Cloud City (bes2) | the same four | `bes2r`: the Republic assault ships. `bes2a`: none |
| Geonosis: Spire (geo1) | `rep_fly_assault_DOME` 2 (Height 200 to 300, VelocityZ 20 to 50, Distance 1500, InDirectionFactor 2); `rep_fly_gunship_DOME` 20 (Height 80 to 140, VelocityY −10 to 10, VelocityZ 80 to 120, Distance 600, InDirectionFactor 0.5); `cis_fly_droidfighter_DOME` 20 (the same, VelocityZ 80 to 100, Distance 300); `cis_fly_techounion_DOME` 8 (Height 0, VelocityY 10 to 12, Acceleration 0 2 0, Distance 1000, LifeTime 80) | `geo1r` and `geo1r_h`: all four (they also read `rep_fly_jedifighter_dome` and `cis_fly_fedcoreship_dome`, which no SkyObject names) |
| Naboo: Plains (nab1) | `cis_fly_fedlander_DOME` and `imp_fly_destroyer_DOME`, 1 each, Height 350 to 500, VelocityZ 30 to 50, Distance 2500, InDirectionFactor 2.5 | `nab1c`: the Federation lander. `nab1i`: the Star Destroyer |
| Tatooine: Dune Sea (tat1) | the four capital ships, 2 each, as on Naboo Plains | `tat1r`: Republic assault ships. `tat1i` (and `tat1i_h`): Star Destroyers |
| Tatooine: Mos Eisley (tat2) | as the Dune Sea | `tat2r`: Republic assault ships. `tat2i`: Star Destroyers. The historical `tat2i_h`: Mon Calamari cruisers |
| The other eleven maps | none | |

Kamino's Clone Wars script reads `cis_fly_fedlander_dome` too, but its sky has no SkyObject and nothing places one. The models are small: 72 to 200 triangles in one or two segments with one texture (32 × 32 for the gunship, droid fighter and core ship, 128 or 256 for the capital ships); the gunship is about 20 m long, the droid fighter 13 m, the core ship 41 m tall, the capital ships 280 to 550 m. All their textures are opaque; their segments are lit (MTRL 0x1) except the assault ship's engines (0x0), and the gunship's canopy blends (0x5).

**Motion.** Everything is about the class's centre, which is always (0, 0, 0), so the ships circle the world's origin, not the player.

- **Start**, with the engine's shared random numbers (the bot names' LCG, a float from its top 23 bits), in this order: a direction d = normalize(r₂ − 0.5, 0, r₁ − 0.5) (drawn again while both are 0; not uniform in angle, the square's corners are favoured); a height, Height min + r × (max − min); the ship stands at d × Distance at that height. Its heading is up × d, across d, negated when the next draw is over 0.5, plus −d × f with f = InDirectionFactor × (0.5 + 0.5 r) when InDirectionFactor is over 0. Its frame is the look-at frame of the heading with the world's up: level, its model's z along the heading, x along up × heading. Then VelocityZ, VelocityY and VelocityX, each min + r × (max − min), the class's Acceleration, an age of LifeTime × (0.8 + 0.2 r), and a fade of 0.
- **Each frame:** a ship with fade ≥ 0 whose position is more than Distance + 10 m from the centre across the ground (x² + z²), or whose age is over LifeTime, gets fade −1. Then its age grows by the frame's time, fade becomes min(fade + 0.25 × dt, 1), and a ship whose fade just reached 0 from below starts again instead of moving. Any other ship adds Acceleration × dt to its velocity and moves by (right × vx + up × vy + forward × vz) × dt. Its frame never changes: no turning, no banking.
- **So** every ship of the class is always in the sky, fading in over 4 s, flying, or fading out over 4 s while it flies on. The fade is drawn as its absolute value, so a ship cut short while still fading in jumps to full alpha as its fade-out starts. With the age starting at 80% to 100% of LifeTime, a ship lives at most a fifth of it: up to 200 s by default, up to 16 s for Geonosis's core ships, which start at height 0 (some 60 m above the plain) 1000 m out and rise at 10 to 12 m/s, 2 m/s faster each second (up to about 450 m before they fade). A straight course from Distance out passes Distance / √(1 + f²) from the centre: the gunships (InDirectionFactor 0.5) cut chords of their 600 m ring 540 to 580 m from it and cross it in 3 to 7 s, so they are seldom wholly faded in before they jump to full and fade out, Geonosis's assault ships pass 670 to 1060 m out, Cloud City's 560 to 940 m, and the capital ships over Naboo Plains and Tatooine (Distance 2500, 2.5) 930 to 1560 m.
- **Updates** run once a rendered frame with the frame's time, not on the simulation's tick. Whether they stop while the game is paused is unknown.

**Drawing.** The class is drawn in the far scene with the far pass's own depth buffer, after the sky dome (which draws its models without depth or fog) and before the world's objects. Fog is off while it draws. Each ship's model is drawn with the tint (255, 255, 255, |fade| × 255), lit by the level's ambient and sun and no point lights, and depth tested. A model with a tint alpha under 255 blends every segment, but its opaque segments keep writing depth while they blend; a segment that blends or adds by its material (MTRL 0x84) never writes depth. So a fading ship still hides a ship behind it. The far scene's far plane (`FarSceneRange` at the view distance) clips them: 1000 m on Geonosis, Naboo Plains and Mos Eisley, 2000 m on the Dune Sea, 500 m on Cloud City, whose assault ships pass at least 557 m from the origin and 170 m up, so only the edge of the city sees them at the original view distance.

`tools/zero_sky.py` writes each map's `sky/` folder beside `map.json` for the battles that draw ships (`flyers.json`: the classes, which battle draws which, the models' parts and textures; `flyers.bin` and `flyers_index.bin`; `textures/`). None of it is in the content fingerprint. `src/sky-flyers.js` flies them page-side and draws each class's model parts as instanced meshes in the far scene, without fog, after the far terrain (docs/environment-effects.md, "The sky's flyers"). That is Classic. Graphics > Ocean and sky's Enhanced (map generation 16) also draws every class of the battle's era, past the far plane, with smooth fades (docs/environment-effects.md, "Enhanced"); `flyers.json` then lists each battle's classes twice, as the original draws them (`missions`) and as the Enhanced sky does (`enhanced`), with each class's `era`.

### Placed haze (`dusteffect`) and attached effects

A `dusteffect` class (Geonosis `geo_prop_dust`, Hoth `hoth_prop_dust` and `hoth_prop_mist`, Kamino `kam1_prop_mist`, Rhen Var Citadel `Rhn2_prop_cloud`, Tatooine Dune Sea `tat_prop_dust`, both Yavins `yav_prop_mist`) is a cloud of camera-facing squares shared by every copy of the class: `NumParticles` (50), `MinPos`/`MaxPos` (±20, ±1, ±20) and `MinVel`/`MaxVel` (±3, ±1, ±3) per axis, `MinSize`/`MaxSize` (10, 15), `MinLifetime`/`MaxLifetime` (2, 3), `Alpha` (0.5), `MinDistance` (0), `MaxDistance`, `RadiusFadeMin`/`RadiusFadeMax` (off), `HeightScale` (0.2) and `Texture` (whitesmoke); `CameraDistance` and `SpawnSound` exist but no stock map sets them. A particle's alpha rises at 1 a second to Alpha; in its last second it is Alpha × life left × that. Past RadiusFadeMin from the copy's vertical axis it fades out, and past RadiusFadeMax it starts again. A placed copy keeps only its position: its frame is the world's with the vertical axis scaled by HeightScale, so the particles and their squares lie flattened into low sheets. A copy draws while its view depth plus the class's radius (half MaxSize, plus RadiusFadeMax when the fade is on, else the fastest speed over the longest life plus the farthest start) lies between MinDistance and MaxDistance, with its alpha × min(1, (MaxDistance − that) / 200).

A class's `AttachEffect` names the effect that every `AttachToHardPoint` after it carries, until the next `AttachEffect` or `AttachOdf` (at most 30 a class; a hardpoint the model lacks attaches nothing, and an effect no archive defines, like Kashyyyk's `kas1_fireflies` and `kas_dryice`, attaches nothing). The retail `576b09cd` property names an effect and its hardpoint together.

## Mission scripts (`mission.lvl`)

Each battle's setup is a compiled Lua 4.0 chunk (version byte 0x40, 4-byte floats) inside a named chunk of `mission.lvl`. Names are the map plus an era letter: Bespin: Cloud City's Clone Wars battle is `bes2r` and its Galactic Civil War battle is `bes2a`; `_h` marks the historical campaign's version. Running the setup function's straight-line code on literal values recovers its calls:

- the sides it loads (`ReadDataFile("SIDE\\rep.lvl", ...)`);
- the teams (`SetTeamName`, `SetTeamIcon`, `AddUnitClass`, `SetHeroClass`);
- unit and reinforcement counts, bleed thresholds, and the attacking team;
- death regions (`AddDeathRegion`).

`bes2r` puts the Republic on team 1 and the CIS on team 2, with 25 units and 200 reinforcements each. Command post ownership is not in the script. It is each post's `Team` instance property.

A map's battles are the scripts named for it: the map, an era letter (`r` and `c` are the Clone Wars, `a` and `i` the Galactic Civil War, as the shell's playlist letters name them) and `_h` for the historical campaign's version. Each reads its world with `ReadDataFile("<PLANET>\\<world>.lvl")`. That is the map's own archive for every battle but two: the campaign's Theed and Naboo Plains (`nab2c_h`, `nab1c_h`) are fought in `NAB/nab2_h.lvl` and `nab1_h.lvl`, worlds of their own (`TAT/tat1h.lvl` and `YAV/yav2_h.lvl`, which `tat1i_h` and `yav2i_h` read, are byte for byte `tat1.lvl` and `yav2.lvl`). Every historical version but `rhn1r_h` calls `SetHistorical`, after which teams 1 and 2 take the vehicle pads' `ClassHisATK` and `ClassHisDEF`. The extraction keeps every battle of a map's world (`tools/extract.py` `map_missions`), the two in other worlds recorded with their `world` and nothing that depends on it.

`common.lvl` holds the interface scripts the same way. Its `globals` script builds plain tables, which running its top-level code with tables recovers. The first-person camera motions are there: `RollLeft`, `RollRight`, `Crouch`, `CrouchToProne`, `StandToProne`, `ProneToStand`, `CrouchToStand`, and `Land`. Each has `Size` (the segment count), `Vec` and `Slope` (eye offsets in the camera's frame, x right, y up, z back, and their tangents, in metres), `Ore` and `RSlope` (Euler angles x, y, z in radians, and their tangents), and `Time` (seconds per segment). The game reads them by name at mission start and scales nothing.

## Localization (`Locl`)

`core.lvl` holds a `Locl` per language: `NAME` (such as `english`) and `BODY`. The body is a run of entries: a u32 key hash, a u16 entry size including this 6-byte header, then UTF-16 text. Keys the port uses:

- A command post's display name is `level.<world>.<Label>`, so `level.bespin2.CP1` is "COURTYARD".
- A weapon class's name is `weapons.` and the class name with its first two underscores as dots: `rep_weap_inf_rifle` is `weapons.rep.weap.inf_rifle`, "BLASTER RIFLE".
- A side's name is `common.sides.<side>.name` ("REPUBLIC", "CIS").
- The message feed's lines are `game.infomsg.captured` ("%s CAPTURED THE %s"), `game.infomsg.lost`, `game.infomsg.killed`, and so on.
- The game asks for some text by hash: 0x0634F877 is "VICTORY IN" and 0xEDD20CDE is "DEFEAT IN".

## Add-ons (`GameData/AddOn/<name>`)

A map can be installed beside the game rather than in `Data/_LVL_PC`. The Steam copy installs Jabba's Palace this way, in `AddOn/Tat3`. The game treats a folder under `AddOn` as an add-on when it holds `addme.script` and `Data/_LVL_PC`. Tat3 spells these `_lvl_pc` and `Side`, so every name matches without regard to case, as on Windows.

- `addme.script` is a ucfb archive holding one compiled Lua 4.0 chunk, `addme`. The game runs it at startup. It appends the add-on's map to the shell's two lists, `sp_missionselect_listbox_contents` and `mp_missionselect_listbox_contents`, at `getn(list) + 1`. The entry has the same fields as the stock `missionlist` entries: `mapluafile = "TAT3"`, `showstr`, one `side_<letter>` flag per era, and `dnldable = 1`. The script then registers each mission with `AddDownloadableContent("TAT3", "TAT3c", 4)` and `("TAT3", "TAT3a", 4)`. Tat3's `showstr` is the literal "TATOOINE: JABBA", not a localization key. The shell's `missionlist_GetLocalizedMapName` shows such a string as written when localization has nothing for it.
- `Data/_LVL_PC` follows the game's own layout. `mission.lvl` holds the add-on's missions (`tat3c`: Republic against CIS; `tat3a`: Alliance against Empire; Gamorrean guards as team 3 in both). `core.lvl` is a full copy of the game's localization plus 22 strings of its own: the command posts `level.tat3.cp1` to `cp5` (BARRACKS, THRONE ROOM, PRISON, KEEPERS CHAMBERS, DROID REPAIR BAY) and the objectives. `TAT/tat3.lvl` is the world. `Side/gam.lvl` is a new army, the Gamorrean guard, whose body is a soft skin (`SKIN` type 3; see Models above). `sound/tat.lvl` holds the sub-levels `tat3cw` and `tat3gcw` and the streams `tat3` and `tat3_emt`. Each sub-level has two banks: one external, whose samples are in the game's `common.bnk`, and one with 18 samples of its own data. `Movies/tat3.mvs` holds one movie, the Instant Action flyover `tat3fly` (720x480, 24 s, silent). `Shell/ENG/shell.lvl` is only the movie config for it. There are no loading pictures, and `Load/load.lvl` has no `World` entry for `tat3`.
- An add-on's mission scripts mark their own files with `dc:`: `ReadDataFile("dc:TAT\\tat3.lvl")`, `ReadDataFile("dc:SIDE\\gam.lvl", ...)`, `ReadDataFile("dc:sound\\tat.lvl;tat3cw")`, `OpenAudioStream("dc:sound\\tat.lvl", "tat3")`. A path without `dc:` is the game's own: `SIDE\\rep.lvl`, the era's music from the game's `sound\\tat.lvl`, and its voice-over from `sound\\cw.lvl`. The add-on's `sound/tat.lvl` and the game's `Sound/tat.lvl` share a name and are different archives, so a path is resolved by its prefix and never by looking in both places.

`tools/zero_addon.py` reads add-ons this way for every tool. A map or side archive comes from the game when it has one, else from an add-on. A map's world is `<PLANET>/<map>.lvl` by name; when neither has that file, it is the world the map's battles read. A community add-on's map id need not name its world: one listing `georuins` reads `GEO\\geo1.lvl`, so the name alone found nothing. The mission scripts are `mission.lvl`'s followed by each add-on's, with the game's script kept when names clash. The text is `core.lvl`'s with each add-on's new keys added. The Instant Action list is the `missionlist` entries followed by the entries each `addme` script appends, and the flyovers come from each add-on's movie archive as well. What the extraction writes for an add-on map is laid out like a stock map's (`maps/tat3/`, `sides/gam/`, `audio/tat3cw/`), so the runtime and the server need nothing of their own for it.

## Fonts (`font`)

`core.lvl` holds the HUD's bitmap fonts: `gamefont_tiny` (18-pixel lines), `gamefont_small` (21), `gamefont_medium` (23), `gamefont_large` (28), `gamefont_super_tiny`, and `starwars_small`. A `font` chunk is:

- `NAME`;
- `HEAD`: the glyph count (u16), page count (u8), line height (u8), and baseline (u8);
- `FTEX`: the glyph pages as `tex_` chunks (A4R4G4B4, white glyphs in the alpha);
- `FBOD`: 24 bytes per glyph. The character code (u16), page (u8), advance (u8), the glyph's box x0, x1, y0, y1 in pixels from the pen and the line's top (u8 each), then u0, u1, v0, v1 (f32) on the page.

Text is laid out glyph by glyph: each quad runs from (pen + round(x0 × s), top + round(y0 × s)) to (pen + round(x1 × s), top + round(y1 × s)), and the pen moves on by round(advance × s) plus a spacing of 1.

## Sound (`Sound/*.lvl`, `common.bnk`)

`GameData/Data/_LVL_PC/Sound` holds one archive per planet (`nab.lvl` for Naboo), `cw.lvl` and `gcw.lvl` (each era's voice-over), `shell.lvl` (the front end), and `common.bnk`. A mission reads its sound sub-level with `ReadDataFile("sound\\nab.lvl;nab2cw")` and opens streams with `OpenAudioStream("sound\\nab.lvl", "nabcw_music")`. `tools/zero_sound.py` reads them (see `docs/audio.md` for what the port does with them).

These are ucfb trees like the other archives, but most chunk tags are FNV hashes of names (`zero_hash.fnv1a`), so they don't read as text:

| Chunk | Contents |
| --- | --- |
| `StreamList` (0x5bb97f21) | One stream: `Stream` (0x5f6f6d65, which happens to spell `emo_`) holding `Info` and `Data`. Each is padded with a `_pad` chunk so data starts on a 2048-byte boundary. |
| `lvl_` | A sub-level: a name hash (`nab2cw`, `nab2gcw`, `nab1cw`, `nab1gcw` in `nab.lvl`) and an inner size, then a `SoundBankList` and config chunks `snd_`, `mus_`, and `ffx_`. |
| `SoundBankList` (0x5e262256) | `SampleBank` (0x9a644d15) holding `Info` and, in `common.bnk` and `core.lvl`, `Data`. |

`common.bnk` is a single bank of every sample the levels use (1554 of them, 71.8 MB). A level's bank lists the ids it needs, with no `Data` (its `694aaa0b` field is 1). Those samples come from `common.bnk`. The level bank's `Info` describes many of them at 44100 Hz while `common.bnk` holds them at 22050 Hz, with the same duration. Only the 22050 Hz data exists on the PC, and it is what plays. `core.lvl` has its own small bank with data (six interface sounds), the buses, and the default reverb settings.

**Info** is a run of (name hash, u32) pairs. `SegmentInfo` (streams) or `SampleInfo` (banks) opens a list of `Sample` … `SampleEnd` records:

- Stream header: `Name`, `Format`, `NumChannels`, `ChannelInterleave`, `NumSegments`, `Size`, `NumSubstreams`, `SubstreamInterleave`, `Padding`.
- Bank header: `Name`, `Format`, `NumSamples` (the count), `Size`, 694aaa0b, `Padding`.
- Per sample or segment: `Id` (the hash of its name), `Frequency`, `Size`, a frame count (1d48feef), `Padding`, and for six samples in `common.bnk` an `Alias`. An alias names another sample whose data it plays, and takes no space.
- Samples in `Data` follow one another; segments are each followed by their padding.

**Formats.** 2 is 16-bit PCM, little-endian, mono: every bank sample. Rates are mostly 22050 Hz; 52 are at 44100, and a few at 16000, 11025, 8000, 7056, and even 320 Hz. 5 is Xbox IMA ADPCM, every stream:

- 36 bytes per channel encode 64 frames. A 4-byte header gives an int16 sample, a step index, and a pad byte; the header sample is the first frame.
- 32 bytes of 4-bit codes follow, low nibble first. Only 63 are decoded; the last is unused.
- Channels interleave every 4 bytes (`ChannelInterleave` 4).
- The reference IMA step rule applies (step/8, plus step, step/2, and step/4 by the code's bits), as in the original game. Decoders that multiply instead (FFmpeg's `adpcm_ima_wav`) differ by a few LSB.
- A stream with two substreams interleaves two of these in `SubstreamInterleave`-byte chunks (36864, or 512 stereo blocks). Theed's ambience streams `nab1` and `nab2` are two stereo pairs; the second is quieter and correlates about 0.8 with the first.
- Segment `Frequency` values are a few hertz off round numbers (48001, 44125). They are used as written.

Theed's streams: `nabcw_music` (24 segments) and `nabgcw_music` (25), stereo at 48 kHz; `nab1` and `nab2`, the ambience (one segment each, two substreams, 44.1 kHz); `nab2_emt` (mono). In `cw.lvl` are `cw_vo` (314 segments, mono, 44.1 kHz) and `cw_tac_vo` (63, 22 kHz).

**Configs** (`snd_`, `mus_`, `ffx_`) are DATA/SCOP trees like `sky_`, but every string is stored as its FNV hash in the value slot. The objects:

- **SoundProperties** (also accepted as `Sound`): `Name`, `Inherit`, `Group`, `Bus`, `Pitch`, `PitchDev`, `Gain`, `GainDev`, `ReverbGain`, `Looping`, `Pan`, `Mode3D`, `ModeDoppler`, `Priority`, `Bias`, `MinDistance`, `MuteDistance`, `MaxDistance`, `RollOff`, `RollIn`, `RollInDistance`, `PlayProbability`, `PlayInterval`, `PlayIntervalDev`, `RandomPlayPos` (3af79d76), and a `SampleList` of `Sample(id, weight[, interval, intervalDev])` with an optional `CyclePlayback` (4d5af670).
- **SoundStreamProperties** (and `SoundStream`, 0x2e93ef4c, the music's): the same keys, plus `Stream` and a `SegmentList` of `Segment(id, weight[, …])`.
- **SoundLayered**: `Layer(layerName, sound, PlaybackDelay[, PlaybackDelayDev])`.
- **SoundParameterized**: a `SoundLayered` and, per layer, `ParameterGraph(input, graph, output)`. The input is a name such as `Speed`, `Acceleration`, or `Health`; the output is `Gain` or `Pitch`.
- **ParameterGraph** (e657a914): `ControlPoint(x, y)` points.
- **Bus**: `Name`, `Gain`, `Parent`, and `AttenuateBus`, `Attenuation`, `AttenuationLeadIn`, `AttenuationLeadOut`; also `Mute` and `Solo`. `core.lvl`'s buses:

  | Bus | Gain | Parent | Notes |
  | --- | --- | --- | --- |
  | root | | | |
  | shellfx | 0.3 | root | |
  | ingame | 1 | root | |
  | main | 1 | ingame | |
  | soundfx | 1 | main | |
  | music | 1 | main | |
  | ingamemusic | 0.5 | music | |
  | shellmusic | 0.6 | music | |
  | shellmovies | 1 | ingame | |
  | ambience | 0.7 | ingame | |
  | voiceover | 0.6 | ingame | attenuates main by 0.5, 0.5 s each way |

- **I3DL2ReverbPreset** (abd36816): `RoomGain`, `RoomHFGain`, `RoomRollOff`, `DecayTime`, `DecayHFRatio`, `ERGain`, `ERDelay`, `ReverbGain`, `ReverbDelay`, `Diffusion`, `Density`, `HFReference`.
- **space**: a preset (`I3DL2ReverbPreset`), `DirectGain`, `DirectHFGain`, `RoomGain`, `RoomHFGain`, `RoomRolloffFactor`, `ObstructionHFGain`, `ObstructionLFRatio`, `OcclusionHFGain`, `OcclusionLFRatio`. Theed's one space has `RoomGain` 0, so it is dry.
- **Music** (`mus_`): `Name`, `Priority`, `FadeInTime`, `FadeOutTime`, `MinPlaybackTime` (6813be83; 5 s on Theed), `MinInactiveTime` (35724887; 90, 60 or 30 s), and the `SoundStream` it plays.
- **FoleyFXSoldier** (`ffx_`): per soldier `Class`, and sounds for `FootstepLeftWalk` (2a9f6673), `FootstepRightWalk` (e79eb7c2), `FootstepLeftRun` (640f6efb), `FootstepRightRun` (a4df1b00), `FootstepLeftStop` (b59a8702) and `FootstepRightStop` (54e8af1f) (never played), `jump`, `Land`, `roll`, `prone`, `squat` (65bfa4e1), `stand`, and `bodyfall` (70dc49d0).
- **FoleyFXGroup** (a1c9cbc5): a surface material (`dirt_foley`, `metal_foley`, `stone_foley`, and the built-in `terrain_foley` (26c04c0e) and `water_foley` (425aa124)), listing its `FoleyFX` entries. Props and buildings name their material with the same key; a soldier class picks its entry with `FoleyFXClass` (99d55922).

**Class sound keys.** Classes name their sounds in ODF properties, a SoundProperties name optionally followed by `defer`. `defer` only delays the name lookup until the banks are loaded.

- **Weapons:**
  - `FireSound`, `FireLoopSound`, `FireEmptySound`, `ReloadSound`, `ChargeSound` and `ChargeSoundPitch`, `WeaponChangeSound`, `OverheatSound` and `OverheatSoundPitch`, `FireSoundStop`.
  - Ten movement slots: four for steps (b77f3981, b989fc18, 1064c0a7, 10a991e4: left and right walk, left and right run; no class sets them), then `JumpSound`, `LandSound`, `RollSound`, `ProneSound`, `SquatSound`, `StandSound`.
  - The game doesn't use `ChangeModeSound` or `OverheatStopSound`, though 45 classes set the latter.
- **Ordnance:** `OrdnanceSound` and `CollisionSound`; grenades and mines `CollisionOtherSound` (edc6251a) for a bounce or landing, `CollisionWaterSound` (2297e47d), and `CollisionFoliageSound` (b8c524f1).
- **Explosions:** `SoundProperty`.
- **Soldiers:** `HurtSound`, `DeathSound`, `LowHealthSound` (f8d3e5d2), `FleeSound`, and unnamed chatter keys (the AI's).
- **Entities:** `EngineSound`, `TurnOnSound`, `TurnOffSound`, `AmbientSound`, `Ambient2Sound`, and `Footstep0Sound` to `Footstep2Sound`.
- **Items:** `PowerupSound`.
- **Command posts:** `AmbientSound` (one per side: `"<side> <sound>"`), `ChargeSound`, `CapturedSound` (f17a02e6), `DischargeSound`, `LostSound`, `DisputeSound`, and `SoundPitchDev` (a33d3401). `CaptureMusic` (6b9b4728) and `LostMusic` (42cb8c86) are read but unused.
- **Turrets:** `TurretYawSound`, `TurretPitchSound`, `TurretYawSoundPitch` and `TurretPitchSoundPitch`, `TurretActivateSound`, `TurretDeactivateSound`.
- **Static emitters:** the world's `com_snd_amb_static` instances set `Sound` (0x0e0d9594), `MinDistance`, and `MaxDistance`. Theed has 36 of them, `canal` and `fountain`.

## Engine behavior

These notes describe how the 2004 PC game behaves with the data above, as the engine reproduces it. Where the port differs, a note says so.

### Soldier class defaults

When a soldier ODF leaves a property unset: `MaxSpeed` 9, `MaxStrafeSpeed` 7, `MaxJumpSpeed` 8, `MaxTurnSpeed` 2, `MaxPitchSpeed` 0.5, `Acceleraton` 70, `PitchLimits` -0.8 1.1 (radians, positive is up), `CollisionThreshold` 10 3 0, `CollisionScale` 1 0 0 (see "Collision damage"). Each camera section defaults to position easing rates of 15 and a rotation easing rate of 30.

### Soldier movement, per tick

Each tick moves the soldier first, then runs the state step (landing, jumping, posture changes) on the ground contact the previous tick found, then runs collision.

- The stick is clamped to [-1, 1] per axis and scaled down to length 1 if longer. Posture scales it: standing 1 forward and 1 sideways; crouched 0.7 and 0.6; prone 0.3 and 0.2 (and turning 0.5); in a jump or fall 0.2 and 0.2 (and turning 0.5). It is also scaled by the ground normal's y. Whenever the last collision found no ground, the ground normal goes back to straight up at the start of the tick.
- Velocity is split along two ground axes: the facing direction and its right, each crossed with the ground normal so they run along the slope.
- On the ground, a downhill pull of `Acceleraton` x 0.4 (along the slope) is added and any velocity into the ground is removed.
- Braking: on ground whose normal y is over 0.75, each axis slows toward zero by `Acceleraton` x 0.6 x a posture factor (1.2 standing, 1.7 standing while held still, 1.0 crouched or prone, 0 in the air) per second.
- Driving: each axis with stick input moves toward `MaxSpeed` x stick (forward and back alike) or `MaxStrafeSpeed` x stick (sideways) by `Acceleraton` x 1.2 per second, times 0.2 in the air and 0.01 when the ground normal's y is under 0.75. It never pushes past the wished speed and never pulls a faster axis back down.
- Gravity is 18 m/s², only when the last collision found no ground (13.5 while thrown). There is no terminal speed.
- The position then moves by the velocity, and turning input turns the body at `MaxTurnSpeed` rad/s times the posture's turn scale.
- Aim pitch is clamped to `PitchLimits`, or ±0.4 prone.
- Getting up from prone holds the soldier still for the length of the get-up clip, and so does a hard landing for 0.5 s: no movement, turning, or looking, and the standing brake is 1.7.

### Soldier collision

The soldier collides with the world through spheres in its own frame (y up, z forward). The game builds them in code rather than reading them from the model:

| Posture | Spheres (center y, center z, radius) |
| --- | --- |
| Standing, and in a jump or fall | lower body (0.57, 0.15, 0.57), upper body (1.48, 0.15, 0.57) |
| Crouched, and diving | lower body (0.57, 0.15, 0.57), upper body (1.18, 0.15, 0.57) |
| Prone | (0.72, 0.2023, 0.8), upper (0.3465, 0.6075, 0.385), lower (0.3465, -0.6075, 0.385), (0.3465, -0.2023, 0.385) |

(The game also builds `main_body` for bumping into other soldiers, droids, and droidekas, and body and head shapes for being shot. They do not touch the world. Against vehicles, buildings, and terrain the soldier uses the spheres above; the vehicle doesn't give way.)

**Other soldiers:** soldiers, droids, and droidekas push each other apart with `main_body`, an upright cylinder per posture: standing radius 0.57, height 1.65, centre 0.9 m up; crouched 0.45, 1.45, 0.65; prone or dead 0.8, 0.45, 0.25 (not scaled by `CollisionRootScale`). A contact whose normal points down does nothing; one pointing up while the soldier isn't rising stands him on the other as ground; otherwise each moves by the depth times the other's share of their bounding-box volumes. Offline the pass only runs within 60 m of a camera. There is no collision damage between soldiers.

**Being shot:** rounds test only `coll_body` and `coll_head`, built per posture (s = `CollisionRootScale`, 1 unless set, and S = max(s, 1); x is the soldier's left):

| Posture | coll_body | coll_head |
| --- | --- | --- |
| Standing, and in a jump, fall, or jet | cylinder r 0.35, height 1.55s, centre 0.8s + 0.17(S − 1) up | sphere r 0.17S at (−0.09, 1.65s, 0.16) |
| Crouched, and diving | cylinder r 0.35, height 0.9s, centre 0.6s + 0.17(S − 1) up | sphere r 0.17S at (−0.12, 1.2s, 0.25) |
| Prone, and dead | box half extents (0.25, 0.2, 0.87s) at (0, 0.2, −0.25s) | sphere r 0.17S at (−0.07, 0.28s, 0.75s) |

A head hit does 3 times the damage (the class's `HitLocation`, defaulting to the head at 3) and plays `headshot`. Blasts have no hit location.

`HitLocation "<primitive> <scale>"` fills a per-class table of up to 10 entries; round damage looks the struck primitive up in it and scales by what it finds, playing `headshot` (hash d7a77563) above 1. Only soldier classes get a default, the head at 3, when the table is empty; no retail class names a `HitLocation`. So nothing but a soldier's head takes more than a round's damage: a droideka has no head hit.

A droideka is struck through its `OrdnanceCollision` primitives (`cis_walk_droideka`, on DummyRoot): `p_ord`, an upright cylinder of radius 0.8 and height 2, its centre 1.0 m up and 0.506 m ahead, which takes in the head and the guns, and `p_rollsphere`, a sphere of 0.65 m resting on the ground. Rolled, its collision keeps only the ball; the port strikes the rolled droideka through the ball alone (whether rounds' rays do the same in the original game is not known). `p_droideka_head` (r 0.4) belongs only to the death chunk `droideka_chunk4`.

Turret gunners shown in the open are struck through their posed body (docs/live-objects.md); a round meets the turret's own collision first where it covers him, as on the Geonosian turret (`p_maingun1`, `p_maingun3` about 2 m in front of his head) and the Hoth dish, so a head shot from in front of those guns stops on the gun.

**Friendly fire:** rounds stop on the first soldier they meet whatever his team, the shooter excepted; blasts reach everyone, the thrower too. A hit from the victim's own team number (an ally of another number takes it in full) is scaled by Options › Game › Friendly Fire, on (×1) by default and off ×0; a bot's hit on a human teammate does nothing.

**Dying:** the body lies prone for 4 s (it stops rounds, takes no damage, and still bumps soldiers), then it is gone. A Character can respawn 4 s after a death; the local human's spawn screen opens then. Only living soldiers on foot whose class has `CapturePosts` (1 for soldiers and droidekas, 0 for the stock heroes) count toward a command post. The last soldier to hurt the dead one is credited; every death costs the team a reinforcement.

- **Steps:** the move since the last collision pass is cut into equal steps no longer than 0.57 m standing, 0.45 m crouched, or 0.1375 m prone. At each step the terrain is tested first, then buildings and props. A move longer than 100 m skips collision.
- **Terrain** is a height field. A sphere touches it when its bottom is below the ground straight under its center, by no more than its diameter. The push is that vertical depth times the ground normal's y, along the ground normal.
- **Buildings and props:** each sphere gathers the polygons near it, takes the nearest one it overlaps, and is pushed out along the line from that polygon's closest point to the sphere's center. It then searches again from where it ended up. Each polygon is used at most once per step.
- **Each contact** (normal n pointing at the soldier, depth d) is handled in turn:
  - A lower-body contact with n.y over 0.6 is ground, unless the soldier is moving away from it faster than 0.1 m/s. The most level ground contact of the pass becomes the ground normal.
  - A steeper lower-body contact stops only horizontal motion. If it faces down, it also pushes only sideways.
  - An upward-facing upper-body contact stops only horizontal motion. A ceiling over sloped ground pushes along the slope.
  - Prone, ground contacts average into the ground normal, and walls push along the ground.
  - Velocity into the contact is removed, and horizontal speeds under 0.1 m/s are zeroed. Then the soldier moves by the push, except that an upward push stops 0.02 m short, so the floor is still touched next tick.
- **No single ground contact:** the mean of the lower body's upward contacts counts as ground if its y is over 0.6. There is no downward ground ray.
- **Results:** a standing soldier stops 0.72 m from a wall it faces, 0.57 m sideways, and 0.42 m backing up. Curbs up to about 0.2 m count as ground at their edge, so running over one turns forward speed into a small hop. Edges from about 0.21 m to 0.57 m high are walls.

### Jumping and landing

- **Space standing:** jumps once the soldier has been standing for 0.5 s. A jump sets vertical speed to `MaxJumpSpeed` and adds `MaxJumpSpeed` x 0.25 x the forward stick along the facing. The jump happens after that tick's move, so the soldier still touches the floor that tick and the next tick has no gravity.
- **Space crouched or prone:** after 0.5 s, it stands the soldier up instead, if a ray from 0.55 m up has 1.35 m of headroom.
- **Space with the stick hard to one side** (sideways over 0.9, more sideways than forward) dives that way from a stand or crouch. A dive drives sideways at 1.7 x `MaxStrafeSpeed`, with 0.1 of the forward stick and 0.6 of turning. It uses the crouched body and lasts 0.75 of the dive clip, then returns to the stance it started from.
- **Landing:** a jump or fall lasts at least 0.3 s before it can land, and landing returns the soldier to standing. A landing after more than 2 s in the air is hard and holds the soldier still for 0.5 s.
- **Thrown:** an explosion's push of more than 2 m/s upward throws the soldier (gravity 13.5 while thrown); he lands as from a fall. A soldier the blast kills is thrown all the same and lands dead in the `thrown_*` rest pose, and a body already lying on the field is thrown too (a droid that blew apart has nothing left to throw). In the retail data every pushing explosion's push radius lies inside its full-damage radius (the thermal detonator's 320 damage and 10 push both reach 4.5 m), so with 300 health it is the killed soldier who flies.
- **Ceiling:** the mission's `SetMaxFlyHeight` (200 unless set; Cloud City sets −10, Theed −20) pushes anyone in the air back down: within 5 m of it, the vertical speed loses 10 × (5 − headroom) m/s per second.
- **Leaving the ground standing:** the stand state's timer doubles as a drop timer. It jumps to 10.3 when the soldier leaves the ground (to 10 if the soldier had stood for 5 s), and past 11 the soldier falls, so after 0.7 s (or 1.0 s). Touching down again from there sets the timer to 5. Crouched or prone, a drop never turns into a fall.
- **Landing hard hurts:** see "Collision damage". A landing plays `Land` and the weapon's `LandSound`, soft and hard alike; the damage plays `HurtSound` or `DeathSound` as any other.

### Collision damage

There is no fall rule as such: falls hurt through collision damage, and the droideka's works the same way with its own defaults. For each contact with something other than a power-up, the body's velocity less the velocity of what it touches is taken along the contact normal n (pointing at the body); for a soldier touching a vehicle his own horizontal speed is dropped first, and against a building or prop that speed is scaled by n.y, so walls and ceilings of buildings never hurt. Terrain counts in full. If it moves into the surface faster than

threshold = T0 + T1 (n · up) + T2 (n · forward)

it takes (S0 + S1 (n · up) + S2 (n · forward)) × (speed − threshold)² as ordinary damage (every health-type and shield scale 1), credited to whoever controls what it hit, so nobody for the ground ("%s DIED"). T and S are the class's `CollisionThreshold` (hash 2cdb588a) and `CollisionScale` (1a87bacf), each read as "%f %f %f" over the defaults, so "1.5" sets the first only.

| Class | `CollisionThreshold` | `CollisionScale` | On level ground |
| --- | --- | --- | --- |
| Soldier default | 10 3 0 | 1 0 0 | hurts past 13 m/s, a fall of about 4.7 m at 18 m/s²; (v − 13)² kills 300 health from 30.3 m/s, about 25.5 m |
| `rep_inf_jet_trooper`, `imp_inf_dark_trooper`, `imp_inf_dark_troopersnow`, `geo_inf_geonosian` | 10 3 0 | 0 0 0 | never hurt |
| Droideka default (`cis_inf_droideka` sets neither) | 7.5 2.5 0 | 1 0 0 | hurts past 10 m/s, a fall of about 5.1 m at 9.8 m/s² |

Measured in the port on Theed's plaza (a clone trooper, 300 health): 4.7 m and less nothing; 5 m 0.3; 6 m 2.9; 8 m 16.8; 10 m 34.8; 12 m 59.3; 15 m 102; 20 m 188; 25 m 289; from 26 m he dies.

Besides, the soldier update kills a soldier whose vertical speed is below −60 m/s, some 100 m of free fall, or who is more than 1000 m from the origin in x or z, or more than 500 m above or below it. Landing in water is no exception (water stops nobody; the bottom does), nor is a dive.

The port leaves out moving bodies: there the original game drops the soldier's horizontal speed and subtracts the hull's, but the port's soldier has no velocity exchange with a hull and would take the hit again every tick. A soldier contact that stops none of his motion (his upper body over a ledge, a slope too steep to stand on) does no damage in the port either, for the same reason. The droideka's rolling ball is driven into a wall again every tick in the port (the original's bounces off it), so only what a droideka comes down on hurts it there.

### Leaving the battlefield

The battlefield is the map's `bnd_` boundary (docs/ai-nav.md, "boundary"): a point is outside when no polygon holds it, tested in x and z only by the half-plane rule. Fourteen stock maps have one; Yavin's Temple and Arena have none, so nothing there is outside.

- **The count** runs while the Character has a body, for players and bots alike, heroes too: it tests the body, what he boarded (the hull or turret he sits in) and the recon droid he flies. With none outside, there is no count. When one is outside and no count is running, a 10 s count starts. Each update takes dt off; once it is strictly below 0, every one still outside is killed outright, with no one credited: his body, a hull (and everyone aboard with it), a droid (his body, if inside, lives). Back inside for a single update and the next time out starts again from 10.
- **The voice-over:** as the count starts, if the Character is a player at this machine, his team's `SetOutOfBoundsVoiceOver(team, line)` plays: "repleaving" or "cisleaving" in the Clone Wars, "allleaving" or "impleaving" in the Galactic Civil War, streams on the era's VO stream. Once per time out. Rhen Var Harbor's Clone Wars script and Yavin Arena's Galactic Civil War script set none.
- **The warning:** while the local player's count is 0 or more, `game.tooltips.leaving_battlefield` ("LEAVING BATTLEFIELD") shows in the middle of the screen (anchor 0.5, 0.5, centred both ways) in `gamefont_large` at scale 1, in the HUD's red (223, 32, 32). Its alpha is a spring: starting at 254 with no speed, each frame value += speed × dt, then speed += (128 − value) × 500 × dt, drawn as the integer clamped to 2..255. That is a flash about 3.6 times a second. No time is shown.
- **Unattended vehicles** are a different rule (VehicleSpawn's decay, docs/ai-vehicles.md).

The port runs the count in the world (src/battlefield.js) for every Character, the dedicated server's network humans included. The page that shows a player works out his warning and voice-over from what he controls as that page sees it, in every mode, so nothing new crosses the wire.

### Jetpack

Jet troopers set `JetJump` 5, `JetPush` 8, `JetType` "hover", `JetEffect` "jetpack", and the fuel values `JetFuelRechargeRate` 0.06, `JetFuelCost` 0.2, `JetFuelInitialCost` 0.25, `JetFuelMinBorder` 0.24. The class defaults are 7, 14, a jump jet, 0, 0.2, 0, and 0.1.

- **Fuel** runs from 0 to 1. It refills at `JetFuelRechargeRate` per second every tick, jetting or not, and a spawn starts at half (empty when it never refills).
- **Lighting it:** Space pressed in a jump or fall (not while thrown), during the jump's first 0.3 s or with no ground under him, with fuel at `JetFuelMinBorder` or more. It costs `JetFuelInitialCost` and adds `JetJump` m/s along normalize((0, 1.1, 0) + facing × max(forward stick, −0.1) − 0.1 × strafe stick × left), so straight up with no stick. It isn't checked from a stand, crouch, prone, or dive.
- **Hovering**, each tick: standing posture; vertical speed gains (13 − 2.5 × (vy − 2)) m/s per second, which with gravity holds his height, plus `JetPush` × 4 × the look's rise (sin pitch) × the forward stick. He drives at 0.3 of the usual rate toward `MaxSpeed` × 2 cos(pitch) × forward and `MaxStrafeSpeed` × 2 × strafe, braking at 0.3 of the usual rate even in the air, and turns at the full rate. So level flight tops out at twice `MaxSpeed` (13 m/s for the trooper), climbing as he looks up. The fuel burns at `JetFuelCost` per second; empty, or Space pressed again, and he falls (that tick he doesn't move). Touching down lands him at once; landing more than 2 s after lighting it is a hard landing, though it plays the soft clip.
- **A jump jet** (any `JetType` but "hover"): vertical speed gains `JetPush` per second, the stick is scaled 0.7 with no braking, no fuel burns, and it lasts until he lands.
- **The rest:** firing and zoom work while hovering, from the standing gun origin. The third-person body plays `human_rifle_jetpack_hover` for every weapon (looping, the legs easing back under the facing, the aim relaxed); weapon actions play their standing clips over it. `JetEffect` is an effect hung from `bone_ribcage` while he hovers. First person keeps the arms' current clip and drops the head bob, and landing plays the `Land` motion.
- **HUD:** the health group's second bar shows the fuel with `hud_jetpack_icon` whenever the class refills fuel: blue, solid red at or below `JetFuelMinBorder`, and pulsing toward red if it gets that low in flight.

### Mouse look

The game reads the mouse's raw counts each frame. It turns them into turn and pitch rates of counts x sensitivity / (2048 x frame time), each clamped to ±16. Those rates turn the soldier at `MaxTurnSpeed` x rate:

- One count turns or pitches `MaxTurnSpeed` x sensitivity / 2048 radians, whatever the frame rate.
- Turning takes the posture's turn scale (half prone, in the air, and 0.6 diving). Pitch does not.
- With a mouse, pitch also runs at `MaxTurnSpeed`. A gamepad pitches at `MaxPitchSpeed`.
- Moving the mouse forward looks up unless the y axis is inverted. Nothing turns while the soldier is held still.
- The options' mouse sensitivity slider runs from 0.5 to 10 (default 5), and sensitivity = 2^((slider - 5) / 5): 1 at the default, 2 at the top.
- Zooming divides the rates by the zoom factor.

### Command posts

A command post is a placed object whose class derives from `commandpost`. On Bespin: Cloud City that class is `com_bldg_controlzone` in `common.lvl`, with `NeutralizeTime` 12 s, `CaptureTime` 10 s, and `HoloTurnOnTime` 2.2 s.

- **Capture:** soldiers inside the post's capture region count for their team's capture team. With N soldiers, progress runs at 1 + 1/2 + 1/4 + ... (N terms) times the base rate: one soldier captures in 10 s, two in 6.7 s. A team's capture team is the team itself, except a local team (above 2), set when the team is set up after the mission script: friendly to exactly one army, it counts for that army (Kashyyyk's Wookiees help the Republic); friendly to both, for nobody (Mos Eisley's Jawas); friendly to neither, for itself (the Tusken Raiders take posts).
- **An owned post** that other capture teams stand in is neutralized over `NeutralizeTime`, at the sum of each such team's rate; defenders standing there don't slow it. When they leave, that progress is lost at once. Once neutralized, the post is neutral.
- **A neutral post** fills toward one team over `CaptureTime`. A different team must first drain it back to empty. Two teams present freezes it, and an empty post keeps its progress.
- **No capture region:** the post never changes hands. Tatooine's Tusken campfires (`tat_bldg_tuskanspawn`, posts ts1, ts4, ts5, team 3) and Hoth's probe droid drop (`com_bldg_invisible_controlzone`) are such posts, with neither `MapTexture` nor `HoloOdf` and `MapScale` 0: no icon, no hologram. `map.json` carries each post's `mapTexture`, `mapScale` and `holoOdf` from its class.
- **Spawning:** a team deploys at its own posts, at a random point of the post's spawn path, set on the floor found by a ray 10 m up and then 20 m down. A command vehicle's class spawn points are placed otherwise (below, "Spawning and the spawn screen").
- **Bleed:** a team scores each post's `ValueBleed` (10 for Bespin's posts) for the posts it holds, plus 1 per neutral post. The mission's bleed thresholds (`AddBleedThreshold`, checked lowest first) give the rate for the first threshold the score doesn't pass: Bespin's are 1 → 3, 11 → 2.25, 21 → 0.75, and 31 → 0. Every 4 units of rate × seconds cost the team one reinforcement.
- **Losing:** a team out of reinforcements loses. An army whose score is 0 (no post of any `ValueBleed` held, none neutral) has 20 s, then its reinforcements drop to zero; holding a post resets the 20 s, a neutral post pauses them. The bleed and this count run for teams 1 and 2 only. A local team instead loses its reinforcements (2147483647 unless the mission sets them) when it is friendly to exactly one army and that army's reinforcements are no more than its living soldiers; otherwise it keeps them all battle.
- **Hologram:** the post's `HoloOdf` class (`com_holo_controlzone`) sits at the model's `hp_hologram` node.
  - The logo is the owner's side's model from `HoloImageGeometry` (`com_icon_republic` and so on). It floats `HoloHeight` 3.5 m up, scaled `HoloSize` 2.5, turning `HoloRotateRate` 0.2 times a second.
  - Its alpha is `HoloImageIntensity` 0.5, times a flicker between 0.4 and 0.8, out to `HoloVisibleDistance` 200 m.
  - It is colored as the viewer's team sees the owner: friendly 30 220 30, enemy 240 30 30, neutral white, natives 240 220 30. Each team keeps one colour: a team of the Locals faction in the natives' colour, friend or foe; any other by the team's relation to it. The maps' post and unit icons use the same table with the HUD's colours (32 223 32, 223 32 32, 192 192 192, 223 223 32).
  - A beam and an 8 m light go with it. A neutral post shows only the beam and light.
  - The logo goes dark when a capture or neutralize is within `HoloTurnOnTime` of landing, and the new owner's logo fades in over `HoloFadeInTime` 2 s.
- **Map:** the minimap image (`<world>_map`) covers the posts' bounding box plus 100 m, as a square: u = 0.5 + (x − cx) / 2h and v = 0.5 + (z − cz) / 2h, with v running down the image. Posts are drawn with their class's `MapTexture` (`hud_flag_icon`) at its `MapScale` (2.5), tinted by the team colour table; a class without a `MapTexture` or with `MapScale` 0 isn't drawn, and neither is its name.

### Spawning and the spawn screen

The player's Character, not his unit, holds his team, class and spawn post between lives. The spawn display (`SpawnDisplay`, updated every frame) only reads and sets the Character. The port's world side is in [human-spawn.md](human-spawn.md).

- **Start:** the Character begins with no unit and its respawn timer at 0, so the display opens on the first frame of the match. Its class index starts at 0, the team's first class.
- **Opening and closing:** the display opens when the local Character has no unit and its timer is at or below zero. It closes when the unit exists again, the timer goes above zero, the game is over, or the player is no longer valid.
- **Death:** a death sets the timer to 4 s, so the display opens 4 s later and shows nothing before. There is no automatic spawn: the time it has been open is counted but never read.
- **The battle keeps running:** single player isn't paused while the display is open. The pause menu's RESPAWN (`game.pause.suicide`, after a confirmation prompt) calls `ScriptCB_PlayerSuicide`. That is an ordinary death: a ticket, the 4 s timer, then the display.
- **Committing:** SPAWN commits the choice on the next frame: team, then class, then spawn post, then the Character's ready flag.
- **Spawning:** the Character's update spawns the unit at a random point of the post's spawn path. Offline, a human skips the spawn wave's wait.
- **Which posts serve a team:** a post with an owner, where either the owner is the team and the post has a spawn path or class spawn points, or the owner is friendly and the post has an ally path. Mobile command posts count.
- **Command vehicles:** a vehicle of the native `CommandWalker` or `CommandHover` class (ODF base `commandwalker`, `commandhover`: the AT-AT, the AT-TE, the MTT, the hovernaut) is a command post while it lives. The port's side is in [human-spawn.md](human-spawn.md), "Command vehicles".
  - When built, it takes over the post of the same class whose vehicle is gone and which stands within 1 m of the new one (where its pad made the last), else a new one after the placed posts. So one post serves a pad all battle. The post iterator visits the first 16.
  - The post runs from the vehicle's activation to its death: when the walker is found dead, its OnDeath deactivates the post. A dying walker is therefore no post.
  - Its owner is the vehicle's team. Taking any seat gives the hull the occupant's team. When the last occupant leaves, a hull goes back to team 0 only if its class says so, and the command classes don't, so a command vehicle keeps its last occupant's team. Setting its team runs the post's change of hands: the AI's reassignment and recapture response, as for a captured post.
  - Where it is, for the screen's nearest-post rules and the AI's destination, is the vehicle's current transform.
  - Spawning, own team only: one draw modulo the class's spawn points (`SpawnPointCount`, then one `SpawnPointLocation` "x y z yawDeg" each: a position and a turn about the up axis); that frame times the vehicle's transform; then a ray from the vehicle's collision sphere centre to the point (terrain, buildings and props, the vehicle left out) brings the point back to the first hit; the frame is stood upright. There is no 10 m up and 20 m down settle as for a path point. The AT-AT's six points stand 8 m out on either side, level with its feet, at 0 and ±4 m along it, turned 5 degrees outward of its heading; the AT-TE's stand 6 and 7 m out. A soldier appears beside the walker, not under it.
  - Its `ValueBleed` (10 on every stock command class) counts in the team's score, and holding it resets the no-post count. On Hoth FORWARD CAMP and the two AT-ATs make 30, under the Empire's last threshold (31): no bleed while both live. Its `Value_ATK_*` and `Value_DEF_*` (10 to attack for the four armies and 0 for the locals, 0 to defend) weigh it as an objective.
  - At its death the bots sent there draw again, "%s LOST THE %s" is announced with the owner's name and the post's, and whoever damaged it last gets a command post point unless his team is friendly to the owner.
- **Which post is selected:**
  - a selected post that still serves is kept;
  - one that no longer serves moves to the serving post nearest it;
  - with none selected, the serving post nearest any post an enemy holds;
  - failing that, opening the display picks a random serving post.

  The check runs every frame, so a lost post moves the selection at once. The choice is remembered across deaths.
- **Classes:** the display offers the team's classes without its heroes (`SetHeroClass` appends those last), so heroes can't be picked. The human's class has no limit; the `AddUnitClass` counts only drive the AI.
- **Team switching:** allowed offline when `ScriptCB_SetCanSwitchSides` has set it and the metagame rules are off.
  - Instant Action allows it; the historical campaigns and Galactic Conquest don't.
  - On SPAWN the Character changes teams. A hero is refused, and so is a full team, but a team holds 200 Characters, so offline there is no balance rule.
  - The Character moves between the rosters, its class index is clamped into the new roster, and its post is chosen again; the display's own choices then replace both.
  - No ticket is spent or refunded.
  - The display resets its class to the first slot on a team change.
- **The body:** a soldier's body lies where it fell through its dead state, 4 s, and is then removed. A death region kills whoever enters it.

### Rendering

The game draws in gamma space with Direct3D 9 shaders: textures aren't decoded from sRGB, and lighting adds and multiplies on the stored values. Lighting is per vertex, and every colour is clamped to 0..1.

- **Light environments:**
  - Props: `AmbientColor` plus the sun, max(0, N·−d) × `SunColor`, times the tint.
  - Soldiers and classes with `Lighting = "Dynamic"` (command posts, droids): the ambient blends from `TopDirectionalAmbientColor` to `BottomDirectionalAmbientColor` by (1 − N.y) / 2. Their sun is scaled by the light region they stand in.
  - Baked geometry (MTRL 0x40): the baked vertex colour is the light.
  - Unlit (MTRL bit 0 clear): the texture times the tint.
  - Point lights add max(0, N·L) × (1 − d²/r²) × colour.
- **Bump mapping** lights the sun per pixel: sat(ambient + sat(n·L) × `SunColor`) × tint, with n from the normal map in the model's tangent, binormal, and normal. Faces turned away from the sun get none. Dynamic objects use `CharacterAmbientColor` there.
- **Blending:** 0x4 is source alpha over, 0x80 is additive; both skip depth writes and go in the transparent queue, drawn after the opaque ones. 0x2 (hard-edged) cuts the texture out at half alpha: the hard-edged states' pixel shaders zero the alpha of texels at or under 0.5 (`normal_shader.xml` state 2) and the alpha test (greater) drops them.
  - A hard-edged material that doesn't add is opaque, 0x4 or not: the Normal, Bump, Scroll and Detail shaders clear its 0x4 when they load it, so it writes depth and goes in the opaque queue. Kashyyyk's thatch and plank floors, Endor's ferns, Tatooine's grates and every other stock hard-edged material draw this way. Only the Specmap shader keeps 0x4, and no stock hard-edged material uses it.
  - An object fading out (its colour's alpha under 255) blends every segment, hard-edged or not.
  - Any of 0x86 makes a segment two-sided: it draws with culling off. Everything else is back-face culled.
- **Terrain:** the layers blended by weight, times the detail texture (0.5 is neutral, ×2), times twice the baked colour.
- **Light regions** are named like `shadow02 a=1.0 d=0.0`. A dynamic object's weight in one ramps from 0 to 1 across a 5 m band centred on the region's surface. Its sun is scaled by the least of 1 − (1 − d) × weight over the regions it is in, and an `a` under 1 pulls its ambient toward black (between 1 and 2, toward white) by the weight. A cylinder with only its radius or only its height in the band ramps by the distance from its axis or middle instead of from its surface. On Bespin `shadow02` covers the levels under the city. That and the stencil shadows are all that shade a soldier there: indoors, the ambient still lights him.
- **LUMI:** on maps whose `DomeInfo` sets `TerrainColorDarkening`, a soldier on the terrain is multiplied by min(LUMI / 255 × 1.2 + 0.3, 1). Bespin doesn't set it.
- **Frame:**
  1. **Far scene:** the sky's models centred on the eye, without depth or fog; then, if the far scene reaches past the near scene's fade start, the low-res terrain and the water (see Water); then each object's `LOWD` model. All but the sky take the `LowResTerrain` fog when it is on (see Fog).
  2. **Near scene:** full detail with no fog, from 0.5 m to the fade end. Between the fade start and end each pixel dissolves into the far scene's image. On Bespin at the longest view distance that is 150 to 200 m.
  3. **Soft focus:** a quarter-size copy of the frame blended over it at `ConstantBlend` (0.3).
- **Shadows:** the original drew stencil shadow volumes and a drop shadow under soldiers. The port uses cascaded shadow maps of the sun instead.
- **Fog:** only the far scene is fogged, by the sky's `DomeInfo` `LowResTerrain` (`SkyInfo`'s `FogColor` and `FogRange` are never read).
  - It is linear in view depth z, clear at `FogNear` and the colour at `FogFar`: the fog factor is (FogFar − z) / (FogFar − FogNear), with range fog off.
  - `FogColor` goes to Direct3D's fog colour with its alpha forced to 255. A fourth value a stretches FogFar to FogNear + (FogFar − FogNear) × 255 / a, unless a is 0 or 255.
  - The far pass sets the colour and range and asks for fog, but the setters switch Direct3D's fog on only when 0 ≤ FogNear < FogFar, and the shaders' factor is 1 (clear) when FogFar ≤ FogNear. Kashyyyk: Islands' sky sets FogNear 9000 and FogFar 8000 (alpha 0), so its far scene is drawn without fog.
  - Every mission starts from opaque black, FogNear = FogFar = 0, which is off: a sky without a `LowResTerrain` (both Rhen Vars) has no fog.
  - Additive surfaces fog toward black, others toward the colour.
  - The far pass puts the fog state back when it ends, and the near scene draws with it off: the eye's near scene is never fogged.
  - The water's reflection fogs too. When the far scene doesn't reach past the near scene's fade start, the reflected near scene is fogged from the fade start to the fade end in `SkyInfo`'s colour 0x91e9c28b, black when the sky doesn't set it. Only Yavin 4: Temple sets it (57, 66, 68). The port's reflections don't do this yet.

The stock skies' far-scene fog, FogFar stretched by the alpha, and how far the far scene reaches (`FarSceneRange` at the longest view distance):

| Map | FogNear | FogFar | Colour (alpha) | Far scene | Fog |
| --- | --- | --- | --- | --- | --- |
| bes1 | 80 | 430 | 82, 45, 25 | 1500 | on |
| bes2 | 80 | 972.5 | 45, 45, 60 (100) | 500 | on |
| end1 | 90 | 450 | 170, 170, 170 | 400 | on |
| geo1 | 300 | 1096.9 | 142, 82, 38 (128) | 1000 | on |
| hot1 | 30 | 1000 | 201, 200, 206 (255) | 1400 | on |
| kam1 | 50 | 800 | 0, 0, 0 (255) | 1000 | on |
| kas1 | 9000 | 8000 | 0, 0, 0 (0) | 1500 | off: it starts past its end |
| kas2 | 2000 | 5984.4 | 0, 0, 0 (128) | 1000 | on, starting past the far scene |
| nab1 | 700 | 1297.7 | 126, 148, 82 (128) | 1000 | on |
| nab2 | 3000 | 6000 | 70, 70, 70 (0) | 800 | on, starting past the far scene |
| rhn1, rhn2 | | | | 20, 40 | off: no `LowResTerrain` |
| tat1 | 400 | 10281.3 | 164, 174, 194 (80) | 2000 | on |
| tat2 | 200 | 3500 | 164, 174, 194 | 1000 | on |
| tat3 | 20 | 40 | 0, 0, 0 (255) | 0 (off) | on, with nothing in the far scene but the sky |
| yav1 | 90 | 350 | 144, 153, 156 | 20 | on, with nothing past the near scene but the sky |
| yav2 | 400 | 1196.9 | 80, 80, 80 (128) | 400 | on |

### Weapons

- **Triggers:** a held trigger fires on the frame it is pressed, then every `ShotDelay`, with the remainder carried over. `TriggerSingle` weapons fire once per press, and a press inside the lockout is dropped. Only one channel fires at a time: a busy or newly pressed secondary releases the primary trigger. Switching weapons is instant, drops the zoom, and resets a reload.
- **Ammo:** a shot spends 1 / (`RoundsPerClip` + 0.0001) of a clip; under one round per clip, the clip never empties. A reload puts the partial clip back in the reserve and takes a full one after `ReloadTime`. Empty clips reload by themselves.
- **Heat:** each shot adds `HeatPerShot`; at 1 the weapon overheats until it cools (at `HeatRecoverRate`) to `HeatThreshold` (it cools every frame whatever the weapon is doing; while it is overheated, no trigger starts anything). The pistol overheats on the ninth quick shot and is out for about 2.8 s. The arc caster (`HeatPerShot` 0.42, raised by its charge levels to 1.0 past 0.99 of a charge; `HeatRecoverRate` 0.3, `HeatThreshold` 0) overheats on a full charge and is back 3.3 s later.
- **Spread** (degrees): each shot is off by S × (rand − rand) in yaw and pitch, with S = base spread + posture spread + max(kick, 0). The kick grows `SpreadPerShot` per salvo up to `SpreadLimit` and recovers at `SpreadRecoverRate`. For players it starts at −`SpreadThreshold`, so a burst's first dozen rifle shots are free of it. Every round of a salvo draws its own pair for each axis (pitch before yaw), so the shotguns' five rounds (`ShotsPerSalvo` 5) fan out over the spread: the Dark Trooper's blast cannon (`imp_weap_inf_shotgun`) over ±3° of yaw and ±1.5° of pitch, the Rebel pilot's blaster cannon (`all_weap_inf_shotgun`) over ±4° and ±2°. Each round spends its share of the clip (30 rounds, six presses) and is a 150 m/s bolt living 0.8 s, so it reaches 120 m with its full 55 damage; nothing in the bolt classes falls off with distance. Online, a client builds each round with its own draws, queues it for the server and flies it at once, so a player's rounds go where he saw them go.
- **Aim:** shots start at a gun origin beside the head (per posture, in the aim's frame), not at `hp_fire`, which only places the muzzle flash. They converge on the point under the crosshair: a ray along the aim, up to 60 m, starting where the camera sits level with the soldier plus `TrackOffset.z` + 2 m (the ray meets terrain, buildings and props, vehicles and soldiers, the shooter left out). A soldier under the crosshair is where the rounds converge, his head when it is on his head.
- **No firing** in the air, while diving, while getting up, or crawling prone. Zoom (`ZoomMin`) narrows the field of view and slows the mouse by the same factor, uses the posture's `ZOOM` camera section, and drops while airborne or reloading. A weapon with `ZoomRate` 0 and a different `ZoomMax` steps through both: the sniper rifles go off, 2x, 8x, off.
- **Charging:** with `MaxPressedTime` above 0 a held trigger charges (from 0, plus that frame) up to `MaxPressedTime`, and letting go fires once with charge = held time / `MaxPressedTime`; `ShotDelay` then locks it. While it charges, the HUD's weapon bar shows the weapon timer against its limit in green: the charge, held time / `MaxPressedTime`, for the arc caster, the bowcaster and the mortar alike. The weapon climbs to each charge level whose threshold the charge passes, and goes back to its own level when the shot is done. A grenade-class weapon (the mortar) releases on the same frame. Throws and charged shots pick `shoot3` above 0.7 of a charge, `shoot2` above 0.4, else `shoot`; weapons that don't charge count as 1, so the thermal detonator always plays `shoot3`. A gun held at a charge plays no clip: its alert timer is refreshed every frame, so the soldier holds the alert stance, aim up (not for the tool, sabre and gam banks); a thrown weapon's windup plays the bank's `charge` clip once. The first-person arms go on with their idle or run. The weapon's `ChargeUpEffect` sits at its fire point while it charges, for every soldier drawn, every emitter's colour and alpha multiplied by the charge (0 to 1), its size and rate unchanged; it is gone the frame the shot leaves. Each charge level has its own `ShotPatternPitchYaw` list (`ShotPatternCount` starts it, and a level copies its class without one): the bowcaster fires 1 bolt to 0.35 of a charge (`_ord_high`, 75 damage, past 0.15), 3 to 0.65 (yaw 0, ∓0.7°), 5 to 0.95 (0, ∓0.7°, ∓1.4°) and 7 beyond (out to ∓2.1°), each bolt a round of the clip. A bot's charge holds for a draw of 0.25 to 1 of the weapon timer's limit when it starts: its `ShotDelay` after a shot (a bowcaster in steady fire fires single bolts), its `ReloadTime` after a reload, 1 s new.
- **Shot patterns:** each round of the salvo takes the pattern's next entry (round robin per weapon) and adds the spread to it.
- **Aim kick:** each round adds 10 × `KickStrength` (degrees, as radians) to a kick rate and (1 / `KickBuildup` + 2) × `ShotDelay` to a hold (`KickBuildup` defaults to 2). Each frame the aim pitch climbs by min(hold, 1) × rate × dt, an offset that only shots use gathers rate × dt and fades at 6/s, the rate fades at 20/s, and the hold drains at 2/s. A sniper rifle (`KickStrength` 2) climbs about 1° a shot; the shrapnel 0.375° a salvo.
- **Muzzle flash:** the `muzzleflash` mesh at `hp_fire`, scaled 0.6 along the barrel and tinted `FlashColor`, for `FlashLength` (full for the first half, then fading), and a `FlashLightColor` light for `FlashLightDuration`. Each round sets its barrel's flash timer to full and moves on to the next fire point, so the rounds of a salvo from one muzzle restart one flash rather than adding up. `MuzzleFlash` names a preset: `small_muzzle_flash` sets the flash's two sizes to 0.6 and 0.15 and its length to 0.1 s, `med_muzzle_flash` 1.0, 0.25 and 0.15 s, `large_muzzle_flash` 1.8, 0.45 and 0.3 s, each with yellow colours that the class's own flash keys, read after it, replace. A class's `MuzzleFlashEffect` plays at the weapon once per round.
- **Grenades** leave the hand `InitialSalvoDelay` (0.3 s) after the press along the aim tipped up by `ShotElevate`, at the ordnance `Velocity`, and the throw lasts `ShotDelay` more. The trooper throws with `human_grenade_stand_shoot3` (prone: `prone_shoot`). With `HideOnFire` (the default) the model leaves the hand with the throw and comes back when the weapon has reloaded; the mortar (`HideOnFire 0`) stays in the hand.
- **Dispensers** throw like grenades but play `shoot` (`FireAnim` 0): the item leaves the gun origin along the aim at `Velocity` × lerp(`MinStrength`, `MaxStrength`, charge) (4 m/s), plus the soldier's velocity. An owner with `MaxItems` (4) of a weapon's items about loses the oldest first. The world holds at most 25 power-ups and 50 mines unless the mission's `SetMemoryPoolSize` says otherwise; with no room nothing is thrown or spent.
- **Repair weapons** (the fusion cutter) lock onto a building each frame: of those the weapon can work on (it gives that kind something, the building still needs it, and it is on a side the weapon serves), the one scoring lowest at 16 × miss² + distance along the aim, inside a cone of slope `LockOnAngle` from the gun origin, within `LockOnRange` of the building's sphere, with nothing in the way; a held trigger keeps the current one with the slope widened by 45°. With the trigger held, a lock, and no overheat, the cutter ticks on the press and every `ShotDelay` after, finishing an interval under way before it stops. Each tick plays `MuzzleFlashEffect` (`repairkit`: sparks, a flare and a flare ring) at the tool's fire point and gives the building `BuildingHealth` (built), `BuildingRebuild` (a wreck) or `BuildingBuild` (a pad), heating the weapon only when it gave something. It plays no third-person shot clip, a lock keeps the soldier alert (a page follows the replicated lock and each tick, so the tool comes up on the aim in the alert stance rather than hanging at the hip), and the health group's second bar shows the target's health with its `HealthTexture` (a pad's is its build progress).
- **The arc caster** (emitter ordnance) throws lightning on release from the gun origin along the shot (the aimer's direction: converged on the crosshair in third person). It gathers up to 32 soldiers, droids, droidekas, landed mines and droideka shields whose bounds meet a box `ConeLength` each way about the point 0.9 × `ConeLength` down the shot, and keeps those whose origin is less than `ConeLength` along the shot and within sin(`ConeAngle` / 2) × that distance of its line. Nothing requires the distance to be positive, so the cone is double: a target in the box behind the shooter, on the line back, counts, and comes first. Nearest first by that distance, the first link must be within `ConeLength` of the gun along the shot, each later one within `MaxJumpDistance` further along than the last; each needs a clear line (terrain, buildings and vehicles) from where its arc starts to the target's origin + 1 m, and the first needs that point within `FirstRadius` of the shot's line. Damage is `Damage` + (`MaxDamage` − `Damage`) × charge, less 75 a link, stopping below `DamageThreshold` or at `MaxTargets`; it goes through the plain damage path with every type's scale 1, so a soldier plays no hit reaction and takes no stun. From the second link on the next arc starts from the same point 30% of the time. With nothing to link, one bolt runs along the shot to what that line meets within `ConeLength`. The first bolt (or that lone one) keeps a handle on the shooter, and every frame its start is put back on his gun origin, so it leaves his gun for as long as it lasts, in either view: in first person from 0.3 m in front of the eye. Later bolts run between fixed points. While it charges, the HUD's weapon bar shows the charge in green (see HUD).
- **Animation:** shoot, reload, and throw clips come from the weapon's `AnimationBank`. They play over the upper body for as long as the weapon is in that state; shoot clips blend in over 0.02 s.

### Ordnance and explosions

- **Bolts** fly straight at `Velocity` for `LifeSpan` (`Gravity` is ignored). When fired, one ray along the whole flight finds the buildings and terrain in the way, and the bolt's life is cut to reach that point, where it plays `ImpactEffectStatic` or `ImpactEffectTerrain`. The drawn streak grows by the distance flown: a core of `LaserLength` × `LaserWidth` (half as wide at the tail) and a glow of `GlowLength` × 2 `LaserWidth` in `LaserGlowColor`. A `LightColor` light rides along. The class caps the three lengths at 128, and reading `LaserLength` also sets `GlowLength` to 3 × and `BlurLength` to 15 × it; a `GlowLength` read after it replaces that. So a bolt class that names no `GlowLength` (every vehicle and turret gun, and the jet trooper's commando pistol) still draws a glow three times its core, in its `LaserGlowColor`, white when none is named. The streak is drawn out to the longest of the three.
- **`FadeOutTime`** (only the sniper rifles name one: 1 s, on `rep_`, `cis_`, `imp_`, `all_` and `tat_weap_inf_tuskensniperrifle_ord`; the class keeps 1 / `FadeOutTime`, or 0): when a bolt's flight ends, at a hit or at the end of its `LifeSpan`, it is not removed but held where it stopped, its drawn length clamped to the distance it flew from where it was fired (or the longest of the three lengths). Each update then takes dt / `FadeOutTime` off its strength, and it is removed when none is left; a class without one is gone at once. The draw scales the core's alpha and all four channels of the glow colour by that strength, and both strips' widths by 0.9 × strength + 0.1, so the streak thins to a tenth as it fades. Its light (`LightColor`, `LightRadius`, carried at the middle of its last step) stays lit until it is gone. A sniper shot therefore leaves up to 20 m of core and 24 m of glow ending where it struck, fading over a second.
- **Sticky grenades** fall at `Gravity` × 9.8. On contact the normal part of the velocity turns around at 0.37 of its speed, and friction takes up to 1.5755 × that speed off the rest. After four slow bounces (under 1 m/s) the grenade rests, and only then does its `LifeSpan` fuse run. It ends in `ExplosionExpire`.
- **Rockets** (`bullet` ordnance: the EMP launcher's) fly their model nose first (+z along the flight, x across the world's up) dead straight at `Velocity` (150 unless set) for `LifeSpan` (2), whatever their `Gravity` says, starting at the gun origin without the shooter's motion. A `TrailEffect` hangs at the tail and a `LightColor` light (its rgb times its alpha) rides at the middle of the last step. Like a bolt, one ray at the start finds the building or terrain in the way. When life runs out they set off `ExplosionExpire` where they are; at a hit the next frame also plays the impact effect and sets off `ExplosionImpact` 0.2 m off the surface, so the EMP rocket, which names the same explosion for both, goes off twice against walls and floors. Direct hits do `Damage` × the target's scale.
- **Missiles** (`missile`; default 35 m/s for 3 s, `TurnRate` 1 rad/s) and **shells** (`shell`; same speed and life, falling at `Gravity` × 9.8, default 1 g) skip the first ray and test what they cross every frame. A missile turns its velocity toward its target's aim point by at most `TurnRate` rad/s (as the sine of the angle off), with no target holding its launch line, and wavers by `WaverTurn` about axes that wander at `WaverRate`; both ease in over `ScaleTime` (1 s). Neither bounces. The impact explosion can also be named `Explosion` or `ExplosionName`, and `HealthScale` and `ArmorScale` set the person, animal, droid and the vehicle and building scales.
- **Impact effects** play exactly at the point of impact on the world's axes, not turned to the surface.
- **Explosions** play their `Effect` and a fading light. They reach soldiers, droids, droidekas, vehicles, buildings (turrets, recharge droids), landed mines and recon droids, the thrower included; never items in flight, power-ups or props, nor soldiers riding in vehicles. Damage and push each fall off linearly from full inside the inner radius to nothing at the outer, over the distance from the blast to the centre of the target's bounding sphere less half the sphere's radius (camera shake measures from the thing's origin instead: below). A soldier's sphere is by stance: its centre 1.0 m above the feet and radius 1.463 standing, 0.75 and 1.102 crouched, 0.35 and 1.622 prone (a body lying dead counts as prone); anything else's is its model's bounding sphere (a vehicle's at least as wide as its collision box). Terrain or a building between the blast and the target's sight point (a soldier's: 1.4, 0.6 or 0.2 m above the feet; else the sphere's centre) blocks damage and push; a soldier or a vehicle never does, nor the target's own body. Damage is `Damage` × falloff × the scale for the target's `HealthType` (`PersonScale` for a trooper, who has 300 health; `DroidScale` for a droid; `VehicleScale` for a hull; `BuildingScale` for turrets and recharge droids), and the battle's friendly fire factor for the thrower's team, for hulls and buildings as for soldiers. A droideka's shield takes it first, by `ShieldScale`. Only soldiers and recon droids are pushed: (away + 0.6 up) × `Push` × falloff is added to the velocity. A round that strikes something deals its own `Damage` × that scale too (×3 on a soldier's head, for rockets as for bolts), and its `ExplosionImpact` goes off 0.2 m off the surface, so the thing struck takes both. The original game takes at most 20 things per blast; the port takes all.
- **Bolts with an impact explosion** (the orbital strike's) set it off where they hit. A round that hits a building's collision does its `Damage` × `BuildingScale` to it.
- **Camera shake** is the camera's, not the soldier's, and is asked for after the blast has been dealt:
  - It is asked for only when `ShakeRadiusOuter` is above 0 and some camera's view reaches a sphere of 60 m about the blast, as it does whenever the blast is near enough to shake (the widest stock shake reaches 30 m).
  - It measures to what the camera follows (the tracked object: his soldier, the hull he drives, the recon droid he flies): d from that thing's origin (not its sphere's centre) to the blast, nothing within 0.0001 m. The share f is the falloff over d less half the thing's bounding sphere's radius (the sphere a blast's damage uses), times the class's `ShakeScale` (0x72ccbde1, 1 unless set; no stock class sets it).
  - With f above 0 the camera takes a shake of `Shake` × f lasting `ShakeLength` × f. `ShakeLength` is 0.5 s unless set (the Gungan haywire detonator's, the Imperial probe droid's and the Wookiee torpedo's explosions leave it). Every explosion takes two shakes, because the PC game asks twice.
  - The camera holds at most four shakes; one more is dropped. Each frame their amounts are summed, then each falls by dt × its first amount over its length (so a shake dies away linearly, at `Shake` / `ShakeLength` whatever the share), and one at 0 or below is gone.
  - The follow camera's view then turns about each of its own axes by (2r − 1)³ × 0.1 × the sum radians, r random in 0..1 each. The death camera is another controller and is not shaken.
  - So a stock blast at its full share starts the view turning by up to 0.2 × `Shake` rad about each axis, dying away over `ShakeLength`; the orbital strike's (`Shake` 2 for 1 s, 10 to 30 m) up to 0.4 rad.
- **Water** is the terrain's `WATR` layer and nothing else. A bolt that crosses it stops there with a ripple and the ordnance's `ImpactEffectWater` (by default `watersplash_ord`). A sticky grenade sinks at 0.2 m/s with its fuse running, and an explosion at the water plays its `WaterEffect`. Bespin: Cloud City has no `WATR`. Its fountain is only a model, so bolts pass through the water and hit the basin floor like any building.

### Particle effects

An emitter waits `StartDelay`, then emits bursts every `BurstDelay` until it has emitted `MaxParticles` (−1 runs until its owner stops it). Each particle starts at `Offset` + direction × `PositionScale` and moves at direction × `VelocityScale`, in the emitter's frame. Particles are stateless: each frame rebuilds them from their spawn values and age, running the transformer keys in turn from where the last left off and coasting after the last position key. An emitter records the path it moves along, so a moving one leaves its particles behind as a trail. One marked `NoRegisterStep` keeps no path: its particles are placed from where the emitter is now and travel with it, which is how a grenade's glow stays around it.

### Water

- **Surface:** the water cells at the layer's height, bobbing 0.06 m at 0.3 Hz (the bob moves hit tests too), drawn after the opaque scene, double-sided. First what lies under it is multiplied by `RefractionColor`; then a reflection at `ReflectionColor`'s alpha plus white glints (N·H)⁸ is blended by a Fresnel term, `FresnelMin` + (`FresnelMax` − `FresnelMin`) × (1 − N·V)². The normal comes from the normal-map frames, cross-faded at their rate and tiled `Tile` times per cell; at the highest quality the reflection is jittered by the bump frames (0.03 of the screen). The middle quality reflects only the sky and far terrain, the highest the near scene too; the lowest draws only the scrolling `MainTexture` with masked glints. Under the surface the screen takes `UnderwaterColor`.
- **Far scene:** when the far scene reaches past the near scene's fade start, the surface is drawn again in the far scene, right after the low-res terrain and in the far scene's fog. It reaches from that fade start out to the look's own `FarSceneRange` (both Kashyyyks: 1500 m), or to the far scene's end when the look has none. The low-res terrain under a sea is its bed: without this pass the bed shows bare past the near scene.
- **Rings:** radius grows 1.5 m/s from its start; alpha min(0.75, 0.25 × life × k), k rising over the first 15% of the life and falling over the rest; a band 0.7 m wide, 0.01 m above the surface, in `FXrippl1`. Bolts and grenades make 1.5 s rings, explosions (3 + r) s with r = min(radius / 2, 0.4), a wading soldier a 2 s wake every 1 / (0.3 v² + 4) s shaped into a V by his speed. Rings more than 120 m from the camera are dropped.
- **Ordnance:** things meet water only coming down through it over a water cell. Bolts stop there with a ring and `ImpactEffectWater` (default `watersplash_ord`); rockets and missiles do the same and set off their impact explosion; grenades sink at 0.2 m/s with their fuse running. An explosion at or just above the surface plays its `WaterEffect` instead of its `Effect`.
- **Soldiers** don't swim or slow down: they walk on the bed. Falling in faster than 5 m/s splashes (`watersplash_md`, at most once a second). With the head under for 3 s a soldier starts drowning, 30 health a second, and with the gun under water it won't fire.

### Recharge droids and turrets

These are placed objects whose classes derive from `powerupstation` (`com_item_healthrecharge`, `com_item_weaponrecharge`) and `armedbuilding` (`com_weap_gunturret`). They are rigs: the model's skeleton, with each part in the space of the bone it hangs from (the segment's `BNAM`), posed in code.

Both are buildings, which are built, destroyed, or unbuilt:

- **Unbuilt:** a class with `UnbuiltGeometryName` starts as that pad (`com_prop_buildzone`, 1 × 0.165 × 1 m, no collision) at 0 health; every turret does. Several of Theed's and Cloud City's pads sit below the floor, so only the hologram shows. The hologram (`UnbuiltHoloOdf`, `com_holo_turret`: the `com_icon_turret` image in the class's `Color`, at the pad's `hp_hologram`) shows only to a soldier carrying a repair weapon. Health reaching `MaxHealth` builds it: its own model and collision, full health; the hologram goes off at once.
- **Destroyed:** at 0 health a built one kills whoever mans it, sets off its `ExplosionName`, and becomes its wreck (`DestroyedGeometryName`, which collides: the recharge droids) or, without one, its pad again (the hologram fading back in over 2 s). Armed buildings then come back at `MaxHealth` / `RespawnTime` health per second (`RespawnTime` 120 by default, so a turret rebuilds itself in 2 minutes); recharge droids wait for a repair weapon.
- **Only built ones** serve, can be entered, and take damage. In the 2004 game a destroyed recharge droid takes 4 cutter ticks to rebuild (20 each), and a pad 10 (30 each, 4.5 s).

- **Droids:** every `PowerupDelay` seconds a droid serves everyone within its `Radius`, whatever their team: `SoldierHealth` points of health up to full, or `SoldierAmmo` clips per weapon up to the weapon's clip count less the loaded clip. Idle, it waits `IdleWaitTime`, then turns the other way through 135 to 225 degrees at `IdleRotateSpeed`.
- **Turrets:** the drum (`TurretNodeName`) turns all the way round (the barge turret within its `YawLimits`) at `MaxTurnSpeed`, mouse rates eased in at `TurnFilter` 10/s, and the view pitches within `PitchLimits`. The gun (`AimerNodeName`) turns toward the point under the crosshair at `AimerYawRate` and `AimerPitchRate` within `AimerYawLimits` and `AimerPitchLimits`, and fires the turret's weapon from its fire point. The gunner is a rider of the turret ("Riders" below): the armed building poses him through the same pilot section. Where the class names a `Pilot9Pose` he stands at `PilotPosition` in it, with the node's whole turn, picked by the view's pitch and the drum's turn speed; the clip's own root offset (0.87 m back) isn't applied, so he stands in the drum. The AAA and Hoth turrets name none and hide him. A shown gunner can be shot, and a blast hurts him at the gun. The use key gets into the turret nearest the line of the aim within reach (score 16 × miss² + distance along the aim) and out again beside it. The turret is a vehicle turret seat (every key after `BuildingSection` is read as a seat key), so it looks and aims as "Vehicle seats" below: the aim frame at the drum's pivot, `PitchLimits` down positive, `YawLimits` held, `EyePointOffset` setting `TrackCenter` too, `TiltValue` 0 unless set, FOV 48, and the drum posed along the aim whatever its bind turn (docs/live-objects.md "Turret views and aim").
- Turrets belong to no one (team 0): either side can build and man them.

### Vehicle seats

A vehicle class lists its controls as sections: the hull's own (`BODY`, seat 0, the pilot), then each `TURRET`n (a mounted turret, seats 1 on). Every section has its own camera keys, field of view, aim limits, weapons and pilot keys.

- **Riders.** An occupant shows only on a seat with a pilot animation: `Pilot9Pose` (a nine-frame aiming pose) or `PilotAnimation` (played in time), each looked up as `human_` plus the name. A seat with neither hides him and takes him out of collision. He stands at `PilotPosition`, or on the creatures at the node a key with hash 11ce2025 names (the kaadu's `bone_rider`, the tauntaun's `bone_root`), with the node's whole transform, so he rides the hull's lean and the turret's swing. The clip's own root offset isn't his: `human_minigun_9pose` carries the root 0.87 m back, `human_dishcannon_9pose` 1.34 m back and 0.15 m up (exactly the dish turret's `hp_active` from its drum: that clip was posed in the turret's frame) and `human_ride` 2.06 m up, and the root stays on the seat. `human_dishcannon_9pose` sits in `human_animation_1` beside the minigun's; the bank stores clip names only as CRCs, and the extractor matches them against the names it knows (`tools/zero_anim.py` `clip_names`). A shown rider can be shot unless the section sets hash 2b60e217 to 0, as the snowspeeder, the cloud car, the gunship's ball turrets and the Geonosian and Jedi fighters do. The pilot pose puts his collision in the riders' list, which every round's ray, melee swing and blast reaches; his shapes are his stance's, on the node, except under `human_drive`, where they are the `coll_drive` model's in `common.lvl` (a body cylinder 0.2 m by 0.75 m tilted 17 degrees, and a 0.16 m head sphere 0.95 m up). The ray takes the nearest hit over every list, so a hull whose ordnance collision encloses its rider (a canopy, a turret's drum, a riding animal's coarse spheres when its class names no `OrdnanceCollision`) takes the round. A round deals him its damage as on foot, the head's three times included; a blast deals him none unless the seat is an armed building's, and its push moves no soldier in a seat (one the blast kills has left the seat by then, and his body is thrown). Killed by a round on his seat, he leaves it through the seat exit without its search for a spot beside the hull: his body stays where it rode, at rest, back among soldiers, and while the hull lives the last bot aboard takes the seat. The fling that may follow (9 m/s along the round, lifted 0.4 to 0.7) applies only where the seat class sets hash 81bce0f9 to 0, which no retail class does.
- **Who shows:** the fighter tank's roof gunner (`hp_active`, the 9-pose); the AT-TE's main gunner (`hp_main_gunner`, `drive`); the snowspeeder's pilot and gunner; the gunship's two ball gunners and its passenger (`standing`); the skiff's pilot (`man_minigun`) and three gunners; the speeder bikes, the STAP, the cloud car and the fighters' pilots; the tauntaun and kaadu riders. The fighter tank's pilot, both AAT seats, the AT-ST, the AT-AT, the Hailfire and the other gunship and AT-TE seats are enclosed. Of the placed turrets, the minigun, Geonosian, barge, Yavin tower and Hoth dish turrets show their gunners; the AAA and Hoth turrets hide theirs (docs/live-objects.md "Turret gunners").
- **The 9-pose** on a turret seat: the pitch is spread over the seat's own `PitchLimits`, −1 at the top and 1 at the bottom; the turn is where the yaw sits in a `YawLimits` arc narrower than 270 degrees, or else the turn speed over `TurnRate`. It blends frame 4 + turn with frame 1 (up) or 7 (down) + turn by the pitch's size.
- **Aim limits.** The hull's own seat pitches up positive within `PitchLimits` (half a radian each way by default on a hover). A turret seat pitches down positive: `PitchLimits -40 15` on the fighter tank's turret is 40 degrees up to 15 down. Its yaw is measured from the hull's heading, positive to the left, and held to `YawLimits` when the arc lies inside ±180 degrees; an arc from the left round past the back (`80 280`, `120 240`) is held in 0 to 360; anything wider turns freely (the turret default is ±10 rad). A player's turret aim keeps its direction in the world while the hull turns under it.
- **The turret node** (`TurretNodeName`) turns to the seat's yaw times `YawTurnFactor` and pitch times `PitchTurnFactor` (both 1 by default; the fighter tank and AAT set the pitch factor to 0, so their turrets only turn), as a frame looking that way from the node's pivot with the node's own up.
- **Aimers** turn toward the seat's aim target from their rest pose. Each update an aimer turns toward it the shorter way round (by the turn from where it points to the target, about its up and its side), by at most `AimerYawRate` and `AimerPitchRate` times the step (2 and 1 rad/s by default), and is then held to `AimerPitchLimits` (up positive, ±45 degrees by default) and `AimerYawLimits` (±30). A yaw arc from the left round past the back is kept in 0 to 360 degrees, as a turret's. The yaw isn't brought back inside ±180: a mount free all the way round (the spider walker's guns, `-360 360`) crosses behind the short way, and could wind a whole turn either way before its limit holds it. A chain, a pitching node with a yawing one under it, turns node by node. Rounds leave the fire point (`FirePointName`) along the aimer's own aim, which it keeps whatever its node is drawn doing. Each barrel's beam stays on that barrel.
- **A node two seats turn:** the AT-AT's head (`aimer_head`) is the gunner's turret node and the aimer of the pilot's guns. Both write the node every update, so which one is drawn comes down to the order their updates run in, which is not known. The port draws the gunner's turn while his seat is held and the pilot's otherwise; the pilot's rounds follow his aimer either way.
- **Changing seats:** the switch moves the controller to the next seat's control, which clears that control's inputs and builds the controller's own input or AI object for it. Nothing turns: every seat keeps its own aim, so the hull never swings to where a gun looked. A bot in the next seat moves on.
- **The aim target** of a player's seat is the first thing along the aim within 512 m (the vehicle itself left out): from the eye in first person, otherwise from where the chase camera rests, brought forward to the eye's plane. A bot's runs along its own aim from the eye.
- **Frames.** The pilot's camera and eye hang from the hull's frame, the vehicle's own matrix (a walker's at its feet); the camera leaves out the hover's lean. A turret seat's hang from its aim frame: at the turret node's pivot, looking along the seat's aim, its up the node's. A section without a node places the pivot itself, in the hull's frame, with a key whose name is unknown (hash f2aa4a90): the AT-ST gunner's is (0, 8, 0), at the walker's head, the cloud car gunner's (0, −1, −15) and the gunship passenger's (1.5, −0.75, 1.75). Without either, the pivot is the vehicle's origin. The eye (the controller position the AI also sees from) is `EyePointOffset` in the seat's frame.
- **Repeated keys:** the parsers read each key anew, so within a section a later value replaces an earlier one. The AT-AT gunner's section gives `PitchLimits` and `YawLimits` twice; `-20 15` and `-25 20` hold.
- **The AI's reach test** (whether a mount can bear on a point) takes the point's yaw in the mount's frame as an angle from −180 to 180 degrees and compares it with `YawLimits` as written, so an arc past 180 (`80 280`) reaches only up to 180, its left part. It takes the pitch up positive and compares it with `PitchLimits`, which a turret's own aim reads down positive.
- **Camera.** It is the soldier's chase camera on the seat's section: it looks along the seat's aim with the frame's up, pitched down by `TiltValue`, from `TrackCenter` in the seat's frame plus `TrackOffset` in its own (right, up, back). `EyePointOffset` sets `TrackCenter` too, and a `TrackCenter` of its own overrides it. The turn eases toward where it should look at `AimTension` (hash 5e3171cc) and each camera axis toward where it should be at `MoveTension` (21050bf6, all three), or `MoveTensionX`, `MoveTensionY` and `MoveTensionZ` (88f1308a, 89f1321d, 86f12d64: one value, or a pair for moving right/left, up/down, back/forward), 30/s unless a section sets them. The flyers' pilot sections set them (docs/vehicles.md "Camera"); no ground vehicle or gunner section does. Five rays from 1.5 m up in the seat's frame, the vehicle left out, pull it in at walls. More than 80 m from where it should be, it jumps. `CockpitTension` (104cd1b8) is the cockpit model's sway.
- **First person:** the eye looks straight along the aim with the frame's up, so a gunner's view swings and tips with his gun. The vehicle isn't drawn for the player looking from inside it. The section's `FirstPerson` names a cockpit model drawn over the view (`rep_1st_cockpit_fightertank`, the gunners' `*_1st_cockpit_securitycam`), swaying by `CockpitTension`.
- **Field of view:** `ThirdPersonFOV`, or `FirstPersonFOV` in first person, 48 degrees across unless the section sets one (the fighter tank's pilot 55 in first person, the AAT's 52). The vertical one follows the window's shape, as for soldiers.

### Flyer animation (landing gear)

- **The bank.** `AnimationName` (hash 98557a5e) names the class's animation bank; the class looks up two clips in it, `takeoff` and `fins`. `FinAnimation` (ef563a98) names another clip for the second. No stock bank holds a `fins` clip and no stock class sets `FinAnimation`; the second clip is never used. A class that names no bank has neither.
- **What is drawn.** A hull with a `takeoff` clip binds the bank to its model when it is built, puts the clip at its first frame (30 fps), and poses the model once. Nothing advances that clip or poses the model again (the walker and hover classes pose theirs every update), so a 2004 craft keeps the clip's first frame (gear down, gear doors open, an X-wing's S-foils shut) for its whole life, landed or flying.
- **The flight state** (0 landed, 1 taking off, 2 flying, 3 landing) and its progress (0 landed, 1 flying) move only the craft: taking off, the progress grows by the step over `TakeoffTime` once the craft is `TakeoffHeight` above its `LandedHeight` (the ray down from its origin, 1024 m), and flight starts at 1; landing, it shrinks by the step over `LandingTime`. `TakeoffTime` and `LandingTime` default to 5 s, `TakeoffHeight` to 5 m and `LandedHeight` to 0; a hull reset to landed has `LandedHeight` added to its height.
- **The stock clips** (docs/vehicles.md "Landing gear"): the Jedi starfighter's, the X-wing's, the Y-wing's and the MAF's move their gear; the cloud car's names bones its model lacks; the snowspeeder's bank has no `takeoff`. Every one also lifts `DummyRoot` by a constant, about the depth of the model's bounding box below its origin.

### Flyer takeoff and landing

- **The key** (read as a press) is taken in every state and refuses nothing. Landed (0), it sets the progress to 0 and the state to taking off (1), lays the `flyerspray` effect on what a ray 1024 m straight down from 1 m over the origin meets and plays the takeoff sound; the AI's lift-off call takes a landed craft up the same way, and a landing one up without the spray. Flying (2), it sets the state to landing (3) wherever the craft is: no height, speed, ground or path test. Taking off with the progress past 0.75, it goes over to landing; landing with the progress under 0.25, back to taking off. The progress carries over either way.
- **Taking off:** a ray 1024 m down from the origin gives the height; once it is over `TakeoffHeight` + `LandedHeight` the progress grows by the step over `TakeoffTime`, and at 1 the craft flies. The velocity is the speed along the nose times the progress plus `TakeoffSpeed` × (0.875 × min(height / `TakeoffHeight`, 1) + 0.125) straight up times what is left of it; the stick turns the craft as much as that minimum and pitches it as much as the progress.
- **Landing** is the same movement with `LandingSpeed` in place of `TakeoffSpeed` and the vertical part pointing down, the progress shrinking by the step over `LandingTime`. So a craft that starts down in level flight sinks ½ × `LandingTime` × `LandingSpeed` and goes on ½ × `LandingTime` × its speed while it goes out of flight, then comes straight down, which is the landing point the AI's flyer agent aims at (docs/ai-vehicles.md "UnitFlyAgent"). After `LandingTime` it keeps sinking until it touches.
- **Touching down:** the same ray gives the origin's height over what lies below. The craft is down when the height is under `LandedHeight` and the ground's normal is more than 0.9375 up (about 20 degrees): it becomes landed (state 0) and its physics settle. A hit on certain objects (flagged by their type or class) sends it back to taking off instead; a slope too steep leaves it landing. The first time the height drops under `LandedHeight` plus a margin that depends on the hull's body, the `flyerspray` effect plays.
- **Defaults:** `TakeoffHeight` 5, `TakeoffTime` 5, `TakeoffSpeed` 3, `LandingTime` (hash 1283209d) 5, `LandingSpeed` (hash 6ae3a957) 3. The stock fighters set `LandingTime` 2 and `LandingSpeed` 5, the LAAT/i and the MAF 1.5 and 8.
- **What it touches with:** a class's `TerrainCollision`, `BuildingCollision` and `VehicleCollision` name the collision primitives it meets those with: the X-wing's `p_left_sphere`, the Y-wing's `p_front_sphere1`, the TIE fighter's `p_buildingsphere`, the Jedi starfighter's `p_spherefront1`, the Geonosian fighter's `p_sphere1`, each a sphere by its name. Their wings are not what holds them off the ground or a roof edge. The other stock flyers name none.

### Droids

- **Bodies:** the B1 (`cis_inf_bdroid`) and the super battle droid (`cis_inf_sbdroid`) have the clone trooper's 25 bones in another order, with their own arm, neck, and head offsets. They have no animation banks of their own: their clips are `human_sbdroid_*` and `human_bdroid_*` in the shared banks, and the two-layer rule (rifle clips below, the weapon's bank on the 13 upper-body joints) gives them their look. The first-person rigs are `cis_1st_bdroid` (the trooper's rig without `hp_camera`) and `cis_1st_sbdroid` (only the right forearm), playing the rifle and tool sets.
- **Blowing apart:** a class with `CHUNKSECTION`s and a `ChunkFrequency` (0.6) rolls against it each time a blast would push it; a hit leaves it unpushed and marks it to blow apart when it dies. At death it rolls again; either way it then plays its `ExplosionName` (`com_inf_droid_exp`: the `droid_explosion` effect, no damage) instead of a death clip, and its pieces fly. Each piece starts at its bone and flies at (the body's velocity + `ChunkSpeed` along the way out from the body's origin, lifted by a quarter of that distance) × f, f being 0.1 for a droid killed at exactly 0 health rising to 1 at 50 below, spinning at `ChunkOmega`, trailing `ChunkTrailEffect`, kicking up `ChunkTerrainEffect` on each of its first `ChunkTerrainCollisions` hits. The port chooses how the pieces bounce and how long they last.
- **Weapons:** the SBD's wrist blaster's model is an invisible quad marking its muzzle; the shrapnel and rocket have none.

### Droideka

`cis_inf_droideka` (ODF class `walkerdroid`, EntityDroideka) is not a soldier: it walks by its own bank (`cis_walk_droideka` in `SIDE/cis.lvl`, clips keyed by bare names: idle, turnl, turnr, death01, the four walking steps, fold, roll, unfold) and rolls as a ball. Its weapons are `WeaponSection` groups: the repeater (`cis_weap_inf_repeater`, with aimers `hp_gun_1` then `hp_gun_2`) and the shield (`cis_weap_walk_droideka_shield`, base `shield`). Two of its keys' names are unknown; the port calls dc524dd9 `BallSpinScale` (2) and 6b8d8679 `AnimationSpeed` (2).

- **Modes:** idle, turnl, turnr, death01, dead, the four steps, fold, the ball, unfold. Every clip plays at `AnimationSpeed` (twice its own speed) and none loops: when one ends, the next mode comes from a fixed table for what the droideka wants (stop, forward, backward, turn, die, roll up), the leftover time running into it. Walking backward plays the steps in reverse.
- **Walking:** deployed, it moves only by its clips, by the change in `DummyRoot`'s forward travel along the way its legs face (0.642 m per step clip, about 2.3 m/s); `MaxSpeed` is never used for that. Turning (`MaxTurnSpeed`, and the strafe keys turn it) swings the body over planted legs; once they trail by more than `TurnThreshold` a turn clip brings them round 30°. The lag shows as `DummyRoot` turned by it and the three spine bones each twisted back a third. There is no jump, strafe, or posture.
- **The ball:** the use key folds it (and unfolds it again, if there is headroom and room on each axis at 1.5 m up). Rolling, its spin grows at `BallAcceleration` × (move² + turn²) up to `BallMoveSpeed` × `BallSpinScale` (20) and loses `BallRollingFriction` a second against the world; on ground no steeper than `MaxBallAngle` its velocity is 0.325 × spin along the slope, and it bounces off walls. It turns at `StoppedTurnSpeed` to `BallTurnSpeed` with the throttle. Both the update and the post-collision step move it, so it rolls at up to 13 m/s and falls at about 4 g.
- **Weapons:** the repeater fires from `hp_gun_1` and `hp_gun_2` in turn at the point under the crosshair (from an eye at `EyePointOffset`, 512 m), never while folding, rolled, or unfolding. The shield starts full and down, and each press of the secondary trigger raises or lowers it: up it drains at `AddShield` (−100/s), down it refills at `AddShieldOff` (300/s), and it drops at 0 and when rolling. Up, it takes all damage first, by the attack's `ShieldScale`, the overflow going to health. The bubble is the `droidekashield` effect, a `Shield` block (texture, colour, radius 1.25, height 1.25, 0.15 s fades) at `ShieldOffset`, at an alpha of 1 − (1 − shield / max)².
- **Camera and first person:** the STAND sections deployed and the one labelled CROUCH rolled; FOV 65 and 70. In first person the eye is at `EyePointOffset` with no pull-back, and the `cis_1st_droideka` arms (skinned to their forearms) play `_1st_droideka_idle`, `_walk` with forward input, `_shoot` looping at twice the rate while firing, and `_reload`; the rig hangs turned 3.2 rad about y and offset (0.01, 0.35, 0).
- **Death:** death01 plays with a random `explosion`, `med_explosion`, or `small_explosion` at one of its nodes every 0.4 s, then its `ExplosionName` (`com_inf_droideka_exp`) goes off and its five chunk sections fly. Rolled, it goes up at once.
- **Side effects:** `droidekashield` and `droideka_explosion` are only in `SIDE/cis.lvl`, so the side's own effects go to `sides/<side>/effects.json` beside the shared ones.

### Mines and power-ups

- **Flight:** items fall at `Gravity` × 9.8 in two half steps around the move. A power-up meeting the world at 1 m/s or more into it loses that part, then `Friction` × it × (1 + `Rebound`) of the rest, and bounces back at `Rebound` of it; slower, it settles upright on what it touches. Soldiers pass through. Mines stick to the first thing that isn't a soldier, upright on it, bounce off soldiers, and sink at 0.2 m/s in water.
- **Power-ups** last `LifeSpan` (30 s), then fade over a second. Anyone touching one once it has landed (or after its first 0.5 s) takes it, whoever threw it: health by the receiver's `HealthType` up to its maximum (not for a unit whose `AddHealth` is below 0), and clips to every weapon but a power-up dispenser, as many as it carries. One that gives nothing stays. Health plays the soldier's `HealthGainEffect` (`Powerup_blue`), ammo its `AmmoGainEffect` (`Powerup_red`). Health100 gives 75, health25 50, dual 100 and 2 clips, ammo 1 clip. Their bands scroll (MTRL 0x410).
- **Mines** last until set off. An enemy within `TriggerRadius` plus his size who isn't closing in any more (still, moving away, or passing) sets one off (`ExplosionTrigger`, 1750 damage within 4.5 m), unless a friend is within `SuppressRadius`; teammates and the neutral never do. Blasts damage them (200 health, taken as a person), and at 0 they pop harmlessly (`ExplosionDeath`). A steady light marks them.
- **Time bombs** (the Wookiee smuggler's `all_weap_inf_remote_charge_dispenser`, three about at once) are mines with no `TriggerRadius` and a `LifeSpan` of 5 s, counted from the throw: when it runs out the bomb sets off its `ExplosionExpire` where it lies (2000 damage within 2.5 m, falling to nothing at 3.5 m, ×1.5 on vehicles, the `timebomb` effect), credited to its thrower. Destroyed first (100 health) it only pops (`ExplosionDeath`), as does the oldest of four.
- **Death drop:** a dying soldier rolls once down the class's drop list and drops at most one item, from the middle of its body (1.05 m up), at its velocity if faster than 3 m/s, else 3 m/s out to its side.

### Recon droid

- **Deploying:** the scout's remote weapon throws the droid like a grenade (0.3 s to the release, 1.4 s in all) from the gun origin at 10 m/s along the aim plus the soldier's velocity, upright and facing the aim; with `SwitchImmediately` the player flies it at once. One at a time: while it lives the remote weapon's fire button flies it again and its reload button blows it up. Flying it, the soldier stands where he was without input; his death sends control back and the droid flies on.
- **Flight:** its collision sphere (0.271 m, centred 0.155 m up) rides a spring toward `SetAltitude` above what is below (terrain, buildings, water): lift = (`SetAltitude` − height) × `LiftSpring` + 9.8 f − `LiftDamp` × speed along the ground's normal, f fading from 1 at that height to 0 nine times higher; above 5 m/s a ray along the velocity looks ahead. Driving moves it toward `MaxSpeed` forward and `MaxStrafeSpeed` sideways at up to `Acceleraton` × f. It only yaws, at `MaxTurnSpeed`; pitch only aims, within `PitchLimits`. It loses 1 health a second (40 s at most). Hitting something faster than 7.5 m/s (10 on the ground) costs 10 × the excess squared; it slides off with a 0.28125 friction and a 0.125 bounce. The use key or a next-weapon press hands control back.
- **Weapons:** the blaster fires from the droid's origin at the point under the crosshair. The orbital strike starts a 3 s countdown on a press (letting go doesn't stop it); then 4 bolts fall 0.75 s apart from 600 m away at 70° elevation from the world's +z, scattered ±2.5 m, onto the spot under the droid (500 damage within 10.5 m each), and the droid is destroyed. Dying, it sets off `exp2`.
- **The strike's bolts** (`<side>_weap_inf_orbital_attack_salvo_ord`, base `bolt`): `Velocity` 300 for a `LifeSpan` of 3 s, so each starts about 515 m up and 205 m off toward +z and lands 2 s later. They draw as any bolt does: a core of `LaserLength` 15 × `LaserWidth` 0.75 in the side's laser texture (`bluelaser_D` for the Republic, `violetlaser_d` for the CIS, `yellowlaser_D` for the Alliance, `greenlaser_D` for the Empire), a glow of 45 m (3 × the core, none being named) in `LaserGlowColor`, and a `LightColor` light of `LightRadius` 7 riding along (none for the Alliance's and the Empire's). Sound: `OrdnanceSound` `com_weap_inf_ord_hum_lg` while it flies, `CollisionSound` at the hit (`imp_weap_ord_exp_lg` for the Republic and the Empire, `imp_weap_ord_exp` for the CIS and the Alliance), and the explosion's `SoundProperty` `com_weap_obj_med_exp`. The hit plays `dirtspray` on terrain (`watersplash_ord` in water) and sets off `ExplosionImpact` 0.2 m off the surface: `atat_explosion` (`watersplash_lg` in water), a light of the side's colour, 500 damage and a push of 10 within 10.5 m, and the widest stock shake, `Shake` 2 for 1 s from 10 to 30 m.
- **Seeing them:** they come down at 70°. The droid's view is 48° across (28° high at 16:9) and pitches up 50° at most, so its top edge is at 64°: whichever way he faces, the pilot sees a bolt only in its last few metres, as a streak slamming into the ground (the frame before it lands, the glow trailing 45 m up its path). Anyone else sees each one the same way for the last 0.1 s or so, longer the farther off he stands. The first bolt's blast destroys the droid (40 health), so the pilot is back in his body for the rest of the salvo, and each later blast shakes his view as it reaches his body.
- **Camera and HUD:** its chase camera (`TrackCenter` (0, 0.5, 0), `TrackOffset` (0, −0.25, 1.5), FOV 48) or its eye 0.3 m behind the eye point (0.5 zoomed). The HUD shows its weapons and health with its `HealthTexture`, and the minimap follows it.

First-person and zoom views hide the probe hull. Enhanced destruction adds a
gentle model-only hover bob and tilt plus animated bursts for recon death and
orbital impacts; the original spring, aim, countdown, salvo, damage and Classic
effects stay unchanged.

### First person

P switches between the chase camera and the soldier's eyes. Dying goes back to the chase camera, which the death camera then takes over (see "Death camera").

- **The eye** is 0.3 m behind the gun origin along the aim (0.6 m zoomed); prone, it is behind the pivot moved 0.12 m to the soldier's right. Standing that is (−0.06, 1.60, −0.05) in the soldier's frame at level aim. The view looks straight along the aim, with no tilt and no easing. First person, and zoomed in from either view, the horizontal FOV is `FirstPersonFOV` (70), narrowed by the zoom.
- **Shots** start at the gun origin, 0.3 m in front of the eye on the view axis, and go straight along the aim, so bolts leave from the middle of the screen.
- **The view model:** the unit class's `FirstPerson` arms (`rep_1st_trooper` from `FPM/REP/reptroop.lvl`) hold the weapon's `HighResGeometry` at `hp_weapons`. The rig hangs from the camera at T(0, 0.13, −0.3645) · Rz(0.05) · Ry(3.0), its root keeping its rotation but not its lift. It is drawn after the world, with depth cleared and a near plane of 0.3 m, and not while zoomed. The muzzle flash is the third-person model's `hp_fire` on the first-person gun. A 500 m ray from the gun toward the sun decides whether the sun lights the arms, easing half the way each frame.
- **Clips** come in sets by `AnimationBank`: rifle (rifle, bdroid), tool (pistol, tool, lightsabre, sbdroid), grenade, and bazooka, each with idle, run, shoot, shoot2, reload, jump, flail, and jump_land, plus `_1st_handsdown`. Each frame one plays:
  - shoot when a round leaves (the weapon's fired flag, which the arms clear as they take it), restarted each shot without a blend; shoot2 when a grenade leaves the hand. A repair tick fires the same way, so the fusion cutter plays `_1st_tool_shoot` on every tick. The class table also loads `_1st_tool_repair` into each set's seventh slot, but the arms never select that slot;
  - reload while reloading, cut when the reload ends;
  - jump in a jump or dive, flail in a fall; after landing, jump_land for 0.5 s (from a jump) or handsdown for 1 s (from a fall);
  - handsdown crawling prone, run when moving (at 0.75 × min(v² / 20, 1)), else idle.
  
  A change of clip blends in: each frame the arms move (1 − b)² of the way to the new pose, with b falling from 1 at 3/s. A switch (the soldier's weapon is no longer the one the arms hold) lowers the arms with the old weapon (handsdown), then raises the new one; firing the new weapon finishes it at once. The page plays the rounds its weapon prediction fires from copies of his weapons, so the arms know a weapon by its class: a predicted round is never a switch. Entering first person starts from handsdown and holds idle for 1 s.
- **Camera motions** play the `globals` tables: a dive plays `RollLeft` or `RollRight`, lying down `StandToProne` or `CrouchToProne`, getting up `ProneToStand` or `CrouchToStand`, and landing `Land`. Each segment is a cubic Hermite from the last key to the next over u = 1 − time left / `Time`; the first starts from wherever the eye is, at its slope. The eye moves by the offset in the view's frame, then the view turns by the angles in its own (Rz · Ry · Rx). The rolls, `CrouchToProne`, and `ProneToStand` also drop the arms (handsdown at once) and play jump_land for the motion's length. `Land` dips the eye a full metre in 0.17 s.
- **Head bob** is built in: four 0.2 s segments with keys (0, 0.1, 0), 0, (0, 0.1, 0), 0 (half that prone) and slopes (0.1, 0.1, 0), (0.2, 0.2, 0), (0.2, 0.1, 0), 0. A cycle starts whenever the soldier moves at 1 m/s or more on his feet with no other motion playing, and the offset drops to zero the moment he stops.
- **Scope:** zoomed in first person (or with a weapon set `ZoomFirstPerson`), the weapon's `ScopeTexture` covers the screen, one quarter per screen quarter, mirrored, with its (1, 1) corner at the centre.

### HUD

The multi-unit radar, targeting, hit/kill feedback, stats and squad display
are implemented in `src/hud-units.js` and `src/hud.js`; their rules,
APIs and validation are in [ai-hud.md](ai-hud.md). Capture credit revisits up
to 64 capture-region occupants on neutral-to-owned transitions and increments
each living Character's tally, without repeating the team, seated-state or
`CapturePosts` contribution gates.

Interface space is 1024 units across the screen and H = 768 + (h / w − 0.75) × 768 units down it, y down. Each group hangs from an anchor (ax, ay) on the screen, most inside a safe area (0.05 + 0.9 × a), through its own transform. Everything is drawn through one camera with a 60 by 46.8 degree frustum, whatever the window's shape, onto a plane 886.81 units away: an anchor sits at ((ax − 0.5) × 1024, −(ay − 0.5) × 768, −886.81), and a group's y units are scaled by 768 / H. Quads are texture × tint, alpha blended, with no depth test. Panels are cut from `hud1` and `hud2` by texture rectangles; u0 > u1 mirrors.

- **Weapons:** anchor (0, 0), turned 30 degrees about y and moved (0, 21, 40). A panel per channel, the weapon's `IconTexture`, and `gamefont_tiny` numbers: clip / reserve, one total for single-round clips or a reserve of one clip or less, the clip alone with an endless reserve, nothing for heat weapons. The bar shows a reload's progress (green) or 1 − heat (blue, red when overheated). On a switch the weapon's name shows to the right and fades over 2 s. (In the original the green bar shows a charge or a launcher's lock under way, with no bar during a reload; the port draws the lock's and still the reload's. Each panel has its own name label, and seated it shows the seat's name, below.)
- **Health:** anchor (1, 0), turned −30 degrees and moved (0, 25, 40), right-aligned. The bar is cut from its left end. It is blue, and at half health or less it pulses toward red over the last 0.25 s of every 4f² + 0.25 s. A loss flashes it toward white over 0.5 s. Its icon is always `hud_health`; a class's `HealthTexture` shows only where others see it, in the hull and enemy panels.
  - Enemy, last in the group: while channel 0's target is an enemy, its health, at full strength, waking the group every frame, then fading over 2 s. hud2's panel (uv 0.132812, 0.359375 to 0.889764, 0.617188) 96 × 33 at (0, 32), its bar (uv 0.25, 0.765625 to 0.734375, 0.867188) 62 × 13 at (−31, 49) cut from its left end, and the target's `HealthTexture` (else `hud_health`) 24 × 24 at (−6, 36), all tinted red (223, 32, 32); the bar never flashes or pulses. Online, a target still spawn-protected shows a full yellow bar. Nothing moves it: with the second panel and the seat pips up, the three hexagons interlock.
- **Fading:** the weapon and health groups fade in at 8/s when something changes, hold 3 s, then fade out at 1/7 per second to 40%. Reloads and heat weapons keep the weapons group awake.
- **Reinforcements:** anchor (0.5, 0), scaled 1.7, alpha 204. Each team's bar runs out from the middle at 55 × count / starting count, with its count in `gamefont_tiny` at 0.6. A bleeding team pulses toward white every 2 / rate seconds. Below, a team holding no posts gets a countdown: "VICTORY IN" or "DEFEAT IN" over the seconds in `gamefont_large` growing to twice its size in the last 20 s, `gamefont_medium` under a minute, m:ss before that.
- **Reticle:** at x = 0.5 and the height of the point 1024 m along the aim, outside the safe area, at its texture's own size. Its colour is (67, 167, 242) on nothing, green on a friend, and red on an enemy. Its alpha is 63 rising to 127 while the weapon waits to fire again, 63 reloading or overheated, and 255 otherwise.
- **Capture:** anchor (1, 0.5), moved up 30. `hud_flag_timer` fills from the bottom in the colour of the team taking the post, over grey. While contested, the grey alternates with the other team's colour every half second. It shows only in posts the player's team doesn't hold.
- **Minimap:** anchor (0, 1), 143.36 units across through the `map_mask` disc, the map at alpha 128. It is turned so the soldier faces up, with 125 m to the rim on foot. Posts are `hud_flag_icon` at 6 × `MapScale` units in relation colours, pinned inside the rim and blinking toward the colour of a team taking them. `player_icon` (16 units) sits in the middle.
- **Messages:** anchor (1, 0.9), right-aligned, five lines at most with the newest at the bottom, 20 units apart, each for 10 s.
- **Damage direction:** `ambush_icon` (128 × 16) at the edge an explosion came from, fading over a second.
- **Leaving the battlefield:** anchor (0.5, 0.5), "LEAVING BATTLEFIELD" in `gamefont_large` centred both ways, red, its alpha flashing while the player is outside (see "Leaving the battlefield").
- **Palette:** green (32, 223, 32), red (223, 32, 32), grey (192, 192, 192), yellow (223, 223, 32), bar blue (0, 152, 255), bar low (203, 57, 64).
- **Dead:** with no body, only the reinforcements, the messages and the stat popup show (the minimap, weapons, health and reticle go at the death and come back with the next body).
- **Seated** (a vehicle's seat, a craft's included, or a turret): the same groups, read from what he controls. Nothing checks for a flyer or for first person: a craft's HUD is a hover's or a walker's.
  - Health: the top bar stays his own soldier's. Under it the hull's panel takes the second bar's place (hud1 84 × 33 at (−27, 16), the bar 48 × 12 at (−59, 18), the icon 24 × 24 at (−35, 20)): the class's `HealthTexture`, `hud_vehiclehealth` for a class with none, blinking and flashing as his own bar does (its own timers), on the group's fader. Every seat of a hull shows the hull.
  - Seat pips, drawn first in the group: one per seat of the hull in its order (the pilot, the turrets, the passengers), the first five, at x = −89 + 7i, y = 35, 11 × 10, right-aligned: hud2's slot (uv 0.015625, 0.351562 to 0.101562, 0.429688), never tinted, and over a held seat its fill (uv 0.015625, 0.5 to 0.101562, 0.578125) tinted white for his own Character, green (32, 223, 32) for any other occupant with a Character (bots included), yellow (223, 223, 32) for one without.
  - Weapons: the seat's channel 0 and channel 1, by the on-foot rules; a weapon with no `IconTexture` leaves its panel without one. Each panel's label (at (123, 1) from the panel, `gamefont_tiny` white over a black copy at alpha 207, fading at 0.5/s) shows the seat's `VehiclePosition` string (hashed `da1b8c0c`: PILOT, CO-PILOT, MAIN GUNNER, BOMBADIER, LEFT TURRET, RIGHT TURRET, PASSENGER) in place of the weapon's name, or nothing for a seat with none. Taking a seat changes both panels, so both show it.
  - Reticle: both channels' at the one anchor, channel 0's art and ticks, then channel 1's, then the hit ring, then each launcher's diamond. Each takes its alpha from its own weapon's state (63 rising to 127 between shots; 63 reloading or overheated; 255 otherwise, a charge or a lock under way included) and its colour from its own weapon's target: the player's targeting runs each channel's weapon apart. A weapon with no `ReticuleTexture` shows four ticks, each from r to r + 5 units at 0, 90, 180 and 270 degrees, r by the lock-tick formula with the weapon's current yaw and pitch spread (its class's plus what shots and kick add) in place of `LockOffAngle`; that channel has no lock display. The anchor is x = 0.5 for every controlled thing; a craft's pilot aims along the nose, so in the chase view it sits above the middle by about `TiltValue`.
  - Minimap: 125 m to the rim at 10 m/s across the ground or slower, widening linearly to 250 m at 100 m/s (the controlled thing's speed, a gunner's his hull's; no smoothing), which scales the map's window, every icon's place and where icons drop off; turned with the seat (a craft's pilot: the frame it flies by, not the camera). On foot the same rule leaves it at 125 m.
  - Capture: the timer shows only for a controlled thing that captures (`CapturePosts`, 0 for vehicles).
  - None: speed, throttle, altitude, heading or power; warnings; a target's name or distance; lock text or tones; messages on entering, leaving or switching seats. The tool tips exist, `game.tooltips.flyertakeoff` ("TO TAKE OFF") among them, but the PC game builds the display disabled and never shows one.
- **Stat popup:** anchor (0, 0.5). Row i (kills, deaths, command post points) has its origin 36 units in and 32 × i down, drawn at 0.75: its icon (`stats_kills`, `stats_deaths`, `stats_cpp`) 32 units square ending 4 units before that origin, its number in `gamefont_small` right-aligned at it. Both are white at alpha 64. A change turns the icon and number yellow if the number rose, red if it fell; a per-row timer starts at 1 s, holds the colour while at 0.5 or more, then blends back by 2 × t. Any change shows the group for 3 s, at full strength until the last second, then t × 255. It watches the player's Character alive or dead, starts from zeros, and hides while paused and once the game is over.

### Scores and the end-of-match screens

Every Character has a stats record. Most fields are kept per team slot t: the Character's team − 1, with any team but 1 and 2 using slot 0. Counters are bytes that wrap, except head shots and command post points, which are 16-bit. Every recorder does nothing once the game is over.

- **Kills and deaths** (on a Character's death): the killer is the Character in the dead soldier's last-damage record. If the killer's team is the victim's enemy (relation below 0), he gets a kill; if it is friendly, his own team included, a kill and a team kill, so a suicide is a team kill; a neutral gets nothing. Then the victim gets a death, killer or not. The scoreboard shows kills − 2 × team kills, deaths and command post points, summed over both slots.
- **Command post points:** capturing a neutral post gives one to every living Character in the post's capture region, the first 64; destroying a command post object gives one to its last damager, unless his team's relation to the owner is 1 or more.
- **The rest of the record**, for DETAILS and AWARDS:
  - per victim, how many times this Character killed him, team kills and suicides included;
  - on an enemy kill, the kills in this life (best kept), and a SURVIVALIST point if his own last health byte (health × 255, kept by the per-frame tally) is under 64;
  - on an enemy kill, the most kills inside 5 s, from a ring of the last 100 kills shared by everyone. Entries 5 s old drop off the tail; the new one goes at the head; then the killer's entries are counted, but each counts only if the slot stored at the new head (the next free entry, not the counted entry) is his. Until the ring wraps that slot is 0, so only slot 0's sprees count;
  - head shots, counted when the soldier takes damage, for a hit to the head (location 2) on a living soldier. They go to whoever is in the soldier's last-damage record before the hit: the record is written after, when the damage applies;
  - vehicles destroyed, counted when a flyer, hover or walker dies: one for its last damager, whatever his team. Turrets and droidekas don't count.
- **Per-frame tallies**, for bots always and for the human from SPAWN on:
  - seconds in his class, kept in 5 entries a slot, and his life's length (best kept). Both run whenever the Character has a class, alive or not;
  - seconds with his controlled entity's channel-0 weapon (10 entries) and in a vehicle seat (5 entries). When a table is full, the entry with the least time is replaced, restarting at the frame's time;
  - camping: if the controlled entity is more than 10 m from the anchor (a squared distance over 100), the anchor moves there and the time resets; otherwise the time adds up, best kept;
  - health: under 0.1 sets a flag; back to 1.0 or more clears it and counts a narrow escape. A death clears it.
- **DETAILS** (`ScriptCB_GetPersonalStats`) lists HEAD SHOTS, FAVORITE CLASS, FAVORITE WEAPON, FAVORITE VEHICLE (each "NAME m:ss" from its table's longest entry, the name its display name), LONGEST LIVING STREAK (" m:ss", the slots' bests summed), NARROW ESCAPES, VEHICLES DESTROYED, NEMESIS (who killed him most, "NAME (n)") and BAIT (whom he killed most). There are no percentages.
- **AWARDS** (`ScriptCB_GetAwardStats`) goes through slot 0 then slot 1, every Character in index order, keeping a value only if it is greater than the best so far, which starts at 0: TANK BUSTER (vehicles destroyed), KILLING SPREE, CAMPER (whole seconds, "(m:ss)"), SURVIVALIST, TRAITOR (team kills), PUBLIC ENEMY (most enemy kills in a life), BANTHA FODDER (deaths), DEAD EYE (head shots). The value shows as "NAME (n)", or nothing without a winner.
- **The sort** of the end table (per team): mode 0 (the default) orders kills descending, deaths ascending, command post points descending, then the name; mode 1 starts with deaths, mode 2 with points. Names compare by UTF-16 code unit, descending: the comparator negates a compare that already runs b − a. Clicking the same header again sets a reverse flag that flips only the first key.
- **Bot names** (at Character creation): a first name drawn from the side's `common.sides.<side>.firstnames.NN` (or two letters A to Z when there are none), a space, and a last name from `.lastnames.NN` (or three digits); locals take `level.<map>.local`. Each team draws from its own generator, seeded with its side type (Rebels 1, Empire 2, Republic 3, CIS 4) just before its Characters are made. A draw steps s = s × 0x19660D + 0x3C6EF35F twice and returns the second step's high half over the first's; a pick is the draw modulo the list length. Taking a hero class renames a Character to the hero for good. The human is "Player %d" offline.
- **The end:** when a team has lost, the game-over flag is set 2 s later. That freezes the scores and closes the spawn screen. The end display runs for 5 s, and the end timer is 7 s offline. The team stats are pushed when either the end timer runs out or the end display's time is up with its music done. At that push the `ingame` sound bus fades to 0 over a second. DONE, Esc or 30 s without input quits the stats.
- **The player list** (Tab, control 17): only in a network game. Held for 0.5 s it closes on release; tapped sooner, it stays until Tab is held and let go again (`ScriptCB_CheckPlayerListDone`). While it is up the HUD hides and the `ingame` bus fades out over a second. It is `ifs_mp_lobby`: Tab opens the pause menu with "skip to player list" set, the pause menu pushes the list at once (`ScriptCB_SkipToPlayerList`) and unpauses when it closes; in a network game the pause menu also has it as PLAYER LIST. Its rows are the network session's player slots, so no bots, sorted by team, then kills (most first), then name: NAME, TEAM (the team's name in its colour), PING and KILLS, the player's own name in his team's colour and the others' white. CONNECTION (`ping_icon`'s bars) replaces PING and KILLS only on Xbox Live.

### Third-person camera

- The camera looks along the soldier's aim (yaw and pitch), then pitches down by the section's `TiltValue`. Its position is the pivot (`TrackCenter` in the soldier's frame) plus `TrackOffset` in the camera's own frame (right, up, back), so the pivot sits at the center of the screen.
- Rotation eases toward that at the section's rotation rate (30/s). Position eases separately along each camera axis at 15/s (half that prone). A target more than 80 m away snaps.
- Five rays from the pivot (the boom and its four corners, 0.4 m out) pull the camera in. It snaps in and eases back out at 0.75 of the boom length per second.
- `ThirdPersonFOV` is the horizontal field of view. The vertical one follows the desktop's height over width, so wider screens see less above and below. Zoomed, the view takes `FirstPersonFOV` narrowed by the zoom. The view's FOV eases toward its target at 25/s.

### Death camera

When the player's soldier dies, a death camera (`DeathCamera`) takes over from the chase camera and runs until he is in a new body. The camera manager keeps one per view beside the chase and free cameras, and whichever is current sets the view everything reads: what is drawn and heard, the Classic AI's scheduling frustum, and the 60 m soldier collision pass.

- **Start:** the fatal hit only marks the soldier dead. His next update kills him, and that starts the death camera. While he lies dead with no unit on his Character, his update asks again.
  - It starts only from the chase camera, taking its last pose.
  - The same kill turns first person off, so from first person it starts at his eyes and his body is drawn.
  - The kill waits while he is thrown or in two other states. Meanwhile the chase camera follows him as a dead soldier: at a tenth of its rates, and with the field of view a dead soldier reports, the camera's default 0.837758 rad = 48 degrees, eased toward at 25/s. Normally that is one frame.
- **Only soldiers** start it. A droideka's death doesn't, so its chase camera stays on the wreck.
- **The killer:** the Character in the body's damage record, unless it is his own. The record is overwritten by every hit that hurts him while alive, so it names whoever dealt the fatal damage. Friendly fire counts.
- **No killer:** a suicide (his own grenade, or the pause menu's RESPAWN) records none, and so do kills with an empty source: a death region, dropping below the map's kill height, drowning, and the out-of-bounds countdown. Falling faster than 60 m/s, or leaving the world's box (1000 m out, 500 m up or down), kills without touching the record, so whoever last hurt him is the killer.
- **Vehicles and turrets:** when a hull or turret is destroyed with him in it, he is killed with its own damage record, so its destroyer is the killer.
- **Its two points:**
  - The body point is 0.8 m above the body's origin (its feet). It follows the body every frame while the body exists, falling with it.
  - The first near point is fixed at the start: 1 m from the body toward where the camera looked from (the camera's back axis, made level), and 1.5 m above the feet.
- **Each frame** it works out a pose looking from a near point toward a far one:
  - **Until 1.5 s, and after with no killer:** from the first near point toward the body point. It rises and backs off to look down on the body at atan(0.7) = 35 degrees, keeping its heading. Behind a standing chase view, that is about 5.1 m back and 4.4 m up.
  - **After 1.5 s with a killer:** from the body point toward the killer's eyes. That is his vehicle or turret's eyes while he sits in one, else his soldier's, checked every frame, so it follows him, into and out of vehicles, however far off and whether or not anything is in the way (nothing tests that it can see him). A soldier's eyes are the aim pivot (0.06, 1.7, 0) standing, (−0.1, 1.2, 0) crouched, (0.06, 0.6, 0.4) prone. A killer who has died has neither (the Character drops both at death), and it looks down on the body again.
  - **Placement:** two rays run 6 m back along the line, from 0.75 m above and below the near point. They meet terrain, buildings and props, not vehicles or soldiers. The camera sits on the line 6 × (the nearer hit's fraction) − 1 behind the near point: 5 m back when clear, else 1 m short of the hit. It looks along the line, world up, with no roll.
  - **Blend:** the camera then moves a fraction t of the way to that pose, lerping the position and slerping the rotation. t is time / 1 s over the first second, then 1. After 1.5 s with a killer it is (time − 1.5) × 0.5, up to 1.
- **The blend applies every frame,** so the camera arrives well before t reaches 1:
  - At 60 frames a second the rise is nearly done by 0.4 s, and it holds still from 1 s to 1.5 s.
  - The turn to the killer is a swing of about half a second: 65% done at 1.75 s, 98% at 2.0 s. A moving killer is trailed a little until t reaches 1 at 3.5 s.
  - Mid-swing the slerp rolls the view, and it settles level.
  - The times and length are constants: 1.0 s, 1.5 s and 6.0 m.
- **Where it ends up:** on level ground the line from the body point up to a standing killer's eyes slopes up behind the body. So the lower ray runs down into the ground within a metre or two, and the camera usually ends at the body, about 0.8 m up, looking at the killer. It backs off further only when the killer is far off or below. A killer within a few metres puts it in front of the body.
- **The view:** it sets only the transform and a zoom of 1, so the scope drops and nothing shakes it. The field of view stays as the chase camera left it. That is `ThirdPersonFOV` (`FirstPersonFOV` if he was zoomed or in first person) after that frame toward 48 degrees: from 65, about 59 degrees at 60 frames a second, and 49 at 10.
- **The end:** anything newly tracked hands the view back to the chase camera: his next body, or the corpse's removal at 4 s, when the spawn screen opens on one of its camera shots.
- **HUD:** it has no text of its own. "Who killed you" is the kill feed's line (`game.infomsg.killed` "%s KILLED %s", `teamkill`, `suicide`, `died`).

The port (`src/camera.js` `DeathCamera`) steps it in the world tick on the world's camera, at 60 Hz, from world state alone. It starts at the death, after one 60 Hz step of the field of view toward 48 degrees (`deadFov`), also when he is thrown; the original waits for him to land. Once his body has left the world it holds its last body point until he spawns or the screen's shot covers it.

What the page draws is its own camera (`src/death-shot.js` `DeathShot`), run on the page whatever hosts the battle (the local world or a server) from the body and the killer as the page draws them. The world names the killer in plain numbers (`DeathCamera.blame`: his unit and life, his Character's slot, or a hull credited by itself and its life); online, the server's damage and death records name him. The Death Camera option picks its style:
- **Classic:** the rules above, with each blend being the original's per-frame fraction at its 60 frames a second integrated over time, so the timing is the same at any frame rate. The rays' reach comes in quickly and goes out slowly. When the killer loses his body it turns back to the body (and to his next body) over about half a second instead of in one frame.
- **Enhanced:** a replay as a sports broadcast shows one. It holds on the body from a high three-quarter angle, drifting round it. From 1 s it cranes round the body to the reverse angle, looking over the body at the killer with both in frame, while the killing shot's trace (`src/death-trace.js`) runs from where he stood to the body. Then it pushes in, the lens narrowing to frame him, and follows him. With no killer it stays on the body.
- **Both:** every point it frames follows what is drawn through critically damped springs whose steps are exact for any frame length. The pose then passes through springs of its own, so a killer getting into a hull, a ray meeting a wall or a change of style can't make it jump. It starts from the drawn view with that view's motion, and its field of view eases instead of popping.

### Soldier animation

Clips are looked up by name: `human_<weapon>_<posture>_<action>`, with `_alert` appended while the soldier is alert, and `human_<weapon>_<action>` for jump, fall, and landings. The rifle has no side-stepping clips standing or crouched, only prone crawls.

- **Idle or moving:** the soldier counts as standing still below 0.3 m/s on both of its own axes, or above 8 m/s. The travel angle is `atan2(left, forward)` in the soldier's frame.
- **Leg direction:** the legs keep an angle off the facing. Standing still they stay planted while the facing turns, until they pass +22.5° (step right) or -40.5° (step left); crouched the left limit is -67.5°, prone both are ±27°. The turn clip then swings them to 0.2 rad past center over its length.
- **Moving upright:** the forward cycle plays for travel within ±72° of the facing (±108° once already moving forward), and the backward cycle otherwise. The legs ease toward the travel angle, or its reverse for backward, clamped to ±72°, a quarter of the way per 1/30 s. Alert narrows that to the forward cycle within -103.5°..+67.5° and legs within -45°..+31.5°. Prone crawls sideways when sideways speed dominates.
- **Walk or run:** forward switches from walk to run above 5.5 m/s and back below 5.0. Backward uses `runbackward` whenever the set has one. A cycle plays at 0.95 x ground speed / authored speed + 0.05.
- **Upper body:** relaxed, the torso and weapon follow the legs and only the neck and head turn toward the aim (2/8 and 5/8 of the offset, cumulative). Alert, the spine takes it instead: `bone_a_spine`, `bone_b_spine`, and `bone_ribcage` end up turned 1/8, 4/8, and 8/8 of the way. Aim pitch is clamped to ±63° and eased 15% per 1/30 s. Prone always aims with the spine and never below level.
- **Alert timer:** firing sets it to 2.5 s, or 2.0 s if already alert. It eases in over the first 0.5 s and back out over the 0.5 s after it runs out.
- **Air and landing:** a jump shows the jump pose for 0.3 s, then the fall pose. A landing after more than 2 s in the air plays `landhard`, otherwise `landsoft`, which is cut short after 0.15 s if the soldier is already moving. Going prone and getting up play `stand_getdown_prone`, `crouch_getdown_prone`, `prone_getup_stand`, and `prone_getup_crouch`.
- **Dive:** a side dive plays `diveforward` with the legs turned a quarter turn toward the dive (right is -90°, left +90°).

### Sound

The Steam and GOG release still runs the 2004 game's sound library (Pandemic's "SoundFL"). It adds an output that plays its DirectSound-style buffers through OpenAL Soft (`OpenAL32.dll`, `alsoft.ini`). The library picks one of five mix configurations: software, DirectSound with hardware 3D and EAX, DirectSound 3D, OpenAL, and none. What follows holds for all of them unless it says OpenAL. Which one a player's profile uses is not documented here; the port's original mode follows the OpenAL one.

- **Properties.** A SoundProperties object leaves unset:

  | Key | Default |
  | --- | --- |
  | `Pitch` | 1 |
  | `Gain` | 1 (0..1) |
  | `ReverbGain` | 0 |
  | `Looping` | off |
  | `Pan` | 0 |
  | `Mode3D` | **off**: a sound is 2D unless it says so |
  | `ModeDoppler` | **on** |
  | `Priority` | 1 |
  | `Bias` | 0.5 |
  | `MinDistance` | 0 |
  | `MuteDistance` | 1 |
  | `MaxDistance` | 1 |
  | `RollOff` | 0 |
  | `PlayProbability` | 1 |
  | `PlayInterval` | 0 |
  | `GainDev`, `PitchDev` | 0 |

  `RollIn` is read but never used.
- **Inherit** is applied as the object is read. The parent must already have been read; a parent read later is ignored. The object takes all the parent's resolved properties except `Name` and `Group`: keys before `Inherit` are overwritten, and keys after it override. Its samples are the parent's own `SampleList`, not whatever the parent inherited.
- **Starting a sound:**
  - If it is asked for before its `PlayInterval` (plus or minus `PlayIntervalDev`) has run out since it last started, it doesn't play. The interval is set before the next check, so a sound that then fails it still uses up its interval.
  - It doesn't play if a random number exceeds `PlayProbability`.
  - It picks a sample by weight (cumulative thresholds, no repeat avoidance), or in list order with `CyclePlayback`. A sample's own interval gates it the same way, without the probability.
  - Gain is clamp(`Gain` + `GainDev` × 2(r − 0.5), 0, 1) and pitch is `Pitch` + `PitchDev` × 2(r − 0.5), drawn once per play.
  - `RandomPlayPos` 1 starts it at a random point.
- **Distance**, as a linear gain, with d measured from the listener:
  - Under `RollInDistance` (rid): d / (rid + 10 × `RollOff` × (rid − d)).
  - From there to `MinDistance`: 1.
  - From `MuteDistance` on: 0.
  - Between: (`MaxDistance` − d) / ((`MaxDistance` − `MinDistance`) + 10 × `RollOff` × (d − `MinDistance`)). The 10 is a listener constant that nothing changes.
  - The rifle's shot (Min 11, Max 400, RollOff 3.5) is at 0.07 at 50 m and 0.024 at 120 m.
  - 2D sounds have no distance gain.
- **Voices:**
  - 64 virtual voices, of which 32 are heard (`/audiomaxchannels` sets 8 to 32).
  - Every frame each voice scores (1 − `Bias`) × its level + `Bias` × `Priority`, its level being the gain with its bus and distance. The top 32 play; the rest keep time silently and resume in place.
  - The game's sounds set `Bias` 0.0001, so in practice the loudest are heard. Interface sounds use 0.5.
  - A sound asked for while all 64 are in use gets a voice only by stealing the lowest-scoring one, and only if (1 − `Bias`) × its bus gain + `Bias` × `Priority` is strictly higher.
  - A sound whose level where it is asked for is exactly 0 (a 3D sound at or past its `MuteDistance`) doesn't start at all when the caller asks for that. The game's entity sounds all do: the one-shot player (impacts, among others) and the voices each entity keeps (weapons, a soldier's hurt and death voice). Streams are not culled this way (see the voice-over's queue below). A battle out of earshot takes no voices, and its shots can't steal the ones near.
- **Buses:** a bus's gain is its `Gain` times its parent's, and a voice plays at clamp(bus gain × its rolled gain, 0, 1) × its distance gain.
  - Ducking: when the first voice starts on a bus with `AttenuateBus`, the bus it names ramps linearly to 1 − `Attenuation` over `AttenuationLeadIn`. When the last ends, it ramps back to 1 over `AttenuationLeadOut` (defaults 0, 1 s, 1 s). Voice-over takes `main` (sound effects and music) down to 0.5 over half a second.
  - The options' sliders (0 to 10 in tenths) set music to the music bus, effects to soundfx, ambience and shellfx, voice to voiceover, and master to root.
- **Doppler**, on unless `ModeDoppler` 0: the pitch is multiplied by 1 + Δ / 344. Δ is how much nearer the source is now than one velocity-second ago, relative to the listener's own motion. Past 172 m/s there is no shift.
- **The listener:** its position is the camera's, or the controlled unit's when a flag on the player says so, plus 2 m along the camera's view (7 m for some vehicles). Its orientation is the camera's. Each source uses the nearest of up to four listeners (split screen).
- **Reverb.**
  - The listener's space picks the environment: its I3DL2 preset, with gains converted to millibels (2000 log10).
  - `Density` is ignored (EAX density comes out as 1), and `Diffusion` is clamped to 0..1, so the authored 60 to 100 are all 1.
  - There is one effect slot.
  - A voice's reverb send comes from its space's `RoomGain`, not from the sound's `ReverbGain`, which only the software mixer and streams use.
  - Switching spaces is immediate, and a voice's space values glide 5% a frame.
- **OpenAL output:**
  - 512 mono sources, two auxiliary sends, the device's own rate.
  - Sources are placed head-relative, with no velocity: the library does Doppler itself.
  - The output gives each source a reference distance of half its `MaxDistance` (and a maximum of twice it) under OpenAL's default inverse distance model. This adds a falloff of (`MaxDistance` / 2) / d past half the maximum: −6 dB at `MaxDistance`.
  - Pitch is kept between 100 Hz and 200 kHz as a frequency.
  - Pitch changes glide (target − current) / 256 twice a frame, and volume changes go halfway a frame.
  - 2D voices never receive their `Pan`, so they play centred.
  - The library itself has no limiter; OpenAL Soft has its own output limiter.
- **OpenAL Soft** itself is version 1.20.1. The release's `alsoft.ini` sets one option, `front-stablizer = true`. What that version does with the library's sources:
  - It pans with built-in 2D ambisonic decoders (N3D): first order on stereo, second on 5.1, third on 7.1. They feed neither the centre nor the LFE. On stereo speakers (no HRTF), a positioned source in front has its azimuth widened 1.5 times, so a source at 60 degrees or more plays from one speaker alone; a source straight ahead gets 0.596 of its level in each speaker, and one straight behind 0.404. On 5.1 a source ahead gets 0.517 in each front speaker and −0.036 in each surround.
  - Multichannel buffers are never positioned: each channel plays at a fixed angle, stereo at ±30 degrees, `AL_FORMAT_REAR16` (a quad stream's second pair) at ±150, mono ahead. On stereo speakers the angles in front are widened 3 times, so a stereo pair plays hard left and right.
  - The front stabilizer (on for 5.1, 6.1 and 7.1) splits the front pair at 5 kHz with a phase-corrected crossover, moves the sum of the pair's lows a third of the way toward the centre and the sum of the highs a quarter (pans of 30 and 22.5 degrees), keeps the difference on the pair, and delays every other channel 256 samples to match. It is the only thing that feeds the centre.
  - With headphones reported by Windows (the endpoint's form factor), its HRTF is on by default (the built-in MIT KEMAR set): every source, and every channel of a multichannel buffer at its angle, goes through it. Otherwise headphones get the stereo panning.

What the game does with these sound objects:

- **Streams** (`OpenAudioStream`): each call opens a slot on a stream file, up to six; Theed opens `nab2` twice. A slot plays one segment at a time. A stream's properties pick a segment from their `SegmentList` the way a sound picks a sample: by weight or in turn, each gated by its own interval, with no second try when the pick is gated.
  - Substreams play in step: the first to the front pair, the second to the back pair, a third to centre and LFE. The OpenAL output sends the second pair as `AL_FORMAT_REAR16`.
  - A multichannel stream placed in the world is set by distance only, not panned (the port does the same).
- **Music.** `SetAmbientMusic(team, amount, track, stage, isFraction)` gives each team a start, middle, and end track (stages 0, 1, 2). With `isFraction` the threshold is trunc(the team's reinforcements when the script runs × `amount`): Theed's 1.0, 0.99, and 0.1 of 200 are 200, 198, and 20.
  - The stage: for team 1 and then team 2, the highest stage whose threshold is at or above the team's tickets. Team 2 is looked at last and its start stage always matches, so in practice team 2's tickets set the stage for both sides.
  - Every frame the listener's team's track for the stage is asked for; when the stage is past 0, both teams' tracks for the stage before are asked to stop.
  - One track plays at a time: of those asked for, the highest `Priority` that is ready, and only once nothing else plays. A track is ready `MinInactiveTime` after it last stopped, and at the start. Once playing it can't be stopped for `MinPlaybackTime`. It fades in over `FadeInTime` and out over `FadeOutTime`, and plays one segment. Still asked for, it plays again when next ready: Theed's start track (39.2 s) rests 90 s between plays.
  - The game is over 2 s after a team has no reinforcements left: every stream and voice stops, sound effects fade out over a second, and the listener's team's victory or defeat track plays (priority 1, no fade-in). No voice-over plays then.
- **Voice-over.** Lines are queued per stream slot: a queue holds up to four, ordered by the voice score, and an idle slot plays the first, at the place and with the gain and pitch it was given when it was asked for. The commander's line template is 2D, `Gain` 0.7, on the voiceover bus.
  - Asking for a line: the stream must have an open slot and its bus a gain over 0; a segment is picked (each gated by its own interval), then the gain is rolled, then `PlayInterval` and `PlayProbability` are checked; last the line is scored, (1 − `Bias`) × level + `Bias` × `Priority`, the level being `Gain` times its bus's and, for a 3D line, the distance gain where it was said. Nothing a line can be scores 0: a 3D line out of earshot scores `Bias` × `Priority`, is queued, and holds the slot unheard while it plays.
  - Queueing: every line waiting is scored again against where the listener is now, the new one goes in by score, and a fifth line drops the last. If the new line is itself last, a 3D line is refused, and a 2D one drops the line ahead of it instead.
  - Lines asked for without the queue play on the slot at once.
  - Command posts: each post's instance properties name a line per listener side, actor side, and event, `VO_<Listener>_<Actor>Capture`, `Lost`, `InDispute`, and `Saved`. Theed sets six per post, for the Republic and CIS listening (your side capturing or losing, the enemy capturing); it sets no `InDispute` or `Saved` lines. A capture also plays the post's `CapturedSound` at the post, and a loss its `LostSound`.
  - Bleeding (`SetBleedingVoiceOver(listener, team, line, normal)`): a team starting to bleed reinforcements plays the listener's line for it. While the listener's own team keeps bleeding, the line repeats every 30 s (`SetBleedingRepeatTime`). An enemy team starting to bleed while the listener's bleeds too plays the line set with `normal` 0, which no mission sets.
  - Leaving the battlefield: outside every boundary, a 10 s timer and the team's leaving line; death when it runs out. The low-reinforcements line plays once a battle, in the Galactic Civil War era only; the planetary bonus lines only in Galactic Conquest.
  - The AI's chatter: a soldier class sets `AcquiredTargetSound`, `HidingSound`, `ApproachingTargetSound`, `FleeSound`, `PreparingForDamageSound`, `HeardEnemySound` and `ChargeSound`, each played at the soldier's place through the one-shot player, queued. The squad command bank (13 AISC* lines for a bot, the same 13 SC* lines for the local player) is played at the sender: a bot's queued and 3D, the local player's own unqueued, with the line's 3D turned off. The stock lines are all streams: a bot's on `cw_vo` (or `gcw_vo`, with the commander's lines, so one plays at a time across the battle), 3D, `Gain` 0.7 on the soundfx bus, full to 11 or 16 m and silent past 150 m, most with a `PlayInterval` of 20 ± 5 s that holds every speaker of the line (some of the Naboo guard's 1 s); the player's own on `cw_tac_vo`. Which soldier says what when is the AI's (docs/ai-controller.md); there is no team or distance test besides the line's own.
- **The map's own sound.**
  - Regions named `soundstream <properties> <v>` (or `soundstatic`) start an emitter of that stream at the region's centre. Its reach R is the length of the region's `SIZE` vector, whatever its shape; `MaxDistance` and `MuteDistance` are R and `MinDistance` is R / v. Theed's `soundstream nab_amb_city 1.0` is 400.5 m: the city's quad bed at full level anywhere within 400.5 m of the centre.
  - `com_snd_amb_static` objects loop their `Sound` at their place; their `MinDistance` and `MaxDistance` (which sets `MuteDistance` too) replace the sound's, and the class's own are 1 and 10.
  - Spaces: the listener keeps its space while still inside it; otherwise it takes the first, in load order, whose box holds it, else the default.
- **Interface sounds** come from the mission's `SetSoundEffect` calls: the scope's zoom (`ScopeDisplayZoomIn`, `ScopeDisplayZoomOut`, `ScopeDisplayAmbient`), the spawn screen (`SpawnDisplay…`), `WeaponUnableSelect`, and the ambient birds (`Bird…`). They are 2D.
- **Foley.** A soldier's sounds come from the FoleyFXSoldier entry of the material under it: terrain contact uses `terrain_foley`, wading `water_foley`, and an object its `FoleyFXGroup` (none leaves the last one). The entry is the first in the group whose `Class` is the soldier's `FoleyFXClass`; CIS droid infantry has none, so it makes no foley.
  - Footsteps come from two detectors sharing a flag per foot, both silent until the soldier has first touched the ground after spawning (a flag set by a contact whose normal is within 53 degrees of up, which only spawning clears).
    - The feet (checked after every pose update): a foot bone (`bone_l_foot`, `bone_r_foot`) falling faster than 0.3 m/s, by its height in the model, is flagged and plays `ClothingRustleSound`. A flagged foot rising as fast is unflagged, and steps if it rose from under 0.17 m.
    - The stride (checked every tick, so in every state but the dive roll): the length of the soldier's whole velocity (vertical included) times dt adds up; with no foot flagged, half a `FootstepStride` (d641f0d6, 1.75 m unless set) rustles and flags the next foot, right first; with one flagged, a full stride, or stopping, steps it. Nothing stops it in the air: a jump or a jetpack flight steps every 1.75 m.
    - The two run side by side, so a foot the stride steps before it lands is flagged again on its way down and steps again at its plant, wherever the animation gives the stride time to get there first. The footstep sounds have no `PlayInterval` to hide it.
  - A step plays `FootstepLeftRun` or `FootstepRightRun`, plus the weapon's step slot. The walk variants need the speed to be under `RunSpeed` (44c8744d), which no class sets, so every step runs.
  - Jumping plays `jump` and the weapon's `JumpSound`; landing from a jump, a fall, or the jetpack `Land` and `LandSound`, soft and hard alike; crouching `squat` and `SquatSound`; going prone `prone` and `ProneSound`; standing `stand` and `StandSound`; a dive roll `roll` and `RollSound`; landing after being thrown `bodyfall`.
- **Weapons** keep three voices each, all following the owner's origin, every soldier's alike (the player's and the bots'): one for the state (charge, overheat, reload, the fire loop), one for firing and the empty click, one for movement. Starting a sound on a voice still playing lets the old one finish, except as noted. The fire sound plays `FireSound` on the firing voice and `FireLoopSound` on the state voice.
  - `FireSound` plays once per fire cycle (a whole salvo), and per shot of a burst only when `SalvoDelay` is over 0. With `FireSoundStop` set, the next shot cuts the last one off. `FireLoopSound` restarts each cycle and stops when the firing does.
  - `FireEmptySound` plays when the trigger is pressed while the weapon is empty, overheated, reloading, or more than a second into a long cycle (`ShotDelay` over 1 s).
  - `ReloadSound` starts with the reload and is cut off when it completes; `WeaponChangeSound` plays on selecting the weapon.
  - `ChargeSound` starts at pitch clamp(P × `ChargeSoundPitch`, 0.2, 4) and then follows the charge, clamp(P × lerp(`ChargeSoundPitch`, 1, charge), 0.2, 0.4), P being the sound's own pitch. It stops on release.
  - `OverheatSound` loops while overheated at P × lerp(`OverheatSoundPitch`, 1, threshold / heat), rising as it cools.
  - The scope's zoom sounds play only with the scope overlay, 2D.
- **Ordnance and explosions.** `OrdnanceSound` loops from a round's creation, follows it, and stops when it expires or is destroyed. `CollisionSound` plays at every impact (foliage aside, which has `CollisionFoliageSound`); grenades play `CollisionOtherSound` at each bounce, and `CollisionWaterSound` entering water. An explosion plays its `SoundProperty` at its centre.
- **The soldier's voice.** `HurtSound` when a hit takes over 2% of the health it leaves and the soldier lives, `DeathSound` when it dies, for every soldier, on one shared voice that each restarts, with no cooldown but the sound's own: the stock wound lines play 85% of the time. `LowHealthSound` loops at or under 30% of full health.
- **Jetpack:** `TurnOnSound` when it fires up, then the parameterized `EngineSound` with `Speed` (the speed), `Acceleration` (the previous speed minus the speed, over dt), and `Health`; when it stops, `TurnOffSound` after `TurnOffTime` (0).
- **Items:** `PowerupSound` when a power-up is taken; a mine lands with its bounce sound; a time bomb plays `TickSound` once each time the whole seconds of its timer change (twice as it is placed, then every second), at P × ((1 − t) × `TickSoundPitch` + t), t being the share of its `LifeSpan` left, so with 0.5 it falls from P to half.
- **Other soldiers' charges** play their weapon's `ChargeSound` at them, pitched as the player's own; a launcher's makes none but for its owner.
- **Turrets:** while manned and turning, `TurretYawSound` and `TurretPitchSound` loop at gain clamp(G × f) and pitch clamp((p + (1 − p) f) P, p, P), f being the turn rate over the axis's top rate, G and P the sound's own, and p `TurretYawSoundPitch` (0.7 on Theed); stopped, the whir fades by 0.6 a frame. `TurretActivateSound` plays when someone gets in and `TurretDeactivateSound` when the seat empties (the turret compares its controller with the last frame's), whoever mans it; a placed turret's seat and a hull's turret seat are the same component (docs/live-objects.md, "Turrets").
- **Recharge droids** loop `AmbientSound` (asked for again each time it ends, at its `PlayProbability`) and play `PowerupSound` for each unit served each pulse.
- **Command posts** loop the owner side's `AmbientSound` (none while neutral). While one is being captured it loops `ChargeSound`, pitched from 0.6 to 1 with the capture; contested, `DisputeSound`; while an owned one is being neutralized, `DischargeSound`, falling from 1 to 0.6. Capture and loss play `CapturedSound` and `LostSound` once, with the voice-over above.
- **Droideka:** `TurnOnSound` and its `EngineSound` while it folds and rolls, `TurnOffSound` 3 s after it starts unfolding; `Footstep0Sound` to `Footstep2Sound` when each leg's `leg2` bone drops below 0.48 m; the shield's sound is its weapon's `FireSound`.
- **Recon droid:** `AmbientSound` (its engine) loops and `Ambient2Sound` (its radio) is asked for again each time it ends; the orbital strike's countdown is the destruct weapon's `ChargeSound`, pitched to 0.4 after its first frame.
