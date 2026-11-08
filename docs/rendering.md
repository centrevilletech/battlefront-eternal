# Rendering audit and controls

The original gamma-space lighting, texture palette, baked lighting, near/far
scene dissolve, map blur, and HUD ordering are preserved. Bloom, FXAA, and soft
particle intersections are optional additions.

Rust owns battle and interface drawing on the shared WebGL2 context. Its
catalogs provide world, actor, effects, post/HUD and interface shader sources;
its native resource families own geometry, material, sampler and target
recipes. Three.js retains scene anchors, host readbacks and some cold utility
program preparation, but submits no battle, menu, spawn or scoreboard draws.
The runtime draw guard rejects accidental Three.js scene rendering. Module or shader-link failure
stops the battle with the GPU name and error. Context loss suspends drawing;
restoration rebuilds Rust's GPU resources and resumes the same battle.
Tow cables and flight/navigation helpers also draw through Rust, preserving
line strips, segments, loops, vertex colours and dynamic buffer updates.

## Texture and target ownership

Rust decodes PNG rows and lossless WebP HUD atlases in a decoder-only worker
module; Basis tables load separately for KTX2 content. Completed images awaiting
paced upload have a bounded queue. A declared asset failure stops loading with
the GPU name and cause, rather than choosing another image format.
It keeps the original gamma-space values, authored compressed mip chain and
premultiplied-alpha flag. Format selection preserves the old capability order,
including BC1/BC3 instead of BC7 on drivers that unpack BC7 into RGBA storage.
CPU texture reads use the same PNG decoder with the original Canvas rounding.

The Rust resource bank creates and uploads textures, applies filtering, wrap,
flip, anisotropy and depth comparison, and owns framebuffer/renderbuffer storage.
It binds far/near, shadow, post, temporal, particle-depth, mirror and probe targets,
resolves MSAA and performs color/depth copies. Battle actor, effect, world
and interface resources use native descriptors. Shared image loading and
remaining screen/UI consumers retain CPU image shells; those shells do not
allocate GPU storage or issue draws. Loading-screen canvas composition and
the frontend video's separate sharpening context remain host presentation.

Sampler clones share source storage while their settings agree. A changed sampler
can split without moving its logical draw id. PNG sources retain encoded bytes
rather than a second full RGBA copy; compressed sources retain one block chain.
The bank detaches consumed PNG transport buffers after retaining encoded bytes
and after the initial upload. Unused interface preloads keep their initial rows
until that upload; CPU discovery keeps its separate pixel arrays.
Dynamic arrays use subimage updates. Restore reacquires context extensions, rebuilds
source images and attachments, then replays draw registrations using stable ids.
No image decoding, upload or target allocation falls back to three.js.

## Findings and fixes

| Area | Finding | Change |
| --- | --- | --- |
| Detached shadows | A normalized bias of -0.0004 across a 2999-unit depth range displaced the depth comparison by about 1.2 world units. | Bias now represents 0.015 world units; normal bias is zero. Front-facing caster surfaces restore contact. |
| Animated self-shadow smudges | A constant normal offset did not correct depth across filter taps on sloping character faces. | Each PCF tap compares against the receiver triangle's depth plane. A deterministic 4×4 tent footprint avoids temporal noise; derivatives are evaluated outside cascade-selection branches. |
| Animated normal maps | Normal-map tangent/binormal directions stayed in the bind pose. Normal transformation also mishandled nonuniform object scale. | Skin the tangent frame with the bones, and use the inverse-transpose normal transform. |
| Moving shadow edges | FOV easing repeatedly changed cascade extents and texel size. Nearby characters shared the initial cascade with substantial distant ground. Cascade selection had a hard boundary. | A conservative shadow FOV stays fixed through normal camera easing; texel snapping is retained. Splits favor nearby detail, and adjacent cascades overlap/blend over 10% of the preceding interval. |
| First-person shadows | Three r170 tests casters against the view camera's layers, not the shadow camera's layers. | The shadow pass receives a camera copy with the hidden-body layer enabled. The normal view remains unchanged. |
| Fence/foliage shadows | Custom material uniforms did not expose the alpha mask to Three's depth material. | Material `map` and `alphaTest` properties now match the visual cutout. |
| Shadow quality changes | `CSM.dispose()` releases shader hooks but not the lights' render targets. | Dispose each light's shadow maps before replacing cascades. |
| Antialiasing | MSAA worked on geometry but binary alpha discard left jagged texture silhouettes. Requested sample counts could collapse to identical hardware settings. | Derivative-smoothed alpha-to-coverage for opaque cutouts, actual format-supported sample counts, explicit fallback labels, and FXAA after every enabled AA mode to catch shader/specular edges. FXAA-only remains the inexpensive option. |
| Texture filtering | Anisotropy was already wired to loaded textures. The difference is strongest at grazing angles and can be obscured by the original blur. | Verified real pixel differences with a mipmapped oblique texture; clamp the effective setting and label the base mode as trilinear. |
| Bloom | Soft focus only mixed a reduced-size frame back into the image. It did not bloom highlights. | Separate soft-threshold extraction and separable blur at quarter width/height, combined without changing exposure or color space. |
| Particle/wall cuts | Depth testing sliced flat sprites sharply at opaque surfaces. Double-sided batches also drew twice. | Snapshot opaque depth once before particles and fade billboards/streaks as they approach it. Keep depth testing and flat decals intact. Each batch now draws once. |
| Frame overhead | Unused depth resolves, two-triangle fullscreen passes, and unconditional far-scene texture reads. | Disable unused resolves, use fullscreen triangles, and sample the far image only within the dissolve. Reject hidden far fragments before texture reads. |
| Shadow submission | Merged material buckets spanned the whole map, so nearly every cascade submitted entire buckets. | Cull triangle groups in 48-unit spatial cells, compacting visible indices into one persistent buffer with a range per cascade. Share vertex buffers and retain one draw per material/cascade. |
| Small point lights | Per-vertex lighting could miss a light pool entirely when no triangle vertex fell inside it. The separate first-person scene also lacked point lights. | Evaluate dynamic-object point lights per fragment and mirror scene-level point lights into the view scene. |
| Outdoor lamps | Extracts contain emissive fixture geometry but no corresponding placed light records. | Optional reconstructed lights illuminate nearby dynamic objects; up to four candidates per object, with cached collision-BVH visibility rays. |

The initial cascade at the tested 640×400 viewport used approximately 1.52 cm
texels on Naboo and 1.40 cm on Bespin at High. Resolution depends on aspect ratio,
shadow preset, and view distance. This remains a shadow-map renderer: it cannot
guarantee perfectly continuous subpixel motion at every distance or setting.

## Cost and controls

- Bloom adds three quarter-width/quarter-height draws; disabling it skips all
  three. Its final combination shares the existing output pass.
- Soft particles add one full-resolution depth blit when a particle batch is
  drawn, with no extra geometry pass. They need a depth snapshot target. The
  blit is skipped when disabled or no particle batch is drawn.
- Particles use one draw per batch instead of two. This reduces submission and
  vertex work; actual frame-time savings depend on the workload and GPU.
- All enabled AA modes finish with one FXAA fullscreen pass; the 2x/4x/8x
  modes also multisample geometry. FXAA-only uses no MSAA buffers. Supported
  sample counts vary by GPU; diagnostics reports both samples and `postAA`.
- Ultra shadows use four 4096² maps. High uses four 2048² maps. Stable cascades
  and closer splits improve quality without increasing those allocations.
- Receiver-plane filtering uses 16 depth comparisons per active cascade, and
  two cascades only in their overlap. This trades fragment work for cleaner
  self-shadows. Spatial index culling reduces vertex submission separately;
  neither change is a guarantee of higher FPS on every GPU.
- Spatial culling allocates four index ranges per large caster bucket plus
  compact CPU cell indices. Vertex attributes remain shared. This exchanges
  index memory for fewer submitted triangles and avoids adding per-cell draws.
- Lamp lighting uses four local-light slots per dynamic draw, not hundreds of
  global point lights. Candidate visibility refreshes at most every 0.1 seconds
  unless an object moves over 0.5 units. Toggle **Lamp light on characters** to compare it.
- The map's original soft focus remains a separate checkbox. Disable it when
  comparing filtering or edge sharpness at a fixed camera position.

Inspect the effective configuration and complete world-frame draw counts:

```js
__bf.graphics.diagnostics()
```

The returned counts include shadows, far/near scenes, the view weapon, and post
effects, before the separately rendered HUD. Water's separate reflection and
underwater-overlay passes are outside these counts and timings. `shadows`
reports shadow-only calls/triangles. `timings` reports rolling CPU submission
and asynchronous GPU-query median/p95/max milliseconds. GPU results are `null`
until available or when unsupported; disjoint results are discarded. No query
blocks for completion. These are render timings, not whole-game FPS.
The frame number is Three's render
invocation counter, not a game-frame or FPS measurement.

Lamp reconstruction is an enhancement, not recovered original light authoring:
source color comes from explicit emissive lamp surfaces, with a modest fixed
radius/intensity. Occlusion is sampled at each object's center, so this does
not provide per-pixel point-light shadows and can switch at a doorway. Dynamic
characters and props receive these lights; baked architecture retains its
original lighting. Future extracted/authored sources can use
`graphics.localLights.add([x,y,z], [r,g,b], radius, intensity)` (RGB 0..1).
The renderer remains rasterized; BVH visibility rays are not hardware ray tracing.

## Future time of day and weather

```js
__bf.graphics.setEnvironment({
  sunDirection: [-0.4, -1, 0.2], // ray direction, normalized by the renderer
  sunColor: [200, 170, 130],     // map-style RGB, 0..255
  ambient: [80, 85, 100],
  top: [100, 110, 130],
  bottom: [70, 65, 60],
  character: [80, 85, 100],
  fogColor: [100, 110, 125],
  fogRange: [100, 500],         // original far-scene fog distances; none
                                // unless 0 <= start < end, as the game's
  shadowStrength: 0.6,         // 0..1, e.g. softer contrast under cloud cover
});
```

Unspecified fields retain their values. These updates reuse the shadow targets
and shared uniforms. They do not rebuild materials, reset the map, or couple
environment controls to gameplay.

This is the rendering interface for future systems, not a complete day/night
or weather implementation. Baked vertex lighting and painted sky textures still
contain the original map's lighting. Fully relighting arbitrary maps will need
an explicit treatment of those assets, sky updates, and near-scene weather fog.
Soft particles currently intersect opaque depth-writing geometry; transparent
water/glass needs its own depth policy. LDR bloom cannot recover unclipped HDR
energy or distinguish an emissive texel from an equally bright sunlit texel.

## Verification

`tests/rendering.mjs` drives a separate headless Chromium session against Vite.
It uses generated geometry/textures and does not require retail game assets.
With Vite running, use an installed Playwright module:

```sh
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tests/rendering.mjs
```

If Playwright is locally resolvable, omit the variable. `RENDER_TEST_URL`
overrides `http://127.0.0.1:18449`; `RENDER_TEST_ARTIFACTS` optionally saves PNGs.

56 behavior checks passed under Chromium/SwiftShader, with WebGL error checks
after every captured frame. Coverage includes pixel differences for shadows,
cutouts, MSAA, FXAA, anisotropy, bloom, and particle intersections; animated
hidden-body shadows; identical-pose stability; FOV grid stability; runtime
environment changes without reallocations; shadow resource recycling; particle
draw counts; resolution changes; receiver-plane self-shadowing; pixel-identical
spatial shadow culling; lamp occlusion; point lights inside triangle interiors;
first-person lighting; and animated normal-map tangent frames. Flat decals remained pixel-identical with
soft particles on/off. Soft-particle tests passed with MSAA off and at 4x.

Separate private-asset smoke checks loaded Naboo and Bespin in third and first
person without console or WebGL errors. These software-GPU checks do not measure
hardware FPS or replace an in-motion review on the player's GPU.

Frozen 640×400 map views at High shadows, with all other state held constant:

| Map | Shadow triangles, original buckets | Spatial culling | Shadow draws | Changed pixels |
| --- | ---: | ---: | ---: | ---: |
| Naboo | 380,760 | 198,748 (48% fewer) | 441 in both | 0 |
| Bespin | 384,880 | 180,751 (53% fewer) | 339 in both | 0 |

These are same-scene submission comparisons, not end-to-end FPS gains. More
complex scenes, camera positions, GPUs, and shadow presets change the benefit.

## Enhanced water and command-post glow

Water quality is independent of the other graphics settings:

- **Low (original)** retains the 512×256 far-scene reflection and flipbook water.
- **High** (default) reflects the full near scene, including architecture and
  characters, at half the drawing-buffer width/height, capped at 1024 on the
  longer side. Cross-flowing world-space normals and directional waves reduce
  repetition; Fresnel reflection and derivative-filtered sun highlights improve
  the surface at grazing angles without changing swimming or collision heights.
  Opaque color/depth supply shallow refraction, depth-dependent absorption,
  small caustic highlights, and a soft shoreline edge.
- **Ultra** uses the same shading at full drawing-buffer reflection resolution,
  capped at 2048 on the longer side.

Detailed reflections draw the near scene again. Existing shadow maps are reused;
water itself and wake rings are hidden during that pass to avoid feedback.
The reflected surface is disabled below and within 3 cm of the water plane; the
underwater view transmits the scene above it instead of projecting a stale mirror.
Surface triangles preserve projected coverage across the near plane and write
their original depth per fragment, clamped only at the near boundary, so close
water does not expose a rectangular dry riverbed. Refraction uses original view
depth. High/Ultra underwater views use depth-dependent color absorption and
scattering, with a light moving distortion, rather than the original dark screen
tint. The late scene pass captures current color/depth even when the surface is
out of view and runs before final antialiasing. Low retains the original tint.
Reflection targets follow render-scale/viewport changes and the additional far
reflection and surface color/depth targets are released when returning to Low.
High/Ultra also add one full-resolution color/depth blit before the water draw,
reusing that image for refraction without another geometry pass. These are planar
reflections and screen-space refraction, not ray tracing. Reflection timing remains outside
`graphics.diagnostics()`; assess whole-frame performance on the target GPU.

Command posts now carry a soft, depth-tested additive halo around their colored
beam. This explicit emission halo remains visible for saturated red/green team
colors, which the generic LDR bright-pass often missed. The **Bloom and hologram
glow** checkbox controls it, and beam/flare/halo all follow capture fade and
flicker. The halo adds one sprite draw per visible post and cannot shine through
opaque walls. It uses a shared procedural texture rather than a new asset.

Additional generated-fixture checks verify nearby objects in High reflections,
quality-dependent resolution, restoration of scene resources, Low target release,
water motion without assets, combined AA, colored glow, toggle behavior, and
wall occlusion. Repeated non-MSAA water and particle captures also verify the
r170 framebuffer read-cache workaround; without it, repeated blits could bind
the same buffer for read and draw. Actual Theed screenshots were also checked at each water quality.

Held weapons now register as shadow casters when their meshes are created,
before luminous muzzle flashes are attached. They receive world shadows too.
The world weapon follows the hand bones and continues casting when hidden in
first person. Graphics shadow toggles retain this registration. Generated GPU
checks in `tests/visual-assets.mjs` compare ground shadows with and without
the held weapon in both view modes.

Banner materials use pinned-top GPU wind deformation with a subdivided mesh,
adjusted normals, expanded culling bounds, and the same motion in shadow depth.
This is restricted to explicit banner/cloth material names; it doesn't move
architecture or collision geometry.

HUD raster upscaling and runtime atlas caching are documented in
[hud-vectors.md](hud-vectors.md).

High and Ultra water also add small animated foam patches and intermittent
droplets where solid map triangles meet the water plane. Shore discovery is
cached in nearby tiles and bounded to two new tiles per frame, with at most
192 patches on High or 384 on Ultra. Low disables these draws. The effects
follow water motion and are hidden during its reflection pass. Tests cover
actual wall intersections, submerged geometry, tile caching, and quality
toggles. No fish assets were found in the inspected Theed data; this pass does
not introduce fish.

Implementation was checked against the installed Three.js r170 source,
especially `CSM`, `WebGLShadowMap`, `WebGLTextures`, and `WebGLRenderer`.
The [LightShadow API](https://threejs.org/docs/pages/LightShadow.html) documents
normalized shadow bias; the [Material API](https://threejs.org/docs/pages/Material.html)
documents alpha-to-coverage and single-pass double-sided rendering.

## Droideka shields and rolling

**Droideka effects** is an independent Graphics setting, defaulting to Enhanced.
Classic uses the extracted cloud texture's alpha, native color and dimensions,
one UV repeat per second, quadratic facing falloff, and the 0.15-second
0→1.06→1 activation curve. The native specular projection and blend flags are
approximated; this is not a pixel-identical reproduction of the original shader.

Enhanced retains a clear center with a bright cyan rim, triplanar cell detail,
moving plasma threads, an expanding activation pop and an upward energy sweep.
An additive outer shell gives the field a narrow luminous halo even without
post bloom; it shares the field's fade and is hidden in Classic and inside views.
Real absorbed damage produces bounded, expanding surface ripples; damage without
a source produces an undirected flash. Charge drain cannot create false hits.
The current gameplay supplies blast sources; adding incoming player projectile
collision remains separate gameplay work. Up to six simultaneous impacts share
one smooth sphere draw (Enhanced adds one lower-resolution halo sphere). Both
looks depth-test against the world, draw only the
visible shell face, and reduce opacity for cameras inside the field. Seen from
inside (first person) the shell is nearly face-on everywhere, so the rim gives
no cue there; Enhanced draws a light veil instead, clear in the middle of the
view and thicker toward its edges, with the drifting cloud (one texture fetch,
no lattice or plasma). Classic keeps the native falloff, which is all but
invisible from inside. Rapid
Enhanced toggles preserve the current radius rather than restarting with a jump.

The folded mesh supplies its own rotation axle, measured once from its skinned
roll pose. Lean rotates about that center, and fold/unfold transitions preserve
pose continuity. Roll, lean, leg lag and clip time are sampled between simulation
ticks alongside rendered position; collision and aiming remain authoritative.

Enhanced rolling emits at most 160 instanced soft dust particles, tinted for the
map and surface. Speed and turns strengthen the trail. Cached ground/sky probes
suppress new dust beneath roofs, over water, without a floor, in the air, or
outside rolling mode. Outdoor paved courtyards can emit dust. Existing dust
settles naturally after stopping or entering cover; Classic clears it immediately.
Depth fading and fog integrate the particles with nearby geometry.

`droideka-shield.test.js`, `droideka-roll.test.js`, `droideka-trail.test.js` and
`soldier-presentation.test.js` cover transitions, damage provenance, the native
folded mesh and emission limits. The shield and trail `*-render.mjs` fixtures
check their actual GPU output; `droideka-roll-browser.mjs` exercises the extracted
rig in Theed. Browser fixtures accept `PLAYWRIGHT_MODULE` and `RENDER_TEST_URL`
(the real-map roll check uses `GAME_URL`).

## Jetpack effects

**Jetpack effects** selects Classic or Enhanced independently of other graphics
options. Classic plays the extracted JetEffect. Enhanced locates the actual
circular outlet rims in the backpack mesh and follows their bone transforms;
the original effect offsets remain a fallback for models without detected rims.
Enhanced combustion is a ray-integrated 3D density field inside an instanced
box volume, with a hot emissive core and a translucent orange envelope. A shared
32³ noise texture advects through the field; 28 bounded samples per fragment
retain a rounded cross-section when viewed from the side or along the jet.
Premultiplied radiance avoids clipping the warm color before alpha blending.

Ignition ramps over roughly a tenth of a second; shutdown decays instead of
cutting off on one frame. The attached plume's tail lags nozzle movement and
turns, while the mouth stays attached. Detached combustion wisps, thin cooling
smoke and sparse embers use separate lifetimes, motion and shading. Birth times
and positions interpolate along the nozzle path rather than clumping at frame
boundaries. Classic clears every enhanced layer immediately.

The effect uses at most four instanced draws, a 32 KiB noise field, and fixed
pools of 96 combustion wisps, 128 smoke wisps and 64 embers. The volume requires
one shared depth snapshot even when soft sprite particles are disabled: rays
stop at opaque geometry, including where the body intersects the plume. The
snapshot is reused by other particles, and idle jets request no copy. This is
a local procedural volume, not a full fluid simulation or HDR renderer.

`jetpack-effects.test.js` checks placement, transitions, stopping, and budgets;
`jetpack-wake.test.js` checks emission paths and timing at different frame rates.
`jetpack-volume-render.mjs` tests interior depth clipping, foreground occlusion,
and depth-copy reuse with soft particles on and off. `jetpack-cinematic.mjs`
captures ignition, turns, cutoff and rear/side/end-on views for visual review.
`bolt-render.mjs` compares bolt emission against the previous draw style, and
`ordnance-visual.test.js` checks view-muzzle convergence, independent light
positions, and draw-resource reuse.

## Turret projectile visibility

All bolt cores now retain their native texture silhouette and hue with a hotter,
nearly white center and broader colored emission. The inner and outer halos
share one instanced batch; a weapon still needs at most two bolt batches.
The gamma-space renderer remains LDR; this improves apparent emission without
changing the exposure or tone mapping of the original scene.

Droideka muzzles are derived from the visible tube ends in both rigs. Pending
shots sample the final body/first-person pose before ordnance advances, and the
visible streak's head and tail both follow its convergence from the view muzzle
to the physical collision ray. The world rig also aims its guns vertically.
Projectile lights copy their request positions into reusable records, preventing
several emitters from inheriting the last scratch vector's position.

Map turret ordnance now exports its laser/glow sprites and scope art with the
map, independent of which army the player chose. The missing `redlaser_D`
dependency previously left Republic turret bolts alive but without a core sprite.
The runtime texture resolver also searches map textures.

Turret aim rays start on the muzzle's forward plane and exclude the turret's
own live collision body. Projectile casts carry that same exclusion; other
bodies, architecture and terrain still stop the shot. An audit of 216 Theed
turret yaw/pitch/view combinations found 45 own-body aim hits before this fix.

`node --test tests/turret.test.js` covers ray exclusions, forward aim, surviving
visible particle submission and extracted map texture availability.
`python3 -m unittest discover -s tests -p test_weapon_textures.py` checks ordinary,
charged and strike ordnance texture dependencies without retail data.

`tests/turret-visual.mjs` compares real game frames with and without bolt batches.
With the extracted maps and both sides, all eight map/side/perspective combinations
showed visible red shot pixels, forward flight and no own-body impact, JavaScript
errors or WebGL errors. It accepts `PLAYWRIGHT_MODULE`, `TURRET_TEST_URL`, and an
optional `TURRET_TEST_ARTIFACTS` directory for captured PNGs.

## Foliage depth and enhanced trees

Leaf, branch and jungle silhouette materials use opaque alpha cutouts with
depth writes and MSAA coverage. Previously their combined alpha-test/blend
flags disabled depth writes for an entire merged material bucket. That let
foliage and transparent holograms draw through each other. The opaque path
also uses the existing near/far scene coverage transition instead of making
the whole tree increasingly transparent.

Theed's original `AttachOdf` (`0xa9d0d48b`) and `AttachToHardPoint`
(`0x3be7b80a`) metadata restores 120 missing tree crowns: 55 tall trees and
65 ordinary trees, attached at `hp_branch_01`. Their native seeded leafpatch
layouts and textures are retained. Tall-canopy taper now uses world height
before the card height scale, and off-screen crowns are culled before their
cards are rebuilt. Dynamic cards share the map's atmospheric uniforms.

Tree detail **Enhanced canopies** reconstructs the near jungle backdrop from
the native `nab2_leaf` art within the 41 original backdrop-model footprints.
Overlapping upright crowns, connected leaf sprays and lower undergrowth give
the nearby forest parallax and smaller foliage detail. This is an optional
visual reconstruction, not a claim to have recovered original 3D tree meshes.
It leaves architecture and collision unchanged. Small planter/arch foliage is
preserved. **Original** restores the authored backdrop cards immediately.

One instanced draw submits at most 12 visible backdrop groups with 912 leaf
cards each: 10,944 cards / 21,888 triangles. The renderer selects nearby groups
by distance and frustum; groups omitted by that budget keep their original
cards. A complementary pixel mask transitions back to original distant cards
from 140 to 220 m. Card centers and their margins remain inside the authored
bounds. The original silhouette supplies approximate forest shadows; enhanced
individual sprays do not add shadow draws. The native foliage textures remain
the source-detail limit, and headless validation is not a hardware FPS claim.

Checks: `node tests/foliage.mjs`, `node --test tests/tree-canopies.test.js` and
`python3 tests/test_foliage_export.py`. When the canopies were added, GPU pixel
checks (since removed with the browser foliage material) verified a glow behind
leaves is occluded, a glow in front remains visible, and transparent leaf holes
remain open. Canopy checks cover footprint containment, the draw budget,
Original/Enhanced switching, retained unmatched cards, and reculling after
stationary-camera FOV changes. Actual Theed canal, close skyline and distant
skyline views were rendered in both modes without shader or WebGL errors.

## Hard-edged materials

Every hard-edged material (MTRL 0x2) that doesn't add (0x80) now draws as
an opaque cutout, whatever its 0x4 blend flag says, as the original's
shaders draw it (`docs/formats.md`, Rendering). `blendingOf()` in
`materials.js` holds the rule for map buckets (`describe()`) and model parts
(`partMaterial()`), and replaces the earlier list of foliage texture names.
Before, a hard-edged bucket with 0x4 (Kashyyyk's thatch roofs and plank
floors, bamboo, awnings, Endor's ferns, Geonosis's rafters, Tatooine's
grates, Theed's flower beds) was blended without depth writes in the
transparent pass. A merged bucket sorts by one centre for the whole map, and
its triangles and its own back faces draw in index order, so a roof's inside
showed over its outside, a bush under a plank floor showed through the
planks, and particles behind the thatch (and every later transparent draw,
bolts included) drew in front of it.

As cutouts these buckets join the opaque pass with alpha-to-coverage, the
static spatial culling and the alpha-cut shadow casters, and cast their
cut-out silhouettes. Blended (0x4 alone) and additive materials (glass,
holograms, fountains, sky cards) are unchanged. Checks:
`tests/material-flags.test.js` and, in `tests/rendering-checks.js`, a cutout
hiding a particle behind it (MSAA and soft particles on and off) and casting
its cut-out shadow.

## Far-scene fog, far water and the mirror's sky

Kashyyyk: Islands drew a black band between its sky's tree line and the sea,
in Classic and Enhanced. Its sky sets the far fog's start past its end
(`FogNear` 9000, `FogFar` 8000), which the game reads as no fog at all
(`docs/formats.md`, Rendering, "Fog"). The port took the range as it stands:
the `FAR` shader's clamp((z − 9000) / (8000 − 9000)) is 1 everywhere nearer
than 8000 m, so the whole far scene (low-res terrain, `LOWD` islands, the
Classic ocean surface) was fogged to the fog's black.

- **The rule:** `materials.js` `fogs(range)` is the game's test, 0 ≤ start
  < end, and `setFog(map)` (called from `Graphics.setMap`) sets
  `lighting.fogRange` to the map's range or to `NO_FOG` (past any far scene).
  `setEnvironment({ fogRange })` follows the same rule. Everything fogged with
  the far scene's fog reads `lighting.fogRange` (the far materials, the
  Classic ocean surface, and the near-scene effects that borrow the far fog:
  foliage, tree canopies, the jetpack wake, droideka trails, destruction
  effects), so it holds for all of them. Kashyyyk: Islands is the only stock
  map whose fog this switches off. Nothing in the content changes: `map.json`
  keeps the authored range, so the content fingerprint
  (`src/runtime/content-hash.js`, which hashes `map.json`) is untouched and
  nothing needs preparing again.
- **Far water** (`water.js`): with the fog gone, Kashyyyk's far scene showed
  its low-res terrain bare, which under a sea is the sea bed. The game draws
  the water in the far scene too, after the low-res terrain and in its fog,
  out to the look's own `FarSceneRange` (1500 m on both Kashyyyks). Each
  surface pass now has a far copy (`farTint`, `farMirror`, `farLow`): the same
  shader built with `FAR_WATER`, its uniforms shared but for the far camera's
  range, on one quad a cell. It draws from the near scene's end (the fade end, where
  the near water stops; the near water doesn't dissolve, so starting at the
  fade start as the game does would tint the band twice) to the look's reach,
  or the far scene's when the look has none, and none at all when the far
  scene stops short of the near scene's fade start (Yavin 4: Temple). At High
  and Ultra the far tint is the colour the near refraction reaches for water
  that deep; Low draws the near Low pass as it is. It is hidden from under the
  surface and while the Enhanced ocean draws (that has its own far mesh). It
  sits on `graphics.eyeLayer` (4), which only the eye's far camera draws, so
  the water's and the fountains' mirrors leave it out and it never samples the
  target it would be drawn into. Kashyyyk: Docks gains it too: its bed showed
  bare past 200 m before.
- **The mirror's sky:** `Water.reflect()` drew the far scene through the
  mirror with its near plane tilted onto the surface. The sky's models stand
  around the mirrored eye, under the surface, so that plane cut every one of
  them away: the mirror held only what of the far scene stands above the
  water, transparent black elsewhere (on Kashyyyk: Islands' east view the
  whole 960x540 target was 0). High's screen-space march hid it for rays that
  stay on screen; Ultra's planar near reflection and all of the far water
  showed it. The sky's models are now drawn first, alone (layer 5), through
  the plain mirror projection, and the rest of the far scene over them through
  the tilted one.
- **Cost:** far water is one or two draws of a quad a cell (Kashyyyk:
  Islands' 64 cells, 128 triangles a pass); the mirror's sky pass is one more
  render of the far scene a reflection, which draws only the sky's models.
- **Left as it was:** at High a step remains where the near water meets the
  far, at the near scene's end: about 7 levels of red on Kashyyyk: Islands'
  east view, up to about 20 of green and blue on Kashyyyk: Docks' shallow sea
  (the near pass refracts the bed through thin water and adds its screen-space
  reflections; the far copy takes the deep colour and the planar mirror).
  None at Low; on Kashyyyk: Islands, none to speak of at Ultra. Fountain basins
  use screen-space and analytic reflection; their unused planar mirror is removed. The game
  also fogs the water's reflection in `SkyInfo`'s 0x91e9c28b colour on maps
  whose far scene stops short of the near scene; the port doesn't.
- **Enhanced ocean:** the storm ocean (`ocean-storm.js`, `storm-sky.js`)
  hazes the sea and the horizon with its own colour and an exponential fog,
  not the map's far fog, so this change leaves it as it was. If Kashyyyk:
  Islands keeps only the Enhanced waves without the storm's haze, they should
  take the map's far fog (`lighting.fogRange`, none there) as the Classic
  ocean surface does.
- **Checks:** `tools/renderer-world-corpus.mjs` (the rule; `setFog`; which stock maps
  are fogged, against the prepared `map.json`), `tests/test_sky_export.py`
  (the alpha stretch as exported, and Kashyyyk: Islands' retail values), and
  `tests/far-scene-browser.mjs` (`tests/far-scene-checks.js`): the inverted
  range leaves a far wall clear while a real one fogs it linearly, and the far
  water draws past the near scene over the sea bed, in the fog, only from the
  eye, not from under the surface or when the far scene stops short, with the
  sky in its mirror.

## Movement presentation

Fixed-rate physics now interpolates visual velocity and keyboard turning as
well as position. Mouse aim stays immediate. The body, held weapon, and camera
share a short, bounded vertical correction for actual stair contacts; collision,
gravity, and jump trajectories remain authoritative. First-person eye placement
uses the same presented heading, including when rendering faster than physics.

Gait crossfades retain normalized weights, outgoing playback rates, and stride
phase. Weapon-bank upper-body layers follow those same crossfades, avoiding a
brief rifle-pose substitution on droids. Foot turns no longer compress a whole
idle clip into the duration of a turn. Side rolls ease into their heading and
recover without flipping the legs into the opposite gait. First-person arms
blend from a captured source pose rather than recursively blending each frame;
bob retains elapsed time and settles when stopping. Light stair contact changes
no longer trigger the original one-metre landing camera plunge.

Chase-camera collision queries include their safety skin, removing repeated
8 cm correction jumps when pitching near floors. Boom release is continuous,
without the previous clearance dead band. Foliage card buffer writes also avoid
per-card temporary arrays and upload only the visible prefix, reducing CPU/GC
work while walking or spinning the camera without changing vertex output.

Checks: `node --test tests/animation-continuity.test.js tests/view-motion.test.js
tests/camera.test.js tests/soldier-presentation.test.js
tests/movement-continuity.test.js tests/rust-render-frame.test.js`.
With Vite and local assets, `tests/animation-browser.mjs` and
`tests/movement-browser.mjs` exercise the integrated game. The latter checks
60 Hz physics with 60/120/144 Hz presentation and both views on Theed/Bespin
uphill/downhill routes. The eight stair/view cases had no height reversals or
airborne frames, with rendered vertical steps below 2 cm at 120 Hz. These are
scripted continuity checks, not a claim that the target GPU sustains that FPS.

### Enhanced Kamino ocean and storm sky

`Ocean and sky` defaults to Enhanced on native ocean maps. `ocean-storm.js`
provides five dispersive Gerstner swells, analytic normals, compression foam,
world-space capillary detail and a shared 65,280-triangle ring mesh in the near
and far scenes. Short waves fade before the mesh undersamples them. A single
384-instance batch adds crest spray and wind-blown mist, using the existing
opaque-depth snapshot for soft intersections. Water and sky share sea-haze
colour and the cloud panorama for reflections.

`storm-sky.js` integrates a 3D noise volume into a 768×384 hemisphere panorama
with 28 bounded ray steps, cached at 10 Hz; large camera translations invalidate
it, while head rotation does not. Branching lightning uses one fixed vertex
buffer, strikes every 7 to 18 seconds after its opening strike, and briefly illuminates
the clouds, water and ambient scene lighting. Classic restores the imported sky
and original water material, including during a flash. Mode switches reuse the
allocated meshes/textures; disposal restores lighting and releases resources.
These are visual enhancements; native water gameplay still uses mean sea level.

Validation: `tests/ocean-storm.test.js` checks mesh winding, shared edges,
bounded geometry, flash cleanup, mode reuse and disposal.
`tests/ocean-storm-browser.mjs` renders platform and water-level views, moving
swells, clouds and lightning; it checks all water-quality settings, camera-turn
cache reuse and stable GPU resource counts across mode switches. Existing
Classic and Enhanced rain browser tests also pass with no WebGL errors.
Software-rendered checks establish correctness, not hardware FPS.

Storm contrast follow-up: Enhanced sky, sea body and horizon haze now use a
much darker palette, while foam and lightning retain their highlights. Cloud
lighting has deeper self-shadow contrast. Enhanced rain uses up to 8,192 drops,
a denser near-camera distribution, stronger gusts and a subpixel width floor so
fine streaks survive the normal soft-focus/AA passes. Classic remains unchanged.
Reviewed platform and open-ocean captures with soft focus, bloom and High shadows
enabled; open-view median luminance fell from 63.5 to 22.7 (8-bit output).

### The storm keeps the sky's horizon models

Kamino's background was missing in Enhanced: from the platforms, Classic shows
Tipoca City's towers on the horizon, Enhanced only storm sky above the sea.
The city is a model of the sky (`kam1_city`, the sky config's
second `DomeModel`), drawn around the eye just after the dome (`world.js`,
`SKY_ORDER`). The storm's panorama, a full-screen pass with neither depth test
nor blending, was ordered at -100000, after every model of the sky, so it
painted over the city as well as the dome. It now draws in the dome's place,
at `SKY_ORDER + 0.5`, and the models on the horizon draw over it as they draw
over the dome. Every part of the dome is hidden while it shows: Kamino's dome
comes in two parts, and only one was hidden before.

In the storm the horizon models draw with a material of their own
(`storm-horizon` in `storm-sky.js`), made once per sky material and kept
through mode switches; Classic gets the sky material back unchanged. It draws
the same texture with the same blending, in the storm's dim, cool, grey light:
the texture at 64% to 88% (blue the most), 40% desaturated so that light
painted into it does not shine through the storm, while lit windows keep their
colour and glow. A quarter of the sea haze lies over it, and the horizon veil
(`ocean-storm.js`) still thickens the haze toward the models' feet; the ocean
covers what lies below the horizon, as in Classic. A flash
lights them as it lights the clouds: the whole skyline a little, and most on
the strike's bearing, with the haze in front of them lit as the sky around
them is. Lighting the veil as well was tried and dropped: once it brightens,
its 850 m sphere shows a seam where it meets the far ocean.

Other maps. Only an Enhanced ocean brings the storm, and only where it rains
(next section): Kamino. Kashyyyk: Islands also takes the Enhanced ocean (its sea
reaches every edge of its water grid, `isSea` in `water.js`); it took the storm
with it at first and lost its mountains and tree line (`kas_mountains`) the
same way, and now keeps its whole sky. No other map has an ocean, and no other
Enhanced setting replaces the sky or hides its models: the rain sheets, mist
and splashes (Kamino, Yavin: Temple) are thin near-scene layers, the
environment volumes are local boxes, and the fountains' planar mirror is no
longer used. Kamino's far-scene geometry lies within about 450 m of the
platforms, inside the veil's 850 m sphere, which is depth-tested, so the sea
haze never covers it.

Checks: `node --test tests/ocean-storm.test.js tests/sky-layers.test.js` (the
storm hides every part of the dome, orders its panorama between the dome and
the horizon models, swaps their materials, and restores them through mode
switches and disposal) and `tests/ocean-storm-browser.mjs` (from the deck the
city changes at least 20,000 pixels of the Enhanced frame, and a flash
brightens those pixels by more than 10%).

### The storm is Kamino's; a sea under a dry sky keeps it

Kashyyyk: Islands is meant to keep its sunset sky and take only the Enhanced
waves. The map's data now decides the storm, not the ocean: an
Enhanced ocean storms where the map's weather rains on it, which is any
`Precipitation` but Hoth's snow, `Type("Quads")`, as `game.js` reads it
(`storms()` in `ocean-surface.js`). Kamino rains on its sea and keeps the storm
as above; Kashyyyk: Islands has no weather. `EnhancedOcean` (`ocean-storm.js`,
formerly `StormOcean`) is the waves and crest spray for any sea, sized by its
look's wind. The storm's parts come only with rain: the panorama, lightning and
flash lighting (`storm-sky.js`), the horizon veil, rain rings, wind-blown mist
and the dark palette. They sit behind a `STORM` define, their lines unchanged.

A sea under a dry sky keeps the map's whole sky, the dome and the models on
its horizon, as Classic draws it. `sky-probe.js` draws that sky, and nothing
else (only the sky's meshes are on its layer, 3), into a 128-pixel cube around
the eye, again every second of the sea's time or when the eye climbs or sinks
4 m. The waves reflect it, softened by a mip bias of 2, as much as the look's
`ReflectionColor` alpha allows (75% on Kashyyyk), over a body in the look's
`RefractionColor` lit by the map's ambient light and sun. The sun glints on
them and shines through thin crests, capillary ripples are as faint as the wind
is light, and far off the sea takes the colour of the sky at the horizon behind
it (1 - e^(-0.0005 d)). It reads none of the far scene's fog
(`lighting.fogRange`), which stays the map's own.

Checks: `tests/ocean-storm.test.js` (the rule; a dry sea has no panorama,
lightning, veil or rain rings and leaves every sky model shown in its own
material; the probe draws only the sky, all six faces, around the eye, once a
second or on a climb, and restores the renderer's state) and
`tests/ocean-calm-browser.mjs` (on Kashyyyk: Islands the sky above the horizon
is pixel-identical to Classic's, the waves replace the flat layer and move,
and the sea reflects the probe's sky). Kamino's frames, Classic, Enhanced and
lightning, are pixel-identical to those before the split.

## Visual pass: bolts, terrain layers, lamps, ocean, rain, fountains

A pass on five reported problems. Checks used private-asset screenshots of
every extracted map in both looks (no console/WebGL errors), the node suites,
and the browser checks listed at the end.

**Terrain layers (both looks).** Each terrain patch paints with its own one to
three layers, listed in its `PTCH/INFO` (count, then LTEX indices in drawing
order). The compact VBUF (kind 20770) holds the weights: byte 15 is the initial
layer's, byte 11 the second's, and the third takes what is left of 255; the
kind-290 colour alpha is only second / (first + second). The port had assumed
"layer 0 blended into layer 1 by that alpha" everywhere. On Hoth, whose
patches mostly paint layers 0 and 2 (and often in the order 2, 0), that drew
the dark gravel layer in square, patch-aligned blocks and inverted half the
blends. Decoded weights agree within 1/255 at about 15,000 shared patch-border
vertices on Hoth, Geonosis, Naboo Plains and Endor; swapping the two bytes or
ignoring the order gives thousands of disagreements. `tools/zero_terrain.py`
now writes `terrain_weights.bin` (8 bytes per terrain vertex, one per map
layer) and `extract.py` records it on the terrain mesh; `python3
tools/zero_terrain.py <map>` refreshes an existing extract without rebuilding
it. The terrain shader sums every painted layer (gradients from world position,
so unweighted layers are skipped without mip errors). Maps without the file
keep the two-layer blend.

**Terrain: Enhanced shading.** Hoth, Theed and Bespin bake their terrain flat
(nearly every vertex at the sunlit value), so dunes had no form. With
**Terrain: Enhanced shading**, terrain whose bake is flat takes the sun by its
slope, normalized so open flat ground keeps its baked value and shadow stays
at half, as in the original model; slopes facing away fall toward the shadow
level, sun-facing ones brighten up to 15%, and shaded snow takes a slight sky
chill. Ground baked with the sun (Geonosis) is unchanged. Original turns it off.

**Blaster bolts.** Vehicle and turret ordnance has no `GlowLength`, so the 2004
renderer, and the port, drew only a 32x32 core texture stretched over 6-8 m.
A white-hot centre added earlier also made every thin bolt read white.
**Classic** now draws the authored texture's own colour (1.4x brighter, no
recolouring). **Enhanced** (`src/blaster-bolts.js`) draws each bolt as a plasma
capsule shaded per pixel from the closest distance between the view ray and
its segment: a hot core inside a saturated sheath and soft halo, saturating
per channel so the core whitens while the sheath keeps its hue, premultiplied
so the hue holds over bright ground. It reads end-on, and far bolts keep about
a pixel of width with their brightness thinned to match. Colours are stock:
glow-less guns take the hue of their laser texture (what the 2004 game showed),
others their `LaserGlowColor`; sizes follow `LaserWidth`/`LaserLength`, with a
heavier minimum for glow-less guns; every bolt carries a light. One instanced
draw for all bolts and beams. `tests/bolt-render.mjs` checks the classic hue,
the enhanced core/sheath/brightness, and end-on visibility.

**Lamp light.** Bespin's corridor lights are self-lit (unlit) regions of building
atlases, not lamp-named materials, so no light was reconstructed for them, and
the 5 m reach could not cover a 5.8 m ceiling anyway. Every self-lit,
non-blended, non-cutout material is now examined when its texture loads:
triangles bright there are gathered into 2.5 m cells, one light per cell, placed
0.3 m in front of the glowing surface, reaching 6-12 m by glowing area; vertical
emitters (facade windows) are weaker than ceiling panels. Triangles are read at
registration: render culling later replaces a world mesh's index with a
compacted per-view buffer. Selection uses an 8 m spatial hash and ranks by
strength x falloff. Bespin has about 2,600 sources; a trooper under the corridor
panels now brightens and dims between them.

**Kamino.** The storm sea composes `(1 - F) * scatter + F * sky` (the old 20%
reflection floor flattened it), scatters light through crests as a green-teal
glow toward the bright low sky or a strike, breaks whitecaps as connected
turbulent foam eroded by crest coverage (no more spotted lace), trails wind
streaks, and adds raindrop ring ripples within about 45 m. Crest spray is
wind-torn. **Rain** adds `src/rain-sheets.js`: three camera-centred cylinders
(4, 9, 18 m) scrolling a generated streak texture with the wind, clipped by the
existing roof-cover map, soft against geometry and lit by scene ambient
(lightning included); one draw. Near drops take the same lighting.

**Fountains (Enhanced).** Fountains no longer feed the ray-marched volume
(steam still does). The basins stop re-rendering the whole scene 20 times a
second: they reflect by a 16-step screen-space march over the opaque copy they
already capture, falling back to an overcast sky tone. On the A/B review frames
(Theed fountain, 640x400, software GPU) draws fell from 363 to 188 and the
median frame from 1928 ms to 1450 ms; relative numbers only, not hardware FPS.
This pass's jet, a glassy column rising 2 m with a sheath falling around it,
read as a trickle and was replaced: it misread the stock jet's
deceleration (45 m/s over a 3 s key is 15 m/s², not 45) and ignored how much
the stock sprites grow. The replacement is an 8 m jet opening into an
umbrella of glassy ropes and strands that break into drops and spray, falling
into the inner tier, which fills to its lip and overflows into the outer
pool, with long arcs landing there too. It is shaded as water by the scene's
sun and sky, with spray laid on as light at half resolution, and foam, chop,
ripples, caustics and a reflection of the plume in both pools; see "Fountain
rework" in `docs/environment-effects.md`.

**Wading (High/Ultra water).** The reflection layer now fades within 18 cm of
anything breaking the surface, so a leg, wall or step shows its contact instead
of a hard mirrored edge. Wet soldiers (reported by the gameplay water step) feed
up to eight `waders` to both surface passes: white foam hugging what pierces the
surface near them and small rings spreading from their legs.

Checks: `node --test tests/*.test.js` (510 pass; the rain test now expects the
sheets), `tests/bolt-render.mjs`, `tests/environment-render.mjs` (updated for
the mirror's removal: basins must not re-render the scene),
`tests/ocean-storm-browser.mjs`, `tests/rain-enhanced-browser.mjs`,
`tests/weather-browser.mjs`, `tests/visual-assets.mjs`. `tests/rendering.mjs`
passes all 135 checks once the water's simulated and drawn bob agree (`level`
now follows `physicsWave` while the surface is drawn at `wave`).

## Frame scheduling, cameras and resources

`rust/bfe-render/src/frame_plan.rs` owns the loaded battle's pass schedule:
cloud and probe cadence, held mirror views, shadow refresh, clears, copies,
resolves, ordered lines, post-processing and temporal completion. The host
publishes view facts and calls `battle_frame`; it does not dispatch those
phases through a chain of JavaScript drawing callbacks.

`rust/bfe-render/src/cameras.rs` computes derived views, cascade windows,
reflection clipping, probe faces and sky-follow transforms.
`rust/bfe-render/src/lights.rs` supplies retained light publications. Shader
blocks borrow the native results. Host camera shells expose readbacks to
remaining scene, audio and input consumers without recomputing the matrices.

Static world, actor, effect, water, ocean, line and interface families have
native resource descriptors. Geometry and sampler identities survive draw
republication and context recovery. Shared browser image loaders and scene
anchors remain for actual host consumers; they do not provide a second drawer.
[renderer-programs.md](renderer-programs.md) describes catalog keys, generated
layouts, cold utility compilation and corpus checks.

The renderer retains static shadow windows while their view slices fit, copies
cached static depth and draws dynamic casters at the defined cadence. Camera
movement, shadow settings and admission of new resources invalidate only the
relevant retained work. Point lights and skin/instance storage use bounded
native banks; saturation clips or refuses records according to the owner.

## Validation and performance

The content-free [renderer smoke](renderer-smoke.md) links every declared
catalog variant and exercises representative draws, texture/target lifetime
and context restoration. Browser and native-session corpora also compare
actual loaded views, including reflection, first person, water and HUD.
A synthetic smoke does not qualify every map or a physical GPU driver.

`tools/profile-deep.mjs` can record runtime phases and a CPU profile for a
controlled battle. Keep CPU submission, GPU completion, authority ticks and
memory separate when assessing performance. Historical renderer measurements
are in [render-performance.md](render-performance.md); they do not establish
this release's frame rate on another device. Current world profiling guidance
is in [performance.md](performance.md).

## Renderer initialization errors

Battle preparation requires the Rust renderer module and its declared program
variants. Fetch, initialization and link failures stop loading with the GPU
name and cause. Context loss suspends drawing; restoration rebuilds the same
native resources and programs. It does not select another renderer.
