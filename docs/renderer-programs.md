# Renderer program sources

`rust/bfe-render/shaders/<family>/manifest.json` declares source-owned program
variants. The `post-hud` family owns twelve stable full-screen and battle HUD
programs. The `world` family owns static near/far geometry, terrain, foliage
and canopy programs, including their Enhanced variants. The FXAA source license is retained in `shaders/post-hud/NOTICE`.
The `interface` family owns menu quads, model/skin/vertex-colour previews,
spawn rooms, the projector, motes and holomap surfaces. Its skinning source
license is retained in `shaders/interface/NOTICE`.
Those passes register Rust source directly. Registration errors fail with a
message naming the variant. The `utility` family owns twenty-four no-map line
variants: plain/RGB/RGBA inputs, solid/blended alpha, cutout and premultiplication.
Their expanded sources retain the upstream license in `shaders/utility/NOTICE`.
Tow cables and flight/debug helpers prewarm those keys and reuse retained dynamic
buffers.

Each variant has a unique `<family>/<name>` key, self-contained vertex and
fragment bodies, explicit attribute locations, and named uniform slots with the
renderer types (float/vector 1..4, integer/bool 5, mat3 9, mat4 16, sampler2D 32,
sampler3D 33, sampler2DShadow 34, samplerCube 35). Arrays declare `{ "type": N, "size": N }`
instead of a numeric type, using the base uniform name without `[0]`.
Order in the uniform object defines slot order. Include every active input;
inactive declared inputs may remain. The linker validates active uniforms and
attribute locations against the declaration. Unknown keys, incompatible layouts
and link failures are errors.

`programs::sources` emits GLSL ES 3.00 with explicit high precision and the
compatibility vocabulary the bodies use in `body` mode; `expanded` mode keeps
the declared version and vocabulary. It injects no lighting, colour-space or
tone-map chunks. Source and arithmetic belong to Rust. Run
`cargo xtask shaders` (`rust/xtask`) to regenerate `program_catalog.rs`.
`cargo xtask check` checks generated outputs in CI. Do not edit them by hand.

`Renderer::add_owned_program(key)` compiles and validates the declared variant
and returns a context-local program ID. Repeated registration returns the same
ID. Post targets are explicitly initialized before adoption.

The battle post chain publishes generated facts to `post_chain.rs`: settings,
render/output dimensions, vehicle blur and resource IDs. Rust chooses the
ordered programs, targets and uniform values, owns camera-cut/jitter/history
state, and commits history only after the complete draw succeeds. Paced
loading registers all eleven post variants before battle drawing. The boundary
comes from `post-chain-layout.json` (`cargo xtask layouts`).

The loaded battle publishes semantic view packets through the generated
`frame-layout.json` boundary. `frame_plan.rs` owns cloud/probe cadence, mirror
holds, cascade refreshes and the complete target/clear/copy/resolve schedule.
`Renderer::battle_frame` borrows the existing resource bank and executes that
schedule directly. Mirror packets retain the previous shadow inputs; the main
shadow refresh follows their execution.

`scene.rs` retains qualified object and parent identities, sibling order,
authored visibility/layers, material groups, bounds and pass resources. Cold
admission and coalesced mutation rows use the generated `scene-layout.json`
boundary. Native queries own the near, far coverage, actor, first-person,
shadow, motion and line selections. Hidden parents suppress descendants;
layer mismatches do not. Painter keys preserve authored object/material IDs,
f64 no-divide clip depth and each producer's original ordering contract.
Static world/depth resources retain their own cell culling.

Selection storage is reserved with content and clips deterministic prefixes
with telemetry when a consumer fills. Material/geometry replacement updates
only admitted users. Motion source visibility and detached-pool retirement
produce retained lifecycle events. Line selections survive intervening queries.
Context recovery moves the same CPU registry into the rebuilt renderer.

The native camera owner imports the gameplay camera's f64 publication and owns
far/first-person views, CSM splits and held windows, shadow cameras, oblique
water clipping, cube probe views and sky following. The light owner computes
map/environment colours, point-light packs and storm flashes. Matrix and light
shader blocks read their native publications directly. Mirrors retain the
preceding shadow matrices until a selected shadow pass publishes its update.

The host snapshots only live record/bone/FX prefixes into retained native view
storage before another camera overwrites the working inputs. Dynamic interface
uploads belong to their UI packet. Lines join the existing native transparent
sorter, alongside the sorted particle and fountain copy commands. The far image
remains after opaque depth. Native post completion commits temporal history only
after the planned chain succeeds; clipped frames invalidate that history.

Interface Batch descriptors carry texture/mask values directly. Menu and spawn
scenes publish generated draw records to `interface_scene.rs`; Rust sorts their
opaque/transparent groups and applies depth, blending, culling and indexed/point
draws. `interface/quad` shares the canonical Rust HUD shader files with a
separate context-local program key, so the two passes cannot invalidate each
other's matrix cache. The record layout comes from `interface-layout.json`
(`cargo xtask layouts`).

Dynamic interface vertices upload into retained GPU buffers without marshalling
a new byte slice each frame. Context reconstruction replays catalog keys and
geometry bindings and uploads their bytes before drawing resumes.

# Parallel shader families

Add a separate `shaders/<family>/manifest.json` and source files. Suggested
namespaces are `world` (world/terrain/foliage), `actors` (actor lighting/shadows),
and `effects` (effects/water/sky). Use stable string keys, not numeric allocations.
The generator discovers family directories, validates uniqueness and emits a
sorted catalog. Regenerate after merging (`cargo xtask shaders`); do not merge
generated output by hand.
A manifest row declares `vertex`, `fragment`, `attributes` pairs and `uniforms`.
Use the `post-hud` manifest as the concrete schema example.

Registration dispatches explicitly by descriptor kind. Actor receivers use
Skinned or Rigid; casters and per-object flashes use Plain (a caster's matrices
and bones are its own uniforms, so it needs no rewrite).
Manifest rows accept `Plain` (the default), `World`, `Skinned`, `Rigid`, `Effect` and `Motion` kinds.
World uses the existing frame/shadow block rewrite and may declare `lamps: true`
for the lamp-table rewrite. `source: "expanded"` supplies complete GLSL ES 3.00
sources, including the version and precision declarations; the default `body`
uses the post/HUD compatibility prefix. Expanded arithmetic is preserved before
kind-specific input rewrites. Reflection validates standalone inputs against the
manifest; block members belong to the shared frame/shadow layout.
Shader registration does not alter
`core.rs`, `post.rs` or `hud.rs` phase records: context-local resource handles,
f64 phase inputs, five view blocks and BfeFrame/BfeShadow layouts remain their
own contracts.

Effect uses the shared frame block and identity model. Motion uses its motion
row; Skinned uses the bone/instance row for effects overlays. Kind-specific
injected inputs are supplied by the matching phase.

A program whose only difference from another is the shadow-map defines is a
shadowed twin: its row declares `shadowOf: {key, vertexLine, fragmentLine}`
instead of source files, and the generator and the Rust registry both build its
text from the base source with those two defines inserted at the recorded lines.
Each such pair therefore stores one copy of the text, and the generated layout
gives the base a `shadowTwin` key used to pick the shadowed program. A test
requires every selector to exist with shadows on and off. Version resource/schema
changes independently of source keys.
Caches belong to one GPU context and must be discarded with it. Context lifecycle
and renderer-wide error policy belong to the renderer lifecycle packet.
