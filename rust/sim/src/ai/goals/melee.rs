//! A bot's melee input: one sample per life per simulation tick for the
//! combo machine.
//!
//! The goal bots make a sample for every living bot they see on each tick,
//! whether or not the bot has a goal and whether or not it is seated, so a
//! bot that loses its goal, becomes unassigned or mounts releases what it
//! held instead of repeating an old edge. A sample:
//!
//! - is stamped with the goal-rules installation that made it (the match) and
//!   the fixed simulation tick it is for: made after the soldier phase of one
//!   tick, it is consumed by the soldier phase of the next;
//! - belongs to one life, keyed by unit slot, slab generation and gameplay
//!   life, and to one melee binding (the selected slot, its weapon and its
//!   channel); a new life, binding or installation starts with every button
//!   up;
//! - carries at most one rising edge per logical button and is given to its
//!   reader once, so a delivered edge is never read again.
//!
//! Two logical buttons are made. Fire is the trigger of the channel holding
//! the selected melee weapon, exactly as the body's controls hold it (the
//! secondary channel's trigger for a blade there). FireSecondary is the
//! dedicated block control: in the default mode one press raises the block
//! and another lowers it; [`BlockMode::Hold`] holds it while a block is
//! wanted. Presses are at least [`TOGGLE_SPACING`] apart. Whether the block
//! is up is the combo machine's to say: the bot retains the guard the machine
//! last reported for this life and equipment
//! ([`MeleeInput::sync_guard`]), and an attack changes the belief only
//! through that report. The machine may hold an early press and apply it
//! later, or drop a press it cannot pay for, and says neither; so a press
//! whose toggle the machine has not committed is made again only after
//! [`RETRY_AFTER`], never as a repeat of the delivered edge. Hold times,
//! taps and double presses are the combo machine's history.

use crate::ai::control::world::{EntRef, UKey};
use crate::gen::F3UnitKey;
use crate::store::DefId;

/// The combo machine's logical buttons, bits 0 to 5.
pub const FIRE: u8 = 1 << 0;
pub const FIRE_SECONDARY: u8 = 1 << 1;
pub const JUMP: u8 = 1 << 2;
pub const CROUCH: u8 = 1 << 3;
pub const SPRINT: u8 = 1 << 4;
pub const RELOAD: u8 = 1 << 5;
/// Seconds between two block presses in toggle mode, longer than a double
/// press.
pub const TOGGLE_SPACING: f64 = 0.3;
/// Seconds before a block press whose toggle the combo machine has not
/// committed is made again: longer than the machine holds an early press.
pub const RETRY_AFTER: f64 = 1.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BlockMode {
    /// A press raises the block, a second press lowers it.
    #[default]
    Toggle,
    /// The block lasts while the button is held.
    Hold,
}

/// One logical button on one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Edge {
    /// Down on this tick.
    pub held: bool,
    /// Went down on this tick.
    pub pressed: bool,
    /// Came up on this tick.
    pub released: bool,
}

impl Edge {
    fn step(&mut self, down: bool) {
        *self = Edge { held: down, pressed: down && !self.held, released: self.held && !down };
    }
}

/// The melee weapon a sample is for: its selected slot, its definition and
/// the channel it is selected on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub slot: usize,
    pub weapon: DefId,
    pub channel: u8,
}

/// What a bot wants on one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wanted {
    /// The selected melee weapon; `None` without one, and while seated.
    pub binding: Option<Binding>,
    /// That channel's trigger is down.
    pub fire: bool,
    /// A block is wanted.
    pub block: bool,
    /// The movement axes the body is driven with.
    pub forward: f64,
    pub right: f64,
}

/// What the coupled combo and mover committed for the delivered frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeleeReport {
    pub guarding: bool,
    pub energy: f64,
    pub exhausted: bool,
}

/// The soldier driver returns an error when its coupled step did not commit.
pub type SoldierDriver = dyn FnMut(&mut crate::world::owner::World, &MeleeInput) -> Result<Option<MeleeReport>, crate::battle::rules::objects::RuleError>;

/// Trigger timing for one uninterrupted opportunity to strike.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct SwingClock {
    opponent: Option<(EntRef, Binding)>,
    entered: u32,
    last: u32,
}

impl SwingClock {
    pub(super) fn pulse(&mut self, tick: u32, opponent: Option<(EntRef, Binding)>) -> bool {
        if self.opponent != opponent || tick.saturating_sub(self.last) > 1 {
            self.entered = tick;
        }
        self.opponent = opponent;
        self.last = tick;
        opponent.is_some() && tick.saturating_sub(self.entered) % 18 < 4
    }
}

/// One bot life's melee input, as of its latest sample.
#[derive(Clone, Debug, PartialEq)]
pub struct MeleeInput {
    /// The goal-rules installation that made the sample.
    pub epoch: u32,
    /// The fixed simulation tick the sample is for.
    pub tick: f64,
    /// The life: unit slot, slab generation and gameplay life.
    pub key: F3UnitKey,
    /// The goal bots' own key for that unit.
    pub unit: UKey,
    pub binding: Option<Binding>,
    pub fire: Edge,
    pub block: Edge,
    pub forward: f64,
    pub right: f64,
    pub mode: BlockMode,
    /// The last guard committed by this life and equipment.
    pub guarding: bool,
    /// Energy and exhaustion after the last committed mover step.
    pub stamina: Option<(f64, bool)>,
    pub(super) swings: SwingClock,
    peak_energy: f64,
    reported: bool,
    /// The sample was given to its reader.
    pub consumed: bool,
    /// The guard the latest block press asked for.
    intent: Option<bool>,
    since_toggle: f64,
}

impl MeleeInput {
    /// A record for a new life, binding or installation with every button up.
    pub fn new(epoch: u32, key: F3UnitKey, unit: UKey) -> Self {
        Self {
            epoch,
            tick: f64::NAN,
            key,
            unit,
            binding: None,
            fire: Edge::default(),
            block: Edge::default(),
            forward: 0.0,
            right: 0.0,
            mode: BlockMode::Toggle,
            guarding: false,
            stamina: None,
            swings: SwingClock::default(),
            peak_energy: 0.0,
            reported: false,
            consumed: false,
            intent: None,
            since_toggle: RETRY_AFTER,
        }
    }

    /// Whether the sample is the one for `key` on `tick`.
    pub fn is_for(&self, key: &F3UnitKey, tick: f64) -> bool {
        self.key == *key && self.tick.to_bits() == tick.to_bits()
    }

    /// Make the sample for `tick`. A second call for the same tick changes
    /// nothing; a sample for another binding starts from every button up.
    pub fn step(&mut self, tick: f64, dt: f64, wanted: Wanted, mode: BlockMode) {
        self.step_with_tap(tick, dt, wanted, mode, false);
    }

    pub(crate) fn step_with_tap(&mut self, tick: f64, dt: f64, wanted: Wanted, mode: BlockMode, fire_tap: bool) {
        if self.tick.to_bits() == tick.to_bits() {
            return;
        }
        if wanted.binding != self.binding {
            // Each machine owns its button history. An unequip still releases
            // the preceding machine's held buttons.
            if wanted.binding.is_some() {
                self.fire = Edge::default();
                self.block = Edge::default();
            }
            self.guarding = false;
            self.stamina = None;
            self.peak_energy = 0.0;
            self.intent = None;
            self.since_toggle = RETRY_AFTER;
        }
        self.tick = tick;
        self.consumed = false;
        self.reported = false;
        self.mode = mode;
        self.since_toggle += dt;
        self.binding = wanted.binding;
        self.forward = wanted.forward;
        self.right = wanted.right;
        let armed = wanted.binding.is_some();
        let fire = armed && wanted.fire;
        let wants_guard = wanted.block && self.stamina.is_none_or(|(energy, exhausted)| energy > 0.0 && !exhausted);
        let mut block = false;
        match mode {
            BlockMode::Hold => {
                let retry = self.stamina.is_some()
                    && !self.guarding
                    && self.block.held
                    && self.since_toggle >= TOGGLE_SPACING;
                block = armed && wants_guard && !fire && !retry;
                if block && !self.block.held {
                    self.since_toggle = 0.0;
                }
            }
            BlockMode::Toggle => {
                // A press the machine has not committed yet waits longer.
                let wait = if self.intent == Some(wants_guard) { RETRY_AFTER } else { TOGGLE_SPACING };
                if armed && wants_guard != self.guarding && !self.block.held && self.since_toggle >= wait {
                    // One press toggles; the button comes up on the next tick.
                    block = true;
                    self.intent = Some(wants_guard);
                    self.since_toggle = 0.0;
                }
            }
        }
        self.fire.step(fire);
        self.fire.pressed |= armed && fire_tap;
        self.block.step(block);
    }

    /// The guard the combo machine committed for this sample's tick.
    pub fn sync_guard(&mut self, guarding: bool) { self.guarding = guarding; }

    /// Leave half the energy observed on this equipment for the next attack.
    pub(super) fn guard_reserve(&self, binding: Option<Binding>) -> bool {
        self.binding != binding || self.stamina.is_none_or(|(energy, exhausted)| !exhausted && energy > self.peak_energy * 0.5)
    }

    pub(crate) fn report(&mut self, guarding: bool, energy: f64, exhausted: bool) -> bool {
        if !self.consumed || self.reported || !energy.is_finite() { return false; }
        self.guarding = guarding;
        self.peak_energy = self.peak_energy.max(energy);
        self.stamina = Some((energy, exhausted));
        self.reported = true;
        true
    }

    /// The combo machine's buttons: (rising edges, held levels).
    pub fn combo_bits(&self) -> (u8, u8) {
        let bit = |edge: &Edge, mask: u8| (if edge.pressed { mask } else { 0 }, if edge.held { mask } else { 0 });
        let (fp, fh) = bit(&self.fire, FIRE);
        let (bp, bh) = bit(&self.block, FIRE_SECONDARY);
        (fp | bp, fh | bh)
    }
}
