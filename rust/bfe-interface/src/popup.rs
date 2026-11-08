//! Modal confirmation, notice, and key-capture popups (`src/menus/popup.js`).
//!
//! Ports `confirmPopup`, `okPopup`, and `keyPopup` from `src/menus/popup.js`
//! with the exact geometry of `draw_popup_box` in `rust/bfe-shell/src/render.rs`
//! and `src/menus/popup.js`.

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
        tab: String,
        action: String,
        code: String,
    },
    /// Player cleared `(tab, action)` via `Delete` or `Backspace`.
    Cleared { tab: String, action: String },
    /// Player cancelled via `Escape`.
    Cancelled,
    /// Key was ignored (not bindable).
    Ignored,
}

/// Active key-binding capture modal state (`keyPopup` in `src/menus/popup.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyCaptureModal {
    pub tab: String,
    pub action: String,
    pub label: String,
    pub prompt: String,
    pub hint: String,
}

impl KeyCaptureModal {
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

    /// Processes a key or mouse button code (`keyPopup.onKey` in `src/menus/popup.js`).
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

/// Builds a `YES` / `NO` confirmation [`MenuScreen`] (`confirmPopup` in `src/menus/popup.js`).
pub fn confirm_popup(message: &str, strings: &HashMap<String, String>) -> MenuScreen {
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
    screen.modal = false;
    screen.dim_behind = false;
    screen.focus_policy = ScreenFocus::First;
    screen.focus_first();
    screen
}

/// Builds a single-button `OK` notice [`MenuScreen`] (`okPopup` in `src/menus/popup.js`).
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
pub fn key_popup(
    tab: &str,
    action: &str,
    label: &str,
    strings: &HashMap<String, String>,
) -> MenuScreen {
    let modal = KeyCaptureModal::new(tab, action, label, strings);
    let mut box_panel = PanelWidget::new("window", 0.0, 0.0, 360.0, 180.0);
    box_panel.safe = false;
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
    let mut screen = MenuScreen::new("rebind", vec![Widget::Panel(box_panel), Widget::Text(msg)]);
    screen.modal = false;
    screen.dim_behind = false;
    screen.key_capture = Some(modal);
    screen
}
