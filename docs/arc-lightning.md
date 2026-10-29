# Bolt caster lightning

The clone pilot's arc caster (`rep_weap_inf_arccaster`; the HUD names it DN
BOLT CASTER) throws `LightningBolt` effects (`arccaster_lightning`, the only
one in the game). `Bolt Caster` on the Video options page selects **Classic**
or **Enhanced** (the default; the Minimum and Performance presets pick
Classic). The setting is `arcCasterEffects` and applies live.

**Classic** is the 2004 renderer's look, unchanged (`src/lightning.js`):
textured ribbons, two random shapes blended, branches and children, faded
over `FadeTime`.

**Enhanced** is drawn by `src/arc-lightning.js`. It changes only how the
bolts look. The world still decides everything about a shot (combat.js
`arc()`): who is linked, in what order, the damage, the heat.
The bolts it draws are the ones `Lightning.spawn` receives, from this page's
own world, from a worker's presentation events, or from a server's, with the
world's ends, charge and hold. Nothing new travels between world and page.

## What Enhanced draws

- **The channel.** A jagged line by midpoint displacement between the drawn
  ends: about 0.3 m segments, the push at each level a share of its segment
  (0.2), at most `SpreadFactor` x 0.75 (less on short bolts and at each finer
  level). The two coarsest levels writhe smoothly with time; the finer ones
  take a new shape with each strike, 20 to 35 times a second, and the
  brightness jumps with them (now and then dipping to under half).
- **The leader and the return stroke.** A bolt grows from its start to its end
  in 35 ms plus 1.4 ms a metre, with a bright head, then flashes (2.5 times as
  bright, decaying over about 0.12 s).
- **The chain.** A bolt that starts where another of the same shot ended is
  the next link: it waits 50 ms after that one, so the lightning is seen
  leaping from soldier to soldier.
- **Children and branches.** `NumChildren` x charge (at least one) thinner
  strands twist about the channel, each on 78% of its own strikes. Up to
  `MaxBranches` (more with charge, one more past 12 m) forks leave the middle
  of the channel along it, turned by `BranchSpreadFactor`, `BranchLength` long
  at most, some forking again. Both are redrawn with each strike.
- **The ends.** A held bolt (the first link) starts at the shooter's gun as
  drawn (the fire point of the gun in his hands, or in first person the
  arms' gun), not at the aimer's gun origin that the world uses. A bolt that
  ends on a soldier ends on his ribcage as drawn and follows him; the next
  link starts there.
- **Light.** The frame's point lights (`LightPool`): at the gun, where it
  lands, and halfway along a bolt longer than 10 m, all flickering with the
  strikes, and a flash where it lands. They light the ground, walls and
  soldiers about it, and the first-person arms.
- **Sparks** where it lands: 10 to 26 by charge, thrown back toward the
  shooter and up, falling, skipping once off the floor (the struck soldier's
  feet, or the collision below), and a few more with later strikes.
- **A bolt that met nothing** (a lone bolt the cone's length long, ending in
  the air) fizzles out in short forks there: no sparks, flash or flare.
- **Cooling.** Over the last 45% of its life a bolt dims, thins and cools
  toward violet. It is drawn for `FadeTime` + 0.1 s.

## A struck soldier

A soldier a bolt ends on is shocked for 0.7 s + 0.7 s x charge (a second
strike extends it):

- **A crackling glow over his body:** his own skinned body drawn again on
  his own bones, 12 mm proud of the skin, additive. Veins of noise fixed to
  the skin (its bind pose) jump to a new pattern 24 times a second, brightest
  edge on. It is on whatever layer his body is (his own body in first person
  is on a hidden one) and hidden with it (an enclosed cockpit).
- **Arcs crawling over him** bone to bone, just off the skin, two to five at
  a time, new 15 to 30 times a second, and now and then one from a foot or a
  hand to the ground.
- **A twitch:** spine, ribcage, neck, head, clavicles, upper arms and
  forearms jerk off their pose, new 15 to 25 times a second, up to 0.18 rad
  for the head. It is laid over whatever the animation posed: a bone the
  animation wrote this frame takes that as its pose; one it left keeps the
  last. When the shock ends the pose is put back. Droidekas, riders and a
  soldier held by a creature don't twitch. Only the page's bones move: hit
  shapes don't use them (units.js `updateSpheres`).
- **Light** from his chest, flickering.

Struck in first person, the bolt ends on the player's own hands (his eye is
just above his chest), throws no sparks, and arcs crawl along his gun, drawn
over it in the first-person weapon's own scene.

## A droideka with its shield up

The shield takes the hit first (droideka.js `absorb`), so while its bubble
is up (the page's `ShieldBubble`, `bubbleOf(unit)`) the bolt stops on the
bubble, on the side it came from, and the droideka inside is not shocked:

- the bubble ripples where it was struck: the world's own hit comes from
  where its arc started (combat.js `arc()` hands each link's start to its
  target's `hurt`), so this is the same in both looks.
- electricity races over the bubble from the strike for 0.55 s + 0.45 s x
  charge: four to eight arcs along its surface, a couple about the strike and
  the rest up to about 130 degrees round, new 15 to 30 times a second, with
  a flare and a flickering light where it struck.
- sparks glance off the bubble.
- a link on from him leaves the bubble on the far side, toward its next
  target.

If the hit takes the shield down, the bubble is gone when the bolt lands and
the droideka's body is shocked as above.

## A charging gun

While an arc caster is held at a charge, its muzzle crackles: arcs reaching
out from it (some up to a metre once well charged) and wrapping back along
the barrel, a pulsing glow and its light, all growing with the charge. The
weapon's own `ChargeUpEffect` still plays as in Classic. In first person it
is drawn over the gun, in the weapon's own scene.

## How it is drawn

Everything is glowing capsules, one instance a segment, in one instanced
draw (plus one in the first-person scene when it has something): the
fragment shader finds the closest distance between the view ray and the
segment, as `blaster-bolts.js` draws plasma, so a bolt reads the same side
on and end on (the shooter's own view down it). Where two segments of a line
meet, each pixel belongs to only one of them: the plane halving their angle,
and which side of it the ray passes the joint on, decides. The glow is then
seamless instead of beaded at every joint. A white core, a saturated sheath
and a soft halo are added as energy and saturated per channel. Far away a
line is held to a couple of pixels with its brightness thinned to match;
close to the eye it narrows, and in the world what passes within half a
metre of the eye fades. Fragments write the depth of their closest point, so
walls hide what is behind them, and fade against the scene's depth (soft
particles) where they meet it.

The batch and a stand-in for the shock glow are shown while a battle loads,
so `graphics.warmup()` builds both programs then rather than at the first
bolt. For its first 90 drawn frames the empty batch draws one dark capsule
(zero brightness, let go by the fragment shader) and the stand-in one small
triangle at intensity 0: a program is linked by `warmup()`, but the driver
builds its pipeline at the first real draw (ANGLE on Vulkan and Metal,
SwiftShader), and with no instance three issues no draw, so the first bolt
still stalled its frame. The stand-in is dropped after those frames.

### Cost

- One interleaved instance buffer (20 floats a capsule), uploaded once a
  frame over the range drawn, through a single kept range record. It is made
  for 4096 capsules, enough for a dozen guns firing at once; three or four
  full-charge chains already pass 1024. When it grows, three's recorded
  instance limit is cleared: three keeps the count of the buffers it first
  bound until the geometry is disposed, so before this a batch grown past its
  first 1024 capsules silently stopped drawing the rest (the sparks and
  muzzle crackle, pushed last, went first).
- A line's segments, its hot loop, and the sparks are written straight into
  the buffer instead of through `push()`; the displacement settings are one
  reused object; bolts and sparks are kept in place, the spent left out;
  sparks, the arcs of a shock, a shield's and a muzzle's crackle, and the
  twitched bones' records are reused; a lightning definition's colours are
  worked out once. A sustained fight makes little garbage for the collector
  to stop the page over.
- Its lights are requests to the frame's fixed `LightPool` (eight point
  lights always in the scene, unused ones at intensity 0), so firing never
  changes the light count three compiles lit programs for. The pool picks
  the heaviest requests without sorting or new arrays.
- A pixel already too faint to draw is let go before the scene's depth is
  read for the soft edge. The fragment's depth comes from the projection's
  z and w rows alone.
- A shocked soldier's bones are placed one chain at a time, stamped per
  frame (`place`), where they are read for his chest, his crawling arcs and
  the links leaving him, instead of his whole actor's world matrices being
  rebuilt every frame; the chest point itself is kept for the frame. The
  channel's displacement and a line's joint normals are written as plain
  arithmetic in the same order as three's vector methods (the same bits).
- Where the SwiftShader time goes, measured: the fragments that reach the
  output. Fragments rasterized and let go (about 70% of a capsule's quad:
  the joint planes and the brightness cut) cost next to nothing there, and
  the kept ones are the visible glow, 6 to 7 deep where children, branches
  and flares overlap. No change that keeps the look moves that much under
  software GL; on a hardware GPU 190k blended fragments are well under a
  millisecond, and the page's time is what matters. Tightening the quad to
  the glow's reach or clipping joints in image space (tried) cut rasterized
  fragments 25 to 50% but moved single-pixel seams at the joints (the
  rasterizer snaps corners to a sub-pixel grid; the seams exist today and
  move when corners move), so they are not pixel-identical and were left
  for a measurement on a real GPU.

A failure inside the Enhanced look is reported once (console) and the look
starts clean; it never stops the battle.

## Checks

- `tests/arc-lightning.test.js`: the channel keeps its ends and spread; the
  shooter and struck soldier found and followed; a chain's delay and anchor;
  a droideka's shield stopping the bolt (and a link leaving it);
  the twitch over a new pose and put back, the glow's layer and skeleton; a
  new life ends a shock; the fizzle; the first-person arcs in the weapon's
  scene; the charge crackle; the setting, presets and Classic switch; the
  batch's growth (every instance drawn, one upload range), a line written
  straight into the batch as `push()` lays it out, and spent sparks reused.
- `tests/arc-caster.test.js` (the world's side) passes unchanged.
- Browser frames on Geonosis and Kamino in third person, first person, from
  the side and struck in first person: about 250 capsules and 0.4 ms of page
  time a frame for a full-charge shot linking two or three droids (software
  GL, loaded machine).
