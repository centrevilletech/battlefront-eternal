//! What a battle's game puts on the wire. Both games share action IDs 0 to 20
//! (21 actions) and the 104 physical codes; the 2005 game adds the roll action, block's
//! durable press and release actions and the on-foot sprint alias. A 2004
//! battle sends exactly the inputs it always sent, in the same encoding.
use crate::generated_input::{ClientAction, ClientHeld};
use bfe_family::GameFamily;
use bfe_transport::layout::{INPUT_ACTIONS, INPUT_CODES, SWBF2_INPUT_ACTIONS, SWBF2_INPUT_CODES};

/// The wire code a held sprint uses on foot, whatever key the player bound.
pub const SPRINT_CODE: &str = "ShiftLeft";

/// What a 2005 soldier holds that no released command carries, each with the
/// wire code that stands for it on foot, whatever key the player bound.
pub const SOLDIER_CODES: [&str; 3] = ["KeyC", "KeyR", "KeyB"];

/// What a 2005 soldier holds or taps beyond his command, for the authority's
/// combat; nothing here predicts it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SoldierInput {
    /// Block went down since the last command, whatever its level is now.
    pub block_pressed: bool,
    pub crouch: bool,
    pub reload: bool,
    /// His setting: block lasts as long as its control is held.
    pub hold_to_block: bool,
    /// The power lock key went down since the last command.
    pub lock_pressed: bool,
}

impl SoldierInput {
    /// The levels, in the order of [`SOLDIER_CODES`].
    pub fn held(self) -> [bool; 3] {
        [self.crouch, self.reload, self.hold_to_block]
    }
}

/// ClientAction bits a 2004 battle sends: every action before the 2005 ones.
const SWBF1_ACTIONS: u32 = (1 << ClientAction::Roll as u32) - 1;
/// ClientAction bits only an on-foot 2005 soldier sends. A weapon's lock-on on
/// foot is resolved by the client and has no wire form; a hero's power lock
/// press is an action edge of its own ([`WireInputs::lock_action`]).
const SWBF2_FOOT_ACTIONS: u32 = 1 << ClientAction::Roll as u32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WireInputs {
    pub family: GameFamily,
}

impl WireInputs {
    pub const fn new(family: GameFamily) -> Self {
        Self { family }
    }
    /// The battle's action table, indexed by wire action ID.
    pub fn actions(self) -> &'static [&'static str] {
        match self.family {
            GameFamily::Swbf1 => &INPUT_ACTIONS,
            GameFamily::Swbf2 => &SWBF2_INPUT_ACTIONS,
        }
    }
    /// The battle's held code table, indexed by bitmap position.
    pub fn codes(self) -> &'static [&'static str] {
        match self.family {
            GameFamily::Swbf1 => &INPUT_CODES,
            GameFamily::Swbf2 => &SWBF2_INPUT_CODES,
        }
    }
    /// The ClientAction bits this battle may send, on foot or flying.
    pub fn action_bits(self, on_foot: bool) -> u32 {
        match self.family {
            GameFamily::Swbf1 => SWBF1_ACTIONS,
            GameFamily::Swbf2 if on_foot => SWBF1_ACTIONS | SWBF2_FOOT_ACTIONS,
            GameFamily::Swbf2 => SWBF1_ACTIONS,
        }
    }
    /// The on-foot holds beyond the shared nine, as (ClientHeld bit, wire code).
    pub fn foot_holds(self) -> &'static [(u32, &'static str)] {
        match self.family {
            GameFamily::Swbf1 => &[],
            GameFamily::Swbf2 => &[(ClientHeld::Sprint as u32, SPRINT_CODE)],
        }
    }
    /// The codes a soldier's own levels use ([`SoldierInput::held`]).
    pub fn soldier_codes(self) -> &'static [&'static str] {
        match self.family {
            GameFamily::Swbf1 => &[],
            GameFamily::Swbf2 => &SOLDIER_CODES,
        }
    }
    /// Block's press and release action IDs, for a battle that has block.
    /// Each change of the held block control on foot is one durable edge.
    pub fn block_actions(self) -> Option<(usize, usize)> {
        Some((self.action_id("blockPress")?, self.action_id("blockRelease")?))
    }
    /// The power lock press's action ID, for a 2005 battle: its soldier sends
    /// the press on foot. A 2004 battle's `targetLock` is a pilot's alone.
    pub fn lock_action(self) -> Option<usize> {
        self.action_id("targetLock").filter(|_| self.family == GameFamily::Swbf2)
    }
    /// The wire action ID of a ClientAction name in this battle, if it has one.
    pub fn action_id(self, name: &str) -> Option<usize> {
        self.actions().iter().position(|n| *n == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_2004_battle_keeps_the_released_tables_and_bits() {
        let inputs = WireInputs::new(GameFamily::Swbf1);
        assert_eq!(inputs.actions(), &INPUT_ACTIONS[..]);
        assert_eq!(inputs.codes(), &INPUT_CODES[..]);
        assert_eq!(inputs.action_bits(true), 0x7fff);
        assert_eq!(inputs.action_bits(false), 0x7fff);
        assert!(inputs.foot_holds().is_empty() && inputs.soldier_codes().is_empty());
        assert_eq!(inputs.action_id("roll"), None);
        assert_eq!(inputs.block_actions(), None);
    }

    #[test]
    fn the_2005_tables_extend_the_2004_ones() {
        let inputs = WireInputs::new(GameFamily::Swbf2);
        assert_eq!(&inputs.actions()[..INPUT_ACTIONS.len()], &INPUT_ACTIONS[..]);
        assert_eq!(&inputs.codes()[..INPUT_CODES.len()], &INPUT_CODES[..]);
        assert_eq!(inputs.action_id("roll"), Some(21));
        assert_eq!(inputs.block_actions(), Some((22, 23)));
        assert_eq!(inputs.codes(), &INPUT_CODES[..]);
        assert_eq!(inputs.codes().iter().position(|c| *c == SPRINT_CODE), Some(64));
        // Each soldier's level has a code of its own, which nothing else uses on foot.
        let others = ["KeyW", "KeyS", "KeyD", "KeyA", "NumpadMultiply", "NumpadDivide", "Mouse0", "Mouse2", "Space", SPRINT_CODE, "ControlLeft"];
        for (at, code) in SOLDIER_CODES.iter().enumerate() {
            assert!(inputs.codes().contains(code) && !others.contains(code) && !SOLDIER_CODES[..at].contains(code), "{code}");
        }
        let held = SoldierInput { crouch: true, hold_to_block: true, ..Default::default() }.held();
        assert_eq!(held, [true, false, true]);
        assert_eq!(inputs.action_bits(true), 0xffff);
        assert_eq!(inputs.action_bits(false), 0x7fff);
    }
}
