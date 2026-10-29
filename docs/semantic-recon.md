# Recon droid authority and prediction

Recon support had to start before serialization. `wireBody` attached
`game.remotes` only to the original viewpoint's combat, and network humans,
factory Units stepped through `Unit.tick`, had no remote controller, so their
remote weapon could consume its throw without creating a usable droid. The
multiplayer mailbox also always applied mouse look to the soldier. A pose
record alone could not repair this.

## Shared authority

Keep `RemoteDroids.fly`, collision, aim, arsenal and strike rules as the single
implementation. Separate a per-owner control context (piloted droid, accumulated
look, zoom and camera) from the world collection. Both the single-player owner
and network owners use the same control step; each world's droids, lifetime
and outstanding orbital strikes advance exactly once per tick.

The ordering remains body movement, droid movement/aim/fire, then soldier combat
(the throw may launch a droid at the end of that tick). While piloted, route
look and droid axes to the context and zero soldier movement intent. Consume
use/weapon-cycle as exit before any mount interaction. Owner death or disconnect
exits control; the existing droid lifetime/strike continues. Reconnect and
respawn cannot accidentally adopt a droid through a reused Unit ID. Launch,
blaster, strike and death explosion must retain original owner identity rather
than a hardcoded `player` callback argument.

## State contract

Recon droids are kind 10
([entity-record-assignments.md](entity-record-assignments.md)). They use
bounded slots with generation and terminal state, like items; spawn IDs are
never retained unbounded. A remote view includes definition, immutable owner
reference, active/dead state, position, velocity, yaw, pitch, health and
expiry/lifecycle information. It draws through shared model/place/present
helpers and cannot launch, collide, damage or grant. Bootstrap droid meshes
must be cleared when semantic presentation takes over.

Own correction names the piloted slot and generation explicitly, or none. The
correction includes enough state to construct that droid even if its remote
view record has not arrived: definition/owner, F64 position and velocity, yaw,
pitch, health, countdown, dead state, zoom and pending look, plus selected
arsenal state (exact sparse state for every weapon slot since simulation 19,
[network-contract.md](network-contract.md)). Orbital strike random samples and
damage remain authoritative. The countdown and salvo progress shown by the HUD
must come from authority, including strikes continuing after control exits.

Restore own state before input replay. Replay through the same movement/control
step with no damage, spawn, strike or presentation side effects. Use the existing
bounded prediction window and input-generation rules; no snapshot-dependent
command production. Position smoothing applies only to display. Shared camera
enter/exit/zoom and droid HUD follow the predicted own context. A stale own
correction cannot resurrect control after a generation change.

## Required evidence

Use actual RemoteDroids and Arsenal objects for launch, movement parity with
collision/water, soldier freeze, exit/reenter, countdown and cooldown checks.
Cover two simultaneous owners and owner death/disconnect while an orbital
strike is outstanding. Then run two rendered clients through launch/control,
remote visibility, firing, exit, lifetime expiry and mid-flight reconnect, with
loss and blackout. Verify body and droid separately. No client gameplay damage
or orbital strike random draw is permitted during replay.

## Launch attribution and lifetime policy

Recon launch attribution captures team, human status, Unit ID/generation and the
score-bearing Character. Reusing the Unit for another life or changing teams
does not rewrite outstanding strikes. Shared Ordnance hit exclusion resolves
the original Unit only while that launch generation still matches. This is an
intentional change from pointer-only exclusion: an old-life projectile may hit
the owner's new life. Damage scaling and score rules still run through the
existing game callbacks. Recon shots carry no soldier rewind metadata, because
their muzzle belongs to the droid. Bootstrap serialization carries scalar
attribution without recursively copying the old soldier/resources.

## Shots and replay window

The droid's own guns are not combat registry definitions: its arsenal is
built from copies of the content's weapon classes, and weapon classes are not
registered anyway. A droid's shot therefore reaches pages as the presentation
event `remoteShot` naming the droid's class (registered, base `droid`) and the
gun's channel (0 the blaster, 1 the destruct weapon); the page finds the gun
in the droid class for its flash. Naming the copied gun class instead would
make the registry lookup throw inside the simulation's own event emit and stop
the world ("combat definition absent from matched registry"), on a dedicated
server too. Presentation capture records any description it cannot make as a
capture fault and never throws into the tick.

`predictRemote` replays at most `REMOTE_REPLAY_TICKS` (120) ticks past the
authoritative checkpoint. A downstream blackout keeps the page's input clock
stamping commands while its checkpoint stands still; commands beyond the
window wait for a newer checkpoint, so the flown droid is shown at the
window's end instead of every frame failing with "recon replay window
exceeded" until packets return.

A pilot keeps his droid while his input stops. The world stills a player's
controls when his world is suspended (`clearInput`, on the local server's
`suspend`, which the pause menu and a hidden tab send, and on `lockstep`) and
when no command has come from him for half a second (`inputsStale`): his
soldier's and his hull's sticks and his droid's keys are zeroed, and the
droid slows to a hover. Only `removePlayer` (he left) hands the droid back,
as his death does; his next command flies it again.
`tests/recon-network.test.js` checks the pause, the gap and leaving against the
shared world.

## The strike drawn and felt on every page

The orbital strike's bolts and the ground shake were once missing on every
page: offline (the page draws its world worker's battle) and online (a client
of bfeserver), for the pilot, his body after the droid is gone, and any other
player. The droid's own blaster bolts were drawn.

- **Bolts.** A bolt reaches a page as its launch record, which the snapshot
  path sends in a compact form, its position quantized into the snapshot
  bounds (`DEFAULT_SNAPSHOT_BOUNDS`, height -400 to 300 m). The strike's bolts
  leave about 515 m up (600 m out at 70 degrees): the page got them at 300 m
  and drew each on a path 215 m too low and 78 m off, under the ground and out
  of view, while the world's bolts landed where they should. A launch or
  impact outside the bounds now travels as its exact payload
  (`src/runtime/snapshot-cosmetics.js`), which every client already reads:
  the fix is the server's (and the world worker's), and older clients draw
  it right once their server has it.
- **Shake.** Camera shake was set on the world's soldiers
  (`Combat.explode`), and only the world ran that; a page, which never steps
  the world, drew its own copy of the soldier, never shaken, for any
  explosion (grenades and mortars too). The shake is now the page's
  (`src/camera-shake.js`): `CombatPresentation` calls `onExplosion` for each
  explosion it shows, and the page shakes its view by what its camera
  follows (the droid he flies, the hull he drives, else his body as drawn),
  as the original game does (docs/formats.md, "Camera shake": two shakes
  per explosion, at most four held, each dying away linearly). The world
  keeps no shake.
- **What a player sees.** The strike's bolts come down at 70 degrees, and the
  droid's view (48 degrees across, pitching up 50 at most) never reaches
  that high, so the pilot sees each bolt only in its last few metres; a
  bystander sees each one for the last 0.1 s or so (docs/formats.md, "The
  strike's bolts"). The first blast destroys the droid, so the pilot is back
  in his body for the rest of the salvo, and the page measures each later
  shake to his body.
- **Checks.** `tests/recon-network.test.js` flies a strike through the world
  and the snapshot host into a client's `CombatPresentation`: each bolt is
  drawn on its way down to its impact (200 m off before), and each blast
  shows and shakes (none before). `tests/camera-shake.test.js` covers the
  shake's rules and `tests/snapshot-codec.test.js` the exact launch.
- **Stress.** `tests/recon-stress.mjs` (Node, no renderer: the world, its
  capture and pages applying its snapshots, 60 scenarios over the four recon
  classes on nab2) and `tests/recon-stress-browser.mjs` (one drawn page on
  its own clock, ten steps from launch to timeout). Neither is part of
  `npm test`.
- **Known gap.** A page plays no `OrdnanceSound` for a replicated round
  (`SceneSounds.updateRounds` walks `ordnance.bolts`, `rounds` and
  `grenades`, which replicated rounds never join), so the strike's falling
  hum is silent on pages, as every other player's bolt's is (read from the
  code, not measured).

