//! Slint model synchronization and action dispatch for the Options screen stack.
use super::{
    settings::AppSettingsStore,
    shell::{ShellHost, ShellTransition},
    slint_models::sync_rows,
    slint_ui::{
        ComponentHandle, GameUi, ListEntry, MenuRowItem, OptionKind as SlintOptionKind,
        OptionsModel, OptionsRowItem, OptionsTabItem,
    },
};
use crate::audio::SharedAudioRouter;
use bfe_interface::{
    input::MenuInputEvent,
    options::OptionKind as InterfaceOptionKind,
    sounds::SoundCue,
    widgets::{TextLook, Widget, WidgetAction},
};
use slint::Model;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Debug)]
pub(super) enum OptionsUiAction {
    HubRowSelected(usize),
    HubRowActivated(usize),
    OptionSelected(usize),
    OptionActivated(usize),
    OptionStepped(usize, i32),
    OptionSliderChanged(usize, f64),
    TabSelected(usize),
    BindingHovered(usize),
    BindingSelected(usize),
    BindingActivated(usize),
    LeftSelected,
    MiddleSelected,
    LeftActivated,
    MiddleActivated,
    ModalButton(usize),
}

/// The lists the Options screens repeat over. Each is installed into the global once and only
/// edited afterwards, so rows keep their hover and press state (see slint_models.rs).
pub(super) struct OptionsModels {
    hub_rows: Rc<slint::VecModel<MenuRowItem>>,
    option_rows: Rc<slint::VecModel<OptionsRowItem>>,
    tabs: Rc<slint::VecModel<OptionsTabItem>>,
    binding_entries: Rc<slint::VecModel<ListEntry>>,
}

impl OptionsModels {
    pub(super) fn install(root: &GameUi) -> Self {
        let model = root.global::<OptionsModel>();
        let models = Self {
            hub_rows: Rc::default(),
            option_rows: Rc::default(),
            tabs: Rc::default(),
            binding_entries: Rc::default(),
        };
        model.set_hub_rows(slint::ModelRc::from(models.hub_rows.clone()));
        model.set_option_rows(slint::ModelRc::from(models.option_rows.clone()));
        model.set_tabs(slint::ModelRc::from(models.tabs.clone()));
        model.set_binding_entries(slint::ModelRc::from(models.binding_entries.clone()));
        models
    }
}

/// One binding row as plain text; the Slint entry (with its nested cell model) is only built
/// when the row differs from what the list already shows.
struct BindingRow {
    cells: Vec<slint::SharedString>,
    disabled: bool,
}

impl BindingRow {
    fn shown_by(&self, entry: &ListEntry) -> bool {
        entry.disabled == self.disabled
            && entry.badge.is_empty()
            && entry.cells.row_count() == self.cells.len()
            && entry.cells.iter().zip(&self.cells).all(|(shown, cell)| shown == *cell)
    }

    fn entry(&self) -> ListEntry {
        ListEntry {
            cells: slint::ModelRc::new(slint::VecModel::from(self.cells.clone())),
            badge: "".into(),
            disabled: self.disabled,
        }
    }
}

/// `sync_rows` for binding rows. A `ListEntry` carries its cells as a nested model object, which
/// never compares equal across frames, so rows are compared by their text and only a row whose
/// text changed is rewritten.
fn sync_binding_entries(model: &slint::VecModel<ListEntry>, next: &[BindingRow]) {
    let current = model.row_count();
    let shared = current.min(next.len());
    for (row, wanted) in next.iter().enumerate().take(shared) {
        if !model.row_data(row).as_ref().is_some_and(|shown| wanted.shown_by(shown)) {
            model.set_row_data(row, wanted.entry());
        }
    }
    for wanted in &next[shared..] {
        model.push(wanted.entry());
    }
    // Trim from the end so the rows that stay keep their delegates.
    for row in (next.len()..current).rev() {
        model.remove(row);
    }
}

pub(super) fn register_callbacks(root: &GameUi, queue: Rc<RefCell<Vec<OptionsUiAction>>>) {
    let model = root.global::<OptionsModel>();
    {
        let q = queue.clone();
        model.on_hub_row_selected(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(OptionsUiAction::HubRowSelected(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_hub_row_activated(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(OptionsUiAction::HubRowActivated(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_option_selected(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(OptionsUiAction::OptionSelected(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_option_activated(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(OptionsUiAction::OptionActivated(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_option_stepped(move |idx, dir| {
            if idx >= 0 && dir != 0 {
                q.borrow_mut()
                    .push(OptionsUiAction::OptionStepped(idx as usize, dir));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_option_slider_changed(move |idx, val| {
            if idx >= 0 {
                q.borrow_mut().push(OptionsUiAction::OptionSliderChanged(
                    idx as usize,
                    val as f64,
                ));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_tab_selected(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(OptionsUiAction::TabSelected(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_binding_hovered(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(OptionsUiAction::BindingHovered(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_binding_selected(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(OptionsUiAction::BindingSelected(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_binding_activated(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(OptionsUiAction::BindingActivated(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        model.on_left_selected(move || q.borrow_mut().push(OptionsUiAction::LeftSelected));
    }
    {
        let q = queue.clone();
        model.on_middle_selected(move || q.borrow_mut().push(OptionsUiAction::MiddleSelected));
    }
    {
        let q = queue.clone();
        model.on_left_activated(move || q.borrow_mut().push(OptionsUiAction::LeftActivated));
    }
    {
        let q = queue.clone();
        model.on_middle_activated(move || q.borrow_mut().push(OptionsUiAction::MiddleActivated));
    }
    {
        let q = queue;
        model.on_modal_button_activated(move |idx| {
            if idx >= 0 {
                q.borrow_mut().push(OptionsUiAction::ModalButton(idx as usize));
            }
        });
    }
}

fn map_option_kind(item: &bfe_interface::widgets::OptionsItem) -> SlintOptionKind {
    if item.id == "reset" || item.id == "panel" {
        return SlintOptionKind::Action;
    }
    match item.kind {
        InterfaceOptionKind::Choice => SlintOptionKind::Choice,
        InterfaceOptionKind::Toggle => SlintOptionKind::Toggle,
        InterfaceOptionKind::Slider => SlintOptionKind::Slider,
        InterfaceOptionKind::Fixed => SlintOptionKind::Fixed,
        InterfaceOptionKind::Action => SlintOptionKind::Action,
    }
}

/// The key-capture prompt and the confirm and OK popups are screens of their own that are not
/// flagged `modal`, but they sit over the page like dialogs, so they go on the dialog layer.
fn shows_as_modal(screen: &bfe_interface::screen::MenuScreen) -> bool {
    screen.modal || screen.key_capture.is_some() || matches!(screen.id.as_str(), "confirm" | "ok")
}

pub(super) fn update_visible_capacities(shell: &mut ShellHost) {
    let h = shell.layout_size[1].max(1) as f64;
    let heading_y = h * 0.07;
    let heading_h = (h * 0.046).clamp(26., 50.) * 1.05;
    let head_gap = h * 0.036;
    let body_y = heading_y + heading_h + head_gap;
    let footer_h = (h * 0.046).clamp(34., 46.);
    let body_h = (h - body_y - h * 0.06 - footer_h).max(80.);
    let note_reserve = (h * 0.015).clamp(11., 15.) * 1.4 + h * 0.012;
    let available_h = (body_h - note_reserve).max(80.);
    let pad_y = h * 0.024;
    let opt_h = (h * 0.048).clamp(36., 50.);

    let Some(base) = shell.options_ctrl.stack.screens.iter_mut().rfind(|s| !shows_as_modal(s)) else {
        return;
    };
    let is_controls = base.id == "options-controls";
    let mut top_rows = 0usize;
    for widget in &mut base.widgets {
        if let Widget::Options(opts) = widget {
            top_rows = opts.items.iter().filter(|i| !i.hidden).count();
            if !is_controls {
                let shown = ((available_h - 2. * pad_y - 2.) / opt_h).floor().max(1.) as usize;
                opts.shown = shown;
                let max_first = top_rows.saturating_sub(shown);
                if opts.first > max_first {
                    opts.first = max_first;
                }
            }
        }
    }
    if is_controls {
        let panel_h = available_h.max(180.);
        let tabs_h = 49.;
        let rows_y = pad_y + tabs_h + h * 0.01;
        let top_options_h = top_rows as f64 * opt_h;
        let heading_y_in_panel = rows_y + top_options_h + h * 0.024;
        let heading_text_h = (h * 0.019).clamp(13., 19.) * 1.25;
        let list_y = heading_y_in_panel + heading_text_h + h * 0.012;
        let list_h = (panel_h - list_y - pad_y).max(120.) - 8.;
        let list_row_h = (h * 0.04).clamp(30., 42.);
        let shown = (list_h / list_row_h).floor().max(1.) as usize;
        for widget in &mut base.widgets {
            if let Widget::List(list) = widget {
                list.shown = shown;
                let max_first = list.rows.len().saturating_sub(shown);
                if list.first > max_first {
                    list.first = max_first;
                }
            }
        }
    }
}

pub(super) fn panel_rect(shell: &ShellHost) -> Option<[f64; 4]> {
    let base = shell.options_ctrl.stack.screens.iter().rfind(|s| !shows_as_modal(s))?;
    if !base
        .widgets
        .iter()
        .any(|w| matches!(w, Widget::Options(_) | Widget::List(_)))
    {
        return None;
    }
    let w = shell.layout_size[0].max(1) as f64;
    let h = shell.layout_size[1].max(1) as f64;
    let gutter = (w * 0.03).clamp(20., 56.);
    let frame_w = (w - 2. * gutter).min(1000.);
    let heading_y = h * 0.07;
    let heading_h = (h * 0.046).clamp(26., 50.) * 1.05;
    let head_gap = h * 0.036;
    let body_y = heading_y + heading_h + head_gap;
    let footer_h = (h * 0.046).clamp(34., 46.);
    let body_h = (h - body_y - h * 0.06 - footer_h).max(80.);
    let note_reserve = (h * 0.015).clamp(11., 15.) * 1.4 + h * 0.012;
    let available_h = (body_h - note_reserve).max(80.);
    let pad_y = h * 0.024;
    let opt_h = (h * 0.048).clamp(36., 50.);

    let has_list = base.widgets.iter().any(|w| matches!(w, Widget::List(_)));
    let (panel_y, panel_h) = if has_list {
        (body_y, available_h.max(180.))
    } else {
        let rows: usize = base
            .widgets
            .iter()
            .filter_map(|w| match w {
                Widget::Options(opts) => Some(opts.items.iter().filter(|i| !i.hidden).count()),
                _ => None,
            })
            .sum();
        let natural_h = rows as f64 * opt_h + 2. * pad_y + 2.;
        let ph = natural_h.min(available_h);
        (body_y + (available_h - ph) / 2., ph)
    };
    let left = (w - frame_w) / 2.;
    Some([left, panel_y, left + frame_w, panel_y + panel_h])
}

pub(super) fn sync(root: &GameUi, shell: &mut ShellHost, models: &OptionsModels) {
    update_visible_capacities(shell);

    let model = root.global::<OptionsModel>();
    let screens = &shell.options_ctrl.stack.screens;
    let Some(base_idx) = screens.iter().rposition(|s| !shows_as_modal(s)) else {
        return;
    };
    let base = &screens[base_idx];
    let top_idx = screens.len().saturating_sub(1);
    let base_active = base_idx == top_idx;

    model.set_subscreen(base.id.as_str().into());
    model.set_dim_behind(base.dim_behind);

    let mut title = String::from("OPTIONS");
    let mut left_label = String::new();
    let mut middle_label = String::new();
    let mut left_focused = false;
    let mut middle_focused = false;
    let mut hub_rows = Vec::new();
    let mut selected_hub_row = 0i32;
    let mut hub_rows_focused = false;
    let mut option_rows = Vec::new();
    let mut selected_option = 0i32;
    let mut first_option = 0i32;
    let mut options_focused = false;
    let mut option_note = String::new();
    let mut tabs_vec = Vec::new();
    let mut active_tab = shell.options_ctrl.controls_tab.clone();
    let mut selected_tab = 0i32;
    let mut tabs_focused = false;
    let mut controls_heading = String::from("CONFIG BINDINGS");
    let mut binding_entries = Vec::new();
    let mut selected_binding = 0i32;
    let mut bindings_focused = false;

    for (wi, widget) in base.widgets.iter().enumerate() {
        if widget.is_hidden() {
            continue;
        }
        let focused = base_active && base.focused == Some(wi);
        match widget {
            Widget::Rows(rows) => {
                if !rows.title.is_empty() {
                    title = rows.title.clone();
                }
                let vis: Vec<_> = rows.visible_rows().collect();
                for item in &vis {
                    hub_rows.push(MenuRowItem {
                        id: item.id.as_str().into(),
                        label: item.label.as_str().into(),
                        disabled: item.disabled,
                    });
                }
                selected_hub_row = rows.selected.min(hub_rows.len().saturating_sub(1)) as i32;
                hub_rows_focused = focused;
            }
            Widget::Text(text) => {
                if text.look == TextLook::Title {
                    title = text.text.clone();
                } else if text.look == TextLook::Heading {
                    controls_heading = text.text.clone();
                }
            }
            Widget::Options(opts) => {
                let mut vis_idx = 0usize;
                let mut selected_vis = 0usize;
                for (raw_idx, item) in opts.items.iter().enumerate() {
                    if item.hidden {
                        continue;
                    }
                    if raw_idx == opts.selected {
                        selected_vis = vis_idx;
                        if focused {
                            option_note = item.note.clone();
                        }
                    }
                    option_rows.push(OptionsRowItem {
                        id: item.id.as_str().into(),
                        label: item.label.as_str().into(),
                        value_text: item.value_text.as_str().into(),
                        kind: map_option_kind(item),
                        fraction: item.ratio as f32,
                        // The browser never dims a fixed row (only a keyboard skip), so it only reads as disabled when it is.
                        disabled: item.disabled && item.kind != InterfaceOptionKind::Fixed,
                        dim_value: item.dim,
                    });
                    vis_idx += 1;
                }
                selected_option = selected_vis as i32;
                first_option = opts.first as i32;
                options_focused = focused;
            }
            Widget::Tabs(tabs) => {
                active_tab = tabs.active.clone();
                selected_tab = tabs.selected as i32;
                tabs_focused = focused;
                for tab in &tabs.tabs {
                    tabs_vec.push(OptionsTabItem {
                        id: tab.id.as_str().into(),
                        label: tab.label.as_str().into(),
                    });
                }
            }
            Widget::List(list) => {
                for entry in &list.rows {
                    binding_entries.push(BindingRow {
                        cells: entry.cells[..entry.cell_count.min(4)]
                            .iter()
                            .map(|s| s.as_str().into())
                            .collect(),
                        // The browser's hotkey rows carry no disabled flag, so none is dimmed there; the cursor still skips them here.
                        disabled: false,
                    });
                }
                selected_binding = list.selected as i32;
                bindings_focused = focused;
            }
            Widget::Button(btn) => {
                if btn.place == "middle" {
                    middle_label = btn.text.clone();
                    middle_focused = focused;
                } else {
                    left_label = btn.text.clone();
                    left_focused = focused;
                }
            }
            _ => {}
        }
    }

    model.set_title(title.into());
    model.set_left_label(left_label.into());
    model.set_middle_label(middle_label.into());
    model.set_left_focused(left_focused);
    model.set_middle_focused(middle_focused);
    sync_rows(&models.hub_rows, &hub_rows);
    model.set_selected_hub_row(selected_hub_row);
    model.set_hub_rows_focused(hub_rows_focused);
    sync_rows(&models.option_rows, &option_rows);
    model.set_selected_option(selected_option);
    model.set_first_option(first_option);
    model.set_options_focused(options_focused);
    model.set_option_note(option_note.into());
    sync_rows(&models.tabs, &tabs_vec);
    model.set_active_tab(active_tab.into());
    model.set_selected_tab(selected_tab);
    model.set_tabs_focused(tabs_focused);
    model.set_controls_heading(controls_heading.into());
    sync_binding_entries(&models.binding_entries, &binding_entries);
    model.set_selected_binding(selected_binding);
    model.set_bindings_focused(bindings_focused);

    let top = &screens[top_idx];
    if shows_as_modal(top) {
        model.set_modal_open(true);
        model.set_key_capture_open(top.key_capture.is_some());
        let mut msg_lines = Vec::new();
        let mut accept_label = String::from("OK");
        let mut cancel_label = String::new();
        let mut selected_btn = 0i32;
        for w in &top.widgets {
            match w {
                Widget::Text(t) if !t.text.is_empty() => msg_lines.push(t.text.as_str()),
                Widget::Button(b) => accept_label = b.text.clone(),
                Widget::HorizontalButtons(hb) => {
                    if let Some(first) = hb.buttons.first() {
                        accept_label = first.label.clone();
                    }
                    if let Some(second) = hb.buttons.get(1) {
                        cancel_label = second.label.clone();
                    }
                    selected_btn = hb.selected as i32;
                }
                _ => {}
            }
        }
        model.set_modal_message(msg_lines.join("\n").into());
        model.set_modal_accept_label(accept_label.into());
        model.set_modal_cancel_label(cancel_label.into());
        model.set_modal_selected_button(selected_btn);
    } else {
        model.set_modal_open(false);
        model.set_key_capture_open(false);
    }
}

fn raw_option_index(opts: &bfe_interface::widgets::OptionsWidget, vis_index: usize) -> Option<usize> {
    opts.items
        .iter()
        .enumerate()
        .filter(|(_, item)| !item.hidden)
        .nth(vis_index)
        .map(|(i, _)| i)
}

fn focus_options_item(
    shell: &mut ShellHost,
    vis_index: usize,
) -> Option<(usize, String, Option<WidgetAction>, InterfaceOptionKind)> {
    let top = shell.options_ctrl.stack.top_mut()?;
    let wi = top
        .widgets
        .iter()
        .position(|w| matches!(w, Widget::Options(_)))?;
    let mut changed = top.focused != Some(wi);
    top.focused = Some(wi);
    let Widget::Options(opts) = &mut top.widgets[wi] else {
        return None;
    };
    let raw_idx = raw_option_index(opts, vis_index)?;
    let item = opts.items.get(raw_idx)?;
    if item.disabled {
        return None;
    }
    if opts.selected != raw_idx {
        opts.selected = raw_idx;
        changed = true;
    }
    let result = (raw_idx, item.id.clone(), item.action.clone(), item.kind);
    if changed {
        shell.options_ctrl.stack.sounds.play(SoundCue::SelectChange);
    }
    Some(result)
}

pub(super) fn drain_actions(
    queue: &Rc<RefCell<Vec<OptionsUiAction>>>,
    shell: &mut ShellHost,
    settings: &mut AppSettingsStore,
    audio_router: &SharedAudioRouter,
) -> Option<ShellTransition> {
    let pending: Vec<OptionsUiAction> = queue.borrow_mut().drain(..).collect();
    if pending.is_empty() {
        return None;
    }
    shell.sync_options_controller(settings);
    for action in pending {
        match action {
            OptionsUiAction::HubRowSelected(vis_idx) => {
                if let Some(top) = shell.options_ctrl.stack.top_mut() {
                    if let Some(wi) = top.widgets.iter().position(|w| matches!(w, Widget::Rows(_))) {
                        let mut changed = top.focused != Some(wi);
                        top.focused = Some(wi);
                        if let Widget::Rows(rows) = &mut top.widgets[wi] {
                            let enabled = rows
                                .visible_rows()
                                .nth(vis_idx)
                                .is_some_and(|r| !r.disabled);
                            if enabled && rows.selected != vis_idx {
                                rows.selected = vis_idx;
                                changed = true;
                            }
                        }
                        if changed {
                            shell.options_ctrl.stack.sounds.play(SoundCue::SelectChange);
                        }
                    }
                }
            }
            OptionsUiAction::HubRowActivated(vis_idx) => {
                let mut act = None;
                if let Some(top) = shell.options_ctrl.stack.top_mut() {
                    if let Some(wi) = top.widgets.iter().position(|w| matches!(w, Widget::Rows(_))) {
                        top.focused = Some(wi);
                        if let Widget::Rows(rows) = &mut top.widgets[wi] {
                            rows.selected = vis_idx;
                            act = rows.accept();
                        }
                    }
                }
                if let Some(a) = act {
                    shell
                        .options_ctrl
                        .apply_event(MenuInputEvent::Action(a));
                }
            }
            OptionsUiAction::OptionSelected(vis_idx) => {
                let _ = focus_options_item(shell, vis_idx);
            }
            OptionsUiAction::OptionActivated(vis_idx) => {
                if let Some((_, key, item_action, kind)) = focus_options_item(shell, vis_idx) {
                    if key == "discord-activity" {
                        shell.step_discord_detail(settings, false);
                    } else if let Some(act) = item_action {
                        shell.options_ctrl.apply_event(MenuInputEvent::Action(act));
                    } else if kind != InterfaceOptionKind::Slider && kind != InterfaceOptionKind::Fixed {
                        shell
                            .options_ctrl
                            .apply_event(MenuInputEvent::StepOption { key, dir: 1 });
                    }
                }
            }
            OptionsUiAction::OptionStepped(vis_idx, dir) => {
                if let Some((_, key, item_action, _)) = focus_options_item(shell, vis_idx) {
                    if key == "discord-activity" {
                        shell.step_discord_detail(settings, dir < 0);
                    } else if key == "viewpoint" {
                        if let Some(act) = item_action {
                            shell.options_ctrl.apply_event(MenuInputEvent::Action(act));
                        }
                    } else if item_action.is_none() {
                        shell
                            .options_ctrl
                            .apply_event(MenuInputEvent::StepOption { key, dir });
                    }
                }
            }
            OptionsUiAction::OptionSliderChanged(vis_idx, ratio) => {
                if let Some((_, key, _, kind)) = focus_options_item(shell, vis_idx) {
                    if kind == InterfaceOptionKind::Slider {
                        shell
                            .options_ctrl
                            .apply_event(MenuInputEvent::SetOptionRatio { key, ratio });
                    }
                }
            }
            OptionsUiAction::TabSelected(tab_idx) => {
                let mut tab_id = None;
                if let Some(top) = shell.options_ctrl.stack.top_mut() {
                    if let Some(wi) = top.widgets.iter().position(|w| matches!(w, Widget::Tabs(_))) {
                        top.focused = Some(wi);
                        if let Widget::Tabs(tabs) = &mut top.widgets[wi] {
                            if let Some(tab) = tabs.tabs.get(tab_idx) {
                                tabs.selected = tab_idx;
                                tab_id = Some(tab.id.clone());
                            }
                        }
                    }
                }
                if let Some(id) = tab_id {
                    shell
                        .options_ctrl
                        .apply_event(MenuInputEvent::Action(WidgetAction::SelectTab(id)));
                }
            }
            OptionsUiAction::BindingHovered(row_idx) => {
                let _ = row_idx;
                if let Some(top) = shell.options_ctrl.stack.top_mut() {
                    if let Some(wi) = top.widgets.iter().position(|w| matches!(w, Widget::List(_))) {
                        top.focused = Some(wi);
                    }
                }
            }
            OptionsUiAction::BindingSelected(row_idx) => {
                if let Some(top) = shell.options_ctrl.stack.top_mut() {
                    if let Some(wi) = top.widgets.iter().position(|w| matches!(w, Widget::List(_))) {
                        top.focused = Some(wi);
                        if let Widget::List(list) = &mut top.widgets[wi] {
                            if row_idx < list.rows.len() && !list.rows[row_idx].disabled {
                                list.selected = row_idx;
                            }
                        }
                    }
                }
            }
            OptionsUiAction::BindingActivated(row_idx) => {
                let mut act = None;
                if let Some(top) = shell.options_ctrl.stack.top_mut() {
                    if let Some(wi) = top.widgets.iter().position(|w| matches!(w, Widget::List(_))) {
                        top.focused = Some(wi);
                        if let Widget::List(list) = &mut top.widgets[wi] {
                            if row_idx < list.rows.len() && !list.rows[row_idx].disabled {
                                list.selected = row_idx;
                                act = list.rows[row_idx].action.clone();
                            }
                        }
                    }
                }
                if let Some(a) = act {
                    shell
                        .options_ctrl
                        .apply_event(MenuInputEvent::Action(a));
                }
            }
            OptionsUiAction::LeftSelected | OptionsUiAction::MiddleSelected => {
                let target_place = if matches!(action, OptionsUiAction::MiddleSelected) {
                    "middle"
                } else {
                    "back"
                };
                if let Some(top) = shell.options_ctrl.stack.top_mut() {
                    if let Some(wi) = top.widgets.iter().position(|w| {
                        matches!(w, Widget::Button(b) if b.place == target_place || (target_place == "back" && b.id == "back"))
                    }) {
                        if top.focused != Some(wi) {
                            top.focused = Some(wi);
                            shell.options_ctrl.stack.sounds.play(SoundCue::SelectChange);
                        }
                    }
                }
            }
            OptionsUiAction::LeftActivated => {
                shell
                    .options_ctrl
                    .apply_event(MenuInputEvent::Action(WidgetAction::Back));
            }
            OptionsUiAction::MiddleActivated => {
                let mut act = None;
                if let Some(top) = shell.options_ctrl.stack.top_mut() {
                    if let Some(wi) = top
                        .widgets
                        .iter()
                        .position(|w| matches!(w, Widget::Button(b) if b.place == "middle"))
                    {
                        top.focused = Some(wi);
                        if let Widget::Button(b) = &top.widgets[wi] {
                            act = b.action.clone();
                        }
                    }
                }
                if let Some(a) = act {
                    shell
                        .options_ctrl
                        .apply_event(MenuInputEvent::Action(a));
                }
            }
            OptionsUiAction::ModalButton(index) => {
                // A click answers with the button it landed on, not with the one the cursor was on.
                if let Some(top) = shell.options_ctrl.stack.top_mut() {
                    for widget in &mut top.widgets {
                        if let Widget::HorizontalButtons(buttons) = widget {
                            if index < buttons.buttons.len() {
                                buttons.selected = index;
                            }
                        }
                    }
                }
                shell.options_ctrl.on_key_down("Enter", false);
            }
        }
    }
    shell.flush_options_controller(settings);
    shell.flush_events(settings, audio_router)
}
