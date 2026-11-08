//! Single menu screen container (`src/menus/screen.js`).
//!
//! Ports `MenuScreen` from `src/menus/screen.js`: widget tree, focus management
//! (`"first"` vs `"keep"`), modal/dimming flags, elapsed time, and sound cue emission.
//!
//! Focus follows the 2004 PC shell convention (`src/menus/screen.js`):
//! - One widget (and one row/button/tab inside it) holds focus at a time, rendered pulsing yellow or bright blue.
//! - Keyboard `ArrowUp`/`ArrowDown` and `Tab` walk focusable items; pointer hover sets focus directly.
//! - `Enter`/`Space` or a left click activates the focused item; `Escape` triggers [`MenuScreen::back`].

use crate::popup::KeyCaptureModal;
use crate::sounds::SoundCue;
use crate::widgets::{Widget, WidgetAction};

/// Focus retention policy when entering a [`MenuScreen`] (`focus: "first" | "keep"` in `src/menus/screen.js`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ScreenFocus {
    /// Reset focus to the first focusable widget on enter (`"first"`).
    #[default]
    First,
    /// Preserve the previously focused widget when returning to this screen (`"keep"`).
    Keep,
}

/// One screen or modal popup on a [`crate::stack::ScreenStack`] (`MenuScreen` in `src/menus/screen.js`).
#[derive(Clone, Debug)]
pub struct MenuScreen {
    /// Screen identifier (`"options"`, `"game"`, `"video"`, `"audio"`, `"controls"`, `"confirm"`, etc.).
    pub id: String,
    /// Ordered list of widgets painted back-to-front by [`crate::theme::ClassicTheme`].
    pub widgets: Vec<Widget>,
    /// Index into `widgets` of the currently focused widget, if any (`MenuScreen.focused` in `screen.js`).
    pub focused: Option<usize>,
    /// Whether `enter()` resets focus to the first focusable widget or preserves `focused` (`focusMode` in `screen.js`).
    pub focus_policy: ScreenFocus,
    /// When `true`, this screen is a modal overlay (`popup: true` in `screen.js`) drawn over the screen beneath it.
    pub modal: bool,
    /// When `true`, draws a half-alpha black backdrop (`DIM`) behind the screen (`dim: true` in `screen.js`).
    pub dim_behind: bool,
    /// Elapsed time in seconds since this screen was last entered (`MenuScreen.time` in `screen.js`).
    pub elapsed: f64,
    /// Optional key-capture state when this screen is a keybinding popup (`grabsKeys` in `popup.js`).
    pub key_capture: Option<KeyCaptureModal>,
}

impl MenuScreen {
    /// Creates a new [`MenuScreen`] with `id` and `widgets`, focusing the first focusable widget.
    pub fn new(id: impl Into<String>, widgets: Vec<Widget>) -> Self {
        let mut s = Self {
            id: id.into(),
            widgets,
            focused: None,
            focus_policy: ScreenFocus::First,
            modal: false,
            dim_behind: false,
            elapsed: 0.0,
            key_capture: None,
        };
        s.focus_first();
        s
    }

    /// Looks up a widget by id (`MenuScreen.widget(id)` in `src/menus/screen.js`).
    pub fn widget(&self, id: &str) -> Option<&Widget> {
        self.widgets.iter().find(|w| w.id() == id)
    }

    /// Looks up a mutable widget by id.
    pub fn widget_mut(&mut self, id: &str) -> Option<&mut Widget> {
        self.widgets.iter_mut().find(|w| w.id() == id)
    }

    /// Returns the index of the currently focused widget, if valid and still focusable.
    /// Guards against widgets that became hidden or disabled after receiving focus.
    pub fn focused_index(&self) -> Option<usize> {
        let idx = self.focused?;
        if self.widgets.get(idx).is_some_and(Widget::can_focus) {
            Some(idx)
        } else {
            None
        }
    }

    /// Returns a reference to the currently focused [`Widget`].
    pub fn focused_widget(&self) -> Option<&Widget> {
        self.focused_index().and_then(|i| self.widgets.get(i))
    }

    /// Returns a mutable reference to the currently focused [`Widget`].
    pub fn focused_widget_mut(&mut self) -> Option<&mut Widget> {
        let idx = self.focused_index()?;
        self.widgets.get_mut(idx)
    }

    /// Focuses the first focusable widget (`this.ring()[0]` in `MenuScreen.enter` in `src/menus/screen.js`),
    /// and resets a [`Widget::Rows`] selection to its first enabled row.
    pub fn focus_first(&mut self) {
        self.focused = self.widgets.iter().position(Widget::can_focus);
        if let Some(Widget::Rows(rows)) = self.focused_widget_mut() {
            if let Some(first) = rows.first_enabled() {
                rows.selected = first;
            }
        }
    }

    /// Focuses the widget with `id` if it is focusable (`MenuScreen.setFocus` in `src/menus/screen.js`).
    /// Returns `true` if the focused widget index changed.
    pub fn set_focus_by_id(&mut self, id: &str) -> bool {
        if let Some(idx) = self
            .widgets
            .iter()
            .position(|w| w.id() == id && w.can_focus())
        {
            let changed = self.focused != Some(idx);
            self.focused = Some(idx);
            changed
        } else {
            false
        }
    }

    /// Cycles focus across focusable widgets by `delta` (`+1` or `-1`), wrapping around
    /// the focus ring (`MenuScreen.move` in `src/menus/screen.js`).
    pub fn cycle_focus(&mut self, delta: i32) -> bool {
        let focusable: Vec<usize> = self
            .widgets
            .iter()
            .enumerate()
            .filter_map(|(i, w)| w.can_focus().then_some(i))
            .collect();
        if focusable.is_empty() {
            self.focused = None;
            return false;
        }
        // Map the current widget index into `focusable`, step by `delta` modulo `focusable.len()`,
        // and write back the new widget index.
        let cur_pos = self
            .focused
            .and_then(|idx| focusable.iter().position(|&i| i == idx))
            .unwrap_or(0) as isize;
        let step = if delta < 0 { -1isize } else { 1isize };
        let next_pos = (cur_pos + step).rem_euclid(focusable.len() as isize) as usize;
        let next_idx = focusable[next_pos];
        let changed = self.focused != Some(next_idx);
        self.focused = Some(next_idx);
        changed
    }

    /// Called when the screen becomes the top of the stack (`MenuScreen.enter` in `src/menus/screen.js`).
    /// Resets `elapsed` so pulse and intro animations start fresh, and re-initializes focus unless
    /// [`ScreenFocus::Keep`] is set (used by the top-level `OPTIONS` menu so returning from a sub-page
    /// keeps the sub-page's row highlighted).
    pub fn enter(&mut self) {
        self.elapsed = 0.0;
        if self.focus_policy == ScreenFocus::First || self.focused_index().is_none() {
            self.focus_first();
        }
    }

    /// Advances the screen clock and all widget timers (`MenuScreen.update(dt)` in `src/menus/screen.js`).
    pub fn update(&mut self, dt: f64) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        self.elapsed += dt;
        for w in &mut self.widgets {
            w.update(dt);
        }
    }

    /// Handles vertical navigation (`ArrowUp` / `ArrowDown`) on the focused widget,
    /// returning `(changed, sound_cue)` (`MenuScreen.move` and `List.key` in `screen.js`/`widgets.js`).
    pub fn move_vertical(&mut self, dy: i32) -> (bool, Option<SoundCue>) {
        let Some(w) = self.focused_widget_mut() else {
            return (false, None);
        };
        let changed = match w {
            Widget::Rows(rows) => rows.move_selection(dy),
            Widget::List(list) => list.move_selection(dy),
            Widget::Options(opts) => opts.move_selection(dy, true),
            _ => false,
        };
        (changed, changed.then_some(SoundCue::SelectChange))
    }

    /// Handles horizontal navigation (`ArrowLeft` / `ArrowRight`) on the focused widget
    /// (`MenuScreen.step` in `src/menus/screen.js`).
    pub fn step_horizontal(&mut self, dx: i32) -> Option<WidgetAction> {
        // On `CONTROL OPTIONS`, pressing Left/Right while the `"bindings"` list is focused
        // steps the `"tabs"` header bar (`INFANTRY` / `VEHICLE` / `STARFIGHTER`).
        if let Some(Widget::List(list)) = self.focused_widget() {
            if list.id == "bindings" {
                if let Some(Widget::Tabs(tabs)) = self.widget_mut("tabs") {
                    return tabs.step(dx).map(WidgetAction::SelectTab);
                }
            }
        }
        let w = self.focused_widget_mut()?;
        match w {
            Widget::HorizontalButtons(hb) => {
                hb.step(dx);
                None
            }
            Widget::Tabs(tabs) => tabs.step(dx).map(WidgetAction::SelectTab),
            _ => None,
        }
    }

    /// Activates the currently focused widget (`Enter` / `Space` / click), returning its
    /// [`WidgetAction`] (`MenuScreen.accept` in `src/menus/screen.js`).
    pub fn accept(&self) -> Option<WidgetAction> {
        let w = self.focused_widget()?;
        // Dispatch to the active widget's selected row, button, or tab; disabled items
        // return `None` so pressing `Enter` on a greyed-out row is a no-op.
        match w {
            Widget::Rows(rows) => rows.accept(),
            Widget::List(list) => {
                let entry = list.rows.get(list.selected)?;
                if entry.disabled {
                    None
                } else {
                    entry.action.clone()
                }
            }
            Widget::Options(opts) => {
                let item = opts.items.get(opts.selected)?;
                if item.disabled {
                    None
                } else {
                    item.action.clone()
                }
            }
            // A `Button` with `place == "back"` and no explicit action defaults to `WidgetAction::Back`
            // (`Button.activate` in `src/menus/widgets.js`).
            Widget::Button(btn) => {
                if btn.disabled {
                    None
                } else {
                    btn.action.clone().or_else(|| {
                        if btn.place == "back" || btn.id == "back" {
                            Some(WidgetAction::Back)
                        } else {
                            None
                        }
                    })
                }
            }
            // Horizontal popup buttons (`YES` / `NO` / `OK`) return the selected button's action.
            Widget::HorizontalButtons(hb) => {
                let item = hb.buttons.get(hb.selected)?;
                if item.disabled {
                    None
                } else {
                    item.action.clone()
                }
            }
            // Activating a focused header tab selects that tab (`INFANTRY` / `VEHICLE` / `STARFIGHTER`).
            Widget::Tabs(tabs) => tabs
                .tabs
                .get(tabs.selected)
                .map(|t| WidgetAction::SelectTab(t.id.clone())),
            _ => None,
        }
    }

    /// Invokes the screen's back action (`Escape` or `BACK` button), matching
    /// `MenuScreen.back()` in `src/menus/screen.js`.
    pub fn back(&self) -> Option<WidgetAction> {
        // Prefer an explicit visible, enabled `back` button widget if the screen has one.
        for w in &self.widgets {
            if let Widget::Button(btn) = w {
                if (btn.id == "back" || btn.place == "back") && !btn.hidden && !btn.disabled {
                    return btn.action.clone().or(Some(WidgetAction::Back));
                }
            }
        }
        // Modal popups (`confirm`, `ok`, `rebind`) have no bottom `BACK` button but still close on `Escape`.
        if self.modal || self.id == "confirm" || self.id == "ok" || self.id == "rebind" {
            Some(WidgetAction::Back)
        } else {
            None
        }
    }
}
