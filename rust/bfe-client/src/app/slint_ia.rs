//! Slint synchronization and callback dispatch for the Instant Action flow
//! (`Singleplayer`, `InstantAction`, `InstantActionEra`) and the loading
//! screen's Saira page overlay (`LoadingPageOverlay`).

use super::{
    settings::AppSettingsStore,
    shell::{ShellHost, ShellTransition},
    slint_models::sync_rows,
    slint_ui::{GameUi, IaModel, ListEntry, MenuRowItem, Theme},
};
use crate::audio::SharedAudioRouter;
use bfe_shell::screens::{IaFocus, ScreenId};
use slint::{ComponentHandle, Model, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};

pub(super) enum IaAction {
    SpRowSelected(usize),
    SpRowActivated(usize),
    Back,
    IaFocused(i32),
    GamePicked(usize),
    IaMapSelected(usize),
    IaMapActivated(usize),
    IaPlaylistSelected(usize),
    IaPlaylistActivated(usize),
    IaAdd,
    IaRemove,
    IaOrderToggled,
    IaLaunch,
    EraRowSelected(usize),
    EraRowActivated(usize),
}

/// What a list row shows, kept beside its model so an unchanged row is never rewritten.
#[derive(Clone, PartialEq)]
struct EntryKey {
    name: String,
    badge: &'static str,
    disabled: bool,
}

/// The list models behind the Instant Action screens. Each lives as long as the screens and is set
/// into `IaModel` once; `sync_ia` only edits rows (see `slint_models`), so a row keeps its hover,
/// held press and click timing from frame to frame.
struct IaModels {
    sp_rows: Rc<VecModel<MenuRowItem>>,
    era_rows: Rc<VecModel<MenuRowItem>>,
    game_choices: Rc<VecModel<SharedString>>,
    game_disabled: Rc<VecModel<bool>>,
    maps: Rc<VecModel<ListEntry>>,
    maps_shown: RefCell<Vec<EntryKey>>,
    playlist: Rc<VecModel<ListEntry>>,
    playlist_shown: RefCell<Vec<EntryKey>>,
}

/// The Slint callbacks' queue of actions, and the list models `sync_ia` keeps up to date.
pub(super) struct IaActionQueue {
    actions: Rc<RefCell<Vec<IaAction>>>,
    models: IaModels,
}

pub(super) fn register_ia(root: &GameUi) -> IaActionQueue {
    let queue: Rc<RefCell<Vec<IaAction>>> = Rc::new(RefCell::new(Vec::new()));
    let ia = root.global::<IaModel>();
    let models = IaModels {
        sp_rows: Rc::new(VecModel::default()),
        era_rows: Rc::new(VecModel::default()),
        game_choices: Rc::new(VecModel::default()),
        game_disabled: Rc::new(VecModel::default()),
        maps: Rc::new(VecModel::default()),
        maps_shown: RefCell::new(Vec::new()),
        playlist: Rc::new(VecModel::default()),
        playlist_shown: RefCell::new(Vec::new()),
    };
    ia.set_sp_rows(models.sp_rows.clone().into());
    ia.set_era_rows(models.era_rows.clone().into());
    ia.set_game_choices(models.game_choices.clone().into());
    ia.set_game_disabled(models.game_disabled.clone().into());
    ia.set_maps(models.maps.clone().into());
    ia.set_playlist(models.playlist.clone().into());

    {
        let q = queue.clone();
        ia.on_sp_row_selected(move |idx| {
            if idx >= 0 {
                q.borrow_mut().push(IaAction::SpRowSelected(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        ia.on_sp_row_activated(move |idx| {
            if idx >= 0 {
                q.borrow_mut().push(IaAction::SpRowActivated(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        ia.on_sp_back(move || q.borrow_mut().push(IaAction::Back));
    }
    {
        let q = queue.clone();
        ia.on_game_selected(move |idx| {
            if idx >= 0 {
                q.borrow_mut().push(IaAction::GamePicked(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        ia.on_ia_focused(move |slot| q.borrow_mut().push(IaAction::IaFocused(slot)));
    }
    {
        let q = queue.clone();
        ia.on_ia_map_selected(move |idx| {
            if idx >= 0 {
                q.borrow_mut().push(IaAction::IaMapSelected(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        ia.on_ia_map_activated(move |idx| {
            if idx >= 0 {
                q.borrow_mut().push(IaAction::IaMapActivated(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        ia.on_ia_playlist_selected(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(IaAction::IaPlaylistSelected(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        ia.on_ia_playlist_activated(move |idx| {
            if idx >= 0 {
                q.borrow_mut()
                    .push(IaAction::IaPlaylistActivated(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        ia.on_ia_add(move || q.borrow_mut().push(IaAction::IaAdd));
    }
    {
        let q = queue.clone();
        ia.on_ia_remove(move || q.borrow_mut().push(IaAction::IaRemove));
    }
    {
        let q = queue.clone();
        ia.on_ia_order_toggled(move || q.borrow_mut().push(IaAction::IaOrderToggled));
    }
    {
        let q = queue.clone();
        ia.on_ia_launch(move || q.borrow_mut().push(IaAction::IaLaunch));
    }
    {
        let q = queue.clone();
        ia.on_ia_back(move || q.borrow_mut().push(IaAction::Back));
    }
    {
        let q = queue.clone();
        ia.on_era_row_selected(move |idx| {
            if idx >= 0 {
                q.borrow_mut().push(IaAction::EraRowSelected(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        ia.on_era_row_activated(move |idx| {
            if idx >= 0 {
                q.borrow_mut().push(IaAction::EraRowActivated(idx as usize));
            }
        });
    }
    {
        let q = queue.clone();
        ia.on_era_back(move || q.borrow_mut().push(IaAction::Back));
    }

    IaActionQueue { actions: queue, models }
}

fn focus_to_slot(focus: IaFocus) -> i32 {
    match focus {
        IaFocus::Available => 0,
        IaFocus::Add => 1,
        IaFocus::Remove => 2,
        IaFocus::Selected => 3,
        IaFocus::Back => 4,
        IaFocus::Order => 5,
        IaFocus::Launch => 6,
    }
}

fn slot_to_focus(slot: i32) -> IaFocus {
    match slot {
        1 => IaFocus::Add,
        2 => IaFocus::Remove,
        3 => IaFocus::Selected,
        4 => IaFocus::Back,
        5 => IaFocus::Order,
        6 => IaFocus::Launch,
        _ => IaFocus::Available,
    }
}

/// Brings `model` to `next` row by row, like `sync_rows`, comparing the `EntryKey`s of what is shown
/// (a row's cell model is new each time it is built, so the rows themselves cannot be compared).
fn sync_entries(model: &VecModel<ListEntry>, shown: &RefCell<Vec<EntryKey>>, next: Vec<EntryKey>) {
    let mut shown = shown.borrow_mut();
    let entry = |key: &EntryKey| ListEntry {
        cells: slint::ModelRc::new(VecModel::from(vec![SharedString::from(key.name.as_str())])),
        badge: key.badge.into(),
        disabled: key.disabled,
    };
    for (row, key) in next.iter().enumerate() {
        match shown.get(row) {
            Some(old) if old == key => {}
            Some(_) => model.set_row_data(row, entry(key)),
            None => model.push(entry(key)),
        }
    }
    for row in (next.len()..shown.len()).rev() {
        model.remove(row);
    }
    *shown = next;
}

fn menu_items(rows: &[bfe_shell::screens::MenuRow]) -> Vec<MenuRowItem> {
    rows.iter()
        .map(|r| MenuRowItem {
            id: r.id.as_str().into(),
            label: r.label.as_str().into(),
            disabled: r.disabled,
        })
        .collect()
}

pub(super) fn sync_ia(root: &GameUi, queue: &IaActionQueue, shell: &mut ShellHost, shell_visible: bool) {
    let models = &queue.models;
    let menu_theme = match shell.app.settings.menu_theme.as_str() {
        "bf1" => "bf1",
        "bf2" => "bf2",
        _ => "eternal",
    };
    root.global::<Theme>().set_menu_theme(menu_theme.into());

    let ia = root.global::<IaModel>();
    if let Some((planet, word)) = &shell.loading_overlay {
        ia.set_loading_visible(!shell_visible);
        if ia.get_loading_planet().as_str() != planet {
            ia.set_loading_planet(planet.as_str().into());
        }
        if ia.get_loading_word().as_str() != word {
            ia.set_loading_word(word.as_str().into());
        }
    } else {
        ia.set_loading_visible(false);
    }

    if !shell_visible {
        return;
    }

    let id = if shell.app.current_screen() == ScreenId::Popup {
        shell.app.stack.last().copied().unwrap_or(ScreenId::Main)
    } else {
        shell.app.current_screen()
    };

    match id {
        ScreenId::Singleplayer => {
            sync_rows(
                &models.sp_rows,
                &menu_items(&shell.app.rows_for(ScreenId::Singleplayer)),
            );
            let sp_sel = shell.app.selected_row_index(ScreenId::Singleplayer) as i32;
            if ia.get_sp_selected() != sp_sel {
                ia.set_sp_selected(sp_sel);
            }
        }
        ScreenId::InstantAction => {
            let game_opts = shell.app.ia_game_picker_options();
            let choices: Vec<SharedString> =
                game_opts.iter().map(|(_, label, _)| (*label).into()).collect();
            let disabled: Vec<bool> = game_opts.iter().map(|(_, _, enabled)| !*enabled).collect();
            sync_rows(&models.game_choices, &choices);
            sync_rows(&models.game_disabled, &disabled);
            let game_idx = game_opts
                .iter()
                .position(|(family, _, _)| *family == shell.app.ia_family)
                .unwrap_or(0) as i32;
            if ia.get_game_index() != game_idx {
                ia.set_game_index(game_idx);
            }

            let maps: Vec<EntryKey> = shell
                .app
                .data
                .maps
                .iter()
                .map(|m| EntryKey {
                    name: m.name.clone(),
                    badge: "",
                    disabled: m.extracted == Some(false),
                })
                .collect();
            sync_entries(&models.maps, &models.maps_shown, maps);
            let avail_idx = shell.app.ia_available_selected as i32;
            if ia.get_map_index() != avail_idx {
                ia.set_map_index(avail_idx);
            }

            let playlist: Vec<EntryKey> = shell
                .app
                .ia_entries
                .iter()
                .map(|entry| {
                    let name = shell
                        .app
                        .data
                        .maps
                        .iter()
                        .find(|m| m.id == entry.map)
                        .map(|m| m.name.as_str())
                        .unwrap_or(entry.map.as_str());
                    let badge = if entry.era.as_deref() == Some("cw") {
                        "REP|CIS"
                    } else {
                        "IMP|REB"
                    };
                    EntryKey { name: name.to_string(), badge, disabled: false }
                })
                .collect();
            sync_entries(&models.playlist, &models.playlist_shown, playlist);
            let p_count = shell.app.ia_entries.len() as i32;
            if ia.get_playlist_count() != p_count {
                ia.set_playlist_count(p_count);
            }
            let sel_idx = if shell.app.ia_entries.is_empty() {
                -1
            } else {
                shell.app.ia_selected_cursor as i32
            };
            if ia.get_playlist_index() != sel_idx {
                ia.set_playlist_index(sel_idx);
            }

            let caption = shell
                .app
                .data
                .maps
                .get(shell.app.ia_available_selected)
                .map(|m| m.name.as_str())
                .unwrap_or("");
            if ia.get_preview_caption().as_str() != caption {
                ia.set_preview_caption(caption.into());
            }

            let [pw, ph] = shell.preview_texture.size;
            if shell.preview_movies.has_frame() && pw > 0 && ph > 0 {
                let image = unsafe {
                    slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                        shell.preview_texture.texture.0,
                        (pw as u32, ph as u32).into(),
                    )
                    .origin(slint::BorrowedOpenGLTextureOrigin::TopLeft)
                    .build()
                };
                ia.set_preview_frame(image);
                ia.set_preview_visible(true);
            } else {
                ia.set_preview_visible(false);
            }

            let order_label = if shell.app.ia_random {
                "RANDOM ORDER"
            } else {
                "PLAY IN ORDER"
            };
            if ia.get_ia_order_label().as_str() != order_label {
                ia.set_ia_order_label(order_label.into());
            }

            let slot = focus_to_slot(shell.app.ia_focus);
            if ia.get_ia_focus() != slot {
                ia.set_ia_focus(slot);
            }
        }
        ScreenId::InstantActionEra => {
            let title = shell.app.screen_title(ScreenId::InstantActionEra);
            if ia.get_era_map_title().as_str() != title {
                ia.set_era_map_title(title.into());
            }
            sync_rows(
                &models.era_rows,
                &menu_items(&shell.app.rows_for(ScreenId::InstantActionEra)),
            );
            let era_sel = shell.app.selected_row_index(ScreenId::InstantActionEra) as i32;
            if ia.get_era_selected() != era_sel {
                ia.set_era_selected(era_sel);
            }
        }
        _ => {}
    }
}

pub(super) fn drain_ia(
    queue: &IaActionQueue,
    shell: &mut ShellHost,
    settings: &mut AppSettingsStore,
    audio_router: &SharedAudioRouter,
) -> Option<ShellTransition> {
    let pending: Vec<IaAction> = queue.actions.borrow_mut().drain(..).collect();
    if pending.is_empty() {
        return None;
    }
    let mut transition = None;
    for action in pending {
        let next = match action {
            IaAction::SpRowSelected(idx) => {
                shell.app.set_selected_row_index(ScreenId::Singleplayer, idx);
                shell.flush_events(settings, audio_router)
            }
            IaAction::SpRowActivated(idx) => {
                shell.app.set_selected_row_index(ScreenId::Singleplayer, idx);
                shell.handle_key("Enter", settings, audio_router)
            }
            IaAction::Back => shell.handle_key("Escape", settings, audio_router),
            IaAction::IaFocused(slot) => {
                let next_focus = slot_to_focus(slot);
                if shell.app.ia_focus != next_focus {
                    shell.app.ia_focus = next_focus;
                    shell.app.sound_queue.push("shell_select_change");
                }
                shell.flush_events(settings, audio_router)
            }
            IaAction::GamePicked(idx) => {
                // A disabled (PLANNED) family is refused by `ia_select_family`; the picker shows the
                // family the model holds on the next sync either way.
                if let Some((family, _, _)) = shell.app.ia_game_picker_options().get(idx).copied() {
                    shell.app.ia_select_family(family);
                }
                shell.flush_events(settings, audio_router)
            }
            IaAction::IaMapSelected(idx) => {
                shell.app.ia_focus = IaFocus::Available;
                if idx < shell.app.data.maps.len() && shell.app.ia_available_selected != idx {
                    shell.app.ia_available_selected = idx;
                    shell.app.sound_queue.push("shell_select_change");
                    let map_id = shell.app.data.maps.get(idx).map(|m| m.id.clone());
                    let ev = shell.app.media.preview(&shell.app.data, map_id.as_deref());
                    shell.app.media_events.push(ev);
                }
                shell.flush_events(settings, audio_router)
            }
            IaAction::IaMapActivated(idx) => {
                shell.app.ia_focus = IaFocus::Available;
                if idx < shell.app.data.maps.len() && shell.app.ia_available_selected != idx {
                    shell.app.ia_available_selected = idx;
                    let map_id = shell.app.data.maps.get(idx).map(|m| m.id.clone());
                    let ev = shell.app.media.preview(&shell.app.data, map_id.as_deref());
                    shell.app.media_events.push(ev);
                }
                shell.app.ia_add();
                if shell.app.current_screen() == ScreenId::InstantActionEra {
                    shell.app.row_cursor.insert("ia-era", 0);
                }
                shell.flush_events(settings, audio_router)
            }
            IaAction::IaPlaylistSelected(idx) => {
                shell.app.ia_focus = IaFocus::Selected;
                if idx < shell.app.ia_entries.len() && shell.app.ia_selected_cursor != idx {
                    shell.app.ia_selected_cursor = idx;
                    shell.app.sound_queue.push("shell_select_change");
                }
                shell.flush_events(settings, audio_router)
            }
            IaAction::IaPlaylistActivated(idx) => {
                shell.app.ia_focus = IaFocus::Selected;
                if idx < shell.app.ia_entries.len() {
                    shell.app.ia_selected_cursor = idx;
                }
                shell.app.ia_remove();
                shell.flush_events(settings, audio_router)
            }
            IaAction::IaAdd => {
                shell.app.ia_focus = IaFocus::Add;
                shell.app.ia_add();
                if shell.app.current_screen() == ScreenId::InstantActionEra {
                    shell.app.row_cursor.insert("ia-era", 0);
                }
                shell.flush_events(settings, audio_router)
            }
            IaAction::IaRemove => {
                shell.app.ia_focus = IaFocus::Remove;
                shell.app.ia_remove();
                shell.flush_events(settings, audio_router)
            }
            IaAction::IaOrderToggled => {
                shell.app.ia_focus = IaFocus::Order;
                shell.app.ia_toggle_order();
                shell.flush_events(settings, audio_router)
            }
            IaAction::IaLaunch => {
                shell.app.ia_focus = IaFocus::Launch;
                shell.handle_key("Enter", settings, audio_router)
            }
            IaAction::EraRowSelected(idx) => {
                shell
                    .app
                    .set_selected_row_index(ScreenId::InstantActionEra, idx);
                shell.flush_events(settings, audio_router)
            }
            IaAction::EraRowActivated(idx) => {
                shell
                    .app
                    .set_selected_row_index(ScreenId::InstantActionEra, idx);
                shell.handle_key("Enter", settings, audio_router)
            }
        };
        if next.is_some() {
            transition = next;
        }
    }
    transition
}
