# Droid deaths and wrecked power-up droids

`Destruction effects` in the graphics menu selects **Classic** or **Enhanced**
(the default; the Performance preset picks Classic). Classic is the 2004
behaviour, unchanged: the class's explosion sprites and `src/debris.js`
pieces. Enhanced is drawn by `src/destruction.js`, with
`src/destruction-debris.js` (rigid pieces) and `src/destruction-fx.js`
(particles and arcs).

## Why droidekas stood upright

All five `cis_inf_droideka` chunks name `dummyroot` as their `ChunkNodeName`,
and their models are built in that node's frame at the standing pose. The
classic code puts every chunk at its node's transform, so at the end of the
death clip the five pieces rebuilt a complete, standing droideka over the
collapsed body. Each launch direction runs from the body origin to the node
origin, which is the same point for all five, so every piece went straight
up. The droideka also explodes from `onExploded` with health exactly 0,
which gives the speed factor 0.1 (0.8 m/s). The first touch is under the
1 m/s rest threshold, so each piece rested at once and stood there for 10 s.
Battle droids don't share a node per chunk, but their pieces only ever
spun at a constant rate and rested in whatever attitude they landed in.

Classic keeps all of this exactly, as the original data behaves.

## What Enhanced does

The simulation is untouched. It still rolls `ChunkFrequency` with the
gameplay random stream, still sets off the class explosion (damage, push,
sound, the AI's explosion event), and still keeps the dead body's hit shapes
for its linger time. Enhanced reads bodies and settings and draws. It isn't
built in the headless or worker world, and it never uses the gameplay random
stream. The only change to the explosion is that its stock `Effect` sprite is
left out, because Enhanced draws its own blast. Its damage, sound and light
are unchanged.

**A droid blown apart** (the 60% roll, a blast's mark, or a droideka at the
end of `death01`) blasts where its pieces are: a blue-white flash and a warm
one, a glow on the floor beneath, a compact fireball that turns to oily
smoke, sparks that skip off the floor, flakes of plating, a ring of dust
thrown along the ground, a scorch mark that fades over 26 s, arcs leaping
from the centre, and the light (`LightPool.flash`). With no floor within 6 m
(a droid killed in the air or over a drop) there is no glow, dust ring or
scorch mark, and sparks and flakes fall freely.

**A droid killed on its feet** (the death-clip 40%) shorts out first, for
0.42 s while its clip begins. Arcs crawl along its limbs between joints,
sparks spit off its bones, blue flashes and a flickering glow play about it,
and a blue light flickers on its surroundings. Then it blows apart as above,
with the class explosion's sound (`emitSound`, audio only; the simulation's
explosion is not set off a second time).

**A droideka** shorts out the same way for its whole death clip. Small pops
replace the stock 0.4 s blasts. Rolled, it goes up at once, as in Classic.

Every body of a model shares that model's geometry (`actor.js`), so what is
worked out once per model is kept by geometry as bone indices and bind-pose
data only, and always read through the dying body's own bones: its arcs,
sparks and light are on its own body, whichever body of the class died first.

**Pieces** are rigid bodies (`DebrisPieces`, `PieceSolver`):

- They launch outward from the blast, from where the body parts were, at
  0.4 to 0.7 × `ChunkSpeed`, with a random tumble. The droid's held blaster is
  thrown too.
- Chunks that share a node (the droideka's) are each matched once, from the
  bind pose, to the bone whose vertices they cover, and are carried by that
  bone's current transform. The pieces then start in the dying pose instead
  of rebuilding a standing droideka.
- Each piece is its model's box (centre of mass, box inertia) touching the
  world with up to 26 of its own extreme vertices. A ray down from its
  centre finds the ground, and a ray along its motion finds walls.
  Contacts get impulses at the touching point, with Coulomb friction, and
  only fast impacts bounce. The solver substeps at 120 Hz and pushes pieces
  apart from one another (box against points).
- A piece sleeps once still. A long piece that comes to rest standing on its
  end (its centre more than 1.8 smallest half-sizes up) is tipped over, at
  most twice. A piece starting through a surface is brought back to the
  blast's side, and one that falls out of the world is dropped.
- They are drawn with their own model and a heated copy of its material.
  They glow white-orange-red in ragged patches where they tore away (cooling
  with a 2.6 s time constant), are charred, and are lit by the map's light
  region and light map like a soldier. Heat goes on before the near scene's
  dissolve into the far one, so a piece or wreck fades out at range like
  everything else. Within 45 m of the camera they cast
  shadows. The temporal upscaler's motion vectors track them.
- While flying they trail smoke and shed sparks. Landing kicks up dust, and
  sparks if still hot. They crackle with arcs for a second or two, then
  smoulder with thin wisps and embers.
- After 22 s at rest a piece crumbles away (a 1.1 s noise-dithered dissolve,
  its shadow off). At most 40 are out at once; the oldest give way. Models
  are pooled and reused; a reused model's upscaler motion starts afresh. A
  frame or motion that isn't finite is refused, and a piece that goes bad is
  dropped before it can touch another.

**A wrecked power-up droid** (the ammo gonk or the FX-7 health droid,
`powerupstation`) is noticed by its state turning to `destroyed`. It blasts,
and its wreck glows and chars. It then smoulders: a wind-drifted smoke
column lit by the sun and ambient, small flames and a flickering ember light
at first, embers, and now and then a crackle of sparks, a blue flash, and an
arc. The fire dies down within ~15 s and the smoke over ~3 minutes. A repair
weapon rebuilding the wreck smothers it in proportion to its progress. When
the droid is built again the smoulder stops and the wreck's own materials
come back. Wrecks more than 160 m from the camera emit nothing.

Switching to Classic clears everything Enhanced has out.

## Cost

- Particles: two fixed pools of instanced quads, **one draw each** when
  anything is alive: `hot` (additive; 1536 slots) and `smoke`
  (alpha-blended; 1536 slots). Each is written once at birth. Motion
  (drag, gravity or buoyancy, the wind, spark bounces) and shape and colour
  over age are worked out in the vertex and fragment shaders. Smoke and fire
  are procedural (noise with its gradient, so the sun lights billows), with
  no textures. The CPU only retires expired records (swap with the last) and
  uploads the changed range, merged across frames stepped without a draw. A
  full pool drops new particles rather than growing. Trails, wisps and the
  sparks pieces shed back off as a pool passes 40% full and stop at 75%,
  which keeps the last quarter for blasts and wrecks.
- Arcs: one dynamic ribbon buffer, **one draw**, at most 48 arcs of 12
  segments plus up to two branches, re-jagged 30 times a second. The ribbon
  is expanded in the vertex shader, never thinner than ~2 pixels.
- Pieces: one draw per piece (the chunk models are one part each), plus its
  shadow draws within 45 m of the camera. Up to 40 pieces.
- Rays: per moving piece, one along its motion each frame (when faster than
  0.25 m/s) and one down to the ground, which is reused until the piece has
  moved 5 cm or 0.25 s has passed (0.1 s in the air). None once asleep. One
  more per piece at spawn, one per blast, and one a frame per droid shorting
  out.
- No per-frame allocation in the pools, arcs or solver. Spawning a piece
  allocates its small record; models are pooled.
- One battle droid's death peaks at about 120 hot particles, 100 smoke
  particles and 10 arcs, and settles within ~3 s to a few dozen. A
  smouldering wreck keeps about 20 to 60 smoke particles.
- A busy moment, measured in headless Chromium on the development machine
  (JavaScript time, not GPU): six droids dying at once (two killed on their
  feet and a droideka among them) and two ammo droids wrecked. That is 35
  pieces in flight, and peaks of ~700 hot and ~990 smoke particles, 47 arcs
  and 41 draws. `Destruction.update` took 1.1 ms a frame on average (median
  0.3 ms, 95th percentile 4.3 ms) over 6 s. Of that, rigid pieces were the
  larger part: 2.8 ms at the 95th percentile while all 35 flew, nearly all
  of it in collision rays. A battle droid's blast and pieces cost ~2 ms in
  the frame it dies. The first droideka to die costs ~10 ms once, when its
  pieces are matched to its bones; later ones reuse that.
- Shader programs (the pools, the arcs, the heated piece and wreck
  materials) compile in `graphics.warmup()` before play. `prepare()` builds
  one model of every droid class's chunks and held weapons at load, so the
  first death loads nothing.

## Checks

```sh
node --test tests/destruction-debris.test.js tests/destruction-fx.test.js tests/destruction-mode.test.js
# With Vite running; set PLAYWRIGHT_MODULE if Playwright is installed elsewhere:
node tests/destruction-browser.mjs
```

The unit tests cover settling on a face without sinking, a rod landing on its
end lying down, walls at 30/60/144 fps, holding on a 15° slope and sliding on
45°, piles separating and sleeping, sleeping pieces casting no rays, the
heated material leaving the stock one untouched, lifetime, pooling and the
piece cap, pieces starting through the ground, shadows by distance, heat
before the far-scene dissolve, refusing non-finite pieces, and a recycled
model's motion. For the effects they cover the presets and mode switch,
Classic doing nothing, Enhanced never using the simulation's randomness, the
short-out sequence and its sound, recycled bodies, a second and third droid
(and droideka) of one model shorting out on their own bodies, the droideka
chunk matching, blasts in mid-air, clearing on a switch to Classic,
`dispose()`, the wreck smouldering, being smothered by repair and stopping
when rebuilt, and the pools' expiry, upload ranges across undrawn frames,
caps, headroom and arcs.

The browser test runs the same scripted kills on Theed with each setting
(battle droid blown apart and killed on its feet, super battle droid,
droideka deployed and rolled, and an ammo droid destroyed, with frames over
100 s). The simulation's event journal, gameplay random state, units and
power-up droids must come out identical. Enhanced pieces must all come to rest
on the floor, none standing on end. Classic must throw its stock pieces with
Enhanced drawing nothing. The player playing a battle droid and a droideka
must come apart too. Last, three battle droids and then three droidekas
(each set three bodies on one model geometry) are killed in turn on their
feet: each must short out with every arc on its own bones and every spark,
flash and light within 3 m of it, then blow apart (`second-droid-*` frames).
`DESTRUCTION_OUTPUT` sets the directory the frames go to.

`tests/runtime-world-worker-browser.mjs` is the wider simulation check. It
replays 720 scripted ticks of a Classic-AI battle through two local server
sessions and requires the two authoritative worlds to match.

## Limits

- Pieces touch the world through planes found by two rays. Ground under a
  piece's centre is right on open floors and terrain. On a stair edge or a
  ledge, part of the piece can overhang into the step for a moment. Walls are
  seen only along the piece's motion.
- A piece's inertia is its box's. Odd shapes tumble a little like their box.
- The droideka's chunk data lists `droideka_chunk3` twice. Both copies are
  thrown, and they start overlapped inside the blast.
- Smoke and sparks don't collide with walls beyond the floor height a spark
  skips off. The smoke drifts on a fixed light wind.
- The dead body's hit shapes still stop rounds for the body's linger time
  (as in Classic for a droid blown apart), though nothing is drawn there.
- There is no heat shimmer: the renderer has no refraction pass to bend the
  frame behind a blast.
- The frames were judged in SwiftShader (software WebGL), not on GPU
  hardware, so no GPU timing is claimed. The browser test films with the
  upscaler off, since its frames are far apart. A separate pass with the
  default temporal upscaler, drawing every frame through a blast, showed no
  ghosting of flying pieces.

## Droideka death presentation

Once a body blows apart, a generation-scoped page latch prevents later unit
summaries, pose records or own corrections from restoring its visibility.
Droidekas hold the final death pose (or their rolled pose) and dissolve for
0.5 seconds; other blown-apart droids hide immediately. A new life clears the
latch and restores original materials. Enhanced mode uses the heated dissolve;
Classic uses the plain dissolve. Shadows stop while the body disappears.

This deliberately preserves the port's longer death animation. The 2004 game
goes almost immediately to the second explosion and removes the body; it has
no half-second dissolve.
