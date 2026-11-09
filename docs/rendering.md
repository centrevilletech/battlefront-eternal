# Rendering audit and controls

The original gamma-space lighting, texture palette, baked lighting, near/far
scene dissolve, map blur, and HUD ordering are preserved. Bloom, FXAA, and soft
particle intersections are optional additions.

`rust/bfe-render` owns all battle and interface drawing on the OpenGL / GLES 3.0
context. Its catalogs provide world, actor, effects, post/HUD and interface
shader sources; its native resource families own geometry, material, sampler and
target recipes. Module or shader-link failure stops the battle with the GPU name
and error. Context loss suspends drawing; restoration rebuilds Rust's GPU
resources and resumes the same battle. Tow cables and flight/navigation helpers
also draw through `bfe-render`, preserving line strips, segments, loops, vertex
colours and dynamic buffer updates.

## Texture and target ownership

`rust/bfe-assets` and `rust/bfe-render` decode PNG rows, lossless WebP HUD
atlases, and KTX2 textures. Completed images awaiting paced upload have a
bounded queue. A declared asset failure stops loading with the GPU name and
cause, rather than choosing another image format. It keeps the original
gamma-space values, authored compressed mip chain and premultiplied-alpha flag.
Format selection preserves the capability order, including BC1/BC3 instead of
BC7 on drivers that unpack BC7 into RGBA storage.

The Rust resource bank creates and uploads textures, applies filtering, wrap,
flip, anisotropy and depth comparison, and owns framebuffer/renderbuffer storage.
It binds far/near, shadow, post, temporal, particle-depth, mirror and probe targets,
resolves MSAA and performs color/depth copies. Battle actor, effect, world
and interface resources use native descriptors.

Sampler clones share source storage while their settings agree. A changed sampler
can split without moving its logical draw id. PNG sources retain encoded bytes
rather than a second full RGBA copy; compressed sources retain one block chain.
Dynamic arrays use subimage updates. Restore reacquires context extensions, rebuilds
source images and attachments, then replays draw registrations using stable ids.

## Findings and fixes

| Area | Finding | Change |
| --- | --- | --- |
| Detached shadows | A normalized bias of -0.0004 across a 2999-unit depth range displaced the depth comparison by about 1.2 world units. | Bias now represents 0.015 world units; normal bias is zero. Front-facing caster surfaces restore contact. |
| Animated self-shadow smudges | A constant normal offset did not correct depth across filter taps on sloping character faces. | Each PCF tap compares against the receiver triangle's depth plane. A deterministic 4×4 tent footprint avoids temporal noise; derivatives are evaluated outside cascade-selection branches. |
| Animated normal maps | Normal-map tangent/binormal directions stayed in the bind pose. Normal transformation also mishandled nonuniform object scale. | Skin the tangent frame with the bones, and use the inverse-transpose normal transform. |
| Moving shadow edges | FOV easing repeatedly changed cascade extents and texel size. Nearby characters shared the initial cascade with substantial distant ground. Cascade selection had a hard boundary. | A conservative shadow FOV stays fixed through normal camera easing; texel snapping is retained. Splits favor nearby detail, and adjacent cascades overlap/blend over 10% of the preceding interval. |
| First-person shadows | Casters were tested against the view camera's layers rather than the shadow camera's layers. | The shadow pass receives a camera copy with the hidden-body layer enabled. The normal view remains unchanged. |
| Fence/foliage shadows | Custom material uniforms did not expose the alpha mask to the depth pass. | Material `map` and `alphaTest` properties now match the visual cutout. |
| Shadow quality changes | Replacing cascades without releasing shadow render targets leaked GPU memory. | Dispose each light's shadow maps before replacing cascades. |
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

Lamp reconstruction is an enhancement, not recovered original light authoring:
source color comes from explicit emissive lamp surfaces, with a modest fixed
radius/intensity. Occlusion is sampled at each object's center, so this does
not provide per-pixel point-light shadows and can switch at a doorway. Dynamic
characters and props receive these lights; baked architecture retains its
original lighting. The renderer remains rasterized; BVH visibility rays are not
hardware ray tracing.

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
reflections and screen-space refraction, not ray tracing.

Command posts carry a soft, depth-tested additive halo around their colored
beam. This explicit emission halo remains visible for saturated red/green team
colors, which the generic LDR bright-pass often missed. The **Bloom and hologram
glow** checkbox controls it, and beam/flare/halo all follow capture fade and
flicker. The halo adds one sprite draw per visible post and cannot shine through
opaque walls. It uses a shared procedural texture rather than a new asset.

Held weapons register as shadow casters when their meshes are created,
before luminous muzzle flashes are attached. They receive world shadows too.
The world weapon follows the hand bones and continues casting when hidden in
first person. Graphics shadow toggles retain this registration.

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
follow water motion and are hidden during its reflection pass.

## Verification

Run the native renderer unit and shader catalog tests and the headless GL smoke gate:

```sh
~/bfe-work/tools/rbuild.sh cargo test --locked -p bfe-render
~/bfe-work/tools/rbuild.sh cargo xtask smoke
```
