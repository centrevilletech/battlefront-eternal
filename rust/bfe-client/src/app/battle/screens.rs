//! Battlefront II presentation entry for either an observed world or a recorded battle.
mod art;
mod flight;
mod launcher;
mod live;
mod view;
pub use art::ScreenArt;
pub(super) use live::Live;

use super::shortcuts::{Overlays, Route};
use crate::input::{BattleInput, InputTrigger};
use bfe_battle_ui::battle2::{
    open_for_map, Action, BattleView, Hit, Phase, Response, Screens, Seat, SlotKind,
};
use bfe_family::GameFamily;
use bfe_interface::hotkeys::ControlContext;
use std::path::Path;
use winit::keyboard::KeyCode;

/// Admission opens only the selected game's resources. The caller owns the released controllers.
pub enum Presentation<T> {
    Released(T),
    BattlefrontII(Box<Screens>),
}

impl<T> Presentation<T> {
    pub fn admit(
        family: GameFamily,
        released: impl FnOnce() -> Result<T, String>,
        root: &Path,
        locale: &str,
        theme: &str,
        settings: &bfe_interface::Settings,
    ) -> Result<Self, String> {
        let mut presentation = open_for_map(
            family,
            || released().map(Self::Released),
            || {
                Screens::open(root, locale, theme)
                    .map(Box::new)
                    .map(Self::BattlefrontII)
            },
        )?;
        presentation.apply_bindings(settings);
        Ok(presentation)
    }

    /// Refresh cached key captions when the player's bindings change during a battle.
    pub fn apply_bindings(&mut self, settings: &bfe_interface::Settings) {
        if let Self::BattlefrontII(screens) = self {
            apply_screen_bindings(screens, settings);
        }
    }
}

pub(super) fn apply_screen_bindings(screens: &mut Screens, settings: &bfe_interface::Settings) {
    screens.takeoff_key = crate::app::control_strings::takeoff_caption(GameFamily::Swbf2, settings, |key| {
        screens.catalog.localized(key)
    });
}

pub fn context(v: &BattleView<'_>) -> ControlContext {
    if let Some(vehicle) = v.vehicle {
        return match vehicle.seat {
            Seat::Pilot => ControlContext::Starfighter,
            Seat::Gunner => ControlContext::Turret,
        };
    }
    if v.body
        .is_some_and(|b| b.slots.iter().flatten().any(|s| s.kind == SlotKind::Saber))
    {
        ControlContext::Jedi
    } else {
        ControlContext::Infantry
    }
}

/// A handled event returns a local UI response. The spawn launcher, shared with
/// the 2004 game, takes its own keys and buttons while the player chooses.
pub fn device_event(
    screens: &mut Screens,
    input: &mut BattleInput,
    v: &BattleView<'_>,
    trigger: InputTrigger,
    down: bool,
    repeat: bool,
    pointer: [f64; 2],
) -> Option<Response> {
    input.set_context(context(v));
    if trigger == InputTrigger::Key(KeyCode::Escape) && matches!(v.phase, Phase::Ended(_)) {
        return None;
    }
    let overlays = Overlays {
        scoreboard: screens.menu.board || matches!(v.phase, Phase::Ended(_)),
        ..Default::default()
    };
    let route = overlays.route(input, trigger, down, repeat, false);
    let action = match route {
        Route::MenuKey("Tab") => Action::Roster(true),
        Route::ReleaseRoster => Action::Roster(false),
        Route::Physical if down && overlays.scoreboard => match trigger {
            InputTrigger::Key(KeyCode::ArrowUp) | InputTrigger::WheelUp => Action::Previous,
            InputTrigger::Key(KeyCode::ArrowDown) | InputTrigger::WheelDown => Action::Next,
            InputTrigger::Key(KeyCode::Enter | KeyCode::Space) if !repeat => Action::Confirm,
            InputTrigger::Key(KeyCode::Escape | KeyCode::Backspace) | InputTrigger::Mouse(2) => {
                Action::Back
            }
            InputTrigger::Mouse(0) => {
                let hit = screens.hit(pointer);
                if hit == Hit::None {
                    return Some(Response::None);
                }
                Action::Hit(hit)
            }
            _ => return None,
        },
        _ => return None,
    };
    Some(screens.input(v, action))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_saved_rebind_refreshes_the_battle_prompt_without_changing_the_other_game() {
        use bfe_interface::{SettingValue, Settings};
        let mut settings = Settings::new();
        settings.set(
            "flightBindings",
            SettingValue::Bindings(std::collections::BTreeMap::from([(
                "takeoff".into(),
                vec!["KeyP".into()],
            )])),
        );
        let mut presentation = Presentation::<()>::BattlefrontII(Box::new(Screens::new(
            bfe_render::hud_model2::catalog::Catalog::default(),
            "eternal",
        )));
        for (code, expected) in [(Some("KeyJ"), "J"), (Some("KeyK"), "K"), (None, "")] {
            ControlContext::Starfighter.rebind(&mut settings, "takeoff", code);
            presentation.apply_bindings(&settings);
            let Presentation::BattlefrontII(screens) = &presentation else {
                unreachable!()
            };
            assert_eq!(screens.takeoff_key, expected);
            assert_eq!(
                crate::app::control_strings::takeoff_caption(GameFamily::Swbf1, &settings, |_| {
                    None
                }),
                "P"
            );
        }
    }

    #[test]
    fn released_admission_needs_no_second_game_directory() {
        let result = Presentation::admit(
            GameFamily::Swbf1,
            || Ok(49),
            Path::new("absent-installation"),
            "xx",
            "bf2",
            &bfe_interface::Settings::new(),
        )
        .unwrap();
        assert!(matches!(result, Presentation::Released(49)));
    }
}

/// HUD art and font roles acquired at admission. The renderer owns all texture handles.
pub struct Surface<'a> {
    pub hud: bfe_battle_ui::HudBatchSurface<'a>,
    pub solid: u32,
    assets: &'a std::collections::HashMap<u32, bfe_render::interface::Asset>,
}
impl bfe_battle_ui::BattleUiSurface for Surface<'_> {
    fn height(&self) -> f64 {
        self.hud.height
    }
    fn aspect(&self) -> f64 {
        self.hud.aspect
    }
    fn solid_texture(&self) -> u32 {
        self.solid
    }
    fn texture(&self, name: &str) -> u32 {
        self.hud.texture(name)
    }
    fn font_height(&self, i: usize) -> f64 {
        self.hud.font_height(i)
    }
    fn font_width(&self, i: usize, s: &[u16], scale: f64, spacing: f64) -> f64 {
        self.hud.font_width(i, s, scale, spacing)
    }
    fn asset(&self, i: u32) -> bfe_render::interface::Asset {
        self.assets.get(&i).copied().unwrap_or_default()
    }
    fn batch_mut(&mut self) -> &mut bfe_render::interface::Batch {
        self.hud.batch
    }
    fn font(&self, i: usize) -> &bfe_render::interface::Font {
        self.hud.font(i)
    }
    fn batch_and_font(
        &mut self,
        i: usize,
    ) -> (
        &mut bfe_render::interface::Batch,
        &bfe_render::interface::Font,
    ) {
        self.hud.batch_and_font(i)
    }
}

#[cfg(test)]
mod device_tests {
    use super::*;
    use bfe_battle_ui::battle2::{BattleView, Caption, Mode, Phase, Team};
    use bfe_render::hud_model2::catalog::Catalog;

    #[test]
    fn rebound_roster_releases_after_a_context_change_and_the_spawn_phase_leaves_keys_to_the_launcher() {
        let mut settings = bfe_interface::Settings::new();
        ControlContext::Infantry.rebind(&mut settings, "ui.roster", Some("Space"));
        let mut input = BattleInput::default();
        input.set_profile(GameFamily::Swbf2, &settings);
        let mut screens = Screens::new(Catalog::default(), "bf1");
        let team = Team {
            id: 4,
            name: Caption::Literal("Asters"),
            icon: "",
            score: 0,
            target: None,
            reinforcements: None,
            show_score: false,
        };
        let mut v = BattleView {
            tick: 1,
            phase: Phase::Playing,
            mode: Mode::Other,
            player: 8,
            team: 4,
            map_name: Caption::Literal("Vale"),
            map_image: "",
            teams: [team, Team { id: 6, ..team }],
            body: None,
            vehicle: None,
            aim: bfe_battle_ui::battle2::Aim::default(),
            target: None,
            contacts: &[],
            systems: &[],
            events: &[],
            objectives: &[],
            timers: &[],
            scores: &[],
            leaving: false,
        };
        let key = InputTrigger::Key(KeyCode::Space);
        assert!(device_event(
            &mut screens,
            &mut input,
            &v,
            InputTrigger::Key(KeyCode::Tab),
            true,
            false,
            [0.; 2]
        )
        .is_none());
        assert_eq!(
            device_event(&mut screens, &mut input, &v, key, true, false, [0.; 2]),
            Some(Response::None)
        );
        assert!(screens.menu.board);
        assert_eq!(
            device_event(&mut screens, &mut input, &v, key, false, false, [0.; 2]),
            Some(Response::None)
        );
        assert!(!screens.menu.board);
        v.phase = Phase::Spawn;
        for code in [KeyCode::ArrowUp, KeyCode::ArrowLeft, KeyCode::Enter, KeyCode::Escape] {
            let trigger = InputTrigger::Key(code);
            assert!(device_event(&mut screens, &mut input, &v, trigger, true, false, [0.; 2]).is_none());
        }
        v.phase = Phase::Ended(bfe_battle_ui::battle2::Outcome::Draw);
        let escape = InputTrigger::Key(KeyCode::Escape);
        assert!(device_event(&mut screens, &mut input, &v, escape, true, false, [0.; 2]).is_none());
    }
}

impl Live {
    pub(super) fn overlay(
        &self,
        receiver: &crate::online::receiver::Receiver,
        hud: &crate::online::hud::Hud,
    ) -> bool {
        self.screens.menu.board
            || receiver
                .mission_view
                .as_deref()
                .is_some_and(|s| view::phase(s, receiver, hud) != Phase::Playing)
    }

    pub(super) fn draw_live(
        &mut self,
        receiver: &crate::online::receiver::Receiver,
        hud: &crate::online::hud::Hud,
        camera: &crate::scene::View,
        projection: bfe_battle_ui::spawn::view::MapProjection,
        batch: &mut bfe_render::interface::Batch,
        size: [i32; 2],
        fraction: f32,
    ) -> bool {
        let Some(state) = receiver.mission_view.as_deref() else {
            return false;
        };
        self.prepare_team_labels(state);
        let cockpit = receiver.space.as_ref().and_then(|history|
            self.flight.sample(history, receiver.clock.shown, Some(camera), &self.screens.catalog));
        let slot_icons = view::slot_icons(state, &self.screens.catalog, &self.art);
        batch.begin();
        view::observed(
            state,
            receiver,
            hud,
            projection,
            self.mode,
            &self.map_name,
            &self.map_image,
            self.team_labels.as_ref().unwrap(),
            &self.names,
            slot_icons,
            cockpit.as_ref(),
            Some(camera),
            |v| {
                let fraction = cockpit.as_ref().filter(|_| v.vehicle.is_some()).map_or(fraction, |c| c.fraction);
                self.art
                    .draw(&mut self.screens, batch, v, size.map(f64::from), fraction)
            },
        );
        true
    }

    pub(super) fn event(
        &mut self,
        receiver: &crate::online::receiver::Receiver,
        hud: &crate::online::hud::Hud,
        projection: bfe_battle_ui::spawn::view::MapProjection,
        input: &mut BattleInput,
        trigger: InputTrigger,
        down: bool,
        repeat: bool,
        cursor: [f64; 2],
        done: &mut bool,
    ) -> bool {
        let Some(state) = receiver.mission_view.as_deref() else {
            return false;
        };
        self.prepare_team_labels(state);
        let cockpit = receiver.space.as_ref().and_then(|history|
            self.flight.sample(history, receiver.clock.shown, None, &self.screens.catalog));
        let slot_icons = view::slot_icons(state, &self.screens.catalog, &self.art);
        let handled = view::observed(
            state,
            receiver,
            hud,
            projection,
            self.mode,
            &self.map_name,
            &self.map_image,
            self.team_labels.as_ref().unwrap(),
            &self.names,
            slot_icons,
            cockpit.as_ref(),
            None,
            |v| {
                let Some(response) =
                    device_event(&mut self.screens, input, v, trigger, down, repeat, cursor)
                else {
                    return false;
                };
                match response {
                    Response::Done => *done = true,
                    Response::None => {}
                }
                true
            },
        );
        handled
    }
}
