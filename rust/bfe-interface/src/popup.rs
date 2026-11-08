//! Modal confirmation, notice, and key-capture popups (`src/menus/popup.js`).
//!
//! Ports `confirmPopup`, `okPopup`, and `keyPopup` from `src/menus/popup.js`
//! with the exact geometry of `draw_popup_box` in `rust/bfe-shell/src/render.rs`
//! and `src/menus/popup.js` (`Popup_YesNo`, `Popup_Ok`, and `ifs.GameOpt.pc_keybindpopup`
//! from the 2004 `common.lvl` scripts).
//!
//! Each popup is constructed as a [`MenuScreen`] with a centered `border_3_pieces` 9-slice
//! [`PanelWidget`] (`safe = false`), a [`TextLook::Popup`] question block at `y = -74.0`,
//! and (for `confirm_popup` and `ok_popup`) a [`HorizontalButtonsWidget`] at `y = 63.0`.

use crate::hotkeys::bindable;
use crate::options::say;
use crate::screen::{MenuScreen, ScreenFocus};
use crate::widgets::{
    HorizontalButtonItem, HorizontalButtonsWidget, PanelWidget, TextLook, TextWidget, Widget,
    WidgetAction,
};
use std::collections::HashMap;

/// Outcome of a key press or mouse button on a [`KeyCaptureModal`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyCaptureOutcome {
    /// Player bound `code` to `(tab, action)`.
    Bound {
        /// Controls tab (`"infantry"`, `"vehicle"`, or `"starfighter"`).
        tab: String,
        /// Action identifier within `tab` (`"forward"`, `"fire"`, etc.).
        action: String,
        /// Captured input code (`"KeyW"`, `"Mouse0"`, `"WheelUp"`, etc.).
        code: String,
    },
    /// Player cleared `(tab, action)` via `Delete` or `Backspace`.
    Cleared {
        /// Controls tab (`"infantry"`, `"vehicle"`, or `"starfighter"`).
        tab: String,
        /// Action identifier whose binding was cleared.
        action: String,
    },
    /// Player cancelled via `Escape`.
    Cancelled,
    /// Key was ignored (not bindable).
    Ignored,
}

/// Active key-binding capture modal state (`keyPopup` in `src/menus/popup.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyCaptureModal {
    /// Controls tab identifier (`"infantry"`, `"vehicle"`, or `"starfighter"`).
    pub tab: String,
    /// Action identifier being rebound (`"forward"`, `"fire"`, `"pitchUp"`, etc.).
    pub action: String,
    /// Localized action label shown above the prompt (`"MOVE FORWARD"`, `"PRIMARY FIRE"`, etc.).
    pub label: String,
    /// Localized prompt string (`ifs.GameOpt.pc_keybindpopup`: `"PRESS NEW KEY, ESC TO CANCEL"`).
    pub prompt: String,
    /// Secondary footer hint (`"DELETE: CLEAR     ESC: CANCEL"`).
    pub hint: String,
}

impl KeyCaptureModal {
    /// Creates a new [`KeyCaptureModal`] for `(tab, action, label)`, resolving the prompt text
    /// from `strings["ifs.GameOpt.pc_keybindpopup"]` (`keyPopup` in `src/menus/popup.js`).
    pub fn new(
        tab: impl Into<String>,
        action: impl Into<String>,
        label: impl Into<String>,
        strings: &HashMap<String, String>,
    ) -> Self {
        Self {
            tab: tab.into(),
            action: action.into(),
            label: label.into(),
            prompt: say(
                strings,
                Some("ifs.GameOpt.pc_keybindpopup"),
                "PRESS NEW KEY, ESC TO CANCEL",
            ),
            hint: "DELETE: CLEAR     ESC: CANCEL".to_string(),
        }
    }

    /// Processes a key or mouse button code (`keyPopup`'s `onKey` and `take` in `src/menus/popup.js`).
    ///
    /// - `Escape` cancels the modal without changing the binding.
    /// - `Delete` or `Backspace` clears the binding for `(tab, action)`.
    /// - Any code accepted by [`bindable`] binds `(tab, action)` to `code`.
    pub fn on_key(&self, code: &str) -> KeyCaptureOutcome {
        if code == "Escape" {
            return KeyCaptureOutcome::Cancelled;
        }
        if code == "Delete" || code == "Backspace" {
            return KeyCaptureOutcome::Cleared {
                tab: self.tab.clone(),
                action: self.action.clone(),
            };
        }
        if bindable(code) {
            return KeyCaptureOutcome::Bound {
                tab: self.tab.clone(),
                action: self.action.clone(),
                code: code.to_string(),
            };
        }
        KeyCaptureOutcome::Ignored
    }
}

/// Builds a `YES` / `NO` confirmation [`MenuScreen`] (`confirmPopup` in `src/menus/popup.js`,
/// porting the 2004 `Popup_YesNo` window).
///
/// Places a `300 x 180` 9-slice `border_3_pieces` window at the screen center (`safe = false`),
/// with the wrapped `gamefont_medium` question starting at `y = -74` and `YES` / `NO` horizontal
/// buttons at `y = 63` (`NO` focused by default so an accidental `Enter` does not confirm).
pub fn confirm_popup(message: &str, strings: &HashMap<String, String>) -> MenuScreen {
    // Center the 300x180 9-slice frame in full-screen coordinates (`safe = false`).
    let mut box_panel = PanelWidget::new("window", 0.0, 0.0, 300.0, 180.0);
    box_panel.safe = false;
    // Wrap the question at 260 units wide starting 74 units above the popup center (`classic/index.js`).
    let msg = TextWidget {
        id: "message".to_string(),
        look: TextLook::Popup,
        text: message.to_string(),
        at: [0.5, 0.5],
        safe: false,
        x: 0.0,
        y: -74.0,
        align: 2,
        font: Some("gamefont_medium"),
        color: None,
        alpha: 1.0,
        flashy: false,
        intro: crate::flashy::TitleIntro::completed(),
        hidden: false,
    };
    // Place `YES` and `NO` 85 units apart at `y = 63` below the popup center.
    let mut buttons = HorizontalButtonsWidget::new(
        "answer",
        63.0,
        vec![
            HorizontalButtonItem {
                id: "yes".to_string(),
                label: say(strings, Some("common.yes"), "YES"),
                disabled: false,
                action: Some(WidgetAction::Named("yes".to_string())),
            },
            HorizontalButtonItem {
                id: "no".to_string(),
                label: say(strings, Some("common.no"), "NO"),
                disabled: false,
                action: Some(WidgetAction::Back),
            },
        ],
    );
    buttons.selected = 1; // "NO" focused by default (`focus = "no"` in `src/menus/popup.js`)
    let mut screen = MenuScreen::new(
        "confirm",
        vec![
            Widget::Panel(box_panel),
            Widget::Text(msg),
            Widget::HorizontalButtons(buttons),
        ],
    );
    // Stand-alone popup screens rendered by `OptionsController` manage their own backdrop
    // and focus the default answer button (`"no"`) immediately on creation.
    screen.modal = false;
    screen.dim_behind = false;
    screen.focus_policy = ScreenFocus::First;
    screen.focus_first();
    screen
}

/// Builds a single-button `OK` notice [`MenuScreen`] (`okPopup` in `src/menus/popup.js`,
/// porting the 2004 `Popup_Ok` window).
///
/// Uses the centered `300 x 180` popup frame and single `OK` button (`common.ok`) that
/// emits [`WidgetAction::Back`] on `Enter`, click, or `Escape`.
pub fn ok_popup(message: &str, strings: &HashMap<String, String>) -> MenuScreen {
    let mut box_panel = PanelWidget::new("window", 0.0, 0.0, 300.0, 180.0);
    box_panel.safe = false;
    let msg = TextWidget {
        id: "message".to_string(),
        look: TextLook::Popup,
        text: message.to_string(),
        at: [0.5, 0.5],
        safe: false,
        x: 0.0,
        y: -74.0,
        align: 2,
        font: Some("gamefont_medium"),
        color: None,
        alpha: 1.0,
        flashy: false,
        intro: crate::flashy::TitleIntro::completed(),
        hidden: false,
    };
    // A single `OK` button is centered horizontally at `x = 0, y = 63` by `paint_horizontal_buttons`.
    let buttons = HorizontalButtonsWidget::new(
        "buttons",
        63.0,
        vec![HorizontalButtonItem {
            id: "ok".to_string(),
            label: say(strings, Some("common.ok"), "OK"),
            disabled: false,
            action: Some(WidgetAction::Back),
        }],
    );
    let mut screen = MenuScreen::new(
        "ok",
        vec![
            Widget::Panel(box_panel),
            Widget::Text(msg),
            Widget::HorizontalButtons(buttons),
        ],
    );
    screen.modal = false;
    screen.dim_behind = false;
    screen.focus_policy = ScreenFocus::First;
    screen.focus_first();
    screen
}

/// Builds a key-capture [`MenuScreen`] (`keyPopup` in `src/menus/popup.js`).
///
/// Uses a wider `360 x 180` window (`size: [360, 180]` in `popup.js`) so the 313-unit
/// `"PRESS NEW KEY, ESC TO CANCEL"` prompt fits on a single line of `gamefont_medium`
/// underneath the stripped action label.
pub fn key_popup(
    tab: &str,
    action: &str,
    label: &str,
    strings: &HashMap<String, String>,
) -> MenuScreen {
    let modal = KeyCaptureModal::new(tab, action, label, strings);
    let mut box_panel = PanelWidget::new("window", 0.0, 0.0, 360.0, 180.0);
    box_panel.safe = false;
    // Strip any trailing colon from the control row label before stacking it above the prompt.
    let clean_label = label.trim_end_matches(':');
    let msg = TextWidget {
        id: "message".to_string(),
        look: TextLook::Popup,
        text: format!("{clean_label}\n{}", modal.prompt),
        at: [0.5, 0.5],
        safe: false,
        x: 0.0,
        y: -74.0,
        align: 2,
        font: Some("gamefont_medium"),
        color: None,
        alpha: 1.0,
        flashy: false,
        intro: crate::flashy::TitleIntro::completed(),
        hidden: false,
    };
    // Attach `KeyCaptureModal` so `MenuInput::on_key_down`, `on_pointer_down`, and `on_wheel`
    // route the very next key, mouse button, or wheel notch to `KeyCaptureModal::on_key`.
    let mut screen = MenuScreen::new("rebind", vec![Widget::Panel(box_panel), Widget::Text(msg)]);
    screen.modal = false;
    screen.dim_behind = false;
    screen.key_capture = Some(modal);
    screen
}
