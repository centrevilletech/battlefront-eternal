//! Map admission selects one presentation for the lifetime of the battle.
mod playing;
mod screens;
mod space;
pub use screens::Screens;
mod menu;
mod paint;
pub use menu::{Action, Hit, Menu, Response};
pub use paint::Style;
pub use playing::POST_CONTACT;
mod state;
pub use state::*;
mod observation;
pub use observation::InfantrySample;

use bfe_family::GameFamily;

/// The closures may open the selected family's prepared assets or construct its controllers.
pub fn open_for_map<T>(
    family: GameFamily,
    classic: impl FnOnce() -> T,
    battlefront_ii: impl FnOnce() -> T,
) -> T {
    match family {
        GameFamily::Swbf1 => classic(),
        GameFamily::Swbf2 => battlefront_ii(),
    }
}

#[cfg(test)]
mod screens_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admission_only_opens_the_incoming_maps_screens() {
        assert_eq!(open_for_map(GameFamily::Swbf1, || 3, || panic!()), 3);
        assert_eq!(open_for_map(GameFamily::Swbf2, || panic!(), || 7), 7);
    }
}

#[cfg(test)]
mod layout_tests;
