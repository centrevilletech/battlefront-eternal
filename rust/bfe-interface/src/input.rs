//! Keyboard and pointer menu input router (`src/menus/input.js`).
//!
//! Ports `bindMenuInput` from `src/menus/input.js` and the pointer/key dispatch in
//! `src/menus/stack.js`: 3D perspective hit-testing (`local_point`), hover selection,
//! click activation, wheel scrolling, slider dragging, and keyboard navigation
//! (`ArrowUp`/`Down`/`Left`/`Right`, `PageUp`/`PageDown`, `Tab`, `Enter`/`Space`, `Escape`).
//!
//! Key and pointer routing rules:
//! - A key-capture modal (`KeyCaptureModal` on `CONTROL OPTIONS`) intercepts every key, mouse
//!   button (`Mouse0`..`Mouse4`), and scroll wheel notch (`WheelUp`/`WheelDown`) before normal navigation.
//! - Pointer hit-testing projects the 2D viewport cursor into WebGL NDC and intersects a 3D
//!   camera ray with each hotspot's local `z = 0` plane (`space::local_point`) in reverse draw
//!   order so frontmost widgets win.

use crate::options::OptionKind;
use crate::popup::KeyCaptureOutcome;
use crate::sounds::SoundCue;
use crate::space::local_point;
use crate::stack::ScreenStack;
use crate::theme::{HitRegion, HitTarget};
use crate::widgets::{Widget, WidgetAction};

/// Event produced when [`MenuInput`] processes a key or pointer event.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuInputEvent {
    /// No state or action changed (unrecognized key, click on empty space, or disabled target).
    None,
    /// Focus or selection moved inside the active screen (`ArrowUp`/`Down`, `PageUp`/`Down`, `Tab`, hover, or wheel).
    SelectionChanged,
    /// The active option row or slider should step by `dir` (`-1` or `+1`) on `option_key`.
    StepOption {
        /// Setting key in `OPTIONS` (e.g., `"fov"`, `"renderScale"`, `"masterVolume"`).
        key: String,
        /// Step direction (`-1` for `ArrowLeft`, `+1` for `ArrowRight` or `Enter`).
        dir: i32,
    },
    /// A slider option was dragged to `ratio` (`0.0..1.0`).
    SetOptionRatio {
        /// Setting key of the dragged slider row.
        key: String,
        /// Clamped `[0.0, 1.0]` horizontal position along the slider track.
        ratio: f64,
    },
    /// A widget action was activated (`OpenPage`, `Back`, `ResetPage`, `StartRebind`, `SelectTab`, `Named`).
    Action(WidgetAction),
    /// A key-capture modal completed (`Bound`, `Cleared`, `Cancelled`).
    KeyCapture(KeyCaptureOutcome),
}

/// Stateful keyboard and pointer router for a [`ScreenStack`] (`bindMenuInput` in `src/menus/input.js`
/// and `ScreenStack` pointer/key handlers in `src/menus/stack.js`).
#[derive(Clone, Debug, Default)]
pub struct MenuInput {
    /// Last pointer position in WebGL normalized device coordinates `(-1..1, -1..1)`.
    pub pointer_ndc: [f64; 2],
    /// Active slider drag state `(option_key, [track_x0, track_x1], widget_matrix)` while the left
    /// mouse button is held down (`ScreenStack.dragging` in `src/menus/stack.js`).
    dragging_slider: Option<(String, [f64; 2], [f64; 16])>,
}

impl MenuInput {
    /// Creates a new [`MenuInput`] router with `pointer_ndc = [0.0, 0.0]` and no active slider drag.
    pub fn new() -> Self {
        Self::default()
    }

    /// Converts pixel coordinates `(px, py)` inside a `(vw, vh)` viewport to WebGL NDC `(-1..1, -1..1)`,
    /// flipping the Y axis so `+1` is the top edge of the viewport and `-1` is the bottom edge.
    #[inline]
    pub fn pixel_to_ndc(px: f64, py: f64, vw: f64, vh: f64) -> [f64; 2] {
        // Guard against zero-sized viewports, then map `[0..vw, 0..vh]` (Y down) into
        // WebGL clip space `[-1..1, -1..1]` (Y up) for `space::local_point` ray casting.
        let w = vw.max(1.0);
        let h = vh.max(1.0);
        [(px / w) * 2.0 - 1.0, 1.0 - (py / h) * 2.0]
    }

    /// Hit-tests `ndc` against `hits` in reverse draw order (front-to-back, matching
    /// `ClassicTheme.hit` in `src/menus/themes/classic/index.js`) by intersecting the camera ray
    /// with each widget's 3D placement matrix `region.m`.
    pub fn hit_test(ndc: [f64; 2], hits: &[HitRegion]) -> Option<(&HitRegion, [f64; 2])> {
        // Iterate from the last emitted region to the first so overlapping popups or buttons
        // drawn later take priority over widgets underneath them.
        for region in hits.iter().rev() {
            let Some(pt) = local_point(&region.m, ndc[0], ndc[1]) else {
                continue;
            };
            // Test whether the ray-plane intersection `[local_x, local_y]` falls inside
            // the widget-local bounding box `[x0, y0, x1, y1]`.
            let [x0, y0, x1, y1] = region.rect;
            if pt[0] >= x0 && pt[0] <= x1 && pt[1] >= y0 && pt[1] <= y1 {
                return Some((region, pt));
            }
        }
        None
    }

    /// Processes a keyboard key press (`code` such as `"ArrowUp"`, `"Enter"`, `"Escape"`, `"KeyW"`),
    /// matching `MenuScreen.key` in `src/menus/screen.js` and `keyPopup` in `src/menus/popup.js`.
    pub fn on_key_down(
        &mut self,
        stack: &mut ScreenStack,
        code: &str,
        shift: bool,
    ) -> MenuInputEvent {
        // Normalize literal `" "` from logical key mappers to `"Space"`.
        let code = if code == " " { "Space" } else { code };

        // Only the topmost screen on the stack receives keyboard input (`ScreenStack.key` in `stack.js`).
        let Some(top) = stack.screens.last_mut() else {
            return MenuInputEvent::None;
        };

        // 1. Key-capture modal (`PRESS NEW KEY, ESC TO CANCEL` on `CONTROL OPTIONS`) grabs
        // every key press before normal menu navigation (`grabsKeys` in `src/menus/input.js`).
        if let Some(modal) = &top.key_capture {
            let outcome = modal.on_key(code);
            return match outcome {
                // Non-bindable keys (such as reserved keys) keep the modal open without playing a sound.
                KeyCaptureOutcome::Ignored => MenuInputEvent::None,
                // `Escape` closes the key-capture popup and plays `shell_menu_exit` (`MenuBack`).
                KeyCaptureOutcome::Cancelled => {
                    stack.sounds.play(SoundCue::MenuBack);
                    MenuInputEvent::KeyCapture(outcome)
                }
                // Binding a new key or clearing plays `shell_menu_accept`.
                KeyCaptureOutcome::Bound { .. } | KeyCaptureOutcome::Cleared { .. } => {
                    stack.sounds.play(SoundCue::MenuAccept);
                    MenuInputEvent::KeyCapture(outcome)
                }
            };
        }

        match code {
            // Up and Down step through the active widget's selectable rows (`MenuScreen.move` in `screen.js`).
            // `ArrowUp` steps backward (-1) through the focused widget's enabled rows, wrapping
            // around from the first item to the last (`MenuScreen.move` in `src/menus/screen.js`).
            "ArrowUp" => {
                let (changed, cue) = top.move_vertical(-1);
                if let Some(c) = cue {
                    stack.sounds.play(c);
                }
                if changed {
                    MenuInputEvent::SelectionChanged
                } else {
                    MenuInputEvent::None
                }
            }
            // `ArrowDown` steps forward (+1) through the focused widget's enabled rows.
            "ArrowDown" => {
                let (changed, cue) = top.move_vertical(1);
                if let Some(c) = cue {
                    stack.sounds.play(c);
                }
                if changed {
                    MenuInputEvent::SelectionChanged
                } else {
                    MenuInputEvent::None
                }
            }
            // PageUp and PageDown jump by one visible page inside scrollable `List` or `Options` widgets.
            "PageUp" => {
                // Step up by `shown` rows so tall lists (`CONTROL OPTIONS` bindings, `VIDEO OPTIONS`)
                // scroll one full visible page at a time.
                match top.focused_widget_mut() {
                    // Scroll `ListWidget` up by one full visible page (`list.shown` rows).
                    Some(Widget::List(list)) => {
                        let step = -(list.shown.max(1) as i32);
                        if list.scroll(step) {
                            stack.sounds.play(SoundCue::SelectChange);
                            return MenuInputEvent::SelectionChanged;
                        }
                    }
                    // Scroll paginated `OptionsWidget` (`VIDEO OPTIONS`) up by `opts.shown` rows.
                    Some(Widget::Options(opts)) => {
                        let step = -(opts.shown.clamp(1, 32) as i32);
                        if opts.scroll_by(step) {
                            stack.sounds.play(SoundCue::SelectChange);
                            return MenuInputEvent::SelectionChanged;
                        }
                    }
                    _ => {}
                }
                MenuInputEvent::None
            }
            "PageDown" => {
                // Step down by `shown` rows, clamping to the last entry inside `scroll` / `scroll_by`.
                match top.focused_widget_mut() {
                    Some(Widget::List(list)) => {
                        let step = list.shown.max(1) as i32;
                        if list.scroll(step) {
                            stack.sounds.play(SoundCue::SelectChange);
                            return MenuInputEvent::SelectionChanged;
                        }
                    }
                    Some(Widget::Options(opts)) => {
                        let step = opts.shown.clamp(1, 32) as i32;
                        if opts.scroll_by(step) {
                            stack.sounds.play(SoundCue::SelectChange);
                            return MenuInputEvent::SelectionChanged;
                        }
                    }
                    _ => {}
                }
                MenuInputEvent::None
            }
            // Left and Right either step horizontal tabs/buttons or increment/decrement an option value
            // (`MenuScreen.step` in `src/menus/screen.js` and `OptionRow.stepBy` in `src/menus/widgets.js`).
            "ArrowLeft" | "ArrowRight" => {
                // First check if the focused widget (or the `CONTROL OPTIONS` tab bar when
                // `bindings` is focused) handles horizontal stepping directly.
                let dir = if code == "ArrowLeft" { -1 } else { 1 };
                if let Some(act) = top.step_horizontal(dir) {
                    stack.sounds.play(SoundCue::SelectChange);
                    return MenuInputEvent::Action(act);
                }
                // If the focused widget is an option list or `OptionsWidget` row without a direct
                // action callback, emit `StepOption` so the controller cycles choices or steps the slider.
                // Otherwise, pressing `Enter` or `Space` on a non-slider option row cycles its
                // choice/toggle value forward (`OptionRow.stepBy(1)` in `src/menus/widgets.js`).
                let step_key = match top.focused_widget() {
                    Some(Widget::List(list)) => list
                        .rows
                        .get(list.selected)
                        .filter(|entry| !entry.disabled && entry.cell_count >= 2)
                        .map(|entry| entry.id.clone()),
                    Some(Widget::Options(opts)) => opts
                        .items
                        .get(opts.selected)
                        .filter(|item| !item.disabled && item.action.is_none())
                        .map(|item| item.id.clone()),
                    Some(Widget::OptionRow(row)) if !row.disabled => Some(row.id.clone()),
                    _ => None,
                };
                if let Some(key) = step_key {
                    stack.sounds.play(SoundCue::SelectChange);
                    return MenuInputEvent::StepOption { key, dir };
                }
                MenuInputEvent::None
            }
            // Tab (and Shift+Tab) cycles focus between distinct focusable widgets on the screen
            // (e.g., jumping between the options list and the bottom `RESET TO DEFAULTS` / `BACK` buttons).
            "Tab" => {
                if top.cycle_focus(if shift { -1 } else { 1 }) {
                    stack.sounds.play(SoundCue::SelectChange);
                    MenuInputEvent::SelectionChanged
                } else {
                    MenuInputEvent::None
                }
            }
            // Enter and Space activate the focused row/button; on choice or toggle option rows
            // (excluding sliders, where `Options.activate` is a no-op in `widgets.js`), they advance
            // the option value forward by `+1`.
            "Enter" | "NumpadEnter" | "Space" => {
                // If the focused widget item has a direct `WidgetAction` (`Rows`, `Button`,
                // `HorizontalButtons`, `Tabs`, or an `Action` option row), play `shell_menu_accept`
                // and return that action immediately.
                if let Some(act) = top.accept() {
                    stack.sounds.play(SoundCue::MenuAccept);
                    return MenuInputEvent::Action(act);
                }
                let step_key = match top.focused_widget() {
                    Some(Widget::List(list)) => list
                        .rows
                        .get(list.selected)
                        .filter(|entry| !entry.disabled && entry.cell_count >= 2)
                        .map(|entry| entry.id.clone()),
                    Some(Widget::Options(opts)) => opts
                        .items
                        .get(opts.selected)
                        .filter(|item| {
                            !item.disabled && item.action.is_none() && item.kind != OptionKind::Slider
                        })
                        .map(|item| item.id.clone()),
                    _ => None,
                };
                if let Some(key) = step_key {
                    stack.sounds.play(SoundCue::SelectChange);
                    return MenuInputEvent::StepOption { key, dir: 1 };
                }
                MenuInputEvent::None
            }
            // MenuScreen.key handles Escape; ordinary Backspace is swallowed.
            // Key capture above still admits Backspace as a gameplay binding.
            "Escape" => {
                if let Some(act) = top.back() {
                    stack.sounds.play(SoundCue::MenuBack);
                    MenuInputEvent::Action(act)
                } else {
                    MenuInputEvent::None
                }
            }
            _ => MenuInputEvent::None,
        }
    }

    /// Processes pointer movement at `(px, py)` in a `(vw, vh)` viewport
    /// (`ScreenStack.pointerMove` in `src/menus/stack.js`).
    pub fn on_pointer_move(
        &mut self,
        stack: &mut ScreenStack,
        hits: &[HitRegion],
        px: f64,
        py: f64,
        vw: f64,
        vh: f64,
    ) -> MenuInputEvent {
        let ndc = Self::pixel_to_ndc(px, py, vw, vh);
        self.pointer_ndc = ndc;

        // 1. Active slider drag (`ScreenStack.pointerMove` in `src/menus/stack.js`):
        // While a slider drag is active, project the pointer ray onto the slider's local plane `m`
        // so the thumb tracks horizontal motion even if the cursor leaves the row's vertical bounds.
        if let Some((ref key, [x0, x1], ref m)) = self.dragging_slider {
            if let Some(pt) = local_point(m, ndc[0], ndc[1]) {
                let ratio = ((pt[0] - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0);
                return MenuInputEvent::SetOptionRatio {
                    key: key.clone(),
                    ratio,
                };
            }
        }

        // 2. Hover hit-testing:
        // When not dragging a slider, ray-cast the pointer against the top screen's hotspots.
        let Some((region, _)) = Self::hit_test(ndc, hits) else {
            return MenuInputEvent::None;
        };
        let Some(top) = stack.screens.last_mut() else {
            return MenuInputEvent::None;
        };
        // Ignore hover focus changes while the key-capture popup is waiting for an input binding.
        if top.key_capture.is_some() {
            return MenuInputEvent::None;
        }

        // Hovering an enabled hotspot moves both screen-level widget focus and the widget's
        // internal row/button selection, playing `shell_select_change` whenever either changes.
        let mut changed = false;
        match &region.target {
            // Angled menu row bar (`RowsWidget`): only enabled visible rows take hover focus.
            HitTarget::Row {
                widget_index,
                row_index,
            } => {
                if let Some(Widget::Rows(rows)) = top.widgets.get_mut(*widget_index) {
                    let enabled = rows
                        .visible_rows()
                        .nth(*row_index)
                        .is_some_and(|r| !r.disabled);
                    if enabled {
                        changed =
                            top.focused != Some(*widget_index) || rows.selected != *row_index;
                        rows.selected = *row_index;
                        top.focused = Some(*widget_index);
                    }
                }
            }
            // Scrollable list box row (`ListWidget`): update both `selected` and `hover_row`
            // so `ClassicTheme::paint_list` draws `listbox_cursor` / `listbox_hilight`.
            HitTarget::ListRow {
                widget_index,
                row_index,
                ..
            } => {
                if let Some(Widget::List(list)) = top.widgets.get_mut(*widget_index) {
                    changed =
                        top.focused != Some(*widget_index) || list.selected != *row_index;
                    list.selected = *row_index;
                    list.hover_row = Some(*row_index);
                    top.focused = Some(*widget_index);
                }
            }
            // Standalone bottom-corner button (`BACK`, `RESET TO DEFAULTS`, `AUDIO MIXER`).
            HitTarget::Button { widget_index } => {
                if let Some(Widget::Button(btn)) = top.widgets.get(*widget_index) {
                    if !btn.disabled {
                        changed = top.focused != Some(*widget_index);
                        top.focused = Some(*widget_index);
                    }
                }
            }
            // Popup answer button (`YES` / `NO` / `OK` in `HorizontalButtonsWidget`).
            HitTarget::HorizontalButton {
                widget_index,
                button_index,
            } => {
                if let Some(Widget::HorizontalButtons(hb)) = top.widgets.get_mut(*widget_index) {
                    changed =
                        top.focused != Some(*widget_index) || hb.selected != *button_index;
                    hb.selected = *button_index;
                    top.focused = Some(*widget_index);
                }
            }
            // Header-bar tab (`INFANTRY` / `VEHICLE` / `STARFIGHTER` in `TabsWidget`).
            HitTarget::Tab {
                widget_index,
                tab_index,
            } => {
                if let Some(Widget::Tabs(tabs)) = top.widgets.get_mut(*widget_index) {
                    changed =
                        top.focused != Some(*widget_index) || tabs.selected != *tab_index;
                    tabs.selected = *tab_index;
                    top.focused = Some(*widget_index);
                }
            }
            // Two-column option row (`OptionsWidget`): skip `Fixed` or disabled rows.
            HitTarget::OptionsItem {
                widget_index,
                item_index,
                ..
            } => {
                if let Some(Widget::Options(opts)) = top.widgets.get_mut(*widget_index) {
                    if opts.is_item_enabled(*item_index) {
                        changed =
                            top.focused != Some(*widget_index) || opts.selected != *item_index;
                        opts.selected = *item_index;
                        top.focused = Some(*widget_index);
                    }
                }
            }
        }
        // Emit `shell_select_change` only when the hovered widget or item index actually changed
        // (`MenuScreen.setFocus` in `src/menus/screen.js`).
        if changed {
            stack.sounds.play(SoundCue::SelectChange);
            MenuInputEvent::SelectionChanged
        } else {
            MenuInputEvent::None
        }
    }

    /// Processes a mouse button press (`button`: `0` = Left `Mouse0`, `1` = Middle `Mouse1`,
    /// `2` = Right `Mouse2`, `3` = Back `Mouse3`, `4` = Forward `Mouse4`) when `KeyCaptureModal`
    /// is open (`listenMouse` in `src/menus/popup.js`).
    pub fn on_mouse_button(&mut self, stack: &mut ScreenStack, button: u8) -> MenuInputEvent {
        if let Some(top) = stack.top() {
            if let Some(modal) = &top.key_capture {
                let code = match button {
                    0 => "Mouse0",
                    1 => "Mouse1",
                    2 => "Mouse2",
                    3 => "Mouse3",
                    4 => "Mouse4",
                    _ => return MenuInputEvent::None,
                };
                let outcome = modal.on_key(code);
                if outcome != KeyCaptureOutcome::Ignored {
                    stack.sounds.play(SoundCue::MenuAccept);
                    return MenuInputEvent::KeyCapture(outcome);
                }
            }
        }
        MenuInputEvent::None
    }

    /// Processes a left mouse button press at `(px, py)` in a `(vw, vh)` viewport
    /// (`ScreenStack.pointerDown` in `src/menus/stack.js` and `listenMouse` in `src/menus/popup.js`).
    pub fn on_pointer_down(
        &mut self,
        stack: &mut ScreenStack,
        hits: &[HitRegion],
        px: f64,
        py: f64,
        vw: f64,
        vh: f64,
    ) -> MenuInputEvent {
        // When the key-capture modal is open, a left click binds `"Mouse0"` (`listenMouse` in `popup.js`).
        let captured = self.on_mouse_button(stack, 0);
        if captured != MenuInputEvent::None {
            return captured;
        }

        // Sync focus to the clicked hotspot first, then check whether the click landed inside a
        // slider track (`Options.press` in `src/menus/widgets.js`).
        let _ = self.on_pointer_move(stack, hits, px, py, vw, vh);
        let ndc = Self::pixel_to_ndc(px, py, vw, vh);
        let Some((region, pt)) = Self::hit_test(ndc, hits) else {
            return MenuInputEvent::None;
        };

        // Clicking inside a slider's horizontal `[sx0, sx1]` track latches `dragging_slider`
        // and immediately snaps the slider ratio to the clicked X position.
        match &region.target {
            // Two-column list row with an embedded slider bar (`slider_x: Some([sx0, sx1])`).
            HitTarget::ListRow {
                widget_index,
                row_index,
                slider_x,
            } => {
                if let Some(top) = stack.top() {
                    if let Some(Widget::List(list)) = top.widgets.get(*widget_index) {
                        if let Some(entry) = list.rows.get(*row_index) {
                            // If the click landed inside the list row's slider track `[sx0, sx1]`,
                            // begin dragging and emit the initial `[0.0, 1.0]` ratio immediately.
                            if let Some([sx0, sx1]) = *slider_x {
                                if pt[0] >= sx0 && pt[0] <= sx1 {
                                    self.dragging_slider =
                                        Some((entry.id.clone(), [sx0, sx1], region.m));
                                    let ratio =
                                        ((pt[0] - sx0) / (sx1 - sx0).max(1.0)).clamp(0.0, 1.0);
                                    return MenuInputEvent::SetOptionRatio {
                                        key: entry.id.clone(),
                                        ratio,
                                    };
                                }
                            }
                        }
                    }
                }
            }
            // Standard option page slider row (`AUDIO`, `VIDEO`, or `CONTROLS` sensitivity).
            // Store the row's 3D anchor matrix `region.m` so subsequent `on_pointer_move` events
            // can re-project the cursor into the same local coordinate frame.
            HitTarget::OptionsItem {
                widget_index,
                item_index,
                slider_x,
            } => {
                if let Some(top) = stack.top() {
                    if let Some(Widget::Options(opts)) = top.widgets.get(*widget_index) {
                        if let Some(item) = opts.items.get(*item_index) {
                            // If the click landed inside the option row's slider track `[sx0, sx1]`,
                            // latch `(item.id, [sx0, sx1], region.m)` for continuous drag updates.
                            if let Some([sx0, sx1]) = *slider_x {
                                if pt[0] >= sx0 && pt[0] <= sx1 {
                                    self.dragging_slider =
                                        Some((item.id.clone(), [sx0, sx1], region.m));
                                    let ratio =
                                        ((pt[0] - sx0) / (sx1 - sx0).max(1.0)).clamp(0.0, 1.0);
                                    return MenuInputEvent::SetOptionRatio {
                                        key: item.id.clone(),
                                        ratio,
                                    };
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        // Non-slider clicks act just like pressing `Enter` on the newly focused item.
        self.on_key_down(stack, "Enter", false)
    }

    /// Ends any active slider drag on pointer release (`ScreenStack.pointerUp` in `src/menus/stack.js`).
    pub fn on_pointer_up(&mut self) {
        self.dragging_slider = None;
    }

    /// Processes a mouse wheel delta (`dy > 0` scrolls down, `dy < 0` scrolls up),
    /// matching `ScreenStack.wheel` in `src/menus/stack.js` and `listenMouse` in `src/menus/popup.js`.
    pub fn on_wheel(&mut self, stack: &mut ScreenStack, dy: i32) -> MenuInputEvent {
        let Some(top) = stack.screens.last_mut() else {
            return MenuInputEvent::None;
        };
        // In key-capture mode, wheel notches bind `"WheelUp"` or `"WheelDown"` (`popup.js`).
        if let Some(modal) = &top.key_capture {
            let code = if dy < 0 { "WheelUp" } else { "WheelDown" };
            let outcome = modal.on_key(code);
            if outcome != KeyCaptureOutcome::Ignored {
                stack.sounds.play(SoundCue::MenuAccept);
                return MenuInputEvent::KeyCapture(outcome);
            }
            return MenuInputEvent::None;
        }
        // Otherwise scroll the first scrollable `List` or `Options` widget on the active screen.
        let step = if dy < 0 { -1 } else { 1 };
        // Step the first scrollable `List` or paginated `Options` widget by 1 row per wheel notch
        // (`List.wheel` and `Options.wheel` in `src/menus/widgets.js`).
        for w in &mut top.widgets {
            match w {
                Widget::List(list) => {
                    if list.scroll(step) {
                        stack.sounds.play(SoundCue::SelectChange);
                        return MenuInputEvent::SelectionChanged;
                    }
                }
                Widget::Options(opts) => {
                    if opts.scroll_by(step) {
                        stack.sounds.play(SoundCue::SelectChange);
                        return MenuInputEvent::SelectionChanged;
                    }
                }
                _ => {}
            }
        }
        MenuInputEvent::None
    }
}
