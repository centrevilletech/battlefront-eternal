# Goal bots

The 2005 game's modes steer bots by goals that a mission script gives each
team: find and fight enemies (Deathmatch), defend or destroy an object,
follow someone, take or defend a flag, or take command posts (Conquest).
The goal bots in `rust/sim/src/ai/goals/` carry those goals out. They are a
controller of their own beside the released bot controller, which runs
unchanged for every battle, and they act on it only through a small set of
goal operations (`rust/sim/src/ai/control/goal_orders.rs`).

## When a battle has goal bots

- Rules supply a goal table through `EventRules::ai_goals`. Rules that
  return one when they are installed get goal bots for that whole
  installation; a later `None` reads as an empty table. Rules that return
  none when installed skip goal assignment. Their living bot bodies still supply
  equipped input during the 2005 soldier phase. Every 2004 battle runs Conquest
  and never constructs them; `ai::goals::census` counts every entry into the
  goal bots, and a battle that never reaches them leaves the count at zero.
- Each mission writer receives the world's handle allocator before it executes.
  Installation reserves the prepared roster capacity and binds the admitted map's
  regions. A refused or failed installation preserves the active handler and orders.
- Replacement retires each steered bot through its controller. A reset during an
  AI loan parks the old goal controller. The next AI phase completes
  retirement before the released controller can make another decision.
- Once per rules tick, after the rules handler ran, the goal bots read the
  table, link bots to goals and steer them. A change a script makes in a
  callback is seen at the end of that tick.

## The goal table

The table is the mission's, held by the rules: the script host owns the
rows, their validation and the handle numbers, and implements `GoalTable`
as a read-only ordered view (revision, occupied length, and per row the
slot, handle, team, kind, the weight exactly as given and both raw
arguments). `GoalSlots` implements the same rules for tests. As in the 2005
game:

- One table of 20 ordered slots shared by every team. A new goal takes the
  lowest free slot; a full table refuses the call.
- Teams 0 to 7. The weight must be finite and positive and is kept exactly.
- The seven kinds are matched ignoring ASCII case. A follower follows a
  character or an object; a flag offense names its area (the flag is the
  optional second argument); Destroy, Defend and flag defense take any first
  argument; Conquest and Deathmatch ignore arguments.
- Handles start at 1 and increase through 16,777,216, the last exact handle.
  The world retains the next number across clears, hidden tables, failed
  preparations, replacement and tail reset. A writer refuses further adds
  after issuing the last handle.

## Which goal a bot carries out

Implemented, following the 2005 game:

- A bot without a goal takes the active goal of its own team with the
  lowest ratio of assigned bots to weight; the earlier slot wins a tie. The
  ratio is compared exactly (counts and weights cross-multiplied).
- Each pass first frees every link that is no longer valid and only then
  fills the vacancies, so a bot choosing a goal sees the counts its wave
  ends with.
- A bot keeps a goal that is still in the table, active and on its team.
  Adding goals or changing weights never takes a bot from a valid goal; a
  new goal fills as bots become free. Players are never assigned.
- A bot is freed when its goal is deleted or cleared, when the goal is
  suspended, when the bot dies (its next life starts without a goal), and
  when its life is on another team than the goal or on a team outside 0 to 7.
- A goal is suspended while its subject is unavailable and resumes when the
  subject returns: Destroy and Follow while the subject is not alive,
  Defend and the flag kinds while the subject cannot be found. A resumed
  goal takes newly free bots and no bot from another goal.

Engine choices: the table starts empty and goals come only from the
mission (the 2005 game's initial seeding is not settled); a bot's goal is
freed when it dies.

## How a goal is carried out

- Each bot's base order sits in the released controller's command queue: a
  wait order whose agent the goal bots steer, or, for Conquest on a map
  with command posts, the released objective order for the bot's post. A
  base order saved behind an interrupt is replaced where it sits; a running
  interrupt returns to the current base order when it ends. Interrupts the
  released controller queued (a fight, a squad order, boarding, recovery
  from being stuck) keep their place in the live queue. An event-mode publication replaces
  its pending base, or appends one behind the accepted interrupts.
- While the wait agent runs the base order, the goal bots send it a route
  destination through the released navigation helper, point its look point
  at the destination once per route, and set the direction the released wait
  agent's look picks stay within. The native controller starts fights and
  handles cover, squad orders and recovery. In a 2005 fight an equipped
  saber selects the melee engagement described below. When the fight ends,
  the bot returns to its goal.
- Mounted bots keep the released seat policies.

What each kind does:

- **Deathmatch.** With a 2005 planning graph, a Deathmatch bot runs a local
  roaming loop instead of tracking the nearest enemy across the map. Each time
  it enters its idle state (at spawn, after arriving within 2.5 m of its
  destination, after a route fails and waits 0.5 s, or on returning from
  combat), it flips an even coin between:
  1. walking to a random surveyed floor point inside another reachable
     navigation hub (falling back to a random offset within 100 m if no other
     connected hub exists), and
  2. picking a uniformly random living enemy that its weapons can engage,
     sampling that enemy's position once with up to 10 m of random horizontal
     jitter, and walking to that static point without continuously tracking
     the enemy while walking.
  While walking a Deathmatch leg, perception, sound, and damage interrupts
  start combat normally. Every 5 s on a Deathmatch walk leg, a jet- or
  force-jump-capable bot whose energy is strictly above its minimum jet fuel
  border rolls an even coin, checks 40 m of overhead clearance, and requests a
  travel jump (deferring by 5 to 15 s when energy is at or below the border).
  Without a planning graph (standalone test worlds), Deathmatch falls back to
  the route-ranked hunter below: allies hunting a target add 15 m to its
  ranking distance, confirmed walks outrank unknown pairs, each character gets
  three route questions per 60 ticks (four globally per tick), pursuit budgets
  are 6 s plus 0.5 s per metre, stalling 4 s lets a shorter route take over,
  and exhausting candidates patrols for 10 s.
- **Defend.** Guards stand on a 4 m ring around the subject, walk back when
  more than 6 m from their spot, and check that every 1.5 s once settled. A
  guard searches no roster: it fights what the released perception shows
  it and then returns, as the game's defense order does.
- **Destroy.** For a subject the released combat can attack, the bot walks
  to it and, within 30 m, attacks it through the released combat order as
  an interrupt that returns to the base order. A queue without room for the
  attack and its saved base order defers it; an attack that ended is started
  again after 2 s; a withdrawn attack leaves the queue at once, and its running combat agent exits on the change tick. The released combat order may
  end at once for a designated object (an invented building does so); the
  bots then bring it down through the fights their perception starts. For a
  subject the released combat cannot attack, the bots hunt instead.
- **Follow.** The destination is the subject's position plus half a second
  of its velocity plus a 3 m offset, recomputed every 0.5 s; the follower
  stops within 5 m of a slow subject.
- **Conquest.** With command posts, the released objective flow. Without
  them, as Deathmatch.
- **Flags.** No flag model yet: a flag offense walks to its area and then
  hunts; a flag defense defends its subject. Planned: pickup, carrying,
  dropping and returning a flag.

The released targeting never starts a fight between two units flagged as
heroes. The 2005 game adds Hero Assault's roster as ordinary classes, and a
2005 battle must not give those units the hero flag, or no fight starts.

## Subjects, names and regions

- A goal's first argument is looked up when the table changes and every 30
  passes otherwise, so a rebuilt object is found again; second arguments
  name flags, which the bots do not carry yet, and are not looked up.
- A name is looked up as exact bytes, ignoring ASCII case, in the battle's
  mission object index with the index's own comparison, and follows the
  object's placement to its current life. An exact object handle suspends across a rebuild. A saved mission prop
  follows its placement through a rebuild while its storage generation matches.
- Only a name no mission object has is looked up among the command posts
  and then among the map's named regions. A name two objects (or two posts,
  or two regions) share is ambiguous and is never found, not even as a post
  or region of that name.
- Where a subject is and whether it lives come from the object itself: a
  building's placement, a trap's point, an animated prop's position, a
  command post, a mobile post's vehicle (or its last position), a hull, a
  character. Whether the released combat can attack it is separate: a named
  station with no combat target is still defended.
- Region placements are read from the prepared 2005 form (a 16-number
  matrix whose translation is elements 12 to 14), the older 12-number
  transform (translation in elements 9 to 11, z negated) or a centre.

## Melee input

In a 2005 event battle an on-foot fighter with a melee weapon re-selects
weapons normally during combat (picking the lightsaber inside 4 m, a thrown
blade inside 4 to 40 m unless reloading during a weapon switch and never out of
band, or an eligible offhand damaging power). While pursuing a live combat target with a melee
weapon, the movement helper walks directly when clear; otherwise it issues a
routed walk, waits for that walk to end (done or failed) and issues the next at
most every 2 s (skipping repaths while airborne). It stops at 2.4 m and resumes
past 2.8 m, keeps a target that no route reaches (as the 2005 game does), and
performs periodic 45-degree combat leaps every 5 to 10 s (retrying in 4 s when
a 25 m probe is blocked, and varying between forward, back, left, and right
leaps within 5 m). Each end of the probe that stands inside a barrier first
moves to that barrier's nearest free edge, so a bot on a barrier-covered roof
can still leap off it.

Melee swing requests use the 2005 single-trigger tap cadence (one-tick pulses
separated by a skill-scaled wait of 0.2 to 2.0 s times `1 - s`). Saber guard
runs on a per-life combat schedule in Hold mode: entering combat raises guard
for 3 to 6 s, then alternates between 1.5 to 4 s lowered and 3 to 7 s raised;
a melee fire request lowers guard immediately so the swing goes through, and
guard stays down until the bot's next combat think tick re-arms it;
Hold mode re-presses cleanly whenever committed guard was broken (for example
by a stagger or jump) while guard is still wanted. Non-2005 goal tests without
a 2005 navigation graph retain the legacy fixed swing pulse and toggle guard
schedule.

The soldier callback can take one frame per living bot, life and fixed tick,
including unassigned bots. The frame identifies the rules installation, soldier
slot, slab generation, gameplay life, weapon slot, definition and channel.
A birth after the last rules pass receives a neutral frame. Mounting releases
held controls on the soldier body while the seat drives its own movement.

Logical Fire reads the trigger on the equipped blade's channel. Bit 1 carries
the separate block control. A recorded press and release within a tick gives
one Fire edge with its held bit clear. Toggle presses are spaced by at least 0.3 s. A
still-wanted, uncommitted press can be retried after one second. Hold mode
keeps the level down while wanted. After a successful coupled step the consumer
calls `World::ai_goal_feedback` with its delivered frame, committed guard,
energy and exhaustion. Guard belief follows that report. Exhaustion suppresses
a new guard request. A changed life, binding or installation invalidates feedback.

## Cost and determinism

The goal bots allocate nothing in any pass of an installed battle,
including passes where more of the admitted roster appears (measured on
invented content). Measured on invented content with every bot alive on a
shared machine, the goal pass costs about 0.025, 0.058 and 0.098 ms per
tick at 32, 64 and 128 bots, and looks at about 4, 15 and 61 enemies per
tick while bots choose whom to hunt; settled guards look at none. Opening
checks of a wave and hunting choices are spread by character index, every
walk is in character and slot order, and the goal bots take no random draw
themselves, so the same battle gives the same bits on every machine.

A navigation provider answers the goal route query with the asking soldier’s
body radius and ground traversal mask. A positive answer supplies walking
distance. An inconclusive answer leaves the target available to try.

A host with a coupled combat machine installs its soldier consumer with
`World::register_bot_melee_driver`. It receives frames during the soldier
phase's existing callback loan and returns the committed guard and mover state.
A failed coupled step can retry that tick. Frames already committed by another
soldier on the same tick are delivered once.


In a Battlefront II mission with admitted soldier poses, the goal controller
delivers each living bot's selected weapon, movement axes, fire and block
edges to its own soldier life on the following fixed tick. The movement step
returns the committed guard, energy and exhaustion before the next goal
decision. Human and bot blade windows use the same posed, solid-clipped
contact and damage functions. The native melee strike stays available to
the 2004 game.
