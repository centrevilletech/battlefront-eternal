//! game.js's flightAssists setting and F2Sim.foreignIn's phase flags.
//! AI's profile tier retains the admitted easy/medium/hard selection;
//! mission skill overrides do not change that player's flight preference.
use crate::abi::context::Context;
use crate::ai::control::difficulty::{HARD, MEDIUM};
use crate::gen::F2PhaseFlag;

#[derive(Default)]
pub(super) struct Policy {
    assists: Option<bool>,
}

impl Policy {
    /// game.js maps the difficulty setting to TIER before admitting AI.
    /// A world without an AI owner retains the default medium setting.
    pub(super) fn admit(&mut self, context: &Context) -> Result<(), String> {
        // An explicit host setting can arrive before admission starts.
        if self.assists.is_some() {
            return Ok(());
        }
        *self = context
            .world(|w| {
                w.ai.battle().map_or_else(
                    || Self::for_tier(MEDIUM),
                    |battle| Self::for_tier(battle.settings.difficulty.borrow().profile_tier),
                )
            })
            .map_err(|e| e.message().to_string())?;
        Ok(())
    }

    fn for_tier(tier: f64) -> Self {
        Self {
            assists: Some(tier != HARD),
        }
    }

    /// The explicit host preference also applies after the first tick.
    pub(super) fn set_assists(&mut self, on: bool) {
        self.assists = Some(on);
    }

    /// Preserve the flight preference alongside the ordinary world flags.
    /// Dropping Assists disables the engine's target and steering helpers.
    pub(super) fn flags(&self) -> u32 {
        (1 << F2PhaseFlag::Poses as u32)
            | (1 << F2PhaseFlag::Flight as u32)
            | (1 << F2PhaseFlag::Cosmetic as u32)
            | if self.assists.unwrap_or(true) {
                1 << F2PhaseFlag::Assists as u32
            } else {
                0
            }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::control::difficulty::EASY;

    #[test]
    fn difficulty_controls_flight_assistance_without_disabling_flight() {
        let assist = 1 << F2PhaseFlag::Assists as u32;
        let flight = 1 << F2PhaseFlag::Flight as u32;
        for tier in [EASY, MEDIUM] {
            assert_ne!(Policy::for_tier(tier).flags() & assist, 0);
        }
        assert_eq!(Policy::for_tier(HARD).flags() & assist, 0);
        assert_ne!(Policy::for_tier(HARD).flags() & flight, 0);
        let context = Context::new();
        let mut policy = Policy::default();
        policy.admit(&context).unwrap();
        assert_eq!(policy.flags(), Policy::for_tier(MEDIUM).flags());
        let mut explicit = Policy::default();
        explicit.set_assists(false);
        explicit.admit(&context).unwrap();
        assert_eq!(explicit.flags() & assist, 0);
        explicit.set_assists(true);
        assert_ne!(explicit.flags() & assist, 0);
    }
}
