# Rendering audit and controls

The original gamma-space lighting, texture palette, baked lighting, near/far
scene dissolve, map blur, and HUD ordering are preserved. Bloom, FXAA, and soft
particle intersections are optional additions.

## Findings and fixes

| Area | Finding | Change |
| --- | --- | --- |
| Detached shadows | A normalized bias of -0.0004 across a 2999-unit depth range displaced the depth comparison by about 1.2 world units. | Bias now represents 0.015 world units; normal bias is zero. Front-facing caster surfaces restore contact. |
| Animated self-shadow smudges | A constant normal offset did not correct depth across filter taps on sloping character faces. | Each PCF tap compares against the receiver triangle's depth plane. A deterministic 4×4 tent footprint avoids temporal noise; derivatives are evaluated outside cascade-selection branches. |
| Animated normal maps | Normal-map tangent/binormal directions stayed in the bind pose. Normal transformation also mishandled nonuniform object scale. | Skin the tangent frame with the bones, and use the inverse-transpose normal transform. |
| Moving shadow edges | FOV easing repeatedly changed cascade extents and texel size. Nearby characters shared the first cascade with substantial distant ground. Cascade selection had a hard boundary. | A conservative shadow FOV stays fixed through normal camera easing; texel snapping is retained. Splits favor nearby detail, and adjacent cascades overlap/blend over 10% of the preceding interval. |
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

The first cascade at the tested 640×400 viewport used approximately 1.52 cm
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

Checks: `node tests/foliage.mjs`, `node --test tests/tree-canopies.mjs`,
`python3 tests/test_foliage_export.py`, and `node tests/foliage-browser.mjs`
(with `PLAYWRIGHT_MODULE` when needed). GPU pixel checks verify a glow behind
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
  None at Low; on Kashyyyk: Islands, none to speak of at Ultra. The fountains'
  mirrors (`fountain-water.js`) still tilt the sky away the same way. The game
  also fogs the water's reflection in `SkyInfo`'s 0x91e9c28b colour on maps
  whose far scene stops short of the near scene; the port doesn't.
- **Enhanced ocean:** the storm ocean (`ocean-storm.js`, `storm-sky.js`)
  hazes the sea and the horizon with its own colour and an exponential fog,
  not the map's far fog, so this change leaves it as it was. If Kashyyyk:
  Islands keeps only the Enhanced waves without the storm's haze, they should
  take the map's far fog (`lighting.fogRange`, none there) as the Classic
  ocean surface does.
- **Checks:** `tests/far-fog.test.js` (the rule; `setFog`; which stock maps
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
tests/movement-continuity.test.js tests/foliage-buffers.test.js`.
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
buffer, strikes every 7 to 18 seconds after the first, and briefly illuminates
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
order). The compact VBUF (kind 20770) holds the weights: byte 15 is the first
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

## Smoothness pass: profiling and frame-time work

Stutter was reported on units and controls. `tools/profile-deep.mjs` drives a
full Classic battle (Theed, about 40 bots) at a fixed 60 Hz cadence, one world
tick per frame, and records the game's runtime phases, a V8 sampling profile,
allocation sampling (collected objects included), GC pauses from a trace, and
WebGL calls per frame. `PROFILE_DRAW=0` profiles simulation and presentation
alone. On this shared Linux host the GPU is SwiftShader, which inflates drawn
frame times; the numbers below compare runs with each other, not hardware FPS.

What it found, and what changed:

| Finding | Change |
| --- | --- |
| Bots beyond 40 m re-posed every 2nd or 4th frame while their bodies glided: limbs stepped. | Everything in view to 120 m poses every frame; beyond, every 2nd; out of view, every 6th. |
| Vehicles were posed only inside the 60 Hz world tick: at 144 Hz a moving tank's drawn position changed in 20 of 47 frames (stalls, then 8 cm jumps). The driver's camera followed it. | The hull and wheels are drawn between the last two ticks every frame (47 of 47 frames, even steps). Before each tick the exact tick pose is restored, so gameplay still reads what it did. |
| 6 programs were compiled at load and 47 in the first drawn frame; more as new materials came into view (getProgramInfoLog stalls; tens of milliseconds each on Windows/ANGLE). | `graphics.warmup()` compiles every material in the scenes, hidden ones included, plus one depth proxy per shadow variant, before the first frame. Ordnance creates its bolt batch first (`prepare()`). |
| Every soldier body had `frustumCulled = false`: drawn in the view, the reflection and all four cascades from anywhere on the map. | A fixed sphere around any pose; culled like everything else. |
| Three r170 draws every shadow caster with one depth material, so alternating skinned/instanced and ordinary casters re-derived the program (parameters and cache key) on every switch, for every cascade (about 2.7 s of a 600-frame profile). | Skinned and instanced casters get depth materials of their own kind. |
| Static culling re-copied and re-uploaded whole visible index ranges per material whenever the view crossed a cell edge (bursts while moving or turning), and the shadow proxies did the same per cascade. | Views and cascades draw one range of a static index buffer (shadows: cells in Morton order). No per-frame copies or uploads. |
| Shadows were about 700 of 941 draws: every static material bucket, small static mesh and soldier material group in every cascade. | Opaque static casters merge into one depth-only mesh per face side (cell-range culled). Soldiers cast through a one-draw proxy sharing the skeleton; soldiers and their guns beyond 70 m, live props beyond 70 m and vehicles beyond 110 m cast none. 701 → 198 shadow draws. |
| High water rendered the whole near scene again from a mirrored camera (171 draws, about a third of the frame's submission). | High reflects the near scene by a 24-step screen-space march over the opaque colour/depth copy the water already takes, over the far scene's planar mirror. Ultra keeps the full planar near reflection. |
| Every renderer.render() re-walked and recomposed the whole scene (3,654 objects, half of them hidden: pooled soldiers, unheld guns, idle effect meshes); the soldier aim code forced each body's matrices (about 300 bones per soldier) on top; three recomputed every bone's Euler angles on every quaternion change. | One matrix update per frame that skips hidden subtrees; static map meshes never recompose; the aim code reads parent chains on demand; bones skip the Euler sync. |
| Allocation (42 MB/s simulation-only; about 80 MB/s drawn) drove minor GCs every frame and 50 to 260 ms major GCs. | Allocation-free animator hot paths, HUD quad batch, effect/particle lists, texture and batch lookups, lamp selection; `Math.sqrt` instead of four-argument `Math.hypot` in the pose sampler. Simulation-only allocation 42 → 31 MB/s. |
| HUD atlas upgrades uploaded image elements synchronously (one long frame each). | They upload decoded ImageBitmaps. |

Results on this host (Theed, 42 soldiers): draw calls per frame 1,148 → 438
(High water 1,110 → 438); drawn-frame CPU p50/p95/p99 88/170/220 ms →
54/100/137 ms with the software GPU; simulation and presentation alone
16.0/26.7/34.0 ms → 12.1/22.1/30.1 ms, worst frame 87 → 49 ms.

Still open, by size: draw submission is now about 8 WebGL calls per draw;
instanced soldier crowds (one draw per model part, bones in one shared
texture) would be the next large cut. The retained JS heap (about 36 MB,
mostly parsed map/AI/effects JSON) sets the length of a major GC. World ticks
(collision queries are the largest share, then AI and soldier movement) ran
on the main thread when this was measured, so at high refresh rates a frame
that carried a tick could miss its deadline; every battle now runs its world
in a worker instead (runtime-architecture.md).

Checks: `node --test tests/*.test.js` (510), `tests/rendering.mjs` (138),
`tests/environment-render.mjs`,
`ocean-storm-browser`, `rain-enhanced-browser`, `weather-browser`,
`visual-assets`, `bolt-render`, `effects-gpu-browser`, `foliage-browser`,
`runtime-cadence-browser`, `runtime-profiler-browser`, `theed-camera-browser`,
`droideka-shield-render`, `jetpack-volume-render`, `vehicles-browser`,
`vehicle-motion-render`, `tauntaun-browser`. Updated expectations: static culling
keeps the source geometry; shadow culling goes through `finishStaticShadows()`;
High water reflects in screen space and Ultra in its planar mirror.

```sh
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs node tools/profile-deep.mjs
# PROFILE_MAP=nab2 PROFILE_WARMUP=25 PROFILE_FRAMES=600 PROFILE_DRAW=0|1
# PROFILE_OUTPUT=/path/to/profile-output PROFILE_PORT=18474 PROFILE_SETTINGS='{"shadows":"high"}'
```

## Upscaling and compressed textures

The render scale and adaptive resolution now shrink only the 3D frame. An
upscaler rebuilds that frame at the canvas's full resolution (device pixel
ratio capped at 2), and the HUD is drawn over the result at full resolution.
Before, they shrank the canvas itself, and the browser stretched everything,
HUD included. The far scene, near scene, soft focus and bloom run at the
render size. Upscaler set to "off" restores the old behaviour.

- Settings:
  - `upscaler`: `temporal` (default), `spatial` or `off`.
  - `upscaleSharpness`: 0 to 1, default 0.4.
  - The render-scale slider now steps by 0.05, so 0.5 (4K from 1080p) and
    0.65 to 0.7 (4K from about 1440p) are reachable.
- **Spatial** (`src/upscaler.js`): one pass of Catmull-Rom (9 bilinear taps),
  clamped to the range of the four nearest texels so edges don't ring.
  Scales above 1 are box-filtered down.
- **Temporal**: TAAU in the FSR 2 / UE TAAU family, written for this
  pipeline.
  - **Jitter:** each frame the far, near and first-person cameras are
    shifted by a Halton(2,3) sub-pixel offset (8 to 32 phases, more when the
    upscale is larger). The shift applies only while drawing; gameplay and
    the HUD see unjittered matrices.
  - **Current frame:** each output pixel takes its 3x3 nearest input texels.
    Each is weighted by the distance from the point it sampled to the pixel
    centre, as `exp(-2.29 d^2)` in output pixels. A wide kernel in input
    pixels gives a smooth estimate from this frame alone. The less history
    a pixel has, the more of the wide estimate it takes. That avoids
    stipple, as FSR 2's kernel bias does.
  - **History:** fetched where the pixel was last frame (5-tap Catmull-Rom)
    and clipped into the neighbourhood's YCoCg box (mean ± σ, within
    min/max). It is then trusted less in three cases:
    - by how far it was clipped;
    - where a pixel's dilated motion differs from its own, i.e. an edge whose
      two sides move apart (this removed a halo around the gun);
    - when bright new content rises from the box's bottom to its top (bolts,
      muzzle flashes), or it would arrive dimmed.
  - **Accumulation:** `(history × N + this frame) / (N + weight)`, with N
    capped at 12 at full resolution and 4 at half. History is two RGBA16F
    targets at output size, with N in alpha.
  - **Output:** a contrast-adaptive sharpen draws to the screen; the history
    keeps the unsharpened image. FXAA is skipped (TAAU antialiases), and MSAA
    still applies as chosen (alpha-to-coverage foliage needs it). Without a
    float colour buffer, temporal falls back to spatial.
- **Motion** (`src/motion-vectors.js`):
  - **Static scene:** taken from the scene depth (captured before the
    first-person weapon clears it) and the last two unjittered
    view-projections.
  - **Bodies that move on their own:** every soldier body, the player's held
    weapons and every vehicle batch get a proxy. It shares the source's
    buffers and skeleton and draws, in a scene of its own, from last frame's
    model matrix, bind inverse and bones (a float bone texture per skeleton).
  - **Proxy depth test:** proxies draw without a depth buffer; a fragment
    counts only where the scene depth shows that surface. Cut-out holes and
    a hidden first-person body therefore drop out.
  - **Range:** soldiers beyond 60 m and vehicles beyond 150 m rely on
    clipping.
  - **Fixed on screen:** the first-person weapon is marked as not moving.
  - **Dilation:** a resolve pass at render size takes the motion of the
    nearest surface in each 3x3, so edges travel with their object.
- **Cost per frame:**
  - at output size, the resolve (9 texels, 5 history taps, 1 motion) and
    the sharpen (5);
  - at render size, the motion resolve and the proxy draws (one per tracked
    mesh in range: about 3 to 10 on Theed in third person).
- **Related fixes:**
  - The enhanced bolts' pixel-size floor now uses the height of the target
    being drawn, not the canvas. Under an upscaler the canvas is twice the
    3D frame, and bolts would thin to half a pixel and alias.
  - Water reflections size from `graphics.renderSize`.
- **Measured (SwiftShader, Theed, 640x400 canvas):**
  - Static view at half resolution, against native MSAA+FXAA: temporal
    31.9 dB, spatial 30.1 dB.
  - Running, turning and firing, against the same frozen frame rendered
    natively: temporal 31.6 dB, spatial 31.5 dB.
  - The temporal soldier keeps belt and chest detail that spatial blurs. No
    trail or halo remains after the fixes above; the first-person weapon is
    clean.
  - `rendering-checks.js` (synthetic scene, against 2x supersampling):
    temporal 33.3 dB against spatial 25.9 dB at half resolution. A settled
    image holds still (mean change 0.86/255). An untracked object moved
    1.2 m leaves no ghost (32.9 dB six frames later).
- **Limits:**
  - Vertex-animated motion (foliage wind, banners, water) isn't in the
    motion buffer; clipping handles it.
  - Bots' guns aren't tracked; they are a few pixels.
  - Particles get only the brightness rule.

KTX2 textures from the import are described in `docs/textures-ktx2.md`:
encoding in `tools/ktx2.py`, loading in `src/textures.js`. On Theed, 165 of
172 world textures load as BC7 (normal maps stay PNG). That is a quarter of
the RGBA8 video memory, and the frame matches the PNG run at 47.7 dB.

## CPU-side frame pass: fewer draws, fewer uploads

Another deep pass on the rendering pipeline, as performant as possible without
losing fidelity, was split in two: the GPU side (shadows, terrain, water,
particles, post passes, shaders; see "GPU-side pass" below) and this one, the
CPU side of a frame: what the page submits, uploads and recomputes.

### What a Theed frame submitted, by object

Measured with `tools/cdp-frame-measure.mjs` (headless Chromium over the
DevTools protocol, SwiftShader, 640x360, no Playwright) at the same point of
the same seeded battle (`nab2`, Classic AI, medium, 47 units). Draws per
frame, grouped by what drew them, before this pass:

| Draws | What |
| ---: | --- |
| 105 | Shadow casters that were plain meshes: the power-up droids and build pads, six parts each (a mesh per part, in four cascades), and every held weapon within 70 m |
| 30 | The bots' skinned shadow proxies (one per soldier per cascade) |
| 39 | Near-scene `zero:dynamic` meshes: the same droid parts and weapons |
| 10 to 25 | The bots' bodies (one per material group per soldier) |
| 17 | The HUD (one per run of the same texture) |
| ~100 | The static world: one bucket per material, near and far, already merged by the extractor; terrain cells; the merged static shadow meshes (4); alpha-cut static casters |
| ~20 | Vehicles (skinned batches), canopies (1 instanced), foliage (instanced), command posts, sprites |

So "static props as individual meshes" was not the case here: the map's
placed objects and buildings are merged into one bucket per material by
`tools/extract.py`, and `world.js` draws one range of each bucket per view
(`render-culling.js`). `BatchedMesh` has nothing left to batch there. The
individual meshes were the *live* objects (`props.js`) and the bots'
shadows and guns.

### Changes

1. **Live-object rigs as skinned batches** (`props.js`, `vehicle-rig.js`).
   A droid or turret rig is now one `SkinnedMesh` per material (texture,
   flags, scroll), each vertex following its part's bone, the way vehicles
   are packed; batches are built once per model and shared by its rigs. A
   turret's drum and gun keep batches of their own, hung under those bones
   with an identity bind matrix, so `Gunner.hideRig` still finds their
   meshes by walking the bones. Six draws per droid per pass became one.
   `vehicleBatches` gained `separate` and `dispose` options and
   `attachVehicleBatches` `byName`, `motion` and `meshes`; vehicles use
   neither, so their batches are what they were.
2. **The crowd's shadows in one mesh** (`crowd-shadows.js`, hooks in
   `units.js` and `game.js`). Every bot's body vertices and every model its
   hand can hold go into one skinned mesh on the hidden layer, with one
   skeleton whose entries are the bodies' own `Bone` objects (and the hand
   bone again, once per weapon model). A body beyond shadow range, pooled,
   or exploded, and a weapon not in the hand, has its bone inverses zeroed
   and its index range left out of the packed index buffer (re-uploaded
   only when the shown set changes). 60 skinned and plain shadow draws per
   cascade became one. The mesh casts front faces, as the bodies'
   materials do (`shadowSide`; three's default for a plain material is the
   back faces, which moved contact shadows by a texel). The player's own
   body and arms stay on their existing path.
3. **HUD uploads only when the HUD changes** (`interface/batch.js`). The
   quad batch re-uploaded four attributes (about 380 KB) every frame. Each
   corner write now compares against what the buffer holds (the arrays are
   exactly what the GPU has), marks its quad dirty, and `end()` uploads one
   run per attribute from the first dirty quad to the last: nothing when
   the HUD is static (menus, the spawn screen, dead), a part of it when a
   widget changes. In battle the minimap blips move and the weapon panel's
   fader changes its tint every frame, so most frames still upload most of
   the panel; the gain is in the quiet frames and in bytes, not calls.
   The turned minimap passes its corner UVs through `quad()` so its quad is
   not dirtied by the plain UVs it used to overwrite.

### Measured (600 frames, one world tick each, same seeded battle)

Chromium with SwiftShader on the shared Linux host; frame times are CPU
times on this host, not the player's. Counters are exact. "Before" is the
tree without these files; everything else identical.

| Per frame, p50 | Before | After |
| --- | ---: | ---: |
| Draw calls (`renderer.info`) | 416 | 285 |
| `drawElements` | 438 | 306 |
| Triangles submitted | 358k | 384k |
| `bufferSubData` | 33 | 33 |
| `texSubImage2D` | 24 | 26 |
| `Object3D.updateMatrix` compositions | 3,594 | 3,569 |
| Runtime profile: presentation | 0.9 ms | 0.6 ms |
| Runtime profile: render (three submission, with SwiftShader stalls) | 9.5 ms | 6.4 ms |
| Runtime profile: whole frame CPU | 17.0 ms | 12.8 ms |

Triangles rise because the crowd mesh is drawn into all four cascades (a
proxy was culled per cascade); that is about 25k extra depth-only triangles
per frame on the GPU, against 130 fewer draws through ANGLE. The two extra
`texSubImage2D` are the crowd skeleton's bone texture (one per render pass:
the view and the reflection). The bots' own 24 bone-texture uploads remain;
see "Left" below. `bufferSubData` stays at 33 because in battle the HUD is
dirty nearly every frame; it is 0 for the HUD when it is static.

Fidelity, pixel for pixel against the same frames (`tools/pixel-diff.mjs`,
upscaler off so no temporal jitter): the mission's three camera shots are
identical (0 of 174,720 pixels differ in each); the live third-person view
differs in 4 pixels by at most 10 of 255, on a shadow edge, from the bind
matrices being identity rather than a root transform times its inverse.
With shadows off, or with the crowd mesh disabled, every view is identical,
so the rig batching and the HUD change are exact.

Checks: `node --test tests/*.test.js` (new `crowd-shadows.test.js`,
`interface-batch.test.js`, `vehicle-rig-batches.test.js`).

### Measuring without Playwright

```sh
# two trees on two ports (vite dev servers), then:
BASE=http://127.0.0.1:18490 OUT=/path/to/out/base node tools/cdp-frame-measure.mjs
BASE=http://127.0.0.1:18492 OUT=/path/to/out/after node tools/cdp-frame-measure.mjs
node tools/pixel-diff.mjs /path/to/out/base /path/to/out/after
```

`MODE=det` (default) pauses the loop, runs a deterministic warmup, and
drives one tick per frame with walking, strafing and firing, the world
worker stepped in lockstep with the frames (`game.lockstep`); `Math.random`
is seeded, with three's UUIDs on a separate stream so that creating more
objects never shifts the battle. `MODE=worker` runs the live loop. On
this host worker mode is not a useful CPU measurement: SwiftShader holds
the page to about one frame a second, so each frame applies a second of
packets.

### Where the page's time goes now

A V8 sampling profile (100 us) of 300 drawn frames on the after tree, one
tick per frame: 90% of samples are idle (waiting for the next animation
frame), 8% are one `getProgramInfoLog` (a first-use shader compile, present
in the before tree too), and of the rest three's submission
(`renderObject`, `setProgram`, uniform uploads) and `graphics.js` take
about four fifths; the game's own presentation (`units.present`, the
animator, lamp selection, soldier presentation) is under a tenth of the
non-idle time. What remains on the page is three's per-draw state setting
(285 draws at about 8 GL calls each), the bots' per-body skeleton update
and bone texture upload, and the one pose per bot in view per frame that
the smoothness pass set.

### Not done, and why

- **GPU-skinned, instanced crowd.** The animator's output is a blend of
  layers (leg clips, a weapon bank on the upper body, a weapon action, aim
  twists), not one clip at one time, so baking clips into a texture does
  not reproduce it; the pose texture written once per frame for all bodies
  does. The pieces are: one skeleton over every bot (as `crowd-shadows.js`
  already builds, 25 bones per body), each body's skin indices offset by
  its slot, and one instanced draw per class and material. What blocks it
  is per-soldier lighting: `game.js` sets `sunScale`, `brightness` and
  `ambientPull` on each body's materials from its light region and the
  LUMI map, and `materials.js` selects four lamp slots per object in
  `onBeforeRender`. Instancing needs those as per-instance attributes (one
  `vec4` for the region light and the lamp slots as a per-instance texture
  row or 4+4 attributes) read in the `zero:dynamic` shader, and
  `motion-vectors.js` binds its proxy to the body's skeleton and geometry
  at track time, so a shared skeleton needs its proxy rebound. Both files
  are GPU-side code, outside this pass; with those changes, the win is the
  24 bone-texture uploads and about 24 near draws (10 to 25 bodies and their
  guns) per frame, plus one skeleton update instead of 24.
- **Bones with `matrixAutoUpdate` off.** Bodies posed every 2nd or 6th
  frame still recompose 25 matrices a frame (about 600 of the 3,569
  compositions). Turning it off needs every writer of a bone (animator,
  droideka, destruction, riders, gunner poses, player body) to call
  `updateMatrix()`; the saving is about 0.05 ms and the risk a silently
  stale limb, so it stays on.
- **`Vehicle.present` forces its hierarchy** (about 75 objects per vehicle
  per frame) because riders and the driver's camera read seat matrices
  right after; `getWorldPosition` would refresh the parent chain itself,
  but the camera code reads `matrixWorld` directly. Left as is.
- **Bolt and particle buffers** (`blaster-bolts.js`, `particles.js`,
  `effects-gpu.js`) are GPU-side code, outside this pass; their instanced
  attributes upload on every frame they change, which in a firefight is every
  frame.

### Render worker: evaluation

Moving rendering to an `OffscreenCanvas` in a worker would put three's
submission (the "render" phase) on a thread that GC pauses, DOM layout,
texture decodes and audio fetches on the main thread cannot delay. What it
costs:

- The whole presentation would move with it: the scene graph, actors,
  animators, effects, water, foliage, the HUD batch and the upscaler all
  live where the `WebGLRenderer` lives. The main thread would keep input,
  audio, the DOM menus and the worker client, and would forward packets
  and input to the render worker instead of applying them itself. With the
  world in a worker, that is two hops for every tick (simulation worker to
  page to render worker) unless the simulation worker posts packets straight
  to the render worker through a `MessageChannel`, which is the right
  shape: page = input, audio, DOM; worker A = world; worker B = scene and
  draw.
- `graphics.js`, `hud.js`, `textures.js`, `raster-hud.js`, `vector-hud.js`
  and the menus read `window`, `document`, `localStorage`, `Image`
  elements and `requestAnimationFrame`. Workers have `OffscreenCanvas`,
  `createImageBitmap`, `fetch` and `requestAnimationFrame` (Chromium and
  Firefox), but no `Image`, no DOM and no `localStorage`; resize events,
  pointer lock and settings would be messages. The KTX2 transcoder already
  runs in a worker.
- GC: the retained heap (about 36 MB of parsed map, AI and effect JSON)
  sets the length of a major GC. Splitting the heap three ways makes each
  worker's major GC shorter, but the render worker still owns the scene
  (the larger half of that heap) and still pauses on its own garbage.

What would justify it, measured: the main thread missing its frame
deadline for reasons other than its own frame work. The runtime profiler
records `frameIntervalMs` and `cpuMs` per frame; frames where the interval
exceeds the budget (16.7 ms at 60 Hz, 8.3 at 120) while `cpuMs` is under it
are the ones a render worker would save. On this host, in the measured
600 frames, the page's own frame CPU (presentation 0.6 ms, world 1.6 ms,
HUD 0.0 ms, submission 6.4 ms with SwiftShader stalls) is under budget;
the long frames are all frames whose own work was long (the occasional
20 to 50 ms world phase, from effects and destruction; and the one
shader-compile frame). A render worker does not shorten those. The
recommendation: not yet. First take the world phase's spikes off the page
(destruction effects and debris are presentation and could be stepped at
a bounded cost per frame), measure the interval-over-budget-with-cpu-
under-budget count on an Apple M1 laptop running Linux at 2560x1600 (the
stats line could show it), and build the render worker if that count is a
visible share of frames (above about 1 in 100 at 60 Hz).

## GPU-side pass: cached cascades, hardware shadow sampling

The frame's GPU work, biggest lever first, measured under SwiftShader (a CPU
rasterizer, so its frame time stands in for fill and shading cost; draw,
triangle and GL call counts are exact). Nothing here lowers a preset's
quality; where an image can differ at all, the section says where and why.

### Shadow maps are hardware depth textures

Each cascade's map was an RGBA8 colour target holding a packed depth, written
by three's depth material (which also sampled every caster's colour texture)
and read back through sixteen `texture2D` fetches and sixteen unpacks a
pixel. The cascades are now `DepthTexture`s in the GPU's comparison mode
(`sampler2DShadow`): the caster pass writes the depth attachment alone, with
a depth material of this port's own (`graphics.js`, `depthVertexShader`) that
does the transform and, for cut-outs, the alpha test and nothing else; the
colour attachment is one byte a texel and never written.

The receiver filter is the same 4x4 tent (texels base-1 to base+2 an axis,
weights (1-f, 1, 1, f)/3, the receiver-plane offset per tap). A comparison
fetch with bilinear filtering returns the blend of four texel comparisons,
so two fetches an axis, weighted 2-f and 1+f and placed where their blends
sum to the tent, make the same sixteen comparisons in four fetches; a
numerical check over random maps agrees to 1e-15. The one approximation is
that a fetch's four texels share its reference depth, so the plane offset
is off by up to a texel's slope at them. On a wall nearly edge-on to the sun
that offset is metres and its own back face is the nearest caster, and the
four-fetch version darkened such walls (14 levels over a facade on Theed).
So the filter is adaptive: where the plane's slope is under 6 cm a texel it
takes the four fetches; steeper, the sixteen exact taps. At rest the result
matches the old renderer within the capture noise (see the measurements).

The shadow coordinates are no longer four `vec4` varyings a vertex: the
fragment projects the world position for the one or two cascades it reads,
and the receiver plane comes from the triangle's normal (the world position's
derivatives, taken outside the cascade branches) carried into map space by
each cascade's inverse-transpose matrix (`lighting.cascadePlanes`). That is
the same plane the old screen-space derivatives estimated, without the
interpolation of 16 floats a vertex and 24 derivatives a pixel.

### Static casters are cached per cascade window

The sun never moves and the map never moves. `shadow-cascades.js` holds
each cascade's window (the square three's CSM fits round the cascade's slice
of the view, snapped to its own texel grid) in place as long as the slice
still fits inside it, and lands it afresh only when the slice leaves it (a
held window and a re-landed one share the same texel grid, so holding is
pixel-identical). The frame a window lands, the static casters (the merged
map meshes and the partitioned alpha-cut proxies, now on layer 2) are drawn
into that cascade's cache; every frame the cache's depth is copied into the
live map in place of three's clear, and the moving casters (soldiers, the
crowd mesh, vehicles, held guns, debris, banners) are drawn over it. The
last cascade's moving casters (31 cm texels, 100 m and further) are redrawn
every other frame. `graphics.cacheShadows = false` draws everything each
frame, for comparisons; `diagnostics().shadowPasses` counts static and
dynamic passes.

On Theed at High, 600 driven frames: shadow draws 181 to 46 a frame and
shadow triangles 481,000 to 150,000 to 190,000 (13 to 20 static passes in
the whole run, the rest the moving casters); the map's triangles a frame
692,000 to 385,000; GL calls 3,257 to 2,600.

### Smaller things

- The far scene's target has no multisampling while the temporal upscaler is
  on: its jittered history antialiases the far scene's few silhouettes (the
  horizon, roofs against the sky), and four samples a pixel cost that pass's
  storage and a resolve every frame. With FXAA or no antialiasing the far
  target keeps the near target's sample count. The far materials' coverage
  test follows `lighting.farAaSamples`.
- The water's far-scene mirror (Low and High quality) is held every other
  frame while the eye stays within 0.3 m and 2 degrees of where it was drawn
  from: the sky and the far buildings are 100 m and more away, so a frame's
  parallax is a fraction of a texel, and at rest the image is the same one.
  Ultra, which mirrors the near scene and its soldiers, draws every frame.
- Lamp light on characters stops at the lit slots (`localLightCount`)
  instead of evaluating four falloffs a pixel with zero colours.
- The terrain's detail texture is neutral past 80 m and is no longer fetched
  there (explicit gradients keep its mip level defined inside the branch).

### What the measurements say

Under SwiftShader at 640x400, High preset (4x MSAA, high shadows, temporal
upscaler, bloom, soft focus, High water), the Theed benchmark of
`tools/profile-deep.mjs` driven over CDP: walking, strafing, turning and
firing for 600 frames after a 20 s warm-up, one tick a frame. Before this
pass: completed-frame p50 779 ms, p95 923 ms, 368 draws, 692,000 triangles,
3,257 GL calls a frame. After the cascade cache and hardware sampling alone:
p50 484 ms over the uncontended part of the run (the host was shared with
another benchmark for the rest), 286 draws, 342,000 triangles, 3,015 GL
calls. Paired runs under the same contention put the whole pass at 15 to 38
percent less frame time depending on the load at the time; the exact counters
above are the stable evidence. The terrain's layer blend was also measured by
forcing one layer and by hiding the ground: see `render-performance.md`.

At rest, captures of the same frame before and after differ within the
run-to-run noise of the capture itself (trees and flower beds sway on the
wall clock; about 2 percent of pixels by more than 8 levels in two runs of
the same code): the shadow change 2.3 percent, the whole pass 3.2 percent,
and the regions the first four-fetch filter got wrong (the left facade, the
pillar) now differ by under 0.5 levels on average.

On an Apple M1 laptop running Linux (2560x1600, High), the receiver side is
the larger saving: every lit pixel of the terrain and the buildings did
sixteen RGBA fetches and unpacks per cascade, now four comparison fetches on
open ground and the exact sixteen only on steep walls; the caster side saves
the map's 300,000 to 450,000 triangles a frame and their depth-only fill, at
the cost of one depth copy a cascade a frame.
