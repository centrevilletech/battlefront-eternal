# Live objects and battle ownership

`F4Live` enters `f4_live` at the live-object slot, before ordnance and items.
Stations, building regeneration, turret idle aim, animated props and traps read
canonical unit, fleet and collision state. Their immutable definitions and clip
bindings are prepared once. Animal hulls retain soldier supply rates. External
turret damage retains the hit heading and scale sample for journal readers.
A held body's collision sample reads the same grab
frame that placed its root.

`F4Tail` enters `f4_tail` after items. It runs battlefield countdowns, posts and
capture awards, destruction regions, mobile posts, battle rules, statistics,
Classic spawning, human commands and corpse removal. Classic post selection
retains its random choice; Enhanced selection can read the current objective.
The two phase entries retain their work buffers and publish only changed words.
The pre-AI viewpoint entry also refreshes mobile-post views from native hulls.
Foreign lifecycle hooks see the same current views without advancing their
reported events; occupation and destruction notices retain their tail order.

Battle and Character facades retain their identities, including nested records.
Roster positions publish from the native list before snapshot capture.
Outside score, objective and controller writes carry generation-qualified
handles. Controller commands also qualify the body's gameplay life. A foreign
AI hook sees changed mirrors before it runs, and its scalar commands publish
before control returns to the native phase. Death and ticket bookkeeping finish
before item-drop draws or another character can observe the result.

The tail calls the unit owner's prepared factory directly. Warm bodies keep
LIFO reuse; cold activation moves retained body, pose and publication buffers
into the canonical mixed registry. Admission reserves a bound for future ids
and enough class storage for padded local rosters and corpses. Local class
changes retain the viewpoint Unit; connections retain their Character and may
change pooled bodies. Streams and recon owners rebind before spawn notices.
Unit lifecycle flags publish before the spawned AI hook, including heroes.

`F4Connections` qualifies admission, spawn, unspawn, team and dead-body clock
commands by Character lifetime. Eligibility and realized bodies read the same
native posts, transport exits and unit store as the tail. Network mailboxes and
control mapping remain outside the phase. `Game.step` directly coordinates the
phase entries; it does not nest them in the older step or network driver.

The old battle switch, callback worlds and per-method adapters are removed.
JavaScript retains construction, read facades and page presentation. Animated
prop `presentStep` poses shown doors and idle clips; published creatures use
`follow` and `presentHeld`. Those page methods cannot seize, swallow, damage or
remove an authoritative body. Debris remains in the page frame loop.
Remote pages derive mobile-post presentation and announcements from replicated
hulls because their post summaries contain only placed posts.
Replica admission retains canonical id order so overlapping map markers draw
consistently when snapshot budgets deliver their bodies out of order.

Layouts come from `rust/sim/schema/f4.mjs`. Publications share the generated
counted-block and sparse-delta formats. Fixed live records send packed changed
values. Battle rows lend their retained native encoding and a generated mask;
ordinary readers decode changed fields without a second numeric row copy.
Detailed statistics retain shared scalar accessors: publication updates list
shapes and map values immediately, while the latest numeric record materializes
on observation, including through a held nested reference. Generated eager masks
avoid walking unchanged statistics tables each tick. Removal and retirement
materialize a pending old record before its native buffer can be reused.
Record `$observe` metadata names scalar leaves; the Character's `defer` field
selects its detailed record. Eager masks and scalar readers come from those same
schemas. Shared accessors preserve own-key enumeration and held-object identity.
Idle observer barriers keep their epoch and skip row scans and descriptor writes.
Boundary counters include borrowed changed words and distinguish phases,
commands, hooks, payload words and actual allocator activity. The synthetic
`f4-*.test.js` and native `f4_*` fixtures require no game files.

Human input admits the protocol's sixteen commands and clamps oversized counts
again in Rust before reading the shared buffer. Excess malformed commands do
not overwrite adjacent input. Connection slots and unavailable player stock
reject without trapping the phase; invalid team and spawn-point indices also
reject. Full team admission remains an error to the asynchronous connection
request, outside the frame loop.

Cold row and heap capacities reserve the normal map working set. They are
storage hints: an exceptional variable-record or undrained notice backlog can
grow retained buffers, as the former arrays did, preserving row identity and
notice order. Subsequent frames reuse that storage. Synthetic burst fixtures
cover large loads, pooled rebirth, paused notice delivery and oversized input.
