# Map water, steam, haze and weather

Theed's five fountain props and the carbon-freezing chamber's nine steam attachments now animate. The previous map export carried their architecture but omitted the attached FX. `Environment effects` in the graphics menu selects **Classic** or **Enhanced** (default).

Classic uses the retail `waterfountain`, `prop_steamgrate`, and `prop_steampipe` definitions, textures, geometry particles, hardpoint positions, and pipe orientations. The fountain has all six authored layers. Culled emitters stop simulation; when they reappear, one analytical lifetime primes them so the fountain is already running.

Enhanced fountains draw the stock plume's size and timing as water lit by the scene: a jet rising 8 m that opens into an umbrella of glassy ropes and strands, breaking into drops and spray as they fall back into the inner tier, which fills to its lip and overflows into the outer pool, with a sparser ring of long arcs landing out there too (see "Fountain rework" below). Both pools have reflective/refractive water with foam, chop, ripples and caustics where the water lands. Walking and jumping through the basin creates expanding ripples and splashes. The chamber combines correlated 3D steam densities with sparse, full-resolution wisps rising from the vents; character motion displaces the steam and leaves a spreading, dissipating wake. These are bounded analytic effects, not a general fluid simulation.

The existing river-water effects remain separate. This change does not modify architecture, soldier models, collision, audio, or the existing asset import pipeline.

## Haze, snow, placed effects and the sky's models

A play test showed Geonosis had lost its blowing haze. The importer only
understood the world's rain, so four things the 2004 engine draws on many
maps never reached the port. They now do, at the Classic look (the formats
and the engine's rules are in docs/formats.md, "Sky and effects config"):

- **FogCloud** (`src/atmosphere.js`): the band of big soft squares 90 to
  120 m out, 16 m over the ground, drifting with the wind and slowly turning.
  Geonosis (brownmist), Endor, Rhen Var Citadel and both Yavins. The same
  random pattern repeats every 256 m and scrolls; each frame the points
  inside the view's slab are stood on the terrain (a cache of terrain
  heights on an 8 m grid) and drawn, faded at both ends of the range.
- **Dust and mist** (`src/atmosphere.js`): the `dusteffect` objects placed in
  the world. Geonosis's 23 copies of brown dust blowing across the plain,
  Hoth's blowing snow dust and mist, Kamino's and Yavin's mist, the Dune Sea's
  dust and the Citadel's clouds rolling below the walls. Each class's few
  dozen particles run once a frame and every copy in view draws them,
  flattened by the class's height scale, faded by distance as the engine
  fades them.
- **Snow** (`src/snow.js`): Hoth's `Precipitation` is `Type("Quads")`; the
  port drew it as rain streaks (and, Enhanced, as rain sheets and splashes).
  It now falls as the engine's flakes: 100 spinning `fx_snow` squares 3 cm
  across in a 5 m box, the box repeated over the world grid through the
  view to 15 m, blown by the gusting wind. Roofs stop it, as they stop the
  rain. Rain is unchanged.
- **Placed effects** (`tools/zero_environment.py`, `src/environment-effects.js`):
  every effect a placed object's ODF attaches, on every map, not only
  Theed's fountains and Cloud City's steam. Geonosis's smoking Techno Union
  wreck, Endor's 95 Ewok fires, Kashyyyk's bonfire and torches, the Dune
  Sea's campfires, the Citadel's torches and blue flame, Rhen Var Harbor's
  dry-ice mist, Kashyyyk Docks' waterspouts, Yavin's waterfall and bazaar
  sprays, and the steam vents of Hoth, Bespin Platforms and Jabba's Palace.
  Effects without an Enhanced form play their stock effect in either mode,
  out to the near scene's end; steam vents take the Enhanced steam like
  Cloud City's.
- **The sky's models** (`tools/extract.py`, `src/world.js`,
  `src/graphics.js`): the importer kept only a map's first `DomeModel`. The
  rest now draw in order after the dome: Hoth's mountains and ion cannon,
  Endor's Death Star, satellite and tree line, Geonosis's arena, spire and
  rim, Rhen Var Citadel's cloud layers, Kamino's city, Kashyyyk's and Naboo
  Plains' mountains, Tatooine's rocks, Yavin's gas giant and Bespin
  Platforms' Cloud City. They keep their textures' alpha and are blended
  in the sky's order, before the far scene's geometry. The Enhanced storm
  (Kamino, where it rains on the sea) replaces only the dome: they draw over
  its panorama, in its haze and lightning. A sea under a dry sky (Kashyyyk:
  Islands) keeps the whole sky and reflects it (docs/rendering.md, "The storm
  keeps the sky's horizon models" and the section after it).

The haze and snow are page-side presentation like the rain: the same code
draws them on the page whether the world worker or a server runs the
battle. Costs, measured in Chromium (SwiftShader) with drawing off: the
haze 0.03 to 0.2 ms of CPU a frame (Hoth's 584 dust squares the most), snow
0.05 ms, Endor's fires 0.3 ms. Each haze texture is one instanced draw of
at most 4,096 squares, sorted back to front; snow is one instanced draw of
at most 32,768 flakes. Dust squares are large and blended, so their cost
is mostly fill rate, as in the original.

Not yet drawn (data noted in docs/formats.md): sun shafts (`Godray` and the
`godray` objects: Endor's 135 clusters, Yavin's trees), Tatooine's heat
shimmer, sky lightning and thunder (Kamino's Enhanced storm sky has its
own), Endor's canopy shadow map, the sun flares, colour control and the
Citadel's stars. The sky's flyers are drawn (below).

Checks: `node --test tests/atmosphere.test.js tests/snow.test.js
tests/environment-effects.test.js`, `python3 -m unittest discover -s tests
-p 'test_atmosphere_export.py'` (with `BFE_GAME_DIR` set it also checks
every map's haze in the retail archives), `test_weather_export.py` and
`test_environment_export.py`.

## The sky's flyers

Many maps were missing the ships in the sky. The sky config's `SkyObject`s
now fly as the 2004 game flies them (the rules and the per-map survey are in
docs/formats.md, "Sky flyers"):

- **Which.** A battle draws the classes whose `*_DOME` model its script
  loads from a side archive. Geonosis: 2 Republic assault ships, 20
  gunships, 20 droid fighters and 8 Techno Union core ships rising from the
  horizon. Cloud City's Clone Wars battle: 2 Republic assault ships.
  Naboo Plains: a Federation lander (Clone Wars) or a Star Destroyer
  (Galactic Civil War). The Dune Sea and Mos Eisley: 2 Republic assault
  ships or 2 Star Destroyers (Mos Eisley's historical battle, Mon Calamari
  cruisers). Bespin: Platforms lists the same four capital ships as Cloud
  City, but neither of its battles loads their models, so it has none, as
  in the original. No other map has any. (That is Classic; Enhanced, below,
  draws every class of the battle's era.)
- **How.** Ships circle the world's origin: each starts `Distance` out
  across the ground at a random height in `Height`, heading across and
  inward by `InDirectionFactor`, level, and flies a straight line at a
  velocity drawn once in its own axes (plus `Acceleration`), never turning.
  It fades in over 4 s; once past `Distance` + 10 m or its lifetime it
  fades out over 4 s while it flies on (from full alpha: Geonosis's fast
  ships, out of their ring in 3 to 7 s, jump to full then), and starts
  again somewhere else. Every class keeps `NumObjects` ships in the sky.
- **Drawn** in the far scene, after the dome and the far terrain, depth
  tested against them, without fog, lit by the map's ambient and sun and no
  lamps, and blended while they fade, still writing depth, as the
  original's opaque segments do. In Classic the far scene's reach clips
  them, as in the original: at the original view distance Cloud City sees
  its assault ships only from its edge, and Naboo Plains and Mos Eisley
  their capital ships only now and then; the 2x and 4x view distances show
  more.

The importer (`tools/zero_sky.py`, from `tools/extract.py`) writes
`maps/<map>/sky/` for the five maps whose battles draw ships: `flyers.json`
(the classes with the game's defaults filled in, which battle draws
which, the models' parts and textures), `flyers.bin` and
`flyers_index.bin` (the models, 32 bytes a vertex) and `textures/` (one
small opaque texture a model). It is presentation only: no world, worker or
server reads it, it is not in the content fingerprint
(`src/runtime/content-hash.js`), and `map.json` and every fingerprinted
file come out byte for byte as before (`node tools/content-report.mjs
--compare` of the maps prepared before and after: all 30 battles match).
A map prepared before generation 15 has no `sky/` and draws no ships.
Generation 16 adds the Enhanced lists and models (below); a map prepared at
15 draws only the original's ships in either mode.

`src/sky-flyers.js` flies them page-side, in every host (the page, under a
world worker and online), and stops with the rest of the presentation while
the game is paused. Each page flies its own ships (`?seed=` repeats them).
Cost: one instanced draw per model part of each class the battle draws
(Geonosis 6, the others 1 or 2), at most about 5,000 triangles (Geonosis:
2 + 20 + 20 + 8 ships of 74 to 200 triangles), a custom shader of a few
lines with no fog, shadow or lamp work, and per frame a few dozen ships
stepped, sorted far to near per class and their matrices and fades
uploaded (at most 20 a class, 1.3 KB). Measured on Geonosis (SwiftShader
host): the far scene went from 17 draws and 24,106 triangles to 23 and
29,030, and the update took 0.03 ms a frame.

Under the temporal upscaler each class also draws its ships' motion
(`motion-vectors.js`, `addExtra`), in both modes: from each ship's true
placement now and a frame ago, only where the near scene drew nothing. Read
back from the upscaler's velocity target, a droid fighter crossing a 15
degree view at 378 m moves 0.00877, 0.00052 of the screen a frame there
against 0.00875, 0.00048 on screen. It is drawn 2 to 3 pixels from where the
plain render puts it with or without its motion (an earlier "5 to 10 pixels
behind" came from measuring every ship in view at once); with it, 231
pixels around it differ from the plain render, against 309 without, so its
history follows it rather than being clipped away.

### Enhanced

Graphics > Ocean and sky, the setting that already holds the Enhanced storm
sky and sea, switches the ships too, live. It defaults to Enhanced; the
Minimum, Performance and Balanced presets choose Classic, which is the
above, unchanged. Enhanced keeps the original's flight and adds:

- **Every class of the battle's era.** A class's era is its side's: that of
  the side archive whose sub-level holds its `*_DOME` model (rep and cis the
  Clone Wars, all and imp the Galactic Civil War), else its model name's
  prefix. A battle's era is its mission's (`tools/extract.py`). Enhanced
  draws the era's classes whose model a side archive holds, whether the
  battle's script loads it or not, and the classes without an era that the
  script loads; never the other era's. The importer takes the extra models
  from the side archives (`tools/zero_sky.py` `dome_sources`). So Bespin:
  Platforms gets 2 Republic assault ships and 2 Federation landers (Clone
  Wars) or 2 Mon Calamari cruisers and 2 Star Destroyers (Galactic Civil
  War, and its historical battle); Cloud City, the Dune Sea and Mos Eisley
  get both of their era's pairs; Naboo Plains and Geonosis keep what they
  had, which is already all of their era's.
- **Every view distance.** A ship whose far side lies past 98% of the far
  scene's far plane is drawn scaled toward the eye by the share that brings
  it inside, so its image is the same, and its depth is put just inside the
  far plane in the order of its true distance: the far terrain still hides
  it and nearer ships still cover it. Far off and low over the horizon it
  takes on up to 70% of the sky behind it (`sky-probe.js`, a 64-pixel cube
  of the far scene's sky, redrawn about once a second), so a Star Destroyer
  1,500 m past Cloud City's 500 m far plane reads as part of the sunset
  rather than pasted on.
- **Smooth fades.** A ship starts 4 s back along its course, so its fade-in
  ends where the original starts it and it is solid while it crosses its
  ring (Geonosis's gunships and droid fighters, which the original cuts off
  half faded and then jumps to full alpha); it fades out from the alpha it
  has.
- **No holes.** Fading ships write no depth and draw after the solid ones,
  so one fading in front never cuts a ship behind it.

Cost (SwiftShader host, far scene, one render): Geonosis 17 draws without
the ships, 23 with them in Classic, 29 in Enhanced (each class's fading
ships are a second instanced draw per part, only while some fade), 29,030
triangles in both; the update 0.024 ms a frame in Classic and 0.079 ms in
Enhanced, the sky probe included. Cloud City's Galactic Civil War battle,
which has no ships in Classic, draws 2 more (672 triangles) in Enhanced.

Checks: `node --test tests/sky-flyers.test.js` (the motion rules, the
draw order of the random numbers, the placement, materials, the instanced
classes, Enhanced's reach, haze, fades and era lists, and each prepared
map's battles), `python3 -m unittest discover -s tests -p
'test_sky_flyers_export.py'` (with `BFE_GAME_DIR` set it also checks every
stock map's SkyObjects and battles against the survey), and with Vite
running `node tests/sky-flyers-browser.mjs` (each battle builds its classes
in the far scene, and a view of a ship in reach shows it) and `node
tests/sky-flyers-enhanced-browser.mjs` (Enhanced shows the era's ships
where Classic shows only the original's, Bespin: Platforms, Cloud City,
Mos Eisley and Naboo Plains at the original view distance; a Geonosis
gunship followed through its life in each mode; a droid fighter's motion
for the upscaler). Frames in `SKY_FLYERS_OUTPUT`.


## Assets

Generate the small FX sidecars from the local retail installation:

```sh
python3 tools/zero_environment.py bes1 geo1 hot1 kas1 nab2 bes2 ...
```

This writes only `public/maps/<map>/environment/`: emitter definitions and transforms, their textures and geometry-particle meshes, and Theed's basins. `tools/prepare.py` runs it for every map it extracts; a map whose objects carry no effects (Kamino, Naboo Plains, Mos Eisley, Yavin Arena) gets none. Like other retail-derived assets, these files are ignored by Git. The exporter reads the existing native parsers but does not run a map, character, or audio export. A map without the sidecar loads normally without them.

## Render budget

- Fountain ropes, particles, droplets, and steam wisps use fixed instanced geometry; their paths run on the GPU. The fountains are twelve draws in all, whatever the number in view, and only the fountains in view run any vertex work (see "Fountain rework").
- Enhanced effect shaders compile during map loading to avoid first-visit compilation stalls.
- Soft volumes render at half width/height into a separate target, then composite with depth-aware upsampling. Foreground silhouettes reject background fog samples. The fountains' soft spray is drawn the same way. The basin surfaces, the fountains' ropes and other particles, and steam wisps remain at full resolution.
- Near fog uses 18 integration samples, distant fog 12. Empty regions of each plume skip density/light/wake work. All volumes share a 32³ density texture and eight fixed character-wake slots.
- Emitters outside the camera frustum or beyond 90 m do no particle/volume rendering.
- Volumes, jets and spray share the renderer's single opaque-depth snapshot. The jets also take one copy of the frame's colour (a blit and a mipmap build) for the water to show what lies behind it. Required volume clipping works with soft sprite intersections disabled and with MSAA.
- The basins reflect by a 16-step screen-space march over the opaque copy they capture once a frame; nothing renders the scene a second time. That copy holds no transparent plume, so the plume's reflection is analytic: where the mirrored eye ray passes a jet's axis within the plume's width at that height, unless the inner bowl's wall is in the way. (The shared 256×256 planar mirror of an earlier design is still in `fountain-water.js` but switched off.)

## Verification and reproduction

```sh
node --test tests/environment-effects.test.js tests/render-pipeline.test.js
python3 -m unittest discover -s tests -p test_environment_export.py
# With Vite running; set PLAYWRIGHT_MODULE if Playwright is installed elsewhere:
RENDER_TEST_URL=http://127.0.0.1:18457 node tests/environment-render.mjs
RENDER_TEST_URL=http://127.0.0.1:18457 ENV_ARTIFACTS=/path/to/environment-images node tests/environment-maps.mjs
RENDER_TEST_URL=http://127.0.0.1:18457 ENV_BENCH_OUTPUT=/path/to/environment-performance.json node tools/benchmark-environment.mjs
# Theed fountain stills, Classic beside Enhanced (see "Fountain rework"):
RENDER_TEST_URL=http://127.0.0.1:18457 FOUNTAIN_ARTIFACTS=/path/to/fountain-stills node tests/environment-fountain-views.mjs
npm run build
```

The browser test checks foreground occlusion, an opaque surface slicing through fog, the view from inside a volume, near-plane intersections, jump displacement and wake clearing, actual reflected geometry, animated water, liquid/spray rendering, one shared depth capture, MSAA, and restoration of the classic water material and renderer state. Unit checks cover bounded wake storage, movement rates, teleports, culling, mode switching, attachment order, native effect dependencies, and disposal.

The benchmark compares identical fixed map views with effects missing (the pre-change visual state), restored Classic, and Enhanced. It includes reflection work and synchronizes each frame with a one-pixel GPU readback. Its SwiftShader results are software-rendering measurements, not hardware gaming FPS. Timings on this shared machine vary; draw counts and effect-update costs are more useful than claiming a precise FPS uplift.

`tests/environment-review.mjs` records a scripted walking and jumping path (640×400, 4× MSAA, bloom, no shadows) and writes each frame's timings and draw counts to `measurements.json` beside the frames. With a frozen copy of an earlier tree in `ENV_BEFORE_ROOT` it alternates frames between that tree and the working tree to reduce shared-machine drift. It serves both trees itself, with file watching off and explicit Vite entry points, so no dev server needs to be running. It is a visual-review workload, not a frame-rate budget.

```sh
PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs \
ENV_ARTIFACTS=/path/to/environment-review \
ENV_BEFORE_ROOT=/path/to/frozen/baseline \
node tests/environment-review.mjs
```

## Fountain rework

Earlier Enhanced designs lost to Classic in review. Refractive tubes read as
wires, sprite "water parcels" as blobs, and velocity-stretched streaks as
white ribbons. A swept column and sheath read as a trickle. That jet stood
2 m because it read the stock deceleration as 45 m/s². The stock key is
45 m/s over 3 s, 15 m/s², so the stock water stands 4.5-5.6 m. It also
ignored how much the stock sprites grow: ten times their size over 3 s.
Measured from the live Classic emitters, the water1 sprites' centres reach
5.5 m. Their outline, faint at the edges, shows the plume to about 7.5 m and
as wide as the inner basin. The old jet was about a quarter of that height,
a single thin layer with none of the plume's mass.

Two replacements followed; both won blind comparisons with Classic and were
then reworked. The first, a plume of lit aerated-water sprites, won narrowly.
Everything in it was shaded as the same overexposed white gauze that ignored
the scene's light: up to 17% of its pixels were near white, its core read as
a frosted icicle, and its drops as confetti. The second, ropes of water
shaded as water, won every view clearly (6-7 out of 10 against Classic's
1-4) but read as a card-based fountain: too little spray and froth, strands
like a tinsel curtain, flat backlit water, a grey glassy jet, and no impacts.
It also had no water flowing into the outer bowl and still looked smaller than
Classic's. The design below is the third.

The jet leaves at 12.5 m/s (stock: 11.6-13) under ordinary gravity, so the
water rises 8 m and opens into an umbrella. It falls back into the inner tier,
which Enhanced fills to its lip, and the tier overflows into the outer pool
(`src/fountain-jets.js`; the pools are in `src/fountain-water.js`).

- **Ropes.** Per fountain, 170 ropes and 160 fine strands fall into the tier,
  and 28 long arcs clear its lip and land in the outer pool. Each is one
  ribbon, 22 points long, through the path its water has taken since leaving
  the crown. The direction, reach, height and thickness of each stream wander
  slowly over time, so neighbouring water follows almost the same arc: the
  plume's outline holds steady while the streams sway. Most streams gather in
  bundles (sheets that tore into ropes), with gaps between them, and two in
  five ropes fall close around the jet. Each has its own width, opacity,
  aeration and break point. A rope thins as it speeds up, carrying the same
  water, and swells and necks as its water passes. Past its break point it
  beads and parts into drops, a bead every few diameters (Plateau-Rayleigh),
  each drawn round and smeared over a sixtieth of a second. Across, each rope
  is shaded as a rod of water.
- **Overflow.** A sheet pours over the tier's lip and runs down its sloping
  outer face, speeding up, with rivulets long down the face and fine across
  it. It curls glassy over the lip and churns white where it meets the outer
  pool. The tier's pool is one 12-sided disc at the lip for all fountains.
- **Column.** The jet is a surface of revolution with a steady width, rising
  from the pool's surface. It is glassy for its first few tens of centimetres
  and a churning white column above, the brightest water in the fountain; its
  streaks are noise in each part's launch time, so they rise at the water's
  own speed. At its foot, a low dome of broken foam heaves.
- **Particles.** Ten kinds, all analytic on the GPU from fixed seeds: streaks
  racing up the jet; aerated clumps boiling up the column into the crown;
  froth thrown off where the jet breaks at the crown; fine spray falling with
  the ropes as a veil of streaks; a skirt of spray, under a metre high, where
  water lands; a faint low haze drifting downwind; drops shaken off the
  streams and thrown from the crown; splashes where a stream strikes either
  pool (a crown sheet flaring and throwing beads from its rim, a spike, a
  puff and five droplets); ripple rings; and foam riding outward from where
  the water lands, in the tier out to its lip, and in the outer pool from the
  arcs and from the overflow's foot.
- **Shading as water.** Clear water shows the scene behind it: the frame so
  far is copied once, just before the jets draw, and read through its
  mipmaps, so it is bent, blurred and faintly tinted blue-green and never
  shows figures. At grazing angles it reflects the sky, the square's walls
  and its floor (Fresnel), and when backlit the sun bent through it rims its
  sides. Aerated water is lit like cloud: sunward faces bright, the sky in
  its shade, and the sun carried through its thin parts toward the eye
  (Henyey-Greenstein, g = 0.45). Its sunlight passes through the plume
  first: the umbrella (a shell of ropes, eight steps) and, exactly, the jet
  and the dense crown. Layers of white water add up to their own colour,
  never past it, and bright water rolls off toward a light grey-white. Only
  glints reach white: ripples riding with the water, and here and there a
  tumbling facet that flashes for a moment, a pixel and a half across.
  Drops are faint and sky-tinted, bright only when they glint.
- **Spray.** The soft spray (froth, skirt, falling spray, haze) is drawn at
  half the frame's width and height and laid over it with depth-aware
  upsampling, as the steam volumes are. Fine drops scatter what lies behind
  them mostly onward, so the spray is laid on as light, screened
  (`light · (1 − frame) + frame · (1 − 0.3 · density)`). It brightens dark
  things, hardly sunlit ones, and never goes past white or greys what lies
  behind into smoke. It is lit by the shape it hangs in: bright on the sun's
  side, blue-grey in shade, glowing against the sun.
- **Draw order.** Twelve draws for all five fountains, far to near. First
  what lies on the pools (rings, foam), then the overflow beyond the jets'
  axes from the eye. Beyond the axes: the spray, the ropes, then the drops,
  splashes and aerated water. Then the columns. Before the axes: the drops,
  splashes and aerated water, the overflow, the ropes, then the spray. That
  keeps the order back to front without per-frame sorting. Each spray half
  is two draws (into its target, then over the frame). Each draw holds one
  block of instances per fountain in view and picks its fountain from the
  instance number, so fountains out of view cost nothing. (Before this, the
  four unseen fountains' vertices dominated the software-rendered frame.)
- **Pools** (`src/fountain-water.js`). The tier's pool churns where the
  umbrella lands and at the jet's foot, with chop, drop rings and a swell,
  and foam carried out to the lip. The outer pool shows white water along
  the overflow's foot, ripples running out from it, and the arcs' landings.
  The plume shades the pools: the sun's ray passes through the falling water,
  with light dappling through the gaps. Caustics play on the floor where the
  water is shallow, calm and lit. The screen-space copy holds no transparent
  plume, so the plume's reflection is analytic, hidden where the tier's wall
  is in the way.
- **Fixed slots.** Each fountain keeps a fixed slot, and with it the levels
  of its pools (`environment-effects.js`): the tier filled to its lip (0.8 m
  above the stock water) and the outer pool. Wading splashes follow those
  levels (`fountain-spray.js`). Bespin's steam is unchanged.

Measured on the review views (`tests/environment-fountain-views.mjs`,
800×500), over the pixels the fountain changes:

| View | Near-white, Classic | Near-white, Enhanced | Luminance p10-p99, Classic | Luminance p10-p99, Enhanced |
| --- | ---: | ---: | ---: | ---: |
| Map check | 0.0% | 0.1% | 105-234 | 76-221 |
| Gameplay (7.5 m) | 0.0% | 0.1% | 86-225 | 48-219 |
| Far (35 m) | 0.0% | 0.0% | 123-228 | 103-215 |
| Backlit | 0.1% | 0.0% | 80-234 | 52-222 |
| Close | 0.0% | 0.1% | 179-218 | 80-224 |
| Basin | 0.0% | 0.2% | 110-242 | 68-218 |

Near white means every channel is at least 240. The first replacement
reached 9-17% in the map-check, gameplay and basin views. The fountain's
outline (pixels it changes by more than 6%) covers 68,487 px at the map-check
view against Classic's 70,570, and 13,572 px at the far view against 12,271;
it stands as tall as Classic's plume and wider.

Cost, Theed. At the map-check view, draws are 190 with effects missing, 205
with Classic and 204 with Enhanced (the jets' twelve, the tier's pool and the
wade splashes); Bespin is unchanged at 93 Classic and 95 Enhanced. Fragments
the fountain draws rasterize, in screens of 800×500 before discards, the
spray counted at its half size, Classic then Enhanced:

| View | Classic | Enhanced |
| --- | ---: | ---: |
| Map check | 12.7 | 4.2 |
| Gameplay | 31.2 | 11.4 |
| Close | 39.8 | 19.1 |

The spray's two composites add a full-screen pass each, which is cheap where
there is no spray. Enhanced shades a third as many fragments as Classic, but
each costs more: a depth fetch, the lighting and, for clear water, a read of
the frame copy. The copy is one blit and one mipmap build a frame.
SwiftShader frame times on this shared machine are noisy (around a hundred
milliseconds run to run). At the map-check view Enhanced measured
0.83-0.91 s a frame against 0.61-0.63 s for Classic and 0.56-0.60 s with
effects missing; at the gameplay view, 0.91-1.0 s against 0.65-0.72 s and
0.49-0.61 s. The second replacement measured 1.1-1.2 s. Per-frame CPU work
is a few uniforms and instance counts, and nothing is allocated. No hardware
GPU was measured.

`tests/environment-fountain-views.mjs` renders both looks from the map-check
view, a third-person distance (7.5 m, eye 1.9 m), a far view (35 m), a
backlit view, a close view, a basin view, and six frames across one second.
`tests/environment-render.mjs` checks the plume's size: water more than
5.5 m above the nozzle, and more than 1.5 m wide at 2 m. The jets no longer
refract the basin's capture, so the old check for that capture is gone.
`tests/environment-effects.test.js` checks the pools' levels, and that each
draw holds one block of instances per fountain in view.
