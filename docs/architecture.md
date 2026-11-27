# Architecture overview

`bfe-client` is the standalone native game binary: it opens a native desktop
window, owns its OpenGL ES 3.0 (or desktop OpenGL 3.3) context and `cpal` audio device,
runs the shell menus, server browser, and battle UI overlays in Rust, steps
Instant Action battles on a dedicated 60 Hz local world thread, and connects to
`bfeserver` instances over `BFE-UDP/1`. `bfeserver` (`rust/bfe-server`) is the
standalone native dedicated server binary, running the same simulation crate
without a renderer or audio device.

For building, testing and schema generation, see [development.md](development.md).

## Engine ownership

| Area | Implementation | Current responsibility |
| --- | --- | --- |
| Authoritative world | `rust/sim/src/world/` | One ordered world tick over battle-local owners, including water, squads, cameras, AI, units, vehicles, recon, live objects, ordnance, items and battle rules. |
| Units and combat | `rust/sim/src/units/`, `rust/sim/src/combat/`, `rust/sim/src/recon/` | Bodies, pooled lives, collision shapes, weapons, damage, hit history and recon droids. |
| Vehicles and battle | `rust/sim/src/vehicles/`, `rust/sim/src/battle/` | Hulls, flight, seats, riders, turret crews, spawning, objectives, roster and statistics. |
| AI | `rust/sim/src/ai/` | Classic and Enhanced decisions, navigation, perception, aim and class jobs over canonical world state. |
| Prediction and publication | `rust/sim/src/client/`, `rust/sim/src/prediction/`, `rust/sim/src/wire/` | Private own-player replay and reconciliation, authoritative snapshot capture, budgets, deltas and encoded publication. |
| Collision and movement | `rust/sim/src/collision/`, `rust/sim/src/movement/` | Map and dynamic-body queries, soldier movement and retained query scratch. |
| Presentation | `rust/bfe-presentation`, `rust/bfe-pose` | Displayed bodies, hulls, riders, props, cameras, items, projectiles, effects, pose sampling and received snapshot timelines. |
| Rendering | `rust/bfe-render` | Battle frame schedule, shader catalogs, geometry/material descriptors, textures, targets, cameras, lights, draws, post-processing, HUD and interface geometry. |
| Environment | `rust/bfe-environment` | Destruction, weather, atmosphere, water and shore state, foliage clocks and placed effects. |
| Audio | `rust/audio`, `rust/audio-control`, `rust/audio-decode` | Sample rendering and master processing, battle sound decisions and asset decoding. `cpal` delivers the rendered samples to the audio device. |
| Content and codecs | `rust/content`, `rust/bfe-content-native`, `rust/basis-transcode` | Prepared content loading (shared typed map admission plus battle roster admission for 2004 and 2005 prepared formats), image decoding and Basis texture transcoding. The Basis boundary includes vendored C/C++ dependencies listed in the third-party notices. |
| Native client and UI | `rust/bfe-client`, `rust/bfe-shell`, `rust/bfe-interface`, `rust/bfe-battle-ui`, `rust/bfe-online`, `rust/bfe-desktop` | Standalone native window/GL/audio host (`Startup` -> `Gate` -> `Shell` -> `InstantAction` / `Online`), shell menus and options, battle overlays (spawn, scoreboard, pause, chat, loading), server browser, 60 Hz local world thread, and desktop platform services. |
| Dedicated server | `rust/bfe-server`, `rust/bfe-transport` | Headless 60 Hz battle authority (`bfeserver`), encrypted `BFE-UDP/1` sessions, master registration, signaling and administration. |
| Shared arithmetic | `rust/jsrt`, `rust/three`, `rust/v8math` | Collection and number semantics, geometry math and cross-architecture deterministic simulation arithmetic. |

## Hosts and boundaries

`bfe-client` (`rust/bfe-client/src/app/`) is the native desktop host.
It runs the shell (`bfe-shell`), battle UI overlays (`bfe-battle-ui`), online
server browser and identity (`bfe-online`), and platform services
(`bfe-desktop`) in a single process. In `InstantAction`, `bfe-client` spawns a
dedicated 60 Hz local world thread (`rust/bfe-client/src/local/`) that communicates
with the render thread exclusively through a pre-allocated non-blocking command
ring and publication slot pool (`try_lock` only on the render thread), feeding
the same `Receiver` / `Sender` / `Replay` / `Scene` presentation pipeline used
by online battles. In `Online`, `bfe-client` exchanges `BFE-UDP/1` packets with
`bfeserver` directly. See [native-client.md](native-client.md).

`rust/sim/src/world/owner.rs` groups the canonical store and phase owners for
one battle. Internal phase services borrow that `World`, and each battle's
native `Context` owns a boxed `World`. Generated block readers operate on
`BlockView` spans.

Map admission for 2005 prepared content is split cleanly from battle roster
admission. `bfe_content::map::MapAdmission` and
`bfe_content_native::AdmittedMap` (`Loader::admit_map`) are 2005-only and
define the shared typed map composition boundary (base and active mission layer
mesh streams, static collision and retained per-triangle masks, void terrain,
placed objects, prop skeletons, lights, regions, paths, command-post spawns,
world bounds and `worldAnimations`) without requiring side or unit files. 2004
battles continue through their released `Battle::load`, `Loader::load`,
`Defs::build`, `fingerprint::hash`, `world_admit`, `world_network_admit`, and
`content_tables_files` paths unchanged.

### 2005 Rust loader admission

The content (`rust/content`), native admission (`rust/bfe-content-native`), and
simulation (`rust/sim`) crates expose family-explicit entry points alongside
the untouched 2004 entry points:

- **Shared typed map admission (2005-only).** `MapAdmission::load` /
  `MapAdmission::load_for_query` and `Loader::admit_map` reject
  `GameFamily::Swbf1` cleanly so 2004 has no second reader. Mission selection
  matches explicit `mission=` names or `mode=` and/or `era=` (matching both
  when both are given, or whichever one is supplied) and returns an error on
  miss. Layer lookup resolves explicit names, case-folded names,
  `layerIndex[hash]`, and unnamed hash-keyed layers.
- **Void terrain, world bounds, and collision masks.** Maps without terrain
  files admit with `has_terrain = false`, `grid = None`, `terrain_bvh = None`,
  require authored `bounds` in `map.json`, and derive `kill_y` below those
  bounds and static geometry (the 100 m void margin is provisional and not yet
  settled). The simulation still applies its 2004 world-box bounds check
  (`|x|, |z| <= 1000`, `|y| <= 500`) until per-map bounds are wired. Base,
  layer, and prop `*_mask.bin` files are retained on `MapCollision` and
  `BodyTriangles` (`collision::mask`) and are not yet applied by the collision
  solver.
- **Placed props, model-space rigs, and moving vs static collision bodies.**
  2005 `props.bin` part vertices are already in model bind space; `placed.rs`
  (`push_rig_parts_swbf2`) reads per-vertex `vertexStreams.joints` (and
  3-influence `weights`) from `props_attributes.bin` ahead of rigid
  `bone`/`bindBone` fallback, preserving model-space positions and normals
  through `bfe_pose::rig::construction::batches` while retaining bone
  attachment grouping and live skinning indices. With no stream, a named
  bone binds rigidly. A prepared numeric-only `bindBone` retains the static
  root fallback (2004's `push_rig_parts`
  remains untouched). Static 2005 props have their model-space
  `props_collision.bin` triangles baked into world space
  (`range_shape_transformed_swbf2`) and installed as fixed kernel bodies
  (`moving = 0`). World-animated 2005 props (objects bound in
  `worldAnimations.groups` or `worldAnimations.hierarchies`, such as orbiting
  frigates and their hierarchy turrets) keep unbaked model-space collision
  triangles (`range_shape_local_swbf2`, `PropBodies.world_animated = true`)
  and are installed once as moving kernel bodies (`moving = 1`) placed at
  their initial instance matrix. The space simulation updates the moving
  body transform and its model-local bounds at 60 Hz. `world_anim::WorldAnimations` evaluates curves with
  `v8math` trig; Euler XYZ order, Hermite tangent layout, `disableHierarchies`,
  and the 256-substep cap remain provisional.
- **2005 materials.** `material_swbf2` and `append_pack_swbf2` read only the
  authored contract keys (`material.{blendMode, alphaCutout, doubleSided,
  textureMode, sourceTexture}`) with optional prop bump maps and legacy flag
  fallback only when explicit fields are absent.
- **Ordnance explosions, items, and stand-in rosters.** `Battle::load_for_family`
  coerces numeric strings in unit, weapon, ordnance, item, and nested explosion
  records, maps `Explosion`/`ExplosionName` -> `ExplosionImpact` on ordnances,
  promotes `dispenser`/`detonator` `mine`/`powerupitem` ordnances to
  `weapon.item` (aliasing `Explosion`/`ExplosionName` -> `ExplosionTrigger` and
  coercing `ExplosionTrigger`/`ExplosionExpire`/`ExplosionDeath`), applies
  `DamageRadius` -> `DamageRadiusOuter` and `HealthScale`/`ArmorScale` aliases
  in authored property order, and applies last-key-wins `Damage`/`MaxDamage` on
  non-`emitterordnance` projectiles while keeping `emitterordnance` `Damage`
  and `MaxDamage` separate. Distance/time damage falloff (`DamageFinalDamage`,
  `DamageTransitionDelay`, `DamageTransitionPeriod`) is not modeled yet. Until
  mission Lua scripts drive team setup, `Battle::load_for_family` builds a
  clearly marked stand-in team roster (`synthesized: true`, `standIn: true`)
  with equal unit counts on both teams.
  On the 2005 roster, players and bots start with the infantry weapons on
  trigger channels 0 and 1 that require no medal unlock. A missing channel
  defaults to primary. The prepared class retains its award slots for content
  agreement and future grants. Vehicle mounts retain their authored channels.
- **Per-bank bone-CRC animation binding.** `clips::load_clips_by_crc` binds
  tracks by Zero CRC-32 and resolves only the 40 soldier table slot suffixes
  queried by `units::soldier_swbf2` and `Defs::build` against each bank's stem
  (`{stem}_sabre`, `{stem}_rifle`, `{stem}`, plain and `_full`) while retaining
  per-character `clipTiming` action aliases. `units::soldier_swbf2` prefers a
  unit's own banks (e.g. `grevious_sabre_*`) and `human_sabre_*` for saber
  heroes ahead of `human_rifle_*`.
- **Family-aware fingerprinting and simulation entry points.**
  `fingerprint::manifest_for_family` and `fingerprint::hash_for_family`
  call the released `fingerprint::manifest` and `hash` for `Swbf1`.
  The 2005 manifest identifies the family and mission. It covers `map.json`,
  the base, selected layer and prop collision vertex, index, polygon and mask
  streams, and the **two terrain collision streams**, `terrain_collision.bin`
  and `terrain_collision_index.bin`. Other map inputs are `props.json`,
  `props.bin`, `props_anims.bin`, `classes.json`, `anim_banks.{json,bin}`,
  the planning file, the vehicle pack and the prepared mission scripts.
  Shared inputs are `common/common.json` and `common/anim_banks.{json,bin}`.
  Each selected side contributes `units.json`, `weapons.json`, `weapons.bin`,
  `combos.json`, `classes.json`, `ai.json`, `anim_banks.{json,bin}` and its
  referenced actor JSON and vertex files.

  JSON hashes preserve array order and compare named fields by value. The
  selected mission also records authored side order, resolved class slots,
  layer and spawn choices, item registration order, animation bank precedence
  and clip aliases. These affect team quotas, fallback spawns or definition
  and clip selection. Whitespace, line endings, equivalent number spellings
  and the order of independent named fields can differ across peers. A local
  practice roster appends explicitly requested soldiers after their side’s
  authored classes. Its agreement also identifies the practice pair and uses
  the same starting weapon channels and actor geometry as admission.

  Actor vertex files contribute the rounded navigation radius and, for a
  droideka, its gun bones and tips. A weapon pack contributes the largest
  part's sphere radius and pickup centre for each item model it can supply.
  A player's side gets first choice of shared models, so the session covers
  the possible sources across playable sides. UVs and normals may differ.
  `bfe_sim` provides `Context::world_admit_for_family`,
  `Context::world_network_admit_for_family`, and
  `host::content_tables_files_for_family` / `content_tables_for_family`.
- **Strict 2005 validation.** The 2005 loaders validate JSON container types,
  skeleton parent indices, acyclicity, and finite `f32`-safe local matrices
  (on both props and actors), spawn node arrays, command post matrices, region
  sizes/matrices, prop part vertex/index/bindBone/`vertexStreams` extents,
  collision ranges, terrain file pairs, animation bank clip arrays/CRCs/finite
  floats, world animation hierarchy acyclicity, relative paths, and explicit
  soldier `AnimationName` banks. The shared JSON parser enforces a 256-level
  nesting bound (`MAX_NESTING_DEPTH`) so deeply nested inputs return an error
  instead of overflowing the stack.

Boundary layouts come from declarative JSON schemas and `cargo xtask`
generators (`rust/sim/schema/*.json`, `rust/bfe-presentation/layout.json`,
`rust/bfe-environment/layout.json`, `rust/bfe-transport/layout.json`,
`rust/audio/layout.json`, `rust/audio-control/layout.json`,
`rust/audio-decode/layout.json`, and `rust/bfe-render/shaders/`).
Entity references carry generation-qualified identities. Changed rows and
ordered events cross at completed boundaries; consumers keep retained storage.

Battle admission selects a rule set by game family and mode. The authority
runs it after the post updates and spawn clock, before character respawns.
The admitted pair is currently `swbf1` and `conquest`. Conquest's post bleed,
reinforcements and end sequence use the canonical battle rows.

Event-driven rules can receive one ordered batch at that same authority
slot. Delivery follows synchronous simulation order. A lethal blast from a
dying soldier can report its victims before that soldier's own death hook.
A destroyed hull reports dead riders before the hull. For a manned turret,
the turret's `ObjectKilled` precedes its gunner's `UnitKilled` when destruction
kills the gunner. The post pass reports
placed capture, neutralisation and destruction, then mobile loss or change
of owner. Due timers join the batch at delivery. Events raised by handlers
wait for the next tick.

`ObjectKilled` names the destroyed life and an optional killer character.
A unit, player or launch snapshot supplies the character when one can be
resolved. The physical cause determines which attacker the callback receives.

| Object | Attribution |
| --- | --- |
| Building, destructible objective or turret | The attacker of the lethal hit. A rules command that sets health to zero has no killer. A gunner killed by turret destruction receives the turret's attacker. |
| Log trap | A lethal hit credits its attacker when the trap springs. A swing triggered by vehicle proximity reports destruction at expiry with no killer character. A scripted kill also has no killer. Each life reports destruction once. |
| Vehicle hull | Combat uses the hull's native attacker record. A hit with no owner can retain the previous attacker. A battlefield boundary wreck or a crushing log trap clears the attacker before crew deaths and hull destruction in both modes. In an event-driven battle, a script setting health to zero or a post's kill region also clears the attacker. |
| Placed or mobile command post | Ownership and loss have post events. Destruction of a linked building or hull has its own `ObjectKilled`. |
| Animated prop or door | The animation owner has no health or object-death callback. |

On a Space battlefield the door's half-open transition switches both collision
flags, its mission-bound panel barrier and each named soldier planning
connection crossing the shutter. A connection crossing several doors stays
blocked until they all clear. Navigation registers separate writers for panel
names and connection names. Locks and explicit mission barrier holds take
effect before the next planning decision.

Admission reserves both buffers from the registry, hull/victim, building and
post capacities, plus the mode's command and timer limits. Neither buffer
grows during play. Overflow signals an admission defect and fails immediately
in development builds and tests. Release builds keep the ordered prefix,
drop the tail and set the batch overflow flag. A due timer that did not fit
stays due until a later delivery.
Admission indexes mission instance names and classes once. Lookup folds ASCII
case and resolves the current life. Commands validate the supplied life.
The `set_building_health` convenience method uses the same prop handle and
property service as `set_health`.
Handlers can query characters, object state, time, points and reinforcements,
change supported object properties, start or stop timers and declare an
outcome. Finite ticket and point changes queue the team and resulting count.
Unlimited reinforcements suppress ticket events. Unavailable properties
return a typed error. A successful object-property write spends one command.
A rejected write leaves the budget available. Health accepts finite values
from zero and maximum health requires a finite, positive value. Setting `Team`
through a placed post or its linked building updates both affiliations.
An ownership change clears capture and neutralisation progress, preserves
the occupancy census, and awards no capture. A mobile post follows its hull.
Team points and timer handles stay on the authority.
The existing match summary carries the end sequence.
A synthetic team deathmatch test reaches a score-limit win without posts
through this same slot.

The cold host can register an event handler factory and request a family,
mode and limits before admission. The selection call records the parameters
and named object bindings. Replay uses the same factory and rejects an
unavailable mode. The existing tail initialization selects Conquest without
adding a call or interning a mode string. Standalone phase callers supply
their selected rules. An event rules slot without a prepared batch or a
producer leaves the handler, timers and rules clock idle.

## Native runtime responsibilities

In `bfe-client`, window management, menus, spawn and scoreboard interaction,
keyboard and pointer events, content verification and extraction, `cpal` audio
routing, update checks, Steam/Discord rich presence, local 60 Hz Instant Action
authority, and online `BFE-UDP/1` sessions all run natively in Rust.

Rust owns the battle's frame schedule and GPU resource lifetime (`bfe-render`
and `bfe-presentation`).

For timing and data flow, see [runtime-architecture.md](runtime-architecture.md).
For server acceptance evidence and its limits, see
[host-qualification.md](host-qualification.md).

## Battlefront II character presentation

During a developer Instant Action battle, the local world advances the
Battlefront II infantry mover and saber state machine at 60 Hz. Class jet settings
supply airborne thrust and select the fuel tank or the soldier’s energy reserve. The battle
uses the prepared sound banks for the selected game. The battle screen names bots
from the player's localized class catalog and humans from their player names.
Direct deployment binds the equipped dispensers to the player’s current life before firing.
A kill retains each participant's class at the time of death. The loading screen
and active battle hide the desktop pointer. Interactive menus restore it. Character captures can
also exercise the renderer with scripted input or with recorded states at
60 or 20 Hz using the normal display clock.

The receiver supplies class and weapon identity, position, velocity, aim,
posture, airborne state, seat identity, health, death and weapon events.
Damage and death feedback supply hit direction when the source point or
attacker is known. Each local infantry sample adds sprint, roll direction and
elapsed time, combo state and animation clocks, and the mask of inactive
blades. The presenter applies it to the matching entity and generation.
Every melee soldier's pose also travels with the battle's world fields (its combat
display, schema 0x0205), the same way in Instant Action and online: its combo state, the tick its clip
clock read zero, the state's loop and clip length, its blade mask and its guard
reaction. The same display carries the pose of a power the soldier casts or holds
(its weapon, fire slot, clock and whether it is held) and the choke that holds it, so
a held choke and a lightning chained from it draw on the caster and the victim. The
presenter applies it to every soldier but the one the local sample already drives,
so a bot's or a remote player's swings, guards and casts play the clips a player's do.
A hit taken during a combo state does not replace it: the hit pose is blended over
the combo pose at half weight, fading out in 0.125 s, above the hips only while the
state moves the whole body.
The local camera interpolates authoritative body samples. Its chase view uses
the released five-ray obstruction test. When an obstruction pulls the near
plane into the local soldier's published bounds, the Battlefront II camera
hides that silhouette until the view clears. Remote players
still need the corresponding action publications. Directional deflection
reactions need a resolved combat contact.
Character rigs admit up to 64 mesh and animation joints combined. The loader
checks this before reserving pose histories and visits each parent link once.
The animation package belongs to the class. Each weapon
resolves its combo states through its own bank-qualified clip timings.
Ground travel selects an available sideways cycle or steers a forward or
backward cycle. A stance that animates only the upper body keeps those channels over the first
parent gait with moving leg channels. Cadence uses ground speed and the lower clip's authored stride.
When the gait bank and mesh share the body's nonzero bind offset, the stride is already in mesh units.
For a different or missing reference, the class's skeleton root scale converts the stride.
An in-place cycle can also inherit a parent’s authored stride. The presenter eases lower-body steering toward travel and back to rest,
capped at eight radians per second. The hips turn up to 72 degrees while
the torso stays on aim. Turning the thighs through the remaining angle is a
presentation choice. A lateral dive turns the whole body toward its bearing from the current gait heading.
The local movement profile publishes the active roll's elapsed time and duration. Both admission paths use
the same class dive slot. The presenter fits that complete clip to the published window, including its recovery.
Standalone pose playback without a movement profile resolves the dive through the weapon stance.
A missing slow backward cycle can use the faster
backward cycle at walking speed.
Moving weapon actions replace the upper body while the gait continues through
the action and its blend. Stationary actions can use the full authored pose.
Directional deaths select the fall away from the published impact source.
A hit uses an animated reaction. Missing stance clips follow the equipped
weapon's declared parent and the standard stance parents within the class's
bound packages. Standing heroes prefer their defeat posture. An ability with
no stance inherits the character's equipped saber stance. If all bound sets lack
a death, the presenter counts one fallback for that life and tips the last pose
away from impact over 0.6 seconds, holding the body on the ground.
Explicit transition times, including zero, take precedence over the outgoing
and incoming blend durations.

The presenter retains sixteen sampled local poses per actor. Each complete snapshot
advances the retained soldier records, including idle and death clips when a
delta contains no change to that soldier. It interpolates
the two surrounding the display clock before building the skin palette.
A marked change of position, a seat change, a respawn or a new lease starts
a fresh pose and blade history. Turning in place after a cut keeps the history. A seat with a pilot clip and a published mount supplies the seated pose and its
world frame. Crew appear when the class binds the clip and the mount is present.
In first person, the camera hides the player's body. Color and shadow draws reuse
the displayed skeleton. Body ranges share vertex buffers and compatible opaque
materials share one skinned record for up to four diffuse ranges. Held models
follow posed attachment joints.

A blade takes its direction and length from its emitter. The blade image runs
from the hilt to the tip, with its glow across the width. The weapon supplies
the trail colour. Each blade uses one ribbon with fade sampled continuously
by endpoint age. A character can show four ribbons and a view can show 64.
Bodies, equipment and blades take the available blended records first. When
space runs out, subsequent ribbons are omitted for that view. Ribbon shape, the fade over the history of each swing, and
the ignite and retract times are presentation choices.

Cloth pieces (capes, robes, skirts, hair) move on the CPU in the drawing path
only; the battle simulation never sees them. Each piece moves in its own frame,
its parent joint's pose times the authored placement, as the 2005 game does:
position Verlet with gravity, the body's acceleration scaled by the class's
Dampening, drag against the air (the body's speed, clamped to 10 m/s, plus the
vertex's own) and wind along the vertex normals (WindSpeed times a uniform random
value per step along WindDirection), divided by ParticleMass. Each fixed vertex then
sits rigidly on its one joint, one pass restores the cross, bend and stretch pairs
to their rest lengths (only the free end moves when the other end is fixed), and
the free vertices are pushed out of the class's spheres, cylinders and boxes on
the body's joints. The game updates its cloth once per drawn frame; here steps are
a fixed 1/144 s (the game drawn at 144 frames a second) with at most ten per drawn
frame; a drawn frame blends the last two steps and places the fixed vertices on the
drawn pose, so the cloth looks the same at any frame rate. A respawn, a teleport or a
new body restarts a piece from its rest shape. So does a pose that jumps at one of the
body's clip changes (a clip entered at its first frame, a new tumble phase) when a fixed
vertex moves faster than 15 m/s there; the game has no such rule and throws its cloth
with the jump, and inside a clip the authored motion always goes into the cloth.
`BFE_DEV_SWBF2_CLOTH_NO_RESTART=1` turns this restart off. Up to 64 characters in view move
their cloth at once; the next one draws its pieces in the rest shape, skinned to
the nearest fixed joints, until a place frees up. Lower-detail body meshes in the
original archives are currently absent from the prepared character data.

In third-person views a soldier near the camera follows the 2005 game's near rule
with everything drawn with it: its torso point (1.4 m above the feet, 0.6 m when
crouched) nearer along the view than the near distance hides it, and within 1.2 m
past that distance its body, cloth and held weapon are drawn in the blended pass
with the opacity that distance gives, never under 0.06, so a camera pressed against
a wall behind a caped hero sees through the cape.
