//! Keyboard and pointer menu input router (`src/menus/input.js`).
//!
//! Ports `MenuInput` from `src/menus/input.js`: 3D perspective hit-testing
//! (`local_point`), hover selection, click activation, wheel scrolling,
//! slider dragging, and keyboard navigation (`ArrowUp`/`Down`/`Left`/`Right`,
//! `PageUp`/`PageDown`, `Tab`, `Enter`/`Space`, `Escape`).

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
    /// No state or action changed.
    None,
    /// Focus or selection moved inside the active screen.
    SelectionChanged,
    /// The active option row or slider should step by `dir` (`-1` or `+1`) on `option_key`.
    StepOption { key: String, dir: i32 },
    /// A slider option was dragged to `ratio` (`0.0..1.0`).
    SetOptionRatio { key: String, ratio: f64 },
    /// A widget action was activated (`OpenPage`, `Back`, `ResetPage`, `StartRebind`, `SelectTab`, `Named`).
    Action(WidgetAction),
    /// A key-capture modal completed (`Bound`, `Cleared`, `Cancelled`).
    KeyCapture(KeyCaptureOutcome),
}

/// Stateful keyboard and pointer router for a [`ScreenStack`] (`MenuInput` in `src/menus/input.js`).
#[derive(Clone, Debug, Default)]
pub struct MenuInput {
    pub pointer_ndc: [f64; 2],
    dragging_slider: Option<(String, [f64; 2], [f64; 16])>,
}

impl MenuInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// Converts pixel coordinates `(px, py)` inside a `(vw, vh)` viewport to WebGL NDC `(-1..1, -1..1)`
    /// (`ndc` in `src/menus/input.js`).
    #[inline]
    pub fn pixel_to_ndc(px: f64, py: f64, vw: f64, vh: f64) -> [f64; 2] {
        let w = vw.max(1.0);
        let h = vh.max(1.0);
        [(px / w) * 2.0 - 1.0, 1.0 - (py / h) * 2.0]
    }

    /// Hit-tests `ndc` against `hits` in reverse draw order (front-to-back).
    pub fn hit_test(ndc: [f64; 2], hits: &[HitRegion]) -> Option<(&HitRegion, [f64; 2])> {
        for region in hits.iter().rev() {
            let Some(pt) = local_point(&region.m, ndc[0], ndc[1]) else {
                continue;
            };
            let [x0, y0, x1, y1] = region.rect;
            if pt[0] >= x0 && pt[0] <= x1 && pt[1] >= y0 && pt[1] <= y1 {
                return Some((region, pt));
            }
        }
        None
    }

    /// Processes a keyboard key press (`code` such as `"ArrowUp"`, `"Enter"`, `"Escape"`, `"KeyW"`).
    pub fn on_key_down(
        &mut self,
        stack: &mut ScreenStack,
        code: &str,
        shift: bool,
    ) -> MenuInputEvent {
        let Some(top) = stack.screens.last_mut() else {
            return MenuInputEvent::None;
        };

        // 1. Key-capture modal intercepts all keys.
        if let Some(modal) = &top.key_capture {
            let outcome = modal.on_key(code);
            return match outcome {
                KeyCaptureOutcome::Ignored => MenuInputEvent::None,
                KeyCaptureOutcome::Cancelled => {
                    stack.sounds.play(SoundCue::MenuBack);
                    MenuInputEvent::KeyCapture(outcome)
                }
                KeyCaptureOutcome::Bound { .. } | KeyCaptureOutcome::Cleared { .. } => {
                    stack.sounds.play(SoundCue::MenuAccept);
                    MenuInputEvent::KeyCapture(outcome)
                }
            };
        }

        match code {
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
            "PageUp" => {
                match top.focused_widget_mut() {
                    Some(Widget::List(list)) => {
                        let step = -(list.shown.max(1) as i32);
                        if list.scroll(step) {
                            stack.sounds.play(SoundCue::SelectChange);
                            return MenuInputEvent::SelectionChanged;
                        }
                    }
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
            "ArrowLeft" | "ArrowRight" => {
                let dir = if code == "ArrowLeft" { -1 } else { 1 };
                if let Some(act) = top.step_horizontal(dir) {
                    stack.sounds.play(SoundCue::SelectChange);
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
            "Tab" => {
                if top.cycle_focus(if shift { -1 } else { 1 }) {
                    stack.sounds.play(SoundCue::SelectChange);
                    MenuInputEvent::SelectionChanged
                } else {
                    MenuInputEvent::None
                }
            }
            "Enter" | "NumpadEnter" | "Space" => {
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
            "Escape" | "Backspace" => {
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

    /// Processes pointer movement at `(px, py)` in a `(vw, vh)` viewport.
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

        if let Some((ref key, [x0, x1], ref m)) = self.dragging_slider {
            if let Some(pt) = local_point(m, ndc[0], ndc[1]) {
                let ratio = ((pt[0] - x0) / (x1 - x0).max(1.0)).clamp(0.0, 1.0);
                return MenuInputEvent::SetOptionRatio {
                    key: key.clone(),
                    ratio,
                };
            }
        }

        let Some((region, _)) = Self::hit_test(ndc, hits) else {
            return MenuInputEvent::None;
        };
        let Some(top) = stack.screens.last_mut() else {
            return MenuInputEvent::None;
        };
        if top.key_capture.is_some() {
            return MenuInputEvent::None;
        }

        let mut changed = false;
        match &region.target {
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
            HitTarget::Button { widget_index } => {
                if let Some(Widget::Button(btn)) = top.widgets.get(*widget_index) {
                    if !btn.disabled {
                        changed = top.focused != Some(*widget_index);
                        top.focused = Some(*widget_index);
                    }
                }
            }
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
        if changed {
            stack.sounds.play(SoundCue::SelectChange);
            MenuInputEvent::SelectionChanged
        } else {
            MenuInputEvent::None
        }
    }

    /// Processes a left mouse button press at `(px, py)` in a `(vw, vh)` viewport.
    pub fn on_pointer_down(
        &mut self,
        stack: &mut ScreenStack,
        hits: &[HitRegion],
        px: f64,
        py: f64,
        vw: f64,
        vh: f64,
    ) -> MenuInputEvent {
        if let Some(top) = stack.top() {
            if let Some(modal) = &top.key_capture {
                let outcome = modal.on_key("Mouse0");
                if outcome != KeyCaptureOutcome::Ignored {
                    stack.sounds.play(SoundCue::MenuAccept);
                    return MenuInputEvent::KeyCapture(outcome);
                }
            }
        }

        let _ = self.on_pointer_move(stack, hits, px, py, vw, vh);
        let ndc = Self::pixel_to_ndc(px, py, vw, vh);
        let Some((region, pt)) = Self::hit_test(ndc, hits) else {
            return MenuInputEvent::None;
        };

        match &region.target {
            HitTarget::ListRow {
                widget_index,
                row_index,
                slider_x,
            } => {
                if let Some(top) = stack.top() {
                    if let Some(Widget::List(list)) = top.widgets.get(*widget_index) {
                        if let Some(entry) = list.rows.get(*row_index) {
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
            HitTarget::OptionsItem {
                widget_index,
                item_index,
                slider_x,
            } => {
                if let Some(top) = stack.top() {
                    if let Some(Widget::Options(opts)) = top.widgets.get(*widget_index) {
                        if let Some(item) = opts.items.get(*item_index) {
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

        self.on_key_down(stack, "Enter", false)
    }

    /// Ends any active slider drag on pointer release.
    pub fn on_pointer_up(&mut self) {
        self.dragging_slider = None;
    }

    /// Processes a mouse wheel delta (`dy > 0` scrolls down, `dy < 0` scrolls up).
    pub fn on_wheel(&mut self, stack: &mut ScreenStack, dy: i32) -> MenuInputEvent {
        let Some(top) = stack.screens.last_mut() else {
            return MenuInputEvent::None;
        };
        if let Some(modal) = &top.key_capture {
            let code = if dy < 0 { "WheelUp" } else { "WheelDown" };
            let outcome = modal.on_key(code);
            if outcome != KeyCaptureOutcome::Ignored {
                stack.sounds.play(SoundCue::MenuAccept);
                return MenuInputEvent::KeyCapture(outcome);
            }
            return MenuInputEvent::None;
        }
        let step = if dy < 0 { -1 } else { 1 };
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
