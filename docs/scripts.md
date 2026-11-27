# Mission Script Runtime (`rust/bfe-script`)

`bfe-script` is a pure-Rust (`#![forbid(unsafe_code)]` on both the library and
CLI binary) Lua 5.0.3 bytecode runtime for the compiled mission scripts shipped
in a player's copy of *Star Wars: Battlefront II* (2005). It has no C code and no
host `libm` / `libc` formatting dependencies, so it builds and produces identical
results on x86-64 and ARM64 across Linux, macOS, and Windows, in debug and
release builds alike.

## What is implemented

- **UCFB `.lvl` script archive reader** (`src/archive.rs`): iteratively reads
  `scr_` entries (`NAME`, `INFO`, `BODY`) directly under `ucfb` and inside
  nested `lvl_` sub-archives under cumulative `ArchiveLimits` (`max_input_bytes`,
  `max_depth`, `max_chunks`, `max_scripts`), with strict structural validation
  (`ArchiveError::MalformedChunk`) and ASCII case-insensitive lookup matching
  `ScriptCB_DoFile`.
- **Binary chunk loader and bytecode verifier** (`src/chunk.rs`,
  `src/opcode.rs`): decodes the 2005 game's Lua 5.0 bytecode header
  (little-endian, 4-byte `int`, 4-byte `size_t`, 4-byte `Instruction`, 6/8/9/9
  operand widths, 4-byte IEEE-754 `f32` numbers, test number `31415926.0`,
  `MAXSTACK = 128`, `LFIELDS_PER_FLUSH = 32`, and optional single trailing
  `0x00` byte from `BODY` chunks) under cumulative `ChunkLimits`
  (`max_input_bytes`, `max_decoded_bytes`, `max_protos`, `max_nesting`).
  `Proto` fields are crate-private and verified before execution: instruction
  boundaries (excluding `OP_CLOSURE` upvalue descriptors), reachable jump and
  skip edges, and a fixed-point pass over the open-stack state and the set of
  registers captured by open upvalues. A call may not place its function at or
  below a register that a closure of the same function still captures (code
  compiled by `luac` never does). Root prototypes with upvalues are rejected, and
  so are encodings `luac` never writes: a vararg flag other than 0 or 1, a
  negative definition line, a debug local range outside
  `0 <= startpc <= endpc <= sizecode`, and a null local name, upvalue name or
  string constant. Malformed or hostile chunks return `Err(LoadError)` without
  panicking. A `Proto` can only be made by this loader, so the virtual machine
  only runs verified code: it fetches instructions, registers and constants
  relying on the verifier's range checks, with Rust's bounds checks behind them
  (a gap in the verifier would stop the thread with a panic, never read out of
  bounds). The robustness tests and fuzzing run verified code only.
- **Virtual machine** (`src/vm.rs`, `src/value.rs`): all 35 Lua 5.0.3 opcodes,
  closures and open/closed upvalues, Lua 5.0 varargs (`arg` table with `.n`),
  checked multi-return stack adjustments, proper Lua tail calls, `OP_TFORPREP`
  table-shorthand compatibility, string/number coercions, the Lua 5.0 tag
  methods (`__index`, `__newindex`, `__eq`, `__add`, `__sub`, `__mul`, `__div`,
  `__pow`, `__unm`, `__lt`, `__le`, `__concat`, `__call`, `__tostring`,
  `__metatable`, `__fenv`, `__mode`; `__gc` is never called, see the list of
  differences), and protected calls (`pcall`/`xpcall`/`error`) with unified
  Lua/native call-chain tracking, raw byte error objects, `getobjname`
  diagnostics, byte-exact `luaO_chunkid` error locations, and pre-unwind
  `xpcall` error handlers. Lua-to-Lua calls do not recurse on the native stack.
- **Generational handles** (`src/value.rs`): `StringId`, `TableId`, `ClosureId`,
  `NativeFnId`, `UserdataId` and `ValueRef` carry a slot index, generation
  counter, and VM identifier, so stale, swept, or foreign-VM handles, and during a
  sweep handles to objects the finished mark did not reach, are rejected with
  `ScriptError::InvalidHandle` (or `None`/`false`) on every host entry point, by
  every getter, and on the native-function side (`CallCtx::push_return`,
  `call_into`, `pcall_into`, `set_upvalue`). A handle is not a root: see the host
  interface summary.
- **Standard libraries** (`src/stdlib/`):
  - `base`: `_G`, `_VERSION`, `assert`, `collectgarbage`, `error`, `gcinfo`,
    `getfenv`, `getmetatable`, `ipairs`, `next`, `pairs`, `pcall`, `print`,
    `rawequal`, `rawget`, `rawset`, `setfenv`, `setmetatable`, `tonumber`,
    `tostring`, `type`, `unpack`, `xpcall`.
  - `table`: `concat`, `foreach`, `foreachi`, `getn`, `setn`, `insert`,
    `remove`, `sort`.
  - `string`: `byte`, `char`, `find`, `format`, `gfind`, `gsub`, `len`,
    `lower`, `rep`, `sub`, `upper` (including the Lua 5.0 pattern matcher with
    length-aware vs C-string NUL handling matching `lstrlib.c`).
  - `math`: `abs`, `acos`, `asin`, `atan`, `atan2`, `ceil`, `cos`, `deg`,
    `exp`, `floor`, `frexp`, `ldexp`, `log`, `log10`, `max`, `min`, `mod`,
    `pi`, `pow` (and global `__pow`, using C99/Lua 5.0 special cases), `rad`,
    `random`, `randomseed`, `sin`, `sqrt`, `tan`.
  - **Intentionally omitted**: `coroutine`, `newproxy`, `string.dump`, `io`,
    `os`, `debug`, `loadfile`, `dofile`, `loadstring`, `loadlib` and `require`,
    and the `_LOADED` table the reference's startup creates. They are left out to
    keep mission execution deterministic, pure-bytecode, and sandboxed; no stock
    2005 script depends on them. Guarded checks such as `if coroutine then` see
    `nil`.
- **Host interface** (`src/vm.rs`): generic `Vm<H = ()>` passing `&mut H` into
  native callbacks (`CallCtx<'_, H>`), calling into a reusable result buffer
  without host allocation on warm paths (`call_into`, see the determinism rules), native function and table registration (`register_fn`,
  `register_closure`, `register_table`), GC-rooted host value references
  (`create_ref`, `get_ref`, `release_ref`), full collection on request
  (`collect_garbage`), the collector's counters (`collector_stats`), the work
  used by the last call (`work_used`), per-call budgets
  (`set_instruction_budget`), table inspection and mutation from Rust,
  host-supplied `ScriptRng`, and configurable `OutputSink`.
- **Recording host and CLI tool** (`src/recorder.rs`,
  `src/bin/bfe-script-tool.rs`): inspects `.lvl` archives and records mission
  setup calls under a named `ImportProfile` into a lossless JSON manifest
  (schema 4) for the preparation tools. The host functions it binds are derived
  from the archive's own scripts at preparation time; the crate ships no list of
  game function names.

## Simulation mission host

A developer authority admits a resolved 2005 family, map, era, mode and
match seed through `Context::world_admit_mission` or
`Context::world_network_admit_mission`. The player availability gate stays
closed. All authority inputs come from the selected prepared family root.
The authority checks the selection, map record, manifest, body
lengths, digests and bytecode before executing the entry. Includes run at
their Lua call sites. Setup uses the match seed once. Its ordered load trace
must agree with the prepared manifest. The load phases share 10,000,000 work units through post-load. All callbacks in one
authority tick share 30,000. The journal retains the selection,
preferences and construction rows. Replay checks its admission digest and compares
the ordered construction rows bit for bit, including NaN payloads and signed
zeros, before returning the new authority. The native local
launcher and dedicated world worker take the same selection. The developer playlist entry
takes the resolved family, map, era and mode directly from its slot. An empty
dedicated worker can restart that selection or accept the next resolved
rotation entry. Either operation admits a new seeded VM before replacing
the finished match. The developer admission digest covers the selection and
exact files read, including the manifest and each verified script body. Its
`bfe-admission-sha256-` prefix identifies it to local replay.
The worker advertises the native content loader's gameplay fingerprint for the
session family, map and mission. It reads the prepared files again at restart
and rotation. Direct and listed clients compute that fingerprint from their own
prepared files for each join. The developer scene
prepares the selected map and its placements. First-person models, HUD and
holograms need the corresponding 2005 presentation resources. [Prepared mission format](formats.md)

The script declares each team's membership, class limits and population.
Each call to `AddUnitClass` appends a slot to that team's roster in script order.
Slot zero supplies the default player class. Spare bot places go to slots
in the same order, within the script's limits and the admission bot preference.
`ScriptCB_GetASSNumBots` reads that preference. `ScriptCB_SetNumBots`
records the requested value for inspection. Admission sets the fielded
population. A roster, score or ticket write for a team outside the playable
range of 1 through 7 returns false and records the operation. Spawns use
integer 60 Hz ticks, a four-second personal delay after death and separate
team release times. A ready solo human skips the team wave. Network
admission selects a 15-second period and retains the declared stagger.
Mission admission passes the script's flight heights into the prepared
movement inputs. Spawned soldiers use the mission ceiling.
An ordinary Hero Assault class keeps its health while idle. A human in a
designated hero slot loses ten health per second. An early credited enemy
soldier kill restores up to 150 health, capped at the class maximum.
Hero Assault rosters use ordinary class slots for both teams. The mission
starts with an empty goal table and publishes ordered goals after setup.
The world supplies the handle allocator before the entry chunk runs.
Replacing or clearing goals and retrying installation preserve its cursor.
A refused installation retains the prepared mission for the next attempt.
2004 Conquest uses its released spawn clock.

`ScriptPostLoad` runs against admitted native objects before spawning
begins. A landing region declared during setup is registered when the
space map loads. Later calls to `AddLandingRegion` register the named
gameplay box immediately and return false if it is unavailable.
The host resolves names with ASCII case folding and delivers
subscribed hit, lethal transition and rebuilt-life events by object
identity. A building's current shield, maximum and recharge rate are live
values. Charge, capacity and recharge are written independently. Setup may set
charge before capacity. The recharge step clamps its result to capacity.
Shield charge absorbs damage and the excess reaches hull health. Native hits
apply immediately. The rules receive one damage notification per object life
in a batch, in initial-hit order and with that impact's credit. Kill and rebuild
close the interval and retain their own positions. A damage callback that
copies charge to linked objects preserves their outstanding native damage.
The mission observes depletion before recharge advances. This coalescing
keeps concentrated fire within the fixed callback and command budgets.
The 4,096-command allowance covers simultaneous linked-section updates, kill
awards and repairs. Diagnostics expose peak commands per tick, count every
command-budget refusal and retain a warning when the limit is reached.
A saved userdata retains the original object's generation and life.

Soldier and system credit carries the attacker's captured team and human
status. The authority stores signed personal points and team totals. Human
friendly fire debits the team down to zero. Each explicit critical-system
award uses the matching impact. A linked destruction without an attacker
carries no credit. The registry holds up to 32 named systems with ordered
HUD links. Both Assault APIs read the same target, which defaults to 180.
An empty craft designated by its fleet owner awards one point to its enemy.
An unassigned or friendly empty craft gives zero.

Object commands return true when applied and false when refused.
`GetObjectTeam` and `GetCharacterTeam` return zero on an unresolved
object or character. Neutral ownership has the same numeric result.
`GetTeamPoints` and `GetReinforcementCount` also use zero for an
unavailable total. These fallbacks are provisional.
`GetObjectShield` returns current charge, capacity and recharge for a
supported building. It returns nil when the object or its shield state
is unavailable. Lock-on,
AI damage thresholds, barrier toggles, planning-arc changes and
landing/death-region declarations return false and increment a capability
counter. Platform memory and rendering hints return `"ignored"` and have
their own counter. Camera, music, team icon and presentation declarations
are available to consumers as records. Local preferences and voice cue
durations are supplied by the host.

An authority timer armed during setup starts on the initial rules tick. Four
seconds occupies 240 ticks. Stopping, resetting or destroying a timer
invalidates queued listeners before the next call. The callback cap
retains the event and listener cursor for the next tick.

A script error or a call-depth limit abandons that invocation. Completed writes
stand and dispatch continues at the next listener in the same tick. The host
logs one failure per chunk, function and instruction and counts repeat
occurrences, including in stripped bytecode. A failure before a Lua frame
exists uses the registration and error kind. Messages are bounded to 512
bytes, and failures beyond the retained site capacity have a separate counter.

All callbacks of a tick run in one VM call on the authority thread. Before
it, on every active rules tick after the load and post-load phases, quiet ticks
included, the host calls `Vm::begin_tick` with the authority tick number. That
call does the tick's paced collection and opens the tick's 60,000-unit
collector balance (see Collector). The tick shares 30,000 VM work units and 2,048
native operations. Each registration remembers the greatest VM and native
costs of its completed calls. The host defers an invocation when either cost
exceeds the remaining allowance. A new registration gets at least half of each
allowance on its first call. Deferred delivery resumes at the same listener
before new events on the next tick. If a more expensive path exhausts a shared
allowance, its partial effects stand and the registration gets a fresh tick on
its next invocation. Only exhausting an entire allowance on its own earns a
strike. A callback refused at the VM memory limit is abandoned with a memory
diagnostic and earns a strike. Its completed writes stand. The rest of that
tick's deliveries wait for the next tick, whose collection runs before them.
Three consecutive strikes disable the registration. A completed invocation or
ordinary error resets the streak. A refusal before entering the dispatcher
defers delivery.

Scores and finite reinforcement counts retain their individual notifications
for a whole batch of accepted commands. The host also reserves the native
producer's full event bound for physical events and timers. If score changes
accumulate across ticks, it coalesces undelivered absolute values to the last
points and ticket count for each of eight teams, in producer order. The
in-flight listener cursor retains its original event. When the physical queue
fills, the host drops each incoming event that does not fit. This preserves
the order of events already accepted, including partially delivered events.
One diagnostic identifies the tick when delivery began losing events and the standings deadline.
Its occurrence count tracks all lost events.

An undecided healthy match has no duration limit. When a primary team first
reaches the declared points target, a 1,800-tick grace period starts. At expiry
the host uses the current scores. If either primary team is at or above the
target, the higher score wins and equal scores draw. If both are below, a new
grace period begins the next time a primary team reaches the target.

A queue overflow, a disabled listener, or an abandoned timer or victory callback starts one
36,000-tick deadline. Victory callbacks include a primary team's target-score
or zero-ticket notification and a callback that attempts a result. The log
names the cause and due tick once. Later failures leave that tick fixed.
At expiry the higher score wins and equal scores draw. A scripted result on
the due tick takes precedence, followed by the score-target decision. All
times use the authority's 60 Hz rules ticks.

An error in the mission chunk, ScriptPreInit, ScriptInit or ScriptPostLoad
immediately ends setup as a draw, even if the script already awarded points or
called a result. The authority logs the mission, phase and failing instruction.
Local Instant Action returns to the menu with the selected map and mode names
from the prepared language table. A dedicated scripted rotation visits each candidate once in order.
A failed post-load publishes its draw before the next service tick advances
the rotation. After a fully failed pass the worker logs the reason once, stays
available for status, and declines joins with that reason. A successful start resets the
pass for the next battle. A 2004 Conquest battle runs without this host.

The VM registry keeps the load, post-load and tick entry points alive. It also
holds subscribed callbacks, cached object userdata and cached message labels.
Evicting a label releases its registry entry.

An invalid runtime state or an exhausted host capacity (mission characters,
object bindings or the userdata reservation) stops script execution. A
committed result stands. Otherwise the primary team that
reached the target earliest wins. Below the target, the team ahead on
points wins and equal scores produce a draw. If the world temporarily refuses
the result, the host retains it and retries on the next active tick.

A network battle's authority sends its clients the mission's state, systems,
notices and roster from this view (`docs/network-two-games.md`, "Mission
records"). Its players deploy from the same population as its bots: a
connection's choice only marks its Character ready
(`battle/f4_connections/mission.rs`), and the mission's waves, slots, class
limits and tickets decide the rest (`battle/spawning/mission.rs`).

`World::mission_view` borrows the host, rules and native battle at one
observation. The host supplies ordered objectives and their text keys,
registered systems and HUD links, score visibility, named timers and the
accepted result. The rules supply team points, score target, personal points
and the object registry. The native battle supplies reinforcements and end
phases. `timer_due(timer_id, observed_tick)` gives the absolute authority
tick of an armed timer. A paused or missing timer has no deadline.
`shown_timer(observed_tick)` gives the timer the script shows on the HUD: its
deadline while it runs (none once the match is over) and the ticks it has left,
running or stopped, which the HUD holds on screen while it stands still.

At join or resync, read this state and `host.last_notification_sequence()`
together. Pass the last delivered sequence, zero before any notification,
to `host.notifications_after(last_delivered)`. An up-to-date cursor gives
an empty iterator. A gap or future cursor requests a fresh snapshot. The
retained entries carry their authority tick, sequence, payload and script
audience. Transports filtering team messages must carry a watermark or
assign recipient sequences. A reader can advance independently of rendering.

Space systems resolve through `mission_system`. State, shielding and ownership
operations check the current life. A fleet owner can classify an admitted
vehicle with `set_mission_vehicle_kind` and identify an eligible empty craft
with `set_mission_empty_vehicle_eligible`. Capture attribution with
`mission_credit` and submit transitions for its additional native systems
with `mission_object_event`. Native soldiers, buildings and fleet hulls
already submit their own transitions.

For replay comparison, `World::mission_digest` covers the installed script
and VM, queued deliveries, native timers, spawn clocks, personal and team
scores, current named lives, building shielding and attribution, and the
role and health of each spawned character. `ScriptRules::authoritative_digest` covers
the VM and host in isolation. `HostState::published_digest` covers the
display state. Replay tools can also hash the published battle summary and
world stream. The checksums are version-local.

Authority timers use simulation time. A four-second arm expires after 240 fixed
ticks. A stop, reset or deletion invalidates the remaining callbacks for that
expiry. Initial ticket, score and timer
writes refused by the world appear in the host diagnostics.

## What is planned (not yet implemented)

- Applying mission goals in the AI controller and admitting the full spacecraft fleet.
- Binding additional host functions required by later 2005 modes (Conquest
  bleed timers and markers, CTF flag/region callbacks, and Ground Assault
  object-class kills).

## Safety limits

Every top-level host call enforces `VmLimits`:

| Limit | Default | When exceeded |
| --- | --- | --- |
| `instruction_budget` (work units per top-level call) | `VmLimits::LOAD_BUDGET` = 10,000,000 | `ScriptError::InstructionLimit` |
| `max_call_depth` (Lua and native frames) | 4,096 (Lua 5.0.3's `LUA_MAXCALLS`) | `ScriptError::StackOverflow` |
| `max_stack_slots` (16-byte value slots) | 32,768 | `ScriptError::StackOverflow` |
| `memory_limit` (accounted bytes) | 16 MiB | `ScriptError::MemoryLimit` |

Two fixed limits protect the native stack: nested calls (a native function, a
metamethod or an iterator calling Lua or another native function) may nest 64
deep, and the pattern matcher may recurse 200 deep; both raise
`ScriptError::StackOverflow`.

These errors are host safety boundaries: Lua `pcall`/`xpcall` cannot catch them,
and they unwind the whole host call. `Vm::with_limits` and `Vm::set_limits`
return `Err(ScriptError::InvalidConfig(_))` for limits that cannot hold the base
VM state.

### Work budget

A work unit is one bytecode instruction. Everything else a call makes the VM do
for the script is charged in proportion to the work it does: library functions
(one unit per call plus one per 16 bytes they scan, copy, compare or build, and
per element, entry or candidate position they visit), pattern matching steps and
backtracking, table growth, migration of keys into the array part and
compaction, `next` scans over deleted entries, metamethod chains and inserting a
`__call` metamethod, call setup, tail-call argument moves and result copying,
opening and closing upvalues, string interning and comparison, number
formatting, building error messages (names and messages by their length), and
loading a chunk from a native function (decoding by the bytes read, verifying by
the steps taken). Each of these is charged before the work it pays for (loading
stops as soon as its work reaches what the call has left), so a call does not do
work its budget cannot pay for. Three kinds of work are not charged to the call:
unwinding after an error (closing upvalues and dropping frames, which must
finish whatever is left of the budget); the host API's table walks
(`table_next`, `table_entries`, `table_getn`), which a native function that walks
a large table inside a call should charge itself with `CallCtx::charge_work`; and
collection, which has an allowance of its own (see Collector). Growing the
string intern index or an object arena is paid for in advance, by the charge for
each new object, rather than when it happens.

The recommended budgets:

- `VmLimits::LOAD_BUDGET` (10,000,000) for loading a mission: running its
  chunks and the setup calls that follow. Run under a harness that answers every
  undefined global call with nothing and charges each included chunk's decoding,
  the 138 entries of the stock `mission.lvl` fit a 9,454-unit allowance for
  each phase (the chunk, `ScriptPreInit`, `ScriptInit`). The largest phase
  uses 9,452 units and the largest sum of all three uses 11,261.
- `VmLimits::TICK_BUDGET` (30,000) for the script callbacks of one simulation
  tick, shared by the callbacks that tick dispatches (set each call's budget with
  `Vm::set_instruction_budget` to what is left of the tick's allowance).

`tests/hostile_bounds.rs` runs twenty-six hostile programs (backtracking, plain
search, class scans, sorting with a Lua comparator, metamethod chains, deleted
entry scans, deep recursion, garbage churn, unbounded growth of tables, strings,
closures and output, pointer chasing, searches and substitutions with very long
patterns, `%.Ns` of a huge string, long separators and error messages, lookups of
deep stack levels, collections over long `__mode` strings) and checks in release
builds that each stops within 3 s under the load budget and within 50 ms under the
tick budget.

### Memory accounting

Every allocation the VM or a library function makes for a script is charged
before it is made, at the size a 64-bit build allocates, against the memory the
VM has left: each object, each arena's slot buffer at its capacity (slots are
reused but never returned, so their bytes stay charged), container capacity
rather than length, allocator block rounding, the string intern index at its
bucket count, table key index nodes, the operand stack, call frames and call
trace (allocated at their configured maximum and resized with `Vm::set_limits`),
host references and the open-upvalue list at their capacity, room reserved with
`Vm::reserve_userdata`, captured `print` output, and temporary library buffers. When a buffer grows, its new block is
charged beside the old one before the copy. Loading a chunk charges the decoded
prototypes at the sizes the heap charges once they are loaded, and the
verifier's scratch while it runs (about 44 bytes per instruction), before each
allocation; the decoder gets only the memory the VM has left. The sizes are
fixed constants, not `size_of`, so the accounting, collection timing and the
memory limit are identical on every target; a unit test checks that the
constants bound the real layouts.

Temporary roots, unknown-global name tags and retained concatenation storage are
charged at capacity. Each native library frame holds its scratch reservation
until return or error, including while it invokes nested Lua functions. Weak
tables link through their own slots. The collector reserves space for deferred
destruction when a table grows a large index or a prototype tree is admitted.
The embedding owns its input, result vectors and any buffers it adopts.

Measured with a counting allocator: on the 26 hostile programs at a 4 MiB limit
the whole process heap peaked at 0.95 times the limit (both architectures);
loading a valid 32,768-statement chunk peaks at 1.5 MB of real allocation, all of
it inside the room the limit leaves, and fails for memory before allocating when
the room is smaller (`tests/load_bounds.rs`); changing the limits and reserving
userdata change the reported figure by what they really allocate. These bound
the inputs measured, not every input.

### Collector

The collector scans roots, traces references, clears weak entries and sweeps
in pieces of at most 8,192 units. A root or entry scan resumes at its next slot.
Writes shade values that move behind the root cursor. A swept string unlinks
directly from its hash bucket. A freed slot changes generation before reuse.
Interning equal bytes during sweep can revive a string until its slot is freed.

**Collection never consumes a script work unit.** It runs in one of two modes.
Before the host ever calls `Vm::begin_tick` (loading a mission, standalone
tools), steps are paid for by allocation debt within the running
call's allowance of two collector units per unit of its work budget, an explicit
request the allowance cannot cover waits until between calls, and an allocation
that would exceed the limit first completes a full collection. The next
incremental cycle then starts at twice the previous live size or after half of
the remaining room is used, with a 64 KiB minimum.

During simulation the host calls `Vm::begin_tick(tick_number)` once per
simulation tick, outside any call and before dispatching callbacks, including on
quiet ticks. Every call then runs to completion, to its work budget or to an
error on the caller's thread, and takes no incremental steps. Each tick has
**60,000 collector units**, shared by the work `begin_tick` does, explicit
collections and every rescue of a reservation that would exceed the hard limit,
including host allocations between callbacks. Repeating the same tick number
does nothing. A lower tick number, or a call from inside a callback, returns
`ScriptError::InvalidConfig`.

Pacing. Occupancy is the whole memory ledger (`Vm::memory_bytes`): objects,
every arena's slot buffer at its capacity (the inline closure arena included),
the string intern index, the stack, frames and call trace, host references,
temporary roots, scratch and the collector's work lists. Room is the memory
limit minus occupancy. Allocation traffic counts every charge to the ledger plus
the slot of each new object, including a slot reused from reserved capacity. A
cycle starts when the traffic since the previous cycle began reaches a quarter of
the room or 256 KiB, whichever is smaller (on entering simulation the count
starts at the occupancy). While a cycle runs, each tick grants the full traversal
bound divided by the number of ticks in which the previous tick's traffic would
use another quarter of the room, and at least one unit per four bytes of that
traffic, never less than 256 or more than 60,000 units. At the observed rate a
cycle therefore ends before that further quarter of the room is used. An
explicit full collection, a rescue, or a full cycle in progress takes the whole
60,000 units. The schedule depends only on ledger values and tick numbers.

Headroom. A call never waits for collection. Once the tick's balance is spent,
allocations that fit proceed and the debt carries to later ticks. An allocation
that would exceed the hard limit spends what is left of the tick's balance on
the cycle in progress or a full collection, then retries. If it still does not
fit, the call returns `ScriptError::MemoryLimit`: its earlier effects stand, the
VM stays usable, and later ticks keep collecting. During simulation a full
collection clears weak tables in slices. While it does, every value written to
the stack or into a table is marked, so a value taken from a weak table cannot
be freed while a script holds it.

`tests/collector.rs` checks exact-cost calls, the 32,768- and 131,072-entry
tables, large root sets, inline captures and allocation churn under a 4 MiB
limit, a busy mission whose live table is half of its limit, weak tables
mutated while full collections clear them across ticks, and a refused callback
with recovery on later ticks. `Vm::collector_stats()` reports deterministic work.

### Native stack

Lua-to-Lua calls use the VM's own stack, so call depth costs no native stack.
What does is nesting: nested calls (at most 64) and pattern matching (at most 200
levels). The deepest nesting measured (60 nested `gsub` callbacks with a 190-deep
pattern match at the bottom, close to the most the limits allow) needs 384 KiB of
thread stack in release builds and 2 MiB in debug builds. A thread that runs scripts should have at
least 512 KiB in release builds (Rust's default 2 MiB thread is enough) and
4 MiB in debug builds. The dedicated world thread reserves 4 MiB. Client admission
and simulation threads reserve 8 MiB. Cargo sets the 4 MiB test-thread default
when launched from the repository root or inside the Rust workspace.

## Determinism rules and fixed platform policies

The simulation runs identically on x86-64 and ARM64 across Linux, macOS, and
Windows. `bfe-script` enforces the following invariants:

1. **IEEE-754 single-precision arithmetic (`f32`) with canonical NaNs**: `+`,
   `-`, `*`, `/`, unary `-`, comparisons, `abs`, `floor`, `ceil`, and `sqrt`
   execute as standard IEEE-754 `f32` operations without fused multiply-add.
   Every NaN entering VM state (constants, arithmetic, transcendentals, string
   parsing, native results, host inputs) becomes the quiet NaN `0x7FC0_0000`.
   The selection is done on the integer bits, which the optimizer cannot fold
   away, so release builds give the same bits as debug builds;
   `tests/lua/21_exceptional_arithmetic.lua` checks every NaN, infinity,
   negative-zero and subnormal path in both.
2. **No host `libm` transcendentals**: `math.sin`, `math.cos`, `math.tan`,
   `math.asin`, `math.acos`, `math.atan`, `math.atan2`, `math.exp`, `math.log`,
   `math.log10`, `math.pow`, and the `^` operator widen to `f64`, call the
   workspace `v8math` crate (`v8math::glibc_pow` for C99/Lua 5.0 `pow` special
   cases), and round once to `f32`. `math.mod`, `math.frexp`, and `math.ldexp`
   are implemented with IEEE-754 integer bit operations.
3. **No host `libc` number formatting, parsing, or locale dependence**:
   - `tostring(number)` (`%.14g`) and every `string.format` conversion use exact
     decimal conversion in `src/numfmt.rs` (round half to even); `tostring` and
     concatenation write into a fixed buffer on the stack. NaN formats as `nan`
     (or `NAN`).
   - `tonumber(s)` and string-to-number coercion follow C99 `strtod` (decimal
     and hexadecimal floats, `inf`/`infinity`, `nan`/`nan(...)`), converted
     through `double` to `float` as the reference build does.
   - C integer conversions follow the x86-64 reference build of Lua 5.0.3, where
     `long` has 64 bits: `(int)` conversions (`luaL_checkint`, `%d`, `%i`, `%c`,
     counts, levels, bases) truncate values in `[-2^31, 2^31)` and give
     `-2147483648` for NaN and everything outside; string positions
     (`string.sub`, `string.byte`, `string.find`) are a 64-bit `long` with the
     same rule at `2^63`; `%u`, `%o`, `%x` and `%X` take the low 32 bits of the
     64-bit conversion; `tonumber(s, base)` is a 64-bit `strtoul`. Defaults
     that the C code passes through `lua_Number` (`table.remove`'s position,
     `gsub`'s count) are rounded the same way.
   - String comparisons, case conversion, and pattern character classes use
     byte order and ASCII C-locale rules rather than `strcoll` / `ctype`.
4. **Defined table iteration order**: every table stores contiguous 1-based
   integer keys in an array part and all other keys in an insertion-ordered
   vector indexed by a `BTreeMap` over canonicalized keys. `next` and `pairs`
   yield array keys in ascending order followed by the other keys in insertion
   order; a deleted key leaves a tombstone for traversals in progress, and a key
   set again after deletion goes to the end, whether or not a collection ran in
   between. The only hash map in the crate is the string intern index, which is
   used for lookups and never iterated.
5. **No host addresses in values**: `tostring` of tables, functions, and
   userdata prints a deterministic id from one VM counter
   (`table: 0x0000002a`) rather than an address.
6. **Host-supplied RNG**: `math.random` and `math.randomseed` draw only from the
   `ScriptRng` installed on the VM (by default a fixed-seed SplitMix64).
7. **No host allocation on warm paths**: the operand stack, frames, collector
   lists and `call_into` output buffer are kept across calls, numbers format
   into a stack buffer, concatenation reuses one buffer, and a closure keeps up
   to five captured variables inline. Measured with a counting allocator over
   warmed calls, these allocate nothing: `return x + 1`, a one-argument `pcall`,
   an eight-element sort with a Lua comparator, `gfind` over 64 to 256 bytes,
   building a string that already exists, creating a closure that captures a
   variable, a simulation tick (`begin_tick` and a capturing callback) and the
   state digest (`tests/alloc_bench.rs`); so do the script host's 18 warm shield
   callbacks. Creating a string, table or closure that
   needs new storage allocates, and so does growing an arena, the intern index
   or a table.

## Intentional differences from Lua 5.0.3

Everything not listed here matches the reference interpreter byte for byte on
the differential programs (see Testing).

- **NaN text**: every NaN is the positive quiet NaN, so `tostring(0/0)` gives
  `nan` where the x86-64 reference prints `-nan`, and `%+f` / `% f` give `+nan`
  / ` nan`.
- **Table traversal order**: insertion order as described above; the reference
  follows its hash layout, and may reuse a deleted key's slot.
- **Object ids**: `tostring` of tables, functions and userdata prints a
  counter, not an address.
- **Random numbers**: from the host's `ScriptRng`, not the C library's `rand`.
- **Omitted functions**: `coroutine`, `newproxy`, `string.dump`, `io`, `os`,
  `debug`, `loadfile`, `dofile`, `loadstring`, `loadlib`, `require`, and the
  `_LOADED` table (in the reference, `_LOADED` is a table and `loadlib` a
  function after startup; here both are `nil`). Only compiled chunks run; there
  is no source compiler.
- **No finalizers**: `__gc` metamethods are never called. No stock mission
  script uses them, and without `newproxy` a script cannot create userdata.
- **Weak tables** are cleared only by full collections (`collectgarbage()` or a
  collection forced by the memory limit), not by incremental cycles. During
  simulation a full collection runs in slices across ticks, and before
  simulation a request too large for the call's collector allowance waits until
  between calls (see Collector), so weak entries can be cleared later than in the
  reference, which always collects at once.
- **Collector figures**: `gcinfo()` and `collectgarbage(limit)` work in KiB of
  bfe-script's own accounting, which differs from the reference allocator's.
- **Resource limits cannot be caught**: running out of work budget, memory
  (the reference raises a catchable "not enough memory"), call depth (4,096;
  the reference raises a catchable "stack overflow" at about the same depth),
  operand stack slots (32,768; the reference has no limit), nested calls (64;
  the reference raises a catchable "C stack overflow" at 200) or pattern
  recursion (200; the reference has no limit) ends the host call with a
  `ScriptError`.
- **A pattern ending in `%`** raises `malformed pattern (ends with `%')` with
  its location. Lua 5.0.3 formats that message through `%'`, which its
  formatter does not know, and prints whatever value lies below it on the C
  stack in place of the text.
- **Verified bytecode**: the loader rejects bytecode that `luac` never emits and
  the reference would run unchecked (out-of-range registers, constants or
  upvalues, jumps into upvalue descriptors, calls at or below registers that an
  open upvalue captures, root chunks with upvalues, and so on).
- **Platform conversions**: where 32-bit Windows (the 2005 game's own build)
  and the x86-64 reference differ, bfe-script follows the x86-64 reference:
  string positions beyond `2^31`, `tonumber` with a base for values beyond
  `2^32`, and the `strtod` grammar (hexadecimal floats, `inf`, `nan`). What the
  game's own build does in these cases has not been settled.

## Host interface summary

```rust
use bfe_script::{ScriptRng, Value, ValueRef, Vm, VmLimits};

struct BattleHost {
    callbacks: Vec<ValueRef>,
}

let mut host = BattleHost { callbacks: Vec::new() };
let mut vm: Vm<BattleHost> = Vm::with_limits(VmLimits::default())?;
vm.open_stdlib()?;

// Register a native function.
vm.register_fn("OnObjectKillName", |ctx| {
    let cb = ctx.check_function(0)?;
    let vref = ctx.vm_mut().create_ref(cb)?;
    ctx.host_mut().callbacks.push(vref);
    Ok(())
})?;

// Load a compiled chunk and invoke its main body into a reusable results buffer.
let mut results = Vec::new();
let chunk = vm.load_chunk_bytes(&bytes)?;
vm.call_into(&mut host, Value::Closure(chunk), &[], &mut results)?;

// During play, once per simulation tick and before any callback, let the
// collector do its paced share of work.
vm.begin_tick(tick)?;
// Give the tick's callbacks the tick budget, then invoke a pinned callback with
// zero host allocations.
vm.set_instruction_budget(VmLimits::TICK_BUDGET);
if let Some(cb_val) = vm.get_ref(host.callbacks[0]) {
    vm.call_into(&mut host, cb_val, &[Value::Number(1.0)], &mut results)?;
}
```

A handle (`TableId`, `NativeFnId`, `Value`, ...) is not a root: the object lives
only while the VM can reach it. Keep anything the host uses later rooted with
`Vm::create_ref` (as `callbacks` above), a global or a table, including a native
function the host calls itself, such as a dispatch trampoline. For a persistent native entry point, `create_rooted_native_fn` returns
the callable handle and its root together. Release the root when the entry
point is retired. Collection counts discarded, never-published natives in
`collector_stats().discarded_host_natives`. Calling a freed handle returns
`ScriptError::InvalidHandle` in every build.

`Vm::collect_garbage` never fails. During simulation it uses at most what is
left of the tick's collector balance, and the rest continues on later ticks (see
Collector). `InstructionLimit` and `StackOverflow` are never caused by
collection. During simulation `MemoryLimit` means the hard limit was reached
after the tick's collector balance was spent: the call's earlier effects stand,
the VM stays usable, and a later call can succeed once later ticks have
reclaimed memory.

## Recording tool (`bfe-script-tool`) and manifest contract

`bfe-script-tool` is built as a binary in `rust/bfe-script`:

```text
bfe-script-tool list <archive.lvl>
bfe-script-tool manifest [--global NAME=VALUE]... [--answer NAME=VALUE]... <mission.lvl> <script>
```

- `list` prints every `scr_` script name in `<archive.lvl>`, one per line, in
  archive order.
- `manifest` loads `<script>` (case-insensitive match) from `<mission.lvl>`,
  configures the `"pc-fresh"` `ImportProfile`, runs the chunk, and then runs
  `ScriptPreInit` and `ScriptInit` if defined. `ScriptPostLoad` is never called.
- `--global NAME=VALUE` sets a global before the chunk runs, and
  `--answer NAME=VALUE` binds `NAME` as a host function that records its call
  and returns `VALUE` (so the call is not listed under `unresolved_queries`).
  `VALUE` is `nil`, `true`, `false`, a number, or any other text as a string.
  The library takes the same answers as `RecorderOptions::answers`
  (`ProfileAnswer::Global` and `ProfileAnswer::Call`).

The `"pc-fresh"` profile answers `OpenAudioStream(...) -> 1, 2, ...` (the public
API documentation describes it returning a handle that later calls use) and
serves `ScriptCB_DoFile` from the archive. These are the only game names the
crate knows. The preparation tool supplies any other value a mission's setup
depends on, such as a platform name or a saved setup, as answers; a
value-returning host call nothing answers is recorded with no results and listed
under `unresolved_queries`, so the preparation tool can see what it did not
supply. Every other host function is derived from the player's own archive
when the manifest is recorded (`archive_host_functions`): a global that some
script of the archive calls (directly, through a local, through `_G.Name` or a
constant `rawget(_G, "Name")`, or through a table it was stored in) without
testing it first, and that no script assigns. Each is bound as a callable
function that records its call; the shipped game's scripts run those calls
unguarded, so its host provides them. A name the scripts only call behind an
existence test (`if Name then Name() end`) stays `nil` unless the caller lists
it in `RecorderOptions::extra_host_functions` (a host that implements it should),
and the manifest lists it under `probed_globals`. The derivation reads registers
in instruction order and cannot see names built at run time. An archive member
that does not load (for example one that fails verification) is left out of the
derivation and listed under `skipped_scripts`, with the error; a preparation
tool recording many entries from one archive scans it once with `scan_archive`
and passes the scan in `RecorderOptions::scan`.

### Manifest JSON shape (schema version 4)

Preparation tools parse stdout as a single UTF-8 JSON object:

```json
{
  "schema": 4,
  "profile": "pc-fresh",
  "script": "<canonical script name>",
  "loaded_scripts": ["<entry script>", "..."],
  "phases": ["chunk", "ScriptPreInit", "ScriptInit"],
  "unresolved_queries": [],
  "probed_globals": [],
  "skipped_scripts": [],
  "calls": [
    {
      "phase": "ScriptInit",
      "name": "ExampleHostCall",
      "args": ["SIDE\\example.lvl", "example_unit_a"]
    }
  ]
}
```

Field definitions:

- `schema` (`integer`): `4` (`MANIFEST_SCHEMA_VERSION`). Version 3 added
  `probed_globals` and the `up` field of cycle markers; version 4 added
  `skipped_scripts`.
- `profile` (`string`): import host profile name (`"pc-fresh"`).
- `script` (`string`): canonical name of the entry script from the archive's
  `NAME` chunk.
- `loaded_scripts` (`string[]`): ordered list of every script loaded (the entry
  script followed by each `ScriptCB_DoFile` target resolved in the archive).
- `phases` (`string[]`): phases executed, in order (`"chunk"`, plus
  `"ScriptPreInit"` and/or `"ScriptInit"` when present as global functions).
- `unresolved_queries` (`object[]`): value-returning host calls (`C != 1`) not
  answered by the active profile or a supplied answer. Each entry has `phase`, `name`, `call_index`
  (index into `calls`), and `expected_results` (integer count or `null` for
  open `LUA_MULTRET`).
- `probed_globals` (`string[]`, sorted): globals the loaded scripts test in a
  condition or comparison (including `_G.Name` and a constant
  `rawget(_G, "Name")`) that were still undefined when recording finished. A
  guarded call of such a name did not run.
- `skipped_scripts` (`object[]`): archive members the host-function scan could
  not load, in archive order, each `{"script": <name>, "error": <message>}`.
  The host functions only they call are not bound.
- `calls` (`object[]`): ordered list of recorded host calls across all executed
  phases. Each entry has `phase`, `name`, and `args` encoded losslessly with
  reserved tag key `"$bfe"`:
  - `nil` -> `null`
  - `boolean` -> `true` / `false`
  - finite `number` other than `-0.0` -> plain JSON number (exact integers in
    `-16777216..=16777216` without a decimal point; others via
    `numfmt::format_json_number`)
  - `-0.0`, `nan`, `inf`, `-inf` -> `{"$bfe": "number", "special": "-0" | "nan" | "inf" | "-inf"}`
  - valid UTF-8 `string` -> plain JSON string
  - non-UTF-8 byte `string` -> `{"$bfe": "bytes", "hex": "<lowercase hex>"}`
  - dense 1-based Lua array (`1..=n`, no other keys, `n >= 1`) -> plain JSON
    array `[v1, ..., vn]`
  - empty table `{}` or table whose keys are all valid UTF-8 strings with no
    key equal to `"$bfe"` -> plain JSON object `{"k": v, ...}` in deterministic
    VM iteration order
  - any table with non-string keys, mixed key types (such as `1` and `"1"` or
    `true` and `"true"`), non-UTF-8 string keys, or a key equal to `"$bfe"` ->
    `{"$bfe": "table", "entries": [[k1, v1], ...]}` in deterministic VM
    iteration order
  - Lua closure or native function -> `{"$bfe": "function"}`
  - host userdata -> `{"$bfe": "userdata"}`
  - a table that is one of its own ancestors -> `{"$bfe": "cycle", "up": n}`,
    where `n` counts the tables from the marker to the one it refers to (1 is
    the table holding the marker, 2 its parent)

A snapshot is a tree: a table reached twice (for example `{s, s}`) is written at
each place it occurs, and only a table that contains itself becomes a cycle
marker. Functions and userdata are opaque, so two different functions used as
keys give two entries that look alike (a general table keeps both, in order).

## Testing

- `tests/vm_tests.rs`: fixtures in `tests/lua/` compiled by a reference Lua 5.0.3
  `luac` with the game's chunk settings, each checked byte for byte against the
  reference interpreter's output (`*.expected.txt`), plus safety, limit,
  collector, host-interface and recorder tests.
- `tests/differential_corpus.rs`: 241 programs (240 generated from a seed and
  fixed cases, `tests/lua/corpus/generate.py`, sources committed) packed with the
  reference output and each source's hash by `tests/lua/corpus/pack.py`; a source
  edited without repacking fails it, and both scripts have a `--check` mode.
- `tests/hostile_bounds.rs`: work, time and memory bounds for hostile programs.
- `tests/collector.rs`: bounded steps, the 60,000-unit tick balance, allocations
  that fit proceeding after the balance is spent, a refused callback and
  recovery on later ticks, a heavy-allocation mission that never reaches its
  limit, weak tables cleared across ticks, destruction of large objects in pieces.
- `tests/load_bounds.rs`: loading within the memory left (peak bytes counted),
  a native include limited by the call's budget, and the reported figure
  following `set_limits` and `reserve_userdata`.
- `tests/swbf2_game.rs` (ignored by default): with `BFE_SWBF2_DIR` set to a
  player's install, loads every script of the four archives and records every
  mission, checking determinism; with `BFE_SWBF2_MANIFESTS` set to a directory
  of `<script>.json` files printed by `bfe-script-tool manifest` from the same
  install, it also compares every manifest byte for byte (ordered calls and
  arguments). Nothing derived from the game is committed.
- The CI scripts run this crate's tests in release builds as well as debug
  builds on Linux x86-64 and ARM64, macOS and Windows
  (`cargo test --release -p bfe-script`), since optimization must not change any
  result.

Sample `Vm::state_digest()` between outer calls with the same embedding inputs.
The digest streams the collector's pending requests, traversal and destruction
state. A native callback's Rust stack and the embedding's RNG are external state.
