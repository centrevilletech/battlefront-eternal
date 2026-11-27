//! Optional game selector and Battlefront II Controls page.
use super::*;
use crate::hotkeys::{hotkey_rows_for, tabs_for, ControlContext};
use crate::screen::ScreenFocus;

/// Physical pointer input delivered to an open Controls capture before UI hit testing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePointer {
    Button(u8),
    Wheel(i32),
}

impl OptionsController {
    pub fn open_page(&mut self, page: &str) {
        if page == "controls" && self.game_selector {
            self.stack.push(self.controls_screen());
        } else {
            self.open_released_page(page);
        }
    }

    pub fn open_battle_controls(&mut self, family: GameFamily) {
        if family.is_available() {
            self.controls_family = family;
            self.controls_tab = "infantry".into();
            self.open_page("controls");
        }
    }

    pub fn controls_family(&self) -> GameFamily {
        self.controls_family
    }

    /// The available-games page uses the released controls layout with one extra choice row.
    pub(super) fn controls_screen(&self) -> MenuScreen {
        let tab = self.controls_tab.as_str();
        let family = self.controls_family;
        let ctx = &self.context;
        let mut screen = if family == GameFamily::Swbf2 {
            // Page headings, tuning labels and buttons also use this game's text.
            let localized = OptionsContext {
                strings: ctx.swbf2_control_strings.clone(),
                surface_height: ctx.surface_height,
                ..Default::default()
            };
            let mut page = controls_page(&self.settings, &localized, tab);
            let strings = &localized.strings;
            let tabs = tabs_for(family);
            let spacing = SAFE_W / tabs.len() as f64;
            if let Some(Widget::Tabs(widget)) = page.widget_mut("tabs") {
                let items = tabs
                    .iter()
                    .enumerate()
                    .map(|(i, t)| TabItem {
                        id: t.id.into(),
                        label: say(strings, Some(t.label_key), t.label),
                        x: 16. + i as f64 * spacing,
                        y: 79.,
                        width: spacing * 0.9,
                        flipped: false,
                    })
                    .collect();
                *widget = TabsWidget::new("tabs", items, tab, 79., 24., FONT_TINY);
            }
            if let Some(Widget::List(list)) = page.widget_mut("bindings") {
                let keys = ControlContext::from_tab(tab).map(|c| c.bindings(&self.settings)).unwrap_or_default();
                list.rows = hotkey_rows_for(family, tab, &keys, strings)
                    .into_iter()
                    .map(|row| {
                        let action = row.action.as_ref().map(|id| WidgetAction::StartRebind {
                            tab: tab.into(),
                            action: id.clone(),
                            label: row.label.clone(),
                        });
                        ListEntry::two_col(
                            row.action.unwrap_or_else(|| row.label.clone()),
                            row.label,
                            row.keys,
                            row.disabled,
                            action,
                        )
                    })
                    .collect();
            }
            page
        } else {
            controls_page(&self.settings, ctx, tab)
        };
        if let Some(Widget::Options(opts)) = screen.widget_mut("rows") {
            opts.items.insert(
                0,
                OptionsItem {
                    id: "controlsFamily".into(),
                    label: "GAME".into(),
                    value_text: family.title().to_uppercase(),
                    kind: OptionKind::Choice,
                    ratio: 0.,
                    min: 0.,
                    max: 1.,
                    step: 1.,
                    disabled: false,
                    dim: false,
                    hidden: false,
                    note: String::new(),
                    action: None,
                },
            );
        }
        if let Some(Widget::Text(heading)) = screen.widget_mut("heading") {
            heading.y += 24.5;
        }
        if let Some(Widget::List(list)) = screen.widget_mut("bindings") {
            let height = (list.look.box_h - 24.5).max(120.);
            list.look.box_y += 24.5 + (height - list.look.box_h) * 0.5;
            list.look.box_h = height;
        }
        screen
    }

    pub fn refresh_active_screen(&mut self) {
        if !self.game_selector || self.stack.top().is_none_or(|top| top.id != "options-controls") {
            return self.refresh_released_screen();
        }
        let new = self.controls_screen();
        let top = self.stack.top_mut().unwrap();
        for id in ["tabs", "rows", "bindings"] {
            match (top.widget_mut(id), new.widget(id)) {
                (Some(Widget::Tabs(old)), Some(Widget::Tabs(new))) => old.clone_from(new),
                (Some(Widget::Options(old)), Some(Widget::Options(new))) => {
                    old.items.clone_from(&new.items);
                    old.selected = old.selected.min(old.items.len().saturating_sub(1));
                }
                (Some(Widget::List(old)), Some(Widget::List(new))) => {
                    old.rows.clone_from(&new.rows);
                    old.look.clone_from(&new.look);
                    old.selected = old.selected.min(old.rows.len().saturating_sub(1));
                    old.reveal();
                }
                _ => {}
            }
        }
    }

    pub fn capturing_swbf2_binding(&self) -> bool {
        self.controls_family == GameFamily::Swbf2 && self.stack.top().is_some_and(|s| s.key_capture.is_some())
    }

    pub fn on_key_down(&mut self, code: &str, shift: bool) {
        // The released menu reserves chat and roster keys. All three are editable in Battlefront II.
        if self.capturing_swbf2_binding() && matches!(code, "KeyT" | "KeyY" | "Tab") {
            let modal = self.stack.top().unwrap().key_capture.as_ref().unwrap();
            let outcome =
                KeyCaptureOutcome::Bound { tab: modal.tab.clone(), action: modal.action.clone(), code: code.into() };
            self.stack.sounds.play(crate::sounds::SoundCue::MenuAccept);
            self.apply_event(MenuInputEvent::KeyCapture(outcome));
        } else {
            self.on_released_key_down(code, shift);
        }
    }

    pub fn capture_swbf2_pointer(&mut self, pointer: CapturePointer) -> bool {
        if !self.capturing_swbf2_binding() {
            return false;
        }
        let event = match pointer {
            CapturePointer::Button(button) => self.input.on_mouse_button(&mut self.stack, button),
            CapturePointer::Wheel(direction) => self.input.on_wheel(&mut self.stack, direction),
        };
        self.apply_event(event);
        true
    }

    pub fn apply_event(&mut self, event: MenuInputEvent) {
        if !self.game_selector {
            return self.apply_released_event(event);
        }
        if matches!(&event, MenuInputEvent::StepOption { key, .. } if key == "controlsFamily")
            && self.stack.top().is_some_and(|s| s.id == "options-controls")
        {
            self.controls_family = match self.controls_family {
                GameFamily::Swbf1 => GameFamily::Swbf2,
                GameFamily::Swbf2 => GameFamily::Swbf1,
            };
            self.controls_tab = "infantry".into();
            let mut page = self.controls_screen();
            page.focus_policy = ScreenFocus::Keep;
            self.stack.replace(page);
            return;
        }
        if self.controls_family != GameFamily::Swbf2 {
            return self.apply_released_event(event);
        }
        match event {
            setting @ (MenuInputEvent::StepOption { .. } | MenuInputEvent::SetOptionRatio { .. })
                if self.stack.top().is_some_and(|top| top.id == "options-controls") =>
            {
                let revision = self.settings.revision;
                self.apply_released_event(setting);
                if self.settings.revision != revision {
                    self.settings.complete = true;
                }
            }
            MenuInputEvent::Action(WidgetAction::StartRebind { tab, action, label }) => {
                let mut popup = key_popup(&tab, &action, &label, &self.context.swbf2_control_strings);
                let prompt = "PRESS A KEY OR MOUSE BUTTON, OR SCROLL. BACKSPACE CLEARS. ESC CANCELS.";
                if let Some(modal) = &mut popup.key_capture {
                    modal.prompt = prompt.into();
                }
                if let Some(Widget::Text(message)) = popup.widget_mut("message") {
                    message.text = format!("{}\n{prompt}", label.trim_end_matches(':'));
                }
                self.stack.push(popup);
            }
            MenuInputEvent::Action(WidgetAction::ResetPage(page)) if page == "controls" => {
                let keys = options_for("controls")
                    .into_iter()
                    .map(|o| o.key)
                    .filter(|key| !matches!(*key, "keyBindings" | "flightBindings"));
                self.settings.complete = true;
                if let Some(context) = ControlContext::from_tab(&self.controls_tab) {
                    self.settings.swbf2_bindings.remove(&context);
                }
                self.settings.reset_keys(keys);
                self.refresh_active_screen();
            }
            MenuInputEvent::KeyCapture(KeyCaptureOutcome::Bound { tab, action, code }) => {
                self.finish_swbf2_rebind(&tab, &action, (code != "Backspace").then_some(code.as_str()));
            }
            MenuInputEvent::KeyCapture(KeyCaptureOutcome::Cleared { tab, action }) => {
                self.finish_swbf2_rebind(&tab, &action, None)
            }
            other => self.apply_released_event(other),
        }
    }

    fn finish_swbf2_rebind(&mut self, tab: &str, action: &str, code: Option<&str>) {
        if let Some(context) = ControlContext::from_tab(tab) {
            context.rebind(&mut self.settings, action, code);
        }
        self.stack.pop();
        self.refresh_active_screen();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paused_capture_rebinds_the_open_context_and_explains_how_to_clear() {
        let mut ctrl =
            OptionsController::new(Settings::new(), OptionsContext { in_battle: true, ..Default::default() });
        ctrl.game_selector = true;
        ctrl.controls_family = GameFamily::Swbf2;
        ctrl.open_page("controls");
        let begin = |ctrl: &mut OptionsController, tab: &str, action: &str| {
            ctrl.apply_event(MenuInputEvent::Action(WidgetAction::SelectTab(tab.into())));
            let Some(Widget::List(rows)) = ctrl.stack.top().unwrap().widget("bindings") else { panic!() };
            let row = rows.rows.iter().find(|r| r.id == action).unwrap().action.clone().unwrap();
            ctrl.apply_event(MenuInputEvent::Action(row));
        };
        begin(&mut ctrl, "jedi", "holds.block");
        assert!(ctrl.stack.top().unwrap().key_capture.as_ref().unwrap().prompt.contains("BACKSPACE CLEARS"));
        assert!(ctrl.capture_swbf2_pointer(CapturePointer::Button(4)));
        assert_eq!(ControlContext::Jedi.bindings(&ctrl.settings)["holds.block"], ["Mouse4"]);
        begin(&mut ctrl, "jedi", "holds.block");
        assert!(ctrl.capture_swbf2_pointer(CapturePointer::Wheel(-1)));
        let jedi = ControlContext::Jedi.bindings(&ctrl.settings);
        assert_eq!(jedi["holds.block"], ["WheelUp"]);
        assert_eq!(jedi["presses.nextSecondary"], ["Digit1"]);
        assert_eq!(ControlContext::Infantry.bindings(&ctrl.settings)["presses.nextSecondary"], ["Digit2", "WheelUp"]);
        begin(&mut ctrl, "jedi", "holds.block");
        ctrl.on_key_down("Escape", false);
        assert_eq!(ControlContext::Jedi.bindings(&ctrl.settings), jedi);
        begin(&mut ctrl, "jedi", "holds.block");
        ctrl.on_key_down("Backspace", false);
        assert!(ControlContext::Jedi.bindings(&ctrl.settings)["holds.block"].is_empty());
        assert!(!ctrl.capture_swbf2_pointer(CapturePointer::Button(4)));
    }
    #[test]
    fn editing_shared_tuning_or_resetting_on_the_2005_page_preserves_a_partial_profile() {
        for event in [
            MenuInputEvent::StepOption { key: "invertMouse".into(), dir: 1 },
            MenuInputEvent::Action(WidgetAction::ResetPage("controls".into())),
        ] {
            let partial =
                Settings::from_json_str(r#"{"complete":false,"values":{"keyBindings":{"presses.jump":["KeyI"]}}}"#);
            let mut ctrl = OptionsController::new(partial, OptionsContext::default());
            ctrl.game_selector = true;
            ctrl.controls_family = GameFamily::Swbf2;
            ctrl.open_page("controls");
            assert!(!ctrl.settings.complete);
            ctrl.apply_event(event);
            let snapshot = ctrl.settings.to_desktop_snapshot();
            assert!(snapshot.complete);
            assert_eq!(snapshot.values["keyBindings"]["presses.jump"], serde_json::json!(["KeyI"]));
            assert!(ctrl.settings.swbf2_bindings.is_empty());
        }
    }
}
