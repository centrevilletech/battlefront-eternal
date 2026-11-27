use crate::input::{BattleInput, InputTrigger};
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Default)]
pub(super) struct Overlays {
    pub chat: bool,
    pub pause: bool,
    pub spawn: bool,
    pub scoreboard: bool,
}

#[derive(Debug, PartialEq)]
pub(super) enum Route {
    Physical,
    Gameplay(KeyCode),
    MenuKey(&'static str),
    ReleaseRoster,
}

impl Overlays {
    pub fn route(self, input: &mut BattleInput, trigger: InputTrigger, down: bool, repeat: bool, modified: bool) -> Route {
        let gameplay = !self.chat && !self.pause && !self.spawn && !self.scoreboard;
        // Menus receive the physical press. A release can still finish a roster hold begun in gameplay.
        if down && !gameplay {
            return Route::Physical;
        }
        if let Some(key) = input.overlay_event(trigger, down, !repeat) {
            if !down {
                return Route::ReleaseRoster;
            }
            let modifier_key = matches!(trigger, InputTrigger::Key(
                KeyCode::AltLeft | KeyCode::AltRight | KeyCode::ControlLeft | KeyCode::ControlRight
                    | KeyCode::ShiftLeft | KeyCode::ShiftRight
            ));
            if modified && !modifier_key && matches!(key, "KeyT" | "KeyY") {
                return Route::Physical;
            }
            return Route::MenuKey(key);
        }
        if gameplay {
            if let InputTrigger::Key(key @ (KeyCode::Tab | KeyCode::KeyT | KeyCode::KeyY)) = trigger {
                return Route::Gameplay(key);
            }
        }
        Route::Physical
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_family::GameFamily;
    use bfe_interface::{hotkeys::ControlContext::Infantry, Settings};

    fn game(settings: &Settings) -> BattleInput {
        let mut input = BattleInput::default();
        input.set_profile(GameFamily::Swbf2, settings);
        input
    }

    #[test]
    fn spawn_and_scoreboard_keep_the_physical_keys_and_buttons() {
        for code in ["Mouse0", "Mouse2", "Space", "Enter", "ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"] {
            for action in ["ui.chat", "ui.teamChat", "ui.roster"] {
                let mut settings = Settings::new();
                Infantry.rebind(&mut settings, action, Some(code));
                let mut input = game(&settings);
                let trigger = InputTrigger::from_dom_code(code).unwrap();
                for overlay in [Overlays { spawn: true, ..Default::default() }, Overlays { scoreboard: true, ..Default::default() }] {
                    assert_eq!(overlay.route(&mut input, trigger, true, false, false), Route::Physical, "{code} {action}");
                    assert_eq!(overlay.route(&mut input, trigger, false, false, false), Route::Physical);
                    let backslash = InputTrigger::Key(KeyCode::Backslash);
                    assert_eq!(overlay.route(&mut input, backslash, true, false, false), Route::Physical);
                }
            }
        }
    }

    #[test]
    fn raw_chat_keys_cannot_open_chat_behind_a_menu() {
        let mut settings = Settings::new();
        Infantry.rebind(&mut settings, "ui.chat", Some("Mouse0"));
        Infantry.rebind(&mut settings, "ui.teamChat", Some("Mouse2"));
        let mut input = game(&settings);
        for key in [KeyCode::KeyT, KeyCode::KeyY] {
            for overlay in [Overlays { spawn: true, ..Default::default() }, Overlays { scoreboard: true, ..Default::default() }] {
                assert_eq!(overlay.route(&mut input, InputTrigger::Key(key), true, false, false), Route::Physical);
            }
        }
    }

    #[test]
    fn the_modifier_bound_to_chat_can_open_its_channel() {
        let focus = Overlays::default();
        for code in ["ControlLeft", "ControlRight", "AltLeft", "AltRight", "ShiftLeft", "ShiftRight"] {
            for (action, shortcut) in [("ui.chat", "KeyT"), ("ui.teamChat", "KeyY")] {
                let mut settings = Settings::new();
                Infantry.rebind(&mut settings, action, Some(code));
                let mut input = game(&settings);
                let key = InputTrigger::from_dom_code(code).unwrap();
                assert_eq!(focus.route(&mut input, key, true, false, true), Route::MenuKey(shortcut), "{code}");
            }
        }
        let mut input = game(&Settings::new());
        assert_ne!(focus.route(&mut input, InputTrigger::Key(KeyCode::KeyT), true, false, true), Route::MenuKey("KeyT"));
    }

    #[test]
    fn releasing_the_roster_opener_leaves_the_physical_release_for_the_menu() {
        let mut settings = Settings::new();
        Infantry.rebind(&mut settings, "ui.roster", Some("Space"));
        let mut input = game(&settings);
        let key = InputTrigger::Key(KeyCode::Space);
        assert_eq!(Overlays::default().route(&mut input, key, true, false, false), Route::MenuKey("Tab"));
        let board = Overlays { scoreboard: true, ..Default::default() };
        assert_eq!(board.route(&mut input, key, false, false, false), Route::ReleaseRoster);
        assert_eq!(board.route(&mut input, key, false, false, false), Route::Physical);
    }
}
