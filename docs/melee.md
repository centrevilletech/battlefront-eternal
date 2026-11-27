# Battlefront II melee combos, blocking and deflection

The 2004 game's melee (`rust/sim/src/combat/weapons.rs` `prepare_melee`,
`targeting.rs` `melee`, and `units/f3_sensors.rs` `block`) stays untouched and
is described in [ai-heroes.md](ai-heroes.md).

The 2005 game authors each melee weapon's behaviour as a data-driven combo state
machine (`comb` config chunks prepared into `sides/<side>/combos.json`), paired
with stamina (`EnergyBar`), animation bank timings (`anim_banks.json`), and
weapon ODF blade and damage fields.

## Implemented in simulation (`rust/sim/src/combat/combo/`)

- **Per-weapon loader and fail-closed runtime representation (`combo/data.rs`)**:
  - Loads one hero weapon's combo machine at battle start through
    `ComboMachine::load_combo_by_hash` or `ComboMachine::from_json_value`,
    taking a validated `WeaponMeleeSpec`, `anim_fps`, and a per-hero
    `clip_duration` resolver.
  - `resolve_binding_clip_duration(hero_bank, package_banks, candidates)`
    computes nominal clip duration `(frames - 1) / fps` and scopes fallback
    candidates to the equipped unit's package, preferring its declared animation bank.
  - Resolves every state target to `StateId(u16)`, interns animation clips,
    sound cues, `DeflectAnimation` clips (`DeflectAnimDef`, retaining
    `blend_in`, `blend_out`, `loop_mode`, `aim_type`, and `low_res_pose_secs`),
    `OffhandAnimation` entries (retaining `aim_type`), and shared nonzero
    low-20-bit `AttackId` identifiers (`attack_shared_id`, `pack_attack_key`).
  - Retains split-body `Animation` pairs (`animation` upper + `lower_animation`
    lower) and per-layer presentation metadata (`StateAnimLayer`: `BlendInTime`,
    `BlendOutTime`, `BlendTimeTo`, `Loop`, `AimType`, `SyncType`, `LowResPose`).
  - Applies fixed 30 Hz `"Frames"` conversion, centisecond truncation, and
    field-specific range clamps (`Duration` `0..=10.22 s`, `Break` `0..=5.11 s`,
    `TimeStart`/`TimeEnd` `-5.12..=5.11 s`, `InputLock`/`DamageTime`/`Sound`
    `0..=10.23 s`, `FromAnim` `0.00..=1.00` on unit-clamped magnitude).
  - Enforces strict JSON typing (numeric fields accept JSON numbers only;
    string-valued `TimeStart`/`TimeEnd` slot 0 reads `0.0`), array checks on
    `children`/`values` and top-level tables, `u16` capacity bounds before ID
    narrowing, matching outer vs inner `nameHash` in `load_combo_by_hash`,
    `Attack.Edge < weapon.num_edges`, a valid clip or positive duration for
    `FromAnim` attacks, and signed edge-relative scaling (`"FromEdge"` or
    negative bare length/width multiplied against `WeaponMeleeSpec.edges[edge]`).
  - Fails closed on unresolved or zero-duration state animation clips (naming
    machine hash, state, and clip), missing `DeflectAngle`, unknown keys,
    unknown tokens/units, negative durations or damages, dangling state
    references, and invalid `WeaponMeleeSpec`. Trailing ignored numeric slots on
    `AnimatedMove` and `Transition` are skipped explicitly.

- **Deterministic 60 Hz combo state machine and 2005 mover hook (`combo/mod.rs`)**:
  - Guarded by `GameFamily::Swbf2`; `GameFamily::Swbf1` battles bypass it.
  - Maintains a fixed 8-slot timestamped `ButtonEventHistory` ring supporting
    all 11 button modes (`Press`, `Tap`, `Hold`, `HoldRelease`, `DoublePress`,
    `DoubleTap`, `DoubleHold`, `DoubleHoldRelease`, `Release`, `Down`, `Up`)
    with distinct `BUTTON_HOLD_MIN_SECS = 0.30` and
    `BUTTON_DOUBLE_PRESS_SECS = 0.25` thresholds.
  - Evaluates timestamped transition windows (`TimeStart` / `TimeEnd`,
    including `FromEnd` carry-over via `grace_state` evaluated before
    current-state interrupts), picks the earliest matching timestamp (authored
    order breaks ties), prunes consumed button events, and continues across
    zero-duration bridges within the same tick (up to 16 steps), stopping when
    entering a `MustShowOneFrame` state.
  - The dedicated stock control uses `with_block_button(pressed, held)`. Release keeps guard up.
    The accessibility control uses `with_hold_to_block(pressed, held)` each tick.
    Releasing it exits standing guard. Finite attacks can intercept throughout their
    authored duration. Read the settled `ComboState::is_guarding` for block feedback.
    Both accept a tap whose press and release fell between ticks. Secondary weapon fire remains independent.
  - On finite state expiry without a matching transition, selects the last
    stored transition's target (at zero energy cost) when transitions exist, or
    the named neutral state when the list is empty. Public state IDs keep file order.
  - Separates the unwrapped evaluation clock (`eval_time`, continuous across
    same-animation states) from the rendered animation clock (`anim_time`,
    wrapped for `LoopMode::Wrap` or clamped for `LoopMode::FinalFrame`).
    The loader encodes each finite clip deadline in centiseconds, up to 10.22 s.
    The clip retains its full playback length. Explicit-duration looping states finish on schedule while indefinite
    `ClipLength` `FinalFrame` attacks stay live. `FromAnim` resolves against
    explicit state duration when present, and finite attack windows test
    `prev_eval <= win_end && curr_eval > win_start` so zero-width point windows
    fire cleanly without dividing by zero.
  - Emits settled-state `InputLock`, `Deflect`, `MovementDirective` (including
    the `ANIMATED_MOVE_BLEND_RATE = 6.6666665` velocity blend ramp), posture
    overrides, and `blades_off_mask` on the entry tick.
  - Integrates with `movement::soldier::swbf2` through `prepare_movement_hook`
    and `step_soldier_combo_and_move`: coordinates stamina rates and transition
    costs via `swbf2::State::apply_energy_rate` and `swbf2::State::charge`.
    Standalone `step` also takes that per-life state and its class tuning. A
    positive matched cost may overdraw once while the exhaustion latch is
    clear. Reaching exactly zero leaves the latch clear. At a finite deadline
    with no match, only the last destination is used and no cost is charged.
    The coupled step forwards `swbf2::InputLockMask(out.input_lock.0)`, `velocity_xz`,
    `air_accel_xz`, `lock_locomotion`, `vertical_impulse`, `aim_yaw`, `gravity_scale`,
    `energy_rate`, `force_jump`, `end_jet`, and `force_roll` inside
    `swbf2::Step::override_` into `movement::advance_profiled` before its
    single displacement and collision pass, and zeroes duplicate jump/roll
    movement costs when the combo transition already paid stamina. Airborne
    `AnimatedMove` accelerates horizontally through `air_accel_xz` instead of
    clamping velocity, and `GravityVelocityTarget` latches inside the combo step
    so `gravity_scale` switches once the target vertical speed is reached without
    forwarding a velocity target override. Pass a `SoldierEnergy` borrowing the
    same mover state for contacts and powers; saber throw charges stamina directly
    through `swbf2::State::charge`.
    Movement failure commits neither stamina nor the combo. Authored standing and
    crouching request the absolute stance through the solver. `AlignedToThrust`
    turns the state toward the movement stick inside the forward hemisphere
    (dead zone `|thrust| < 0.3`, backward sticks mirrored forward, and lateral
    angles clamped to `+-45` degrees) without overwriting `mover.aim_yaw`. The
    turn applies to `AnimatedMove` (its stick part is measured in the turned
    frame, then the whole velocity is turned) and, as
    `StepOutput.swing_turn`, to the swing's damage sweep and to the drawn body
    above the hips, except in a state with `Deflect`. `AlignedToThrust("Initial")`
    keeps the stick of the state's entry tick for the whole state. Meanwhile
    `InputLock("All")` leaves turning unlocked. The `Thrust` token covers the
    whole movement stick, so `InputLock("All", "!Thrust")` keeps walking
    forward, backward and sideways free; a `Strafe` token has no effect, as in
    the 2005 game. The active state's
    `TurnFactor` (or `0.0` during an active `InputLock("Turn")` window) scales
    both keyboard and mouse turning for local and network players.
  - `StepOutput.entries` gives all state entries in order. Each `StateEntry`
    exposes its public state ID, accepted clock and untimed entry sounds.
    `timed_cues.sounds(machine)` yields due outgoing cues with their record
    indices. A returned blade emits the same event through `catch_entry`.
  - Complete, validated checkpoints (`ComboCheckpoint` including `eval_time`,
    `animated_move_blend`, `button_history`, `hit_history`, `grace_state`, and
    the last button edge times, plus `SaberThrowCheckpoint`) restore atomically via
    `try_restore`, rejecting out-of-range indices, inconsistent derived flags
    (`deflecting`, `blades_off_mask`), inconsistent clocks, mismatched
    `hit_history` `shared_id`, non-unit active throw directions, and non-finite
    values (`CheckpointError`).

- **Swept blade hit testing, adaptive sampling, and melee blocking (`combo/sweep.rs`)**:
  - `BladeSweep` tests thickened blade triangles against `HitShape` spheres,
    capsules, yaw-rotated boxes (reading canonical `half_vector()` /
    `half_number()`), and upright finite cylinders (exact vertical-slab radial
    test, cap-slice polygon clipping, support generator tests, and convex
    golden-section cap-rim minimization).
  - Adaptive blade pose sampling (`DamageSample`, `DamageSampleList`,
    `build_damage_samples`, `resolve_melee_hit_samples`) starts at `1/30 s`,
    refines whenever base or tip squared travel exceeds `0.25`, stops refining
    below a `1/300 s` step, clips at the interval end, and caps at
    `MAX_DAMAGE_SAMPLES = 255` samples without heap allocation.
  - Each emitted attack carries its outgoing public state ID and clipped
    `sample_start` and `sample_end`. `ActiveAttack::sample` evaluates the
    bound clip on this interval, including the outgoing samples on a state change.
  - Per-attack (or shared nonzero low-20-bit `AttackId`) hit lists (`HitHistory`
    and `HitListRecord`) store up to `MAX_TARGETS_PER_ATTACK = 8` distinct
    targets across `MAX_ACTIVE_HIT_LISTS = 16` concurrent lists, record blocked
    contacts before deflect resolution, survive state exit via clock rebasing,
    and retire `HIT_LIST_LINGER_SECS = 0.30` after their stored window end.
  - Blocking an enemy melee attack inside `DeflectAngle` (ordinary or wrapped
    intervals) cancels health, shield, and push completely
    (`MELEE_BLOCK_DAMAGE_SCALE = 0.0`), charges only `DeflectDef::energy_cost`
    (`MELEE_BLOCK_EXTRA_ENERGY_COST = 0.0`), and does not stagger the attacker.

- **In-place projectile deflection (`combo/deflect.rs`)**:
  - Tests incoming ordnance against active `DeflectAngle` (including wrapped
    rear arcs such as `150..-150`), fail-closed `DeflectableKind` eligibility,
    unit-length `incoming_dir`, and per-shot `EnergyCost`.
  - Classifies 6 horizontal reaction sectors (`Forward`, `FrontLeft`,
    `FrontRight`, `Left`, `Right`, `Back`) at `22.5`, `67.5`, and `135.0`
    degree cutoffs, falling back to an authored clip via
    `select_deflect_animation` / `select_deflect_direction`.
  - Successful interceptions report the selected machine-local reaction record and direction.
    A contact with no authored reaction still intercepts.
  - Builds an orthonormal aim basis (`compute_aim_basis`) with deterministic
    seeded spread (`try_deflect_with_spread`).
  - `reflect_in_ordnance` validates the live `Shot::Bolt` or `Shot::Round`
    handle and ownership, takes a `RedirectAim` with the shot seed and subtype spread,
    and retains the remaining lifetime as the upper bound on recast distance.
    It charges stamina once and redirects the existing
    projectile in place (updating shooter attribution, `ignore_body`, position, direction,
    velocity, remaining life, and re-cast static impact) without firing expiry
    explosions or spawning duplicate shots.

- **Force Push / Pull and Saber Throw (`combo/powers.rs`)**:
  - Zero-allocation `PowerValidationError` (`Copy`) plus load-time validated
    wrappers `ValidatedForcePushDef` and `ValidatedSaberThrowDef` keep hot-path
    and error-path allocations strictly zero.
  - `execute_force_push` / `execute_force_push_validated` evaluate conical
    knockback/pull with `EnergyDrain`, `MaxTargets`, `AffectFriends`, and
    caller-supplied `los_clear`. Modeling choice: damage and impulse are uniform
    across `[0, cone_length]` with no distance falloff.
  - `SaberThrowDef` carries the thrown ordnance's own `damage_scales` and
    `shield_scale` (resolving `"FromEdge"` dimensions from the primary weapon
    edge via `with_primary_edge_dimensions`), gates melee block interception on
    `PROVISIONAL_DEFLECT_SABER_THROW`, and models outbound spin (full rate) and
    boomerang return (half rate, `CATCH_SABRE` entry on catch).
  - The local infantry offhand throw follows these rules:
    - The press pays `EnergyDrain` at once through `swbf2::State::charge`. Only
      the exhaustion latch gates it, so one press may overdraw while the latch is
      clear; nothing is charged again at the launch.
    - A new press is refused while the soldier is held (choked) and while the
      current state's `InputLock` covers the offhand trigger
      (`InputLockMask::RELOAD`, which `InputLock("All")` includes). A windup or a
      flight that is already running is never cancelled by a choke or by any
      other input.
    - When the press is accepted from a state that is not `IDLE` and has a
      transition to `IDLE` (a guard that does not lock the trigger, for example),
      the combo returns to `IDLE` first.
    - The blade leaves `InitialSalvoDelay` after the press (rounded up to whole
      ticks), from the weapon mount of the throw animation sampled at that time,
      carried with the soldier's position and heading. The launch tick itself
      does not advance the flight.
    - When the throw leaves the hand, one ray along its path over the full range
      (`Velocity x LifeSpan`) finds the nearest static geometry. The outbound pass
      lasts `LifeSpan` times the free share of that range and the return pass
      `ReturnDuration` times the same share, so a throw into a wall two metres
      away is out and back in a few ticks, and a throw into open air flies the whole
      range. The blade ends its outbound pass exactly where the free range ends.
    - The return flight chases the release point carried with the soldier: each tick
      it covers the share of the remaining way that the clock advanced, so a soldier
      who runs away does not drag the blade along. The throw ends on the earliest tick
      whose clock has used up the return time; that tick does not move the blade, and
      the catch hands over to `CATCH_SABRE` on the same tick, with the hilt in the
      hand from then on.
    - Blades stay lit during the flight; the hand shows no hilt while it flies.
    - Every viewer gets the throw as the hero's world field (see
      [network-contract.md](network-contract.md)) and runs this same flight rule from
      it, so a remote player's throw and the local player's are drawn alike.
  - Presentation spins the released hilt around the cross product of its blade
    axis at the release and the throw direction, keeping the release orientation
    intact. Modeling choices: the combat sweep blade segment remains horizontal
    during flight, and caster death cancels the in-flight blade immediately.

## Fixed-capacity limits and runtime conventions

Hot-path tables are fixed-size to avoid per-tick allocations:

- `ButtonEventHistory`: `MAX_BUTTON_EVENTS = 8` timestamped events.
- `HitHistory`: `MAX_ACTIVE_HIT_LISTS = 16` concurrent lists, each holding up
  to `MAX_TARGETS_PER_ATTACK = 8` target IDs (`HIT_LIST_LINGER_SECS = 0.30`).
- `DamageSampleList`: `MAX_DAMAGE_SAMPLES = 255` pose samples per interval.
- `ActiveAttackList`: `MAX_ACTIVE_ATTACKS = 8` active attack windows per tick.
- `StateEntries` holds up to 16 entries per tick. Timed and entry sounds iterate their bound records.
- A thrown blade holds two lists of up to eight target identities each.
- `ForcePushResult`: `MAX_FORCE_PUSH_TARGETS = 16` nearest targets per emission.

The loader checks the eight-window limit on each fixed step of a state, including
point windows and held final frames. It also checks the sixteen-list limit through
the state's attack sequence, accounting for shared IDs and the retention interval.
Lists can carry over from previous states. If that history fills the table, or a
custom timestep crosses too many windows, `StepOutput.first_omitted_attack` gives
the packed state and record index. Callers can report the incompatible equipment
before applying that tick's damage.

Runtime conventions currently implemented:

- `MustShowOneFrame` stops same-tick transition chaining upon entering the state.
- `Break` delays, `TimeStart` / `TimeEnd` and damage windows are all measured
  on the unwrapped `eval_time` state clock. It restarts at zero on entry,
  except that a state continuing its predecessor's clip continues its clock,
  as in the 2005 game.
- Deflection is active immediately on the tick a `Deflect` state is entered.
- Melee block angle is measured horizontally from the defender's origin toward
  the attacker's origin, ignoring height.
- Simultaneous same-tick melee swings resolve per pair in caller iteration
  order with no clash interruption.

## Testing an installed package

The ignored `swbf2_combos_real` test requires two paths. Set
`BFE_SWBF2_CONTENT_DIR` to the prepared package root containing `sides/` and
`BFE_SWBF2_COMBO_ORACLE` to a separately held JSON file of expectations.
Run `cargo test -p sim --test swbf2_combos_real -- --ignored --nocapture` from
the Rust workspace.

To produce an oracle, use an unmodified copy of the package. For each unit and
equipped combo, record its action-to-bank bindings, clip identifiers and frame
counts under `bindings["<unit>/<combo>"].timing`. Work out the expected
`cases` independently using the state and weapon records, class stamina and
observed input behaviour. Give each case a fixed 60 Hz input sequence. For
each tick record the settled state, stamina, and the ordered outgoing attack
records with clipped start and end times and damage to a stationary target.
Include expected catch and exhaustion flags where applicable. The test's
`check_case` function reads those fields. Calculate deadlines from the
encoded duration or bound clip, and keep clip playback time separately.

Once the per-tick expectations pass on a verified build, freeze its whole-run
`fingerprint` alongside them. Keep this file with the package's test material
and reuse it unchanged for subsequent runs and for runs against altered data.
A changed roster or timing needs independently reviewed expectations before
recording a new fingerprint.

## Local battle contacts

A developer Battlefront II battle binds each admitted soldier class to its
equipped combo, rig, ordered damage edges and bank-qualified animation clips.
Each living saber wielder owns its combo, movement energy, hit lists and blade
history. Human input and the native bot controller's primary trigger advance
those machines at 60 Hz. A new life requires a fresh press or a release before
an inherited button can start an attack. Block toggles the standing guard by
default. The hold option releases the guard when the key goes up. Secondary
fire uses the equipped offhand, including a returning saber throw. The throw
rules are listed under Force Push / Pull and Saber Throw above.

The simulation samples each authored contact interval and interpolates the
attacker's two fixed-tick roots. It clips the posed edge to the collision world
and tests exposed soldiers using the projectile hurt shapes. Damageable bodies
use the native prop and hull hit services. The native unit-hit service applies
friendly fire, shields, health-category scaling, damage direction and death
credit. The posed contact is the single source of blade damage for both players
and bots. A delayed hit from the gun controller's native melee swing is consumed
at the 2005 soldier boundary. A guard redirects an eligible bolt or bullet in its
own flight slot and pays the attack state's energy cost. The projectile keeps its
original weapon and credits its next hit to the defender.

Presentation reads a bounded journal of fixed-tick contacts, health changes,
reactions and directions. The mission receives each native soldier death through
the rules event queue with both lives, teams and the attack or projectile that
caused the death. Scoring and replacement belong to the mission.

## Remaining integration

- Online input and publication still need hold-to-block and lock-on inputs,
  validated own-body `ComboCheckpoint` and `SaberThrowCheckpoint` values,
  and remote presentation fields.
- **Remaining hero weapons**: smolder burn damage over time, piercing beams,
  guided remote missiles, and two-tier charge bowcasters. Force push, pull,
  choke and lightning and the aura abilities are described in
  [powers.md](powers.md).
- **Remaining provisional constants**: details still requiring live gameplay
  measurements (lunge auto-aim blend, `DamageWidth` full-width vs radius
  scaling, synthetic fallback arc/height, deflection aim mode/spread and
  non-bolt ordnance eligibility, reflected origin clearance, Force Push vertical
  lift/tumble threshold, and Saber Throw per-pass hit cap) remain
  isolated in named `PROVISIONAL_*` constants inside `rust/sim/src/combat/combo/`.


A mission bot input producer can install its rules generation with
`World::install_bot_combat_input`. Deliver each `bot_input::Frame` through
`stage_bot_combat_input` in the soldier phase. The native mover checks the
physical slot, slab generation, gameplay life, fixed tick, selected weapon
definition, loadout slot and channel before consuming it. A fire tap may have
a rising edge with the level already up. Guard has its own edge, level and
block mode. The frame's axes go into the same coupled step as the player's.
After the soldier loop, `take_bot_combat_feedback` returns the standing guard,
energy and exhaustion committed for that exact stamp, once. A seat or an
absent binding supplies neutral input. Re-equipping a weapon resets its button
and combo history and preserves the soldier's movement energy. Instant Action
uses the released controller's primary level until a mission bot input
producer is installed.
