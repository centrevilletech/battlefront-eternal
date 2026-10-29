# Semantic combat replication

The shared `Ordnance` simulation owns trajectories, collision, lag compensation,
damage, ammunition and explosions. Networking observes `onLaunch`, `onImpact`,
`onProjectile` and `onExplode`;
it does not implement a second firing simulation. `SimEvents` supplies damage and
death notifications. Observation failures are reported through `drainFaults()` and
never interrupt a shared simulation step.

`network-entity-combat.js` carries kind 8 records. Schema 1 describes one bolt;
schema 2 terminates it. Every projectile gets its own ID, including pellets and
repeated automatic shots. The input fire ID is only a predicted-effect identity.
Position, direction, visual offset, speed, remaining lifetime and merge distance
are float64 values. Lag-compensated launches retain their original sampled tick.

Launch and terminal records share an ID. A terminal wins even at the same tick
and remains remembered until the original flight expiry plus 120 ticks. Cosmetic
impact effects are limited to 18 ticks, but that limit does not remove terminal
identity. The receiver advances an independent monotonic expiry frontier, so an
old packet cannot revive an expired projectile after garbage collection. Session
epoch changes clear identity state. Capture and presentation have explicit
capacity limits; the scheduler must separately bound bytes and retained IDs.

A bolt class with a `FadeOutTime` (the sniper rifles) stays drawn where it
stopped while that runs (`docs/formats.md`, "Bolts"; `Ordnance.drawBolt`). Its
terminal does not end the page's bolt: the flight is cut at the terminal's point
and `linearBoltFrame` then reports it ended and fading. When the terminal has
replaced its launch before the page saw it (a round that struck within the tick
it was fired, or a lag-compensated shot that caught up to its target at once),
the page draws it from the shooter's gun as drawn there to the terminal's point,
starting at the launch tick the terminal carries. The player's own predicted
round is cut at the terminal's point the same way (`PredictedShots.stop`). This is
presentation only; the records are unchanged.

Schemas 3 and 4 carry hit feedback and kill notifications. They never change
health. Actor dead state drives persistent corpse animation, independently of
notification delivery. Generation checks prevent old damage/death notifications
from affecting a respawned body.

The client uses shared `Ordnance.drawBolt`, animator, weapon flash and sound APIs.
It never calls `stepBolt`, collision, or a damage hook for replicated projectiles.
Matched local data defines the ordnance registry; the handshake compares sorted
content paths before accepting numeric IDs.

Tests exercise actual shared bolt launch and impact, cosmetic flight,
feedback, duplicate/reordered delivery, exact fractional lifetime, pellet identity,
expiry garbage collection and observer fault isolation. Existing local bolt
replication tests remain required. The two-client browser harness
(`tests/multiplayer-browser.mjs` with `BFE_TEST_DYNAMIC=1`) covers a grenade's
remote samples, removal and explosion delivery; that functional check does not
qualify rendering performance or carrier-network loss.

Schema 5 carries short-lived explosions through shared `presentExplosion`, which
plays effects, sound and light without applying client damage. Schemas 6 and 7
carry dynamic projectile samples and removal. Grenades, missiles, shells and beams
use authoritative absolute position/velocity/orientation or beam endpoints; the
client does not replay collision, homing or random bounces. Samples retain at most
eight points, render three ticks behind, and extrapolate velocity by at most three
ticks. Dynamic records expire after 18 ticks and cannot resurrect after removal.
Their 81-byte sample payload is separate from the containing record/packet headers.

The combat schemas are part of the one mandatory simulation contract
([network-contract.md](network-contract.md)): a client that cannot decode them
is refused at the handshake, and no feature capability negotiation exists. Human
network flight also uses a fixed steering-assist policy so locally sampled remote
targets cannot change the shared flight simulation's steering gain; offline
flight and weapon assist retain their existing behavior.

When gameplay gains a mutable projectile field, determine whether it changes
simulation or only presentation. Keep simulation changes in shared Ordnance and
add an explicit semantic field or event only when the remote presentation needs
it. Own prediction recoil uses the shared `PlayerCombat.updateKick` method on a
private copied kick state; authoritative combat and weapon state remain untouched.

### Delivery and display timing

Capture retains both an undrained launch and its terminal, even at the same
tick. The per-client cosmetic outbox resends launches, first projectile samples,
terminals and explosions until snapshot acknowledgement, within expiry and
capacity bounds. Split own packets acknowledge only after both parts arrive.

Remote terminals shorten a bolt to its impact point; impacts and explosions
play at the displayed world clock, rather than receipt. A late launch can begin
at the drawn muzzle within 24 ticks of execution. Sniper streaks then fade at
the shortened endpoint. Own predicted hits retain immediate feedback. Dynamic
projectiles coast between samples and end at the terminal tick.

Accepted explosion events wait for the visual clock, independently of newer
snapshot receipt. Once received within their wire lifetime, they display once
on the first eligible rendered frame, including after a slow frame. Expired
retransmissions are still rejected; they cannot restart an explosion.

### Sniper rounds online

A sniper rifle's round is a 2000 m/s bolt and the head a 0.17 m sphere, so
lag compensation has to be exact. `tools/sniper-lag-probe.mjs` models a
shooter whose page sees the target as history recorded it (round trip, input
lead and playout delay back), leads it by the round's flight and presses;
the target strafes at 7 m/s. Before these changes a strafing shooter's head
shots landed 3 to 10% of the time at any round trip, and a standing shooter's
fell from 80% to 13% between 0 and 160 ms with a 10-tick playout delay and
jitter. After them every case measured lands 97 to 100%.

- **The shooter is not rewound.** His page predicts his body ahead to the
  tick his press runs on and fires from there; the host moved his muzzle
  back to the view tick, shifting the round by his own movement over the
  round trip (`Ordnance.bolt`). Only the others are sampled at the view tick.
- **The rewind budget allows for the round trip.** The lead the host
  measures (`command.tick - server tick` on arrival) leaves out the transit,
  since the stamp follows the server clock as estimated one way behind it.
  `rewindBudgetTicks` now adds 18 ticks (300 ms) of round trip and the
  playout clock's 12-tick snap to the lead, playout maximum and 2, within the
  48 ticks kept. A view older than the budget is still clamped.
- **A late press is locked out from when it was pressed.** A press whose edge
  ran after its stamped tick (`late`, multiplayer-world.js) starts the
  single-shot lockout that much earlier (weapons.js), so the shooter's next
  on-time press, which his page drew and sounded, is not dropped as inside
  the lockout.
- **Scoped targets stay fresh.** Anything within 0.98 of the aim is sent every
  3 ticks out to 320 m (was 200 m), past the rifles' 300 m reach, so the body
  drawn under the reticle is not coasting between rare samples.
- **The hit marker is predicted.** A sniper round's cosmetic stops on the
  soldier as the page draws him (`PredictedShots`); that strike now shows the
  hit marker and, on a head, the Enhanced flourish at once. The host's damage
  feedback for the same target within 1.5 s is not shown again
  (`claimPredictedHit`); the host still decides the damage.
