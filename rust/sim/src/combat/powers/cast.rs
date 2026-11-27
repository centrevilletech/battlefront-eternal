//! One soldier's power trigger: the press, the wind-up, the cast, a held
//! target and the recovery, stepped once per fixed tick.

use super::def::{Effect, PowerDef, TICKS_PER_SECOND};
use crate::movement::soldier::swbf2;

/// Weapon slots a character can fill with powers.
pub const MAX_POWERS: usize = 4;
/// An area weapon checks that its held targets are still inside its area
/// three times a second.
pub const HOLD_RECHECK_TICKS: u32 = 20;

/// The powers of one character class, in weapon slot order.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PowerKit {
    powers: [Option<(u32, PowerDef)>; MAX_POWERS],
    /// The explosion the blade's combo can set off where the character stands.
    pub blast: Option<super::def::Blast>,
    /// Case-folded FNV-1a of the blade that names that explosion.
    pub blast_weapon: u32,
    /// The largest `LockOnRange` among all the class's weapons: the reach of
    /// the on-foot lock while no power with its own range is selected.
    pub lock_range: f64,
}

impl PowerKit {
    /// Add the power found at a weapon `slot` of the class. Returns false
    /// when the kit is full.
    pub fn push(&mut self, slot: u32, def: PowerDef) -> bool {
        match self.powers.iter_mut().find(|p| p.is_none()) {
            Some(free) => {
                *free = Some((slot, def));
                true
            }
            None => false,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.powers.iter().all(Option::is_none) && self.blast.is_none() && !(self.lock_range > 0.0)
    }

    /// The kit index of the power in weapon `slot`.
    pub fn index_of_slot(&self, slot: u32) -> Option<u8> {
        self.powers.iter().position(|p| p.is_some_and(|(s, _)| s == slot)).map(|i| i as u8)
    }

    pub fn get(&self, index: u8) -> Option<&PowerDef> {
        self.powers.get(usize::from(index))?.as_ref().map(|(_, d)| d)
    }

    pub fn slot(&self, index: u8) -> Option<u32> {
        self.powers.get(usize::from(index))?.map(|(s, _)| s)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u8, &PowerDef)> {
        self.powers.iter().enumerate().filter_map(|(i, p)| p.as_ref().map(|(_, d)| (i as u8, d)))
    }
}

/// The trigger as the controls or a bot deliver it on this tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerInput {
    /// Kit index of the power selected on the offhand channel, if that
    /// weapon is a power.
    pub selected: Option<u8>,
    /// The offhand trigger went down on this tick.
    pub pressed: bool,
    /// The offhand trigger is down at the end of this tick.
    pub held: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Phase {
    #[default]
    Idle,
    /// Counting down to the cast.
    Windup { left: u32 },
    /// An area weapon holding its targets.
    Hold { recheck: u32 },
    /// After a cast, before the next one.
    Recover { left: u32 },
}

/// What the world must resolve for this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    /// An emitter fires now: strike the cone or give the aura.
    Fire(u8),
    /// An area weapon finished its wind-up: find targets to hold.
    Acquire(u8),
    /// An area weapon holds its targets for one more tick; `recheck` asks the
    /// world to test that each one is still inside the area.
    Sustain { power: u8, recheck: bool },
}

/// The outcome of one tick of the trigger.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PowerStep {
    /// What each power asks of the world on this tick, in kit order. Several
    /// powers can act on one tick: a held choke goes on while the power the
    /// wheel selected after it winds up and fires.
    pub requests: [Option<Request>; MAX_POWERS],
    /// Energy charged by the trigger on this tick.
    pub energy_spent: f64,
    /// Stamina rate to use instead of ordinary recovery while the selected
    /// power holds a target.
    pub energy_rate: Option<f64>,
    /// Scale for the caster's own movement input.
    pub thrust: f64,
    /// The power being wound up or held, for presentation: the selected one
    /// first.
    pub casting: Option<u8>,
}

impl PowerStep {
    /// The request of the lowest kit index on this tick.
    pub fn request(&self) -> Option<Request> {
        self.requests.iter().flatten().next().copied()
    }

    /// Every request of this tick, in kit order.
    pub fn requests(&self) -> impl Iterator<Item = Request> + '_ {
        self.requests.iter().flatten().copied()
    }
}

/// Per-life trigger state. `Copy` and fixed size; a new life starts from
/// [`PowerState::new`].
///
/// Each power runs its own phase, as each weapon of the 2005 game runs its own
/// firing state every tick whether it is selected or not: an area power that
/// is winding up or holding goes on after the wheel selects another power,
/// while an emitter drops a cast it was still winding up. Only the selected
/// power can start. All of a channel's weapons read one trigger. A single-shot
/// cast spends the trigger as it starts, and a held single-shot area power
/// spends it as it lets go; a spent trigger starts nothing more until it is
/// released.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PowerState {
    phases: [Phase; MAX_POWERS],
    rounds: [u16; MAX_POWERS],
    spent: bool,
}

impl PowerState {
    pub fn new(kit: &PowerKit) -> Self {
        let mut rounds = [0; MAX_POWERS];
        for (i, def) in kit.iter() {
            rounds[usize::from(i)] = def.cost.rounds.unwrap_or(0);
        }
        Self { phases: [Phase::Idle; MAX_POWERS], rounds, spent: false }
    }

    /// Casts left for a power that spends rounds.
    pub fn rounds(&self, index: u8) -> u16 {
        self.rounds.get(usize::from(index)).copied().unwrap_or(0)
    }

    fn phase(&self, index: u8) -> Phase {
        self.phases.get(usize::from(index)).copied().unwrap_or_default()
    }

    fn first(&self, pick: impl Fn(Phase) -> bool) -> Option<u8> {
        self.phases.iter().position(|&p| pick(p)).map(|i| i as u8)
    }

    /// The lowest kit index wound up or held.
    pub fn active(&self) -> Option<u8> {
        self.first(|p| matches!(p, Phase::Windup { .. } | Phase::Hold { .. }))
    }

    /// The lowest kit index holding its targets.
    pub fn holding(&self) -> Option<u8> {
        self.first(|p| matches!(p, Phase::Hold { .. }))
    }

    /// The lowest kit index winding up.
    pub fn winding_up(&self) -> Option<u8> {
        self.first(|p| matches!(p, Phase::Windup { .. }))
    }

    /// The lowest kit index recovering.
    pub fn recovering(&self) -> Option<u8> {
        self.first(|p| matches!(p, Phase::Recover { .. }))
    }

    pub fn is_active(&self, index: u8) -> bool {
        matches!(self.phase(index), Phase::Windup { .. } | Phase::Hold { .. })
    }

    pub fn is_holding(&self, index: u8) -> bool {
        matches!(self.phase(index), Phase::Hold { .. })
    }

    pub fn is_winding_up(&self, index: u8) -> bool {
        matches!(self.phase(index), Phase::Windup { .. })
    }

    pub fn is_recovering(&self, index: u8) -> bool {
        matches!(self.phase(index), Phase::Recover { .. })
    }

    /// Whether the trigger was spent and starts nothing until it is released.
    pub fn spent(&self) -> bool {
        self.spent
    }

    /// Stop whatever the trigger is doing, as death, a throw or a choke does.
    /// A held single-shot area power lets go and spends the trigger.
    pub fn interrupt(&mut self, kit: &PowerKit) {
        for (i, phase) in self.phases.iter_mut().enumerate() {
            if matches!(phase, Phase::Hold { .. }) && kit.get(i as u8).is_some_and(|d| d.trigger.single) {
                self.spent = true;
            }
            *phase = Phase::Idle;
        }
    }

    fn affordable(&self, def: &PowerDef, index: u8) -> bool {
        def.cost.rounds.is_none() || self.rounds(index) > 0
    }

    fn begin(&mut self, kit: &PowerKit, index: u8, energy: &mut swbf2::State, tuning: &swbf2::Tuning, out: &mut PowerStep) {
        let Some(def) = kit.get(index) else { return };
        // A power that costs energy does not start while the stamina latch
        // holds: no wind-up, and the trigger stays unspent for when it clears.
        if !self.affordable(def, index) || !energy.can_charge(def.cost.energy) {
            return;
        }
        if def.cost.on_press {
            if !energy.charge(def.cost.energy, tuning) {
                return;
            }
            out.energy_spent += def.cost.energy;
        }
        // A single-shot cast spends the trigger as it starts; a held area
        // power spends it only when it lets go.
        if def.trigger.single && !matches!(def.effect, Effect::Area(_)) {
            self.spent = true;
        }
        self.phases[usize::from(index)] = Phase::Windup { left: def.trigger.windup };
        self.finish_windup(def, index, energy, tuning, true, out);
    }

    fn finish_windup(&mut self, def: &PowerDef, index: u8, energy: &mut swbf2::State, tuning: &swbf2::Tuning, held: bool, out: &mut PowerStep) {
        let i = usize::from(index);
        let Phase::Windup { left: 0 } = self.phases[i] else { return };
        match def.effect {
            Effect::Area(_) => {
                if held {
                    out.requests[i] = Some(Request::Acquire(index));
                    self.phases[i] = Phase::Hold { recheck: HOLD_RECHECK_TICKS };
                } else {
                    self.phases[i] = Phase::Recover { left: def.trigger.recovery };
                }
            }
            Effect::Cone(_) | Effect::Aura(_) => {
                let paid = def.cost.on_press || energy.charge(def.cost.energy, tuning);
                if paid && self.affordable(def, index) {
                    if !def.cost.on_press {
                        out.energy_spent += def.cost.energy;
                    }
                    if def.cost.rounds.is_some() {
                        self.rounds[i] -= 1;
                    }
                    out.requests[i] = Some(Request::Fire(index));
                }
                self.phases[i] = Phase::Recover { left: def.trigger.recovery };
            }
        }
    }

    /// Advance one fixed tick. `can_act` is false while the caster is dead,
    /// seated, thrown or held; it stops any wind-up or hold.
    pub fn step(&mut self, kit: &PowerKit, input: PowerInput, can_act: bool, energy: &mut swbf2::State, tuning: &swbf2::Tuning) -> PowerStep {
        let mut out = PowerStep { thrust: 1.0, ..PowerStep::default() };
        if !can_act {
            self.interrupt(kit);
            return out;
        }
        // A release, or a fresh press, makes the trigger usable again.
        if input.pressed || !input.held {
            self.spent = false;
        }
        for i in 0..MAX_POWERS {
            let index = i as u8;
            let Some(def) = kit.get(index) else {
                self.phases[i] = Phase::Idle;
                continue;
            };
            match self.phases[i] {
                Phase::Idle => {}
                // An emitter that is no longer selected drops the cast it was
                // winding up; an area weapon goes on to take its targets.
                Phase::Windup { .. } if input.selected != Some(index) && !matches!(def.effect, Effect::Area(_)) => {
                    self.phases[i] = Phase::Idle;
                }
                Phase::Windup { left } => {
                    self.phases[i] = Phase::Windup { left: left.saturating_sub(1) };
                    self.finish_windup(def, index, energy, tuning, input.held, &mut out);
                }
                Phase::Hold { recheck } => match def.effect {
                    Effect::Area(area) if input.held && !energy.exhausted => {
                        let recheck = recheck.saturating_sub(1);
                        out.requests[i] = Some(Request::Sustain { power: index, recheck: recheck == 0 });
                        // Only the selected weapon is asked for its energy
                        // rate and its hold on the caster's movement.
                        if input.selected == Some(index) {
                            out.energy_rate = (def.cost.rate > 0.0).then_some(-def.cost.rate);
                            out.thrust = area.thrust;
                        }
                        self.phases[i] = Phase::Hold { recheck: if recheck == 0 { HOLD_RECHECK_TICKS } else { recheck } };
                    }
                    _ => {
                        self.phases[i] = Phase::Recover { left: def.trigger.recovery };
                        self.spend_on_let_go(def, input.held);
                    }
                },
                Phase::Recover { left } => {
                    let left = left.saturating_sub(1);
                    self.phases[i] = if left == 0 { Phase::Idle } else { Phase::Recover { left } };
                }
            }
            // The selected power starts from the trigger while it is down and
            // unspent: a repeating trigger casts again for as long as it is held.
            let start = input.selected == Some(index) && (input.pressed || input.held) && !self.spent;
            if start && self.phases[i] == Phase::Idle {
                self.begin(kit, index, energy, tuning, &mut out);
            }
        }
        out.casting = input.selected.filter(|&i| self.is_active(i)).or(self.active());
        out
    }

    fn spend_on_let_go(&mut self, def: &PowerDef, held: bool) {
        if def.trigger.single && held {
            self.spent = true;
        }
    }

    /// The world found no target for `power` to hold, or lost the held one.
    /// A single-shot area power spends the trigger as it lets go.
    pub fn release(&mut self, kit: &PowerKit, power: u8, held: bool) {
        let i = usize::from(power);
        if let (Some(Phase::Hold { .. }), Some(def)) = (self.phases.get(i).copied(), kit.get(power)) {
            let left = def.trigger.recovery;
            self.phases[i] = if left == 0 { Phase::Idle } else { Phase::Recover { left } };
            self.spend_on_let_go(def, held);
        }
    }
}

/// Seconds to ticks for timers measured in seconds.
pub fn seconds_to_ticks(seconds: f64) -> u32 {
    v8math::ceil(seconds * TICKS_PER_SECOND - 1e-9) as u32
}
