//! Slint adapter for the first-run setup and content preparation screens.
//!
//! All setup state lives in [`super::first_run::Setup`], [`super::first_run::TransferWork`],
//! and [`super::startup::StartupContentState`]; this module synchronizes that state into
//! [`SetupModel`] and forwards user callbacks back to the host.
use std::{cell::RefCell, rc::Rc};

use super::{
    slint_models::sync_rows,
    first_run::{self, SetupView, NO_ACTION},
    shell::ShellHost,
    slint_ui::{
        GameUi, SetupActionItem, SetupCopyItem, SetupFactItem, SetupModel, SetupPhaseItem,
        SlintHost, Theme,
    },
    startup::StartupContentState,
    AppState, StandaloneWindowApp,
};
use slint::{Color, ComponentHandle, SharedString, VecModel};
use winit::{
    event::{ElementState, WindowEvent},
    keyboard::{Key, NamedKey},
};

#[derive(Clone, Copy, Debug)]
pub(super) enum SetupUiAction {
    Hovered(usize),
    Activated(usize),
    PointerLeft,
    SoundToggled,
    CopyPicked(usize),
    DetailsToggled,
}

#[derive(Default)]
pub(super) struct SetupHostState {
    actions: Rc<RefCell<Vec<SetupUiAction>>>,
    // One model per list for the life of the window, set into SetupModel once and kept current
    // with `sync_rows`: a replaced model rebuilds its repeater and drops hover and held presses.
    facts: Rc<VecModel<SetupFactItem>>,
    problems: Rc<VecModel<SharedString>>,
    other_copies: Rc<VecModel<SetupCopyItem>>,
    phases: Rc<VecModel<SetupPhaseItem>>,
    left_out_items: Rc<VecModel<SharedString>>,
    action_row_1: Rc<VecModel<SetupActionItem>>,
    action_row_2: Rc<VecModel<SetupActionItem>>,
    disk_leds: Rc<VecModel<Color>>,
    /// The viewport size and look the backdrop pattern image was built for.
    pattern_key: Option<(usize, usize, usize)>,
}

impl SlintHost {
    pub(super) fn register_setup_callbacks(&self) {
        let theme = self.root.global::<Theme>();
        theme.on_format_label(|text, uppercase| {
            let formatted = bfe_ui::format_label(&text, uppercase);
            if uppercase {
                formatted.into()
            } else {
                replace_whole_word_gog(&formatted).into()
            }
        });

        let model = self.root.global::<SetupModel>();
        model.set_facts(self.setup.facts.clone().into());
        model.set_problems(self.setup.problems.clone().into());
        model.set_other_copies(self.setup.other_copies.clone().into());
        model.set_phases(self.setup.phases.clone().into());
        model.set_left_out_items(self.setup.left_out_items.clone().into());
        model.set_action_row_1(self.setup.action_row_1.clone().into());
        model.set_action_row_2(self.setup.action_row_2.clone().into());
        model.set_disk_led_colors(self.setup.disk_leds.clone().into());
        {
            let q = self.setup.actions.clone();
            model.on_action_hovered(move |idx| {
                if idx >= 0 {
                    q.borrow_mut().push(SetupUiAction::Hovered(idx as usize));
                }
            });
        }
        {
            let q = self.setup.actions.clone();
            model.on_action_activated(move |idx| {
                if idx >= 0 {
                    q.borrow_mut().push(SetupUiAction::Activated(idx as usize));
                }
            });
        }
        {
            let q = self.setup.actions.clone();
            model.on_actions_pointer_left(move || {
                q.borrow_mut().push(SetupUiAction::PointerLeft);
            });
        }
        {
            let q = self.setup.actions.clone();
            model.on_sound_toggled(move || {
                q.borrow_mut().push(SetupUiAction::SoundToggled);
            });
        }
        {
            let q = self.setup.actions.clone();
            model.on_copy_picked(move |idx| {
                if idx >= 0 {
                    q.borrow_mut().push(SetupUiAction::CopyPicked(idx as usize));
                }
            });
        }
        {
            let q = self.setup.actions.clone();
            model.on_details_toggled(move || {
                q.borrow_mut().push(SetupUiAction::DetailsToggled);
            });
        }
    }

    pub(super) fn setup_visible(&self) -> bool {
        self.root.global::<SetupModel>().get_visible()
    }

    pub(super) fn set_setup_visible(&self, visible: bool) {
        self.root.global::<SetupModel>().set_visible(visible);
    }

    pub(super) fn drain_setup_actions(&self) -> Vec<SetupUiAction> {
        self.setup.actions.borrow_mut().drain(..).collect()
    }

    pub(super) fn sync_setup(&mut self, shell: &ShellHost, state: &StartupContentState) {
        let (view, cursor, focus) = match state {
            StartupContentState::Ready(_) => {
                self.set_setup_visible(false);
                return;
            }
            StartupContentState::Searching(_) => (
                SetupView {
                    title: "SEARCHING FOR STAR WARS BATTLEFRONT".into(),
                    detail: "Looking through your Steam and GOG libraries.".into(),
                    busy: true,
                    ..Default::default()
                },
                0,
                None,
            ),
            StartupContentState::Preparing { latest, .. } => {
                (first_run::progress_view(latest), 0, None)
            }
            // prepare.js lights no button while a transfer runs: CANCEL carries no `primary` flag.
            StartupContentState::Transfer(work) => (work.view(), NO_ACTION, None),
            StartupContentState::FirstRun(model) => (model.view(), model.cursor, model.focus),
        };

        let menu_theme = match shell.app.settings.menu_theme.as_str() {
            "bf1" => "bf1",
            "bf2" => "bf2",
            _ => "eternal",
        };
        let look = match menu_theme {
            "bf1" => 0,
            "bf2" => 1,
            _ => 2,
        };
        self.root.global::<Theme>().set_menu_theme(menu_theme.into());

        let w = shell.layout_size[0].max(1) as f64;
        let h = shell.layout_size[1].max(1) as f64;
        let panel_w = 860f64.min(w - (w * 0.03).clamp(20., 56.) * 2.);
        let pad_side = (w * 0.026).clamp(20., 40.);
        let inner = (panel_w - 2. * pad_side).max(120.);
        let body_size = (h * 0.0185).clamp(14., 17.);

        let balanced_detail = balance_prose(&shell.page.prose, &view.detail, inner, body_size);
        sync_view_to_model(
            &self.root,
            &mut self.setup,
            &shell.page.body,
            &shell.page.bold,
            &view,
            &balanced_detail,
            cursor,
            focus,
            w,
            h,
            inner,
            shell.loading_time,
            look,
        );
    }
}

impl StandaloneWindowApp {
    pub(super) fn sync_slint_setup(&mut self) {
        let Some(ui) = &mut self.slint_ui else {
            return;
        };
        match (&self.state, &self.shell) {
            (AppState::StartupLoader(loader_state), Some(shell)) => {
                ui.sync_setup(shell, loader_state);
            }
            _ => {
                ui.set_setup_visible(false);
            }
        }
    }

    pub(super) fn drain_slint_setup_actions(&mut self) {
        let actions = self
            .slint_ui
            .as_ref()
            .map(SlintHost::drain_setup_actions)
            .unwrap_or_default();
        for action in actions {
            match action {
                SetupUiAction::Hovered(index) => {
                    if let AppState::StartupLoader(StartupContentState::FirstRun(model)) =
                        &mut self.state
                    {
                        if !model
                            .actions()
                            .get(index)
                            .is_some_and(|(_, a)| matches!(a, first_run::Action::Sound))
                        {
                            model.cursor = index;
                        }
                    }
                }
                SetupUiAction::PointerLeft => {
                    if let AppState::StartupLoader(StartupContentState::FirstRun(model)) =
                        &mut self.state
                    {
                        // prepare.js: leaving the buttons lights the state's own
                        // choice again, unless the keyboard's focus is on one.
                        if !model.focus.is_some_and(|f| model.is_group_button(f)) {
                            model.cursor = model.primary_action();
                        }
                    }
                }
                SetupUiAction::Activated(index) => match &mut self.state {
                    AppState::StartupLoader(StartupContentState::Transfer(work)) => {
                        work.cancel();
                    }
                    AppState::StartupLoader(StartupContentState::FirstRun(_)) => {
                        self.first_run_action(index);
                    }
                    _ => {}
                },
                SetupUiAction::SoundToggled => {
                    if let AppState::StartupLoader(StartupContentState::FirstRun(model)) =
                        &view_first_run(&mut self.state)
                    {
                        if let Some(idx) = model
                            .actions()
                            .iter()
                            .position(|(_, a)| matches!(a, first_run::Action::Sound))
                        {
                            self.first_run_action(idx);
                        }
                    }
                }
                SetupUiAction::CopyPicked(copy_idx) => {
                    if let AppState::StartupLoader(StartupContentState::FirstRun(model)) =
                        &view_first_run(&mut self.state)
                    {
                        if let Some(idx) = model.actions().iter().position(
                            |(_, a)| matches!(a, first_run::Action::Pick(i) if *i == copy_idx),
                        ) {
                            self.first_run_action(idx);
                        }
                    }
                }
                SetupUiAction::DetailsToggled => {
                    if let AppState::StartupLoader(StartupContentState::FirstRun(model)) =
                        &view_first_run(&mut self.state)
                    {
                        if let Some(idx) = model
                            .actions()
                            .iter()
                            .position(|(_, a)| matches!(a, first_run::Action::Details))
                        {
                            self.first_run_action(idx);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn handle_setup_window_event(&mut self, event: &WindowEvent) -> bool {
        if !matches!(self.state, AppState::StartupLoader(_)) {
            return false;
        }
        if let WindowEvent::KeyboardInput { event, .. } = event {
            if event.state == ElementState::Pressed && !event.repeat {
                match &mut self.state {
                    AppState::StartupLoader(StartupContentState::Transfer(work)) => {
                        if matches!(
                            &event.logical_key,
                            Key::Named(NamedKey::Escape | NamedKey::Enter | NamedKey::Space)
                        ) {
                            work.cancel();
                            return true;
                        }
                    }
                    AppState::StartupLoader(StartupContentState::FirstRun(model)) => {
                        let shift = self.modifiers.shift_key();
                        match &event.logical_key {
                            Key::Named(NamedKey::Enter | NamedKey::Space) => {
                                let index = model.focus.unwrap_or(model.cursor);
                                self.first_run_action(index);
                                return true;
                            }
                            Key::Named(NamedKey::Tab) => {
                                model.move_focus(shift);
                                return true;
                            }
                            Key::Named(NamedKey::ArrowRight | NamedKey::ArrowDown) => {
                                model.move_focus(false);
                                return true;
                            }
                            Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowUp) => {
                                model.move_focus(true);
                                return true;
                            }
                            Key::Named(NamedKey::Escape) => {
                                model.message = None;
                                model.cursor = 0;
                                model.focus = None;
                                return true;
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
        }
        false
    }
}

fn view_first_run(state: &mut AppState) -> &mut AppState {
    state
}

#[allow(clippy::too_many_arguments)]
fn sync_view_to_model(
    root: &GameUi,
    cache: &mut SetupHostState,
    body_font: &bfe_render::interface::Font,
    button_font: &bfe_render::interface::Font,
    view: &SetupView,
    balanced_detail: &str,
    cursor: usize,
    focus: Option<usize>,
    w: f64,
    h: f64,
    inner: f64,
    time: f64,
    look: usize,
) {
    let model = root.global::<SetupModel>();
    model.set_visible(true);
    model.set_listing(view.listing);
    model.set_busy(view.busy);
    model.set_warning(view.warning);
    model.set_error(view.error);
    model.set_planet_text(view.planet.as_str().into());
    model.set_title(view.title.as_str().into());
    model.set_detail(balanced_detail.into());
    model.set_code_text(view.code.as_deref().unwrap_or("").into());

    let confirm_visible = !view.facts.is_empty()
        || !view.problems.is_empty()
        || !view.help.is_empty()
        || !view.repair.is_empty()
        || view.sound.is_some()
        || !view.other_copies.is_empty();
    model.set_confirm_visible(confirm_visible);

    let fact_label_w = view
        .facts
        .iter()
        .map(|f| {
            body_font.width(
                &f.name.encode_utf16().collect::<Vec<_>>(),
                11. / 64.,
                2.42,
            )
        })
        .fold(0.0_f64, f64::max)
        .ceil()
        .max(52.0);
    model.set_fact_label_width(fact_label_w as f32);

    let facts: Vec<SetupFactItem> = view
        .facts
        .iter()
        .map(|f| SetupFactItem {
            name: f.name.as_str().into(),
            value: f.value.as_str().into(),
            monospace: f.monospace,
            status: f.status.into(),
        })
        .collect();
    sync_rows(&cache.facts, &facts);

    let problems: Vec<SharedString> = view
        .problems
        .iter()
        .map(|p| SharedString::from(p.as_str()))
        .collect();
    sync_rows(&cache.problems, &problems);
    model.set_help_text(view.help.as_str().into());
    model.set_repair_text(view.repair.as_str().into());

    if let Some(sound) = &view.sound {
        model.set_sound_visible(true);
        model.set_sound_checked(sound.checked);
        model.set_sound_focused(focus == Some(sound.action));
        model.set_sound_description(sound.description.as_str().into());
    } else {
        model.set_sound_visible(false);
        model.set_sound_focused(false);
    }

    let copies: Vec<SetupCopyItem> = view
        .other_copies
        .iter()
        .map(|c| SetupCopyItem {
            index: c.index as i32,
            dir: c.dir.as_str().into(),
            how: c.how.as_str().into(),
            focused: focus == Some(c.action),
        })
        .collect();
    sync_rows(&cache.other_copies, &copies);

    if let Some(progress) = &view.progress {
        model.set_progress_visible(true);
        model.set_progress_estimating(progress.estimating);
        let display_fraction = if progress.estimating {
            0.18
        } else {
            progress.fraction.clamp(0.0, 1.0)
        };
        model.set_progress_fraction(display_fraction as f32);
        model.set_progress_step(progress.label.as_str().into());
        model.set_progress_numbers(progress.numbers.as_str().into());
        model.set_phases_visible(progress.show_phases);
        let labels = [
            "GALAXY",
            "ARMIES",
            "INTERFACE",
            "BATTLEFIELDS",
            "SOUND",
            "TEXTURES",
            "MOVIES",
        ];
        let phases: Vec<SetupPhaseItem> = labels
            .iter()
            .enumerate()
            .map(|(i, label)| SetupPhaseItem {
                label: (*label).into(),
                state: progress.phases[i] as i32,
            })
            .collect();
        sync_rows(&cache.phases, &phases);
    } else {
        model.set_progress_visible(false);
        model.set_phases_visible(false);
    }

    let left_out_visible = !view.left_out_items.is_empty();
    model.set_left_out_visible(left_out_visible);
    model.set_left_out_title(view.left_out_title.as_str().into());
    let left_out_items: Vec<SharedString> = view
        .left_out_items
        .iter()
        .map(|s| SharedString::from(s.as_str()))
        .collect();
    sync_rows(&cache.left_out_items, &left_out_items);
    model.set_left_out_help(view.left_out_help.as_str().into());

    // The width of each action button as PageButton sizes it: the label in the look's
    // casing, its tracking, 1.4em of padding each side, and the small button's minimum.
    let pattern_key = (w as usize, h as usize, look);
    if cache.pattern_key != Some(pattern_key) {
        let pixels = super::page_surfaces::pattern_masked(
            pattern_key.0,
            pattern_key.1,
            look,
            super::page_surfaces::SETUP_MASK,
        );
        let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(
            pattern_key.0 as u32,
            pattern_key.1 as u32,
        );
        buffer.make_mut_bytes().copy_from_slice(&pixels);
        model.set_pattern(slint::Image::from_rgba8(buffer));
        cache.pattern_key = Some(pattern_key);
    }
    let (fs, tracking) = match look {
        0 => ((h * 0.017).clamp(13., 18.), 0.16),
        1 => ((h * 0.019).clamp(14., 19.), 0.02),
        _ => ((h * 0.017).clamp(13., 18.), 0.2),
    };
    let gap = (w * 0.008).clamp(8., 14.);
    let min_bw = (w * 0.11).clamp(120., 200.);
    let mut rows: Vec<Vec<SetupActionItem>> = vec![Vec::new()];
    let mut used = 0.0;
    for (i, label) in view.actions.iter().enumerate() {
        if view.sound.as_ref().is_some_and(|s| s.action == i)
            || view.link_action == Some(i)
            || view.other_copies.iter().any(|c| c.action == i)
        {
            continue;
        }
        let shown = if look == 1 {
            replace_whole_word_gog(&bfe_ui::format_label(label, false))
        } else {
            label.to_uppercase()
        };
        let chars: Vec<_> = shown.encode_utf16().collect();
        let bw = (button_font.width(&chars, fs / 64., fs * tracking) + fs * 2.8).max(min_bw);
        if used + bw > inner && !rows.last().unwrap().is_empty() && rows.len() < 2 {
            rows.push(Vec::new());
            used = 0.0;
        }
        rows.last_mut().unwrap().push(SetupActionItem {
            label: label.as_str().into(),
            action_index: i as i32,
        });
        used += bw + gap;
    }
    let row1 = rows.first().cloned().unwrap_or_default();
    let row2 = rows.get(1).cloned().unwrap_or_default();
    sync_rows(&cache.action_row_1, &row1);
    sync_rows(&cache.action_row_2, &row2);
    model.set_selected_action(if cursor == NO_ACTION {
        -1
    } else {
        cursor as i32
    });

    if let Some(link_idx) = view.link_action {
        model.set_details_visible(true);
        model.set_show_details(!view.logs.is_empty());
        model.set_details_focused(focus == Some(link_idx));
        let start = view.logs.len().saturating_sub(80);
        model.set_log_text(view.logs[start..].join("\n").into());
    } else {
        model.set_details_visible(false);
        model.set_show_details(false);
        model.set_details_focused(false);
        model.set_log_text("".into());
    }

    model.set_disk_time(time as f32);
    let glint = ((time / 2.2) % 1.0) as f32;
    model.set_glint_progress(glint);
    let pulse = (0.725 + 0.275 * (time * std::f64::consts::TAU / 1.4).cos()) as f32;
    model.set_phase_pulse(pulse);

    let stops = match look {
        0 => ["#0b0d0f", "#34373b", "#5c6166", "#8b8f92", "#b9bcbe"],
        1 => ["#101826", "#3d0c08", "#a5180e", "#e0362a", "#ff6450"],
        _ => ["#15100c", "#4a2407", "#b5560b", "#ff9a1f", "#ffd27a"],
    };
    let leds: Vec<Color> = (0..28)
        .map(|i| {
            let level = bfe_shell::loading::light_level(time / 1.667 - i as f64 / 28.);
            let rgb = bfe_shell::loading::mix_color_rgb(&stops, level);
            Color::from_rgb_u8(rgb[0], rgb[1], rgb[2])
        })
        .collect();
    sync_rows(&cache.disk_leds, &leds);
}

/// Balances prose lines like CSS `text-wrap: balance` so `#detail` keeps the
/// minimal line count while narrowing the widest line.
fn balance_prose(
    font: &bfe_render::interface::Font,
    text: &str,
    width: f64,
    size: f64,
) -> String {
    if text.is_empty() || text.contains('\n') {
        return text.to_string();
    }
    let spacing = size * 0.015;
    let wrap = |limit: f64| -> Vec<String> {
        let mut lines = Vec::new();
        let mut current = String::new();
        for word in text.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            let chars: Vec<u16> = candidate.encode_utf16().collect();
            if !current.is_empty() && font.width(&chars, size / 64., spacing) > limit {
                lines.push(current);
                current = word.to_string();
            } else {
                current = candidate;
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
        lines
    };
    let initial = wrap(width);
    if initial.len() <= 1 {
        return text.to_string();
    }
    let count = initial.len();
    let (mut lo, mut hi) = (width / 2., width);
    for _ in 0..16 {
        let mid = (lo + hi) / 2.;
        if wrap(mid).len() > count {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    wrap(hi).join("\n")
}

/// Preserves uppercase `GOG` when title-casing button labels in the `bf2` look,
/// matching `prepare.js` (`label.toLowerCase().replace(/\bgog\b/g, "GOG")`).
fn replace_whole_word_gog(s: &str) -> String {
    s.split(' ')
        .map(|w| if w == "Gog" { "GOG" } else { w })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::replace_whole_word_gog;

    #[test]
    fn preserves_uppercase_gog_in_title_case() {
        let formatted = bfe_ui::format_label("GET IT ON GOG", false);
        assert_eq!(replace_whole_word_gog(&formatted), "Get It On GOG");
    }
}
