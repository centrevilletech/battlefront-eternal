# Sniper shots

The sniper rifles (`rep_`, `cis_`, `imp_`, `all_weap_inf_sniperrifle` and the
Tusken rifle) fire 2000 m/s bolts that live 0.15 s and, alone of every bolt,
name a `FadeOutTime` of 1 s: the 2004 game holds the streak where it stopped
and fades it over that second (docs/formats.md "FadeOutTime"). Video > SNIPER
SHOTS picks how that is drawn (`sniperEffects`, applied live); Enhanced is the
default, Classic in the Minimum and Performance presets. Neither changes where
a round goes, what it hits or when.

## Classic

The 2004 streak (`Ordnance.drawBolt`): up to 20 m of core and 24 m of glow,
held at the impact, fading and thinning to a tenth over the `FadeOutTime`, with
its light lit until it is gone. Under BLASTER BOLTS Enhanced it is the lit
plasma bolt instead, faded the same way.

## Enhanced

`src/sniper-trace.js`, a batch of the ordnance's particle renderer. Each round
`drawBolt` draws is handed over with the gun as drawn, the round's own origin
and line, and whether its flight has ended; the trace keeps its own record of
the shot, so the smoke and sparks outlive the bolt. Each army keeps its round's
`LaserGlowColor`, held at least 0.65 saturated: Republic blue, CIS red,
Imperial and Tusken green, Rebel yellow (224, 224, 160 would otherwise draw
nearly white). A bolt's hot core is tinted toward its colour. A red-dominant
bolt (the CIS's) is drawn as the films draw droid bolts: a thinner white-hot
core inside a wider, brighter red glow that covers a little of the ground
behind it. Whitening its whole core added green and blue and turned it pink
over bright ground.

- **The muzzle puff**: the bolt flashes the water in the air at the muzzle to
  vapour, a small cloud along the shot's first half metre that blooms out,
  throws seven wisps round it, then slows, swells, rises and thins over
  0.9 s. It shows in the shooter's own eye view too.
- **The bolt** is 6 m long, a white-hot core in a sheath of the round's colour,
  drawn flying at 650 m/s from the gun as drawn to where the round ends: the
  round's own 2000 m/s crosses 50 m inside two frames, too fast to be seen.
  Its tail runs in to the impact over 0.05 s.
- **The tracer** is a thin line along the path, lit for 0.6 s after the bolt
  lands. It cools from the gun outward at 140 m/s after 0.03 s, so nothing is
  left by a gun that kicks and moves on.
- **The smoke** is a wisp along the path where the shot was made. It swells to
  about 0.45 m across, rises 0.12 m/s and tears apart over 2.2 s.
- **The impact** plays when the drawn bolt arrives. The world's own impact
  effect and sound are held for it (`Ordnance.deferSniperImpact`). It adds a
  white flash and a coloured glow, a dozen sparks thrown back against the
  round's way, six embers that fall and cool, and a 0.4 s light. A round that
  runs out of life in the air has none.
- **A head shot** (the world's `headshot` effect, or the page's own round
  striking a head as it draws him) adds a gold star, 26 sparks, ten embers and
  a brighter light, once per hit, when the drawn bolt gets there.
- **The shooter's own view.** A shot whose muzzle is within 2.5 m of the
  camera is his own. Its bolt leaves his first-person gun as drawn each frame
  (it kicks and settles after the shot) and fades in over its first 1.2 m,
  the first 0.6 m is left out, and his smoke starts 2.5 m out. Its muzzle
  gets no flare, the gun having its own flash.

The bolt, tracer, vapour and sparks are one instanced draw of capsules, shaded
per pixel from the closest approach of the view ray to the segment (as
`blaster-bolts.js` draws plasma), with soft particles against the scene's
depth; flares are one instanced draw of quads. The vapour is the only part
that covers what is behind it; the rest adds light.

### Cost

- Both draws' programs are compiled while the battle loads: the trace is
  made by `Ordnance.prepare()`, before `graphics.warmupPaced()`. Made at the
  first sniper shot, as before, they stalled that frame on compiling (two
  programs, about 100 ms of the page's time under software GL). For its
  first 90 frames each empty batch also draws one dark instance (zero
  brightness, let go by its fragment shader): the driver builds a program's
  pipeline at its first real draw, and with no instance three issues none,
  so the first shot still stalled its frame.
- Each batch is one interleaved instance buffer, uploaded once a frame over
  the range drawn (a single kept range record). The streak buffer is sized
  for everything the trace keeps at its caps (48 shots, 25 puffs, 161
  sparks: 505 capsules), so it never grows in a fight. When a buffer does
  grow, three's recorded instance limit is cleared: three keeps the count of
  the buffers it first bound until the geometry is disposed, so a grown
  batch silently stopped drawing its newer instances.
- Shots, puffs, flashes and sparks are kept in place, the spent left out,
  and sparks and embers are reused from a pool: no list or object is made
  each frame.
- The vapour's noise is the costliest part per pixel. A pixel where even
  its densest vapour would fall under the discard threshold is let go before
  the noise; any faint pixel is let go before the scene's depth is read for
  the soft edge. Neither changes a pixel that is drawn.
- Each capsule's quad reaches only as far from its segment as the fragment
  shader can light a pixel: each kind's brightest term solved for the
  discard cut from the instance's own brightness (the vapour's 4.8 radii
  shipped before against under 2 needed, and 1.6 once faded; a tracer line
  3.2 glow against 2.7; a bolt or spark 3.2 against 2.3), each end widened
  for the ray's slant off the screen's centre. The vapour was nine tenths of
  the sniper's fill: 3.4 screens of quad a frame at six rifles, 3.6 times
  less now. Moving a quad's corners shifts the rasterizer's sub-pixel
  interpolation, so single noise cells of the vapour flip (0.1% of the
  pixels, the same as widening the old quads by 0.4% does); nothing a
  viewer can tell apart.

Only the drawing is slower than the round. Damage and the hit marker land at
the round's own time, so a hit marker can show about a tenth of a second
before the drawn bolt reaches a target 60 m away.

## Feedback

A sniper round's hit marker shows as the page's own round strikes the soldier
it draws, not a round trip later (docs/semantic-combat.md "Sniper rounds
online"). The authority still decides the damage.

## Checks

`tests/sniper-deep-dive.test.js` covers the trace's life (the muzzle puff,
the bolt drawn in flight, the world's impact and one head shot flourish held
until it arrives, nothing left after the smoke), the late-press lockout, the
unrewound shooter, the droideka's shapes and the crosshair's convergence.
`tests/sniper-bolts-browser.mjs` covers the Classic fade.
