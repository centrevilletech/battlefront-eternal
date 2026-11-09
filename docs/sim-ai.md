# Native AI ownership

Classic and Enhanced bot decisions run in the required simulation module.
`rust/sim` prepares authored content once and constructs the
`AiWorld` host in `rust/sim`. There is no JavaScript bot implementation
or AI backend switch. The mode setting still chooses Classic, Enhanced or no
bots.

The simulation instance owns `ai::owned::Owner`. Its battle retains the
controller FSMs, scheduler, navigation planner, deferred LOS, reservations,
class jobs, combat memory and aim state. Navigation and immutable class facts
are prepared before activation. Registration and attack-pattern caches that
persist across battles live in the owner's process state.

Before each tick the host writes one generated phase header, current camera
facts and new journal records into the AI exchange (`ai.prepare`, which calls
`AiWorld.prepare` in `rust/sim`). The native world tick
(`world_step_run`, `rust/sim/rust/world/step.rs`) then runs the AI phase
(`ai_world_phase`) after squad commands and before the unit phase.
Rust runs `Core::decide` followed by `Core::execute` and
writes held intent into the canonical unit or seat owner. The host does not
serialize each controller or ask JavaScript navigation, geometry, population
or combat services between decisions. Changed identity and presentation
notices return at the outer boundary.

Within that tick the native unit phase immediately follows AI and reads
its canonical controls directly. Ordinary bot intent edits join that phase's
completed body publication instead of crossing to the host twice. Navigation avoidance, foot weapon
tuning and selection share that boundary, including clear/restore writes whose
final values equal the prior snapshot. A synchronous observer still flushes
the complete touched arsenal before its callback. Damage,
lifecycle and synchronous foreign observation barriers still publish their
touched rows immediately. The production coordinator owns AI and danger
execution; the host has no standalone AI step. Seat steering stays with the vehicle phase ([sim-f2.md](sim-f2.md)); seat facades
publish only their qualified control, lease and controller identities.

## Canonical state and callbacks

Units, registry members, hulls, controls, weapons, Characters and posts resolve
through native stores. References include storage generations and gameplay
lives where required. A retained controller facade carries identity only;
reusing its physical slot does not change an older facade's generation.
Displaced generations remain addressable until the outer publication journal
has drained, then leave the host lookup bank. A held facade keeps its original
lease and generation; monotonically increasing controller indices allocate no
additional host slots.

Vehicle policy reads native seats and leases. Building repair candidates keep
their authored insertion order and a separate identity for the repair wrapper.
Cold content identifies command-post buildings explicitly so rebuilding does
not infer that policy from a nearby post.

Population initializes the native roster before AI attachment. Spawn, death,
boarding, weapon and post callbacks borrow the same active Core and random
streams through an explicit service scope. A callback can observe a death or
new lease immediately, at the original lifecycle point. It does not wait for
a later page publication.

Classic input and visibility cadence remain gameplay. The scheduler keeps the
original cycle, LOS budget, identity parity, carried input time, near radius
and camera-frustum order. Navigation also retains the source's
field-presence behavior: Ground/Fly helpers omit `hasPath`, and the placed
post array has no named or nearest-post lookup for landing hints. Enhanced substitutes its own decisions and aim while
using the same canonical state. Decision and bot-weapon stream A, command
stream B and private Enhanced stream C retain separate state and draw counts;
weapon callbacks draw from A at their actual firing point.

Post-movement aim remains in the unit owner's native phase. Grenade danger
warnings (`ai_world_dangers`) run after the battle tail, at the end of the
same native world tick. The host makes one call per tick; foreign observers
such as sounds and presentation receive the tick's events after it returns.

## Boundary and validation

`rust/sim/schema/ai-phase.json` generates the cold content, phase, camera,
journal, notice and counter layouts. Controller store rows provide identity
rather than a second copy of decision state. Counters expose phase calls,
input/output words, random draws, services, allocations and reallocations.

Navigation search storage is prepared with authored barriers before routing.
Released seat controls remain until their Core unbind callbacks finish;
retirement never redirects an old captured identity to a replacement. Turret
control views reserve the shared seat
slot capacity, including slots previously used by hull riders. Mounted weapon
edits dirty the active seat owner, preserving sparse publication across loans.

Scheduler and combat queries borrow compact canonical motion and policy facts.
Immutable class and weapon definitions are shared; reading a definition does
not copy runtime ammunition, clocks or spread. Seat geometry reads retained
native frames. Mounted sensor, controller, velocity and life observations keep
the full reader's qualified holder gates without resolving an unrelated rider
Character, tow state or class snapshot. Foot forward observations reuse exact angle bits until either
angle, storage generation or gameplay life changes. Non-finite angles retain
the original calculation, and capacity overflow computes without caching.
Pure weapon/range queries and post-movement aim prepare only
their observed fields; Character identity is retained for the controller's
qualified storage life. Fire consumption prepares current targeting, difficulty
and fire requests, then returns only the two requests; alert and helper state
stays retained. Fire and spread flags are prepared as one byte, and idle
burst requests do not copy unused weapon clocks. Foot tuning writes touch only
changed values, preserving the source's separate yaw clear and restore writes.
Target scoring reads its canonical objective only
when the target is a post. A body's canonical controller reference resolves
post-movement aim without a second controller map lookup. Tracker reads,
candidate cleanup and the skip marker use their
retained combat state without copying unrelated controller requests. Empty
journal intake does no combat feed. Targeted shot and damage intake coalesces
only each notified controller's latest source and last known point, then applies
the original notifyShot operation to Core. Other journal records still consume
ring capacity and advance the reader. Unrelated alert queues and policy requests
never make a control/combat round trip; deferred sight prepares only qualified
queued clients. Queue order, life validation and budgets remain unchanged.
Reused controller storage clears its sight-visit mark, including across counter
wrap. The Classic LOD calculations and observation order stay intact.

Bot publications retain the held axes, button counts and aim needed by host
observers. Their facade reads the retained generated words on demand instead
of decoding each field every tick. Consumed movement inputs and trigger latches stay native;
foreign player controls retain complete restoration. Changing control owners
restores the complete held record even when zero words equal the old cache.
Generation-stamped membership preserves first-touch order. Unchanged inputs
publish no row. A
pending full body update still takes precedence, and post-movement aim marks
only its actual weapon or intent writes before the unit phase publishes its
completed body. Lifecycle callbacks keep their immediate observation barriers.

The unit phase omits foreign rows for canonically controlled bots without
remote or network facts. Supplied rows retain source order, including passive
rider requests. A qualified slot index resolves omitted bot facts before any
body advances; body ticks retain canonical factory order. Human,
network and unbound bodies still supply explicit intent. Rust checks ownership
again before accepting an omitted request, so a stale page cannot turn missing
input into authority. Boundary counters count the words actually written.

`cargo test --locked --workspace` exercises the actual native owner with synthetic
content. The native fixtures in `rust/sim/rust/ai/owned/tests/phase.rs`,
`rust/sim/rust/ai/owned/tests/native_phase.rs` and the policy tests under
`rust/sim/tests/` cover missing services, lifecycle ordering, control,
vehicle and combat behavior. External
capture replays and world hashes supplement these fixtures; retail content
and captured battle data stay outside the repository.

The detailed AI documents describe behavioral contracts. Function names in
them are the Rust names where the port keeps one (usually the JavaScript
name in snake_case); where a game class name such as `UnitFlyAgent` appears,
it names the original game's behavior, not a runtime import path. Page weapon prediction still
uses `rust/sim (ai/random)`. Player, mounted and HUD weapon readers receive
cold cooked policies from the Rust content owner through `rust/sim` (`weapon-policy`).
The presentation page cooks its resource policies through the same Rust
content parser without constructing an AI world. Charge levels share their
definition's published policy; separate battles never share a class-name cache. These helpers never schedule bots.
