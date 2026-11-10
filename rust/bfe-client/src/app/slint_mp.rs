//! Slint multiplayer screen model & action wiring (`rust/bfe-ui/ui/screens/mp.slint`).
//!
//! Feeds `MultiplayerScreen`, `HostDialogModal`, `DirectConnectModal`, and
//! `OnlineNoticeModal` via `MpModel` from `ShellHost` (`ShellApp` + `LiveServerBrowser`)
//! and routes UI callbacks back to the existing shell state machine.
use super::{
    slint_models::sync_rows,
    settings::AppSettingsStore,
    shell::{ShellHost, ShellTransition},
    slint_ui::{BrowserRowItem, FactItem, MpModel, SlintHost, UiAction},
};
use crate::audio::SharedAudioRouter;
use bfe_online::{default_consent_path, write_online_consent, ONLINE_NOTICE_VERSION};
use bfe_online::browser::{
    compatible, country_of, era_label, mode_label, ping_quality, BrowserState, FilterUpdate,
    MapCatalogEntry, PingFilter, ServerBrowserService, ServerContract, SortColumn, Source,
};
use bfe_shell::screens::{HostControl, MultiplayerKind, PopupState, ScreenId};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;

/// The persistent list models behind `MpModel`'s repeaters. Each is created once and set into the
/// global once; every sync updates it in place with `sync_rows`, so delegates (and the hover, press
/// and double-click state they carry) live as long as their row does.
pub(super) struct MpModels {
    pub(super) notice_paragraphs: Rc<VecModel<SharedString>>,
    pub(super) map_options: Rc<VecModel<SharedString>>,
    pub(super) server_rows: Rc<VecModel<BrowserRowItem>>,
    pub(super) info_chips: Rc<VecModel<SharedString>>,
    pub(super) info_facts: Rc<VecModel<FactItem>>,
    pub(super) host_visibility: Rc<VecModel<SharedString>>,
    pub(super) host_map: Rc<VecModel<SharedString>>,
    pub(super) host_side: Rc<VecModel<SharedString>>,
    pub(super) host_connection: Rc<VecModel<SharedString>>,
    /// Map count the filter's map options were built for: the extracted-map list only changes
    /// with the content index, so it is rebuilt when that count changes, not every frame.
    pub(super) map_options_key: Option<usize>,
}

impl MpModels {
    /// Creates every model and hands it to `MpModel` once.
    pub(super) fn install(root: &super::slint_ui::GameUi) -> Self {
        let models = Self {
            notice_paragraphs: Rc::new(VecModel::default()),
            map_options: Rc::new(VecModel::default()),
            server_rows: Rc::new(VecModel::default()),
            info_chips: Rc::new(VecModel::default()),
            info_facts: Rc::new(VecModel::default()),
            host_visibility: Rc::new(VecModel::default()),
            host_map: Rc::new(VecModel::default()),
            host_side: Rc::new(VecModel::default()),
            host_connection: Rc::new(VecModel::default()),
            map_options_key: None,
        };
        let mp = root.global::<MpModel>();
        mp.set_notice_paragraphs(ModelRc::from(models.notice_paragraphs.clone()));
        mp.set_map_options(ModelRc::from(models.map_options.clone()));
        mp.set_server_rows(ModelRc::from(models.server_rows.clone()));
        mp.set_info_chips(ModelRc::from(models.info_chips.clone()));
        mp.set_info_facts(ModelRc::from(models.info_facts.clone()));
        mp.set_host_visibility_options(ModelRc::from(models.host_visibility.clone()));
        mp.set_host_map_options(ModelRc::from(models.host_map.clone()));
        mp.set_host_side_options(ModelRc::from(models.host_side.clone()));
        mp.set_host_connection_options(ModelRc::from(models.host_connection.clone()));
        models
    }
}

/// Labels of a `host_choices` list as Slint strings.
fn choice_labels(choices: &[(String, String)]) -> Vec<SharedString> {
    choices
        .iter()
        .map(|(_, label)| SharedString::from(label.as_str()))
        .collect()
}

#[derive(Debug)]
pub(super) enum MpAction {
    TabCreate,
    SourceSelected(i32),
    SearchEdited(String),
    SearchSubmitted(String),
    MapSelected(i32),
    EraSelected(i32),
    PingSelected(i32),
    ToggleHideFull,
    ToggleHideEmpty,
    SortBy(i32),
    RowSelected(i32),
    RowDoubleClicked(i32),
    ReviewNotice,
    Refresh,
    DirectConnect,
    JoinSelected,
    Back,
    OpenSponsorUrl(String),
    HostNameEdited(String),
    HostVisibilitySelected(i32),
    HostMapSelected(i32),
    HostSideSelected(i32),
    HostPlayersEdited(String),
    HostConnectionSelected(i32),
    HostSubmit,
    HostClose,
    DirectAddressEdited(String),
    DirectPortEdited(String),
    DirectSubmit,
    DirectCancel,
    /// Accept clicked with the notice version the dialog shows (`bridge.request('accept', notice.version)`).
    NoticeAccept(String),
    /// Second phase of accept: the frame showing `Saving your choice…` has been drawn, so write consent now.
    NoticeCommit,
    NoticeDecline,
}

impl SlintHost {
    pub(super) fn register_mp_callbacks(&self) {
        let mp = self.root.global::<MpModel>();
        let actions = &self.actions;

        {
            let q = actions.clone();
            mp.on_tab_create(move || q.borrow_mut().push(UiAction::Mp(MpAction::TabCreate)));
        }
        {
            let q = actions.clone();
            mp.on_source_selected(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::SourceSelected(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_search_edited(move |val| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::SearchEdited(val.into())));
            });
        }
        {
            let q = actions.clone();
            mp.on_search_submitted(move |val| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::SearchSubmitted(val.into())));
            });
        }
        {
            let q = actions.clone();
            mp.on_map_selected(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::MapSelected(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_era_selected(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::EraSelected(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_ping_selected(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::PingSelected(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_toggle_hide_full(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::ToggleHideFull));
            });
        }
        {
            let q = actions.clone();
            mp.on_toggle_hide_empty(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::ToggleHideEmpty));
            });
        }
        {
            let q = actions.clone();
            mp.on_sort_by(move |col| {
                q.borrow_mut().push(UiAction::Mp(MpAction::SortBy(col)));
            });
        }
        {
            let q = actions.clone();
            mp.on_row_selected(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::RowSelected(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_row_double_clicked(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::RowDoubleClicked(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_review_notice(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::ReviewNotice));
            });
        }
        {
            let q = actions.clone();
            mp.on_refresh(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::Refresh));
            });
        }
        {
            let q = actions.clone();
            mp.on_direct_connect(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::DirectConnect));
            });
        }
        {
            let q = actions.clone();
            mp.on_join_selected(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::JoinSelected));
            });
        }
        {
            let q = actions.clone();
            mp.on_back(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::Back));
            });
        }
        {
            let q = actions.clone();
            mp.on_open_sponsor_url(move |url| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::OpenSponsorUrl(url.into())));
            });
        }

        // Host dialog callbacks
        {
            let q = actions.clone();
            mp.on_host_name_edited(move |val| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::HostNameEdited(val.into())));
            });
        }
        {
            let q = actions.clone();
            mp.on_host_visibility_selected(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::HostVisibilitySelected(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_host_map_selected(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::HostMapSelected(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_host_side_selected(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::HostSideSelected(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_host_players_edited(move |val| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::HostPlayersEdited(val.into())));
            });
        }
        {
            let q = actions.clone();
            mp.on_host_connection_selected(move |idx| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::HostConnectionSelected(idx)));
            });
        }
        {
            let q = actions.clone();
            mp.on_host_submit(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::HostSubmit));
            });
        }
        {
            let q = actions.clone();
            mp.on_host_close(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::HostClose));
            });
        }

        // Direct connect callbacks
        {
            let q = actions.clone();
            mp.on_direct_address_edited(move |val| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::DirectAddressEdited(val.into())));
            });
        }
        {
            let q = actions.clone();
            mp.on_direct_port_edited(move |val| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::DirectPortEdited(val.into())));
            });
        }
        {
            let q = actions.clone();
            mp.on_direct_submit(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::DirectSubmit));
            });
        }
        {
            let q = actions.clone();
            mp.on_direct_cancel(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::DirectCancel));
            });
        }

        // Online privacy notice callbacks
        {
            let q = actions.clone();
            mp.on_notice_accept(move |version| {
                q.borrow_mut()
                    .push(UiAction::Mp(MpAction::NoticeAccept(version.into())));
            });
        }
        {
            let q = actions.clone();
            mp.on_notice_decline(move || {
                q.borrow_mut().push(UiAction::Mp(MpAction::NoticeDecline));
            });
        }
    }

    pub(super) fn mp_controls_active(&self) -> bool {
        if self.root.get_gameplay() || !self.root.get_shell_visible() {
            return false;
        }
        let mp = self.root.global::<MpModel>();
        mp.get_browser_active()
            || mp.get_host_active()
            || mp.get_direct_active()
            || mp.get_notice_active()
    }

    pub(super) fn mp_text_focused(&self) -> bool {
        self.root.global::<MpModel>().get_any_input_focused()
    }

    /// The shell is not drawn (a battle is loading, running or being hosted): none of the multiplayer
    /// screens or dialogs may stay on, and their state starts fresh the next time the shell shows.
    pub(super) fn hide_mp(&mut self) {
        let mp = self.root.global::<MpModel>();
        mp.set_browser_active(false);
        mp.set_modal_open(false);
        mp.set_host_active(false);
        mp.set_direct_active(false);
        mp.set_notice_active(false);
        mp.set_notice_busy(false);
        mp.set_notice_status_text("".into());
        mp.set_notice_status_error(false);
        mp.set_has_selected_server(false);
        mp.set_has_preview_image(false);
    }

    pub(super) fn sync_mp(&mut self, shell: &mut ShellHost) {
        let current = shell.app.current_screen();
        let top_non_popup = if current == ScreenId::Popup {
            shell.app.stack.last().copied().unwrap_or(ScreenId::Main)
        } else {
            current
        };
        let base_screen = if matches!(
            top_non_popup,
            ScreenId::DirectConnect | ScreenId::HostDialog
        ) {
            shell
                .app
                .stack
                .iter()
                .rev()
                .skip(1)
                .copied()
                .find(|id| *id != ScreenId::Popup)
                .unwrap_or(ScreenId::Multiplayer)
        } else {
            top_non_popup
        };

        let mp_active = matches!(
            base_screen,
            ScreenId::Multiplayer
                | ScreenId::MultiplayerList(_)
                | ScreenId::HostDialog
                | ScreenId::DirectConnect
        ) || matches!(
            current,
            ScreenId::Multiplayer
                | ScreenId::MultiplayerList(_)
                | ScreenId::HostDialog
                | ScreenId::DirectConnect
        );
        let host_active = top_non_popup == ScreenId::HostDialog;
        let direct_active = top_non_popup == ScreenId::DirectConnect;
        let (notice_active, notice_selected, notice_paragraphs) = match &shell.app.popup {
            Some(PopupState::OnlineNotice {
                message,
                selected_accept,
            }) => {
                let paras: Vec<SharedString> = message
                    .split("\n\n")
                    .skip(1)
                    .map(SharedString::from)
                    .collect();
                (true, if *selected_accept { 1 } else { 0 }, Some(paras))
            }
            _ => (false, 0, None),
        };
        let any_modal = host_active
            || direct_active
            || notice_active
            || shell.app.popup.is_some()
            || shell.app.profile.editing
            || shell.page.discord_offer
            || shell.steam_offer.open;

        let mp = self.root.global::<MpModel>();
        mp.set_browser_active(mp_active);
        mp.set_modal_open(any_modal);
        mp.set_host_active(host_active);
        mp.set_direct_active(direct_active);
        mp.set_notice_active(notice_active);
        mp.set_notice_selected(notice_selected);
        mp.set_notice_version(ONLINE_NOTICE_VERSION.into());
        // The team limit is the simulation's (`MAX_TEAM_SIZE`, src/battle-limits.js).
        mp.set_max_team_size(bfe_sim::battle::roster::MAX_TEAM_SIZE as i32);
        if !notice_active && (mp.get_notice_busy() || !mp.get_notice_status_text().is_empty()) {
            // A closed notice starts fresh the next time it opens.
            mp.set_notice_busy(false);
            mp.set_notice_status_text("".into());
            mp.set_notice_status_error(false);
        }
        if let Some(paras) = notice_paragraphs {
            sync_rows(&self.mp_models.notice_paragraphs, &paras);
        }

        if mp_active {
            shell.sync_page_browser();
            self.sync_mp_browser(shell);
        }
        if host_active {
            self.sync_mp_host(shell);
        }
        if direct_active {
            self.sync_mp_direct(shell);
        }
    }

    fn sync_mp_browser(&mut self, shell: &ShellHost) {
        let mp = self.root.global::<MpModel>();
        let browser = &shell.browser;
        let source = browser.source();
        mp.set_source_index(if source == Source::Lan { 1 } else { 0 });

        let filters = browser.filters();
        if !mp.get_search_focused() && mp.get_search_value().as_str() != filters.name {
            mp.set_search_value(filters.name.clone().into());
        }

        // Map filter options: ALL MAPS + extracted maps. The list follows the content index, so
        // its labels are built only when the map count changes; the selection is found without
        // building anything.
        let extracted = || {
            shell
                .app
                .data
                .maps
                .iter()
                .filter(|m| m.extracted != Some(false))
        };
        let map_idx = extracted()
            .position(|m| m.id == filters.map)
            .map_or(0, |at| at + 1);
        if self.mp_models.map_options_key != Some(shell.app.data.maps.len()) {
            let labels: Vec<SharedString> = std::iter::once(SharedString::from("ALL MAPS"))
                .chain(extracted().map(|m| SharedString::from(m.name.to_uppercase())))
                .collect();
            sync_rows(&self.mp_models.map_options, &labels);
            self.mp_models.map_options_key = Some(shell.app.data.maps.len());
        }
        mp.set_map_index(map_idx as i32);

        // Era filter options
        let era_idx = match filters.era.as_str() {
            "cw" => 1,
            "gcw" => 2,
            _ => 0,
        };
        mp.set_era_index(era_idx);

        // Ping filter options
        let ping_idx = PingFilter::ALL
            .iter()
            .position(|p| *p == filters.ping)
            .unwrap_or(0);
        mp.set_ping_index(ping_idx as i32);

        mp.set_hide_full(filters.hide_full);
        mp.set_hide_empty(filters.hide_empty);
        mp.set_is_filtered(filters.is_filtered());

        // Sort specification
        let sort = browser.sort_spec();
        let sort_col = SortColumn::ALL
            .iter()
            .position(|c| *c == sort.column)
            .unwrap_or(5) as i32;
        mp.set_sort_column(sort_col);
        mp.set_sort_dir(sort.dir as i32);

        // Browser state & error
        let (state_str, err_str) = match browser.state() {
            BrowserState::Loading => ("loading", String::new()),
            BrowserState::Notice => ("notice", String::new()),
            BrowserState::Error(msg) => ("error", msg.clone()),
            BrowserState::Idle | BrowserState::Ready => ("ready", String::new()),
        };
        mp.set_browser_state(state_str.into());
        mp.set_browser_error(err_str.into());

        // Rows
        let contract = ServerContract::current();
        let rows = browser.rows();
        let total_servers = browser.inner.servers().len();
        let selected_id = browser.selected().map(|s| s.id.as_str());
        let mut selected_idx: i32 = -1;
        let mut row_items = Vec::with_capacity(rows.len());
        let mut total_players: u32 = 0;

        for (idx, s) in rows.iter().enumerate() {
            total_players += s.players;
            if selected_id == Some(s.id.as_str()) {
                selected_idx = idx as i32;
            }
            let b = s.battle.as_ref();
            let map_name_str = b
                .map(|v| {
                    if !v.map_name.is_empty() {
                        v.map_name.clone()
                    } else if let Some(m) = shell.app.data.maps.iter().find(|m| m.id == v.map) {
                        m.name.clone()
                    } else if !v.map.is_empty() {
                        v.map.clone()
                    } else {
                        "-".into()
                    }
                })
                .unwrap_or_else(|| "-".into());
            let mode_str = b
                .map(|v| {
                    if v.mode.is_empty() {
                        "-".into()
                    } else {
                        mode_label(&v.mode)
                    }
                })
                .unwrap_or_else(|| "-".into());
            let era_str = b
                .map(|v| {
                    let lbl = era_label(&v.era);
                    if lbl.is_empty() {
                        "-".into()
                    } else {
                        lbl
                    }
                })
                .unwrap_or_else(|| "-".into());
            let (ping_q, _) = ping_quality(s.latency_ms);
            let ping_text = s
                .latency_ms
                .map(|ms| ms.to_string())
                .unwrap_or_else(|| "-".to_string());
            let fill = if s.max_players > 0 {
                (s.players as f32 / s.max_players as f32).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let is_compat = compatible(s, &contract);

            row_items.push(BrowserRowItem {
                id: s.id.clone().into(),
                name: (if s.name.is_empty() {
                    "UNNAMED SERVER".to_string()
                } else {
                    s.name.clone()
                })
                .into(),
                players_text: format!("{}/{}", s.players, s.max_players).into(),
                fill_ratio: fill,
                map_name: map_name_str.into(),
                mode_label: mode_str.into(),
                era_label: era_str.into(),
                ping_text: ping_text.into(),
                ping_bars: ping_q.bars() as i32,
                location: (if s.location.is_empty() {
                    "-".to_string()
                } else {
                    s.location.clone()
                })
                .into(),
                country: country_of(s).into(),
                compatible: is_compat,
            });
        }

        sync_rows(&self.mp_models.server_rows, &row_items);
        mp.set_selected_row_index(selected_idx);

        // Status line (`count` + `updated`) matching `server-browser-view.js`
        let count_label = if matches!(browser.state(), BrowserState::Ready | BrowserState::Idle) {
            let filtered_out = total_servers.saturating_sub(rows.len());
            let filtered_suffix = if filtered_out > 0 {
                format!("  \u{00b7}  {filtered_out} FILTERED OUT")
            } else {
                String::new()
            };
            format!(
                "{} {}  \u{00b7}  {} {}{}",
                rows.len(),
                if rows.len() == 1 { "SERVER" } else { "SERVERS" },
                total_players,
                if total_players == 1 {
                    "PLAYER"
                } else {
                    "PLAYERS"
                },
                filtered_suffix
            )
        } else {
            String::new()
        };
        mp.set_count_label(count_label.into());
        let updated_label = match browser.state() {
            BrowserState::Loading => "SEARCHING\u{2026}".to_string(),
            BrowserState::Ready | BrowserState::Idle => {
                if !browser.inner.stale().is_empty() {
                    "CAN'T UPDATE, RETRYING\u{2026}".to_string()
                } else {
                    "UPDATED JUST NOW".to_string()
                }
            }
            _ => String::new(),
        };
        mp.set_updated_label(updated_label.into());

        // Selected server `GAME INFO` panel + map preview texture
        if let Some(selected) = browser.selected() {
            let catalog: Vec<MapCatalogEntry> = shell
                .app
                .data
                .maps
                .iter()
                .map(|m| MapCatalogEntry::new(m.id.clone(), m.name.clone()))
                .collect();
            let info = browser.describe(selected, &catalog);
            mp.set_has_selected_server(true);
            mp.set_preview_caption(info.preview_caption.into());
            mp.set_info_name(
                (if selected.name.is_empty() {
                    "UNNAMED SERVER".to_string()
                } else {
                    selected.name.clone()
                })
                .into(),
            );
            mp.set_info_description(selected.description.clone().into());

            let chips: Vec<SharedString> = info
                .chips
                .iter()
                .map(|c| SharedString::from(c.as_str()))
                .collect();
            sync_rows(&self.mp_models.info_chips, &chips);

            let facts: Vec<FactItem> = info
                .facts
                .iter()
                .map(|(k, v)| FactItem {
                    label: (*k).into(),
                    value: v.clone().into(),
                })
                .collect();
            sync_rows(&self.mp_models.info_facts, &facts);
            mp.set_info_country(info.country.into());

            if let Some(sponsor) = &info.sponsor {
                mp.set_info_has_sponsor(true);
                mp.set_info_sponsor_title(sponsor.name.clone().into());
                mp.set_info_sponsor_display_link(
                    sponsor.display_link.clone().unwrap_or_default().into(),
                );
                mp.set_info_sponsor_url(sponsor.url.clone().unwrap_or_default().into());
            } else {
                mp.set_info_has_sponsor(false);
                mp.set_info_sponsor_title("".into());
                mp.set_info_sponsor_display_link("".into());
                mp.set_info_sponsor_url("".into());
            }

            // Live map preview texture from `shell.preview_texture`
            let has_frame = shell
                .preview_movies
                .layers
                .iter()
                .flatten()
                .any(|l| l.frame.is_some());
            let [tw, th] = shell.preview_texture.size();
            if has_frame && tw > 0 && th > 0 {
                // SAFETY: `shell.preview_texture.texture.0` is a valid `GL_TEXTURE_2D` RGBA8
                // texture owned by the same OpenGL context and kept alive by `ShellHost`.
                let img = unsafe {
                    slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                        shell.preview_texture.texture.0,
                        (tw as u32, th as u32).into(),
                    )
                    .build()
                };
                mp.set_preview_image(img);
                mp.set_has_preview_image(true);
            } else {
                mp.set_has_preview_image(false);
            }

            mp.set_join_enabled(info.compatible);
        } else {
            mp.set_has_selected_server(false);
            mp.set_has_preview_image(false);
            mp.set_join_enabled(false);
        }

        mp.set_footer_focus(shell.page.browser.focus as i32);
    }

    fn sync_mp_host(&mut self, shell: &ShellHost) {
        let mp = self.root.global::<MpModel>();
        let app = &shell.app;

        if !mp.get_host_name_focused() && mp.get_host_name().as_str() != app.host_config.server_name
        {
            mp.set_host_name(app.host_config.server_name.clone().into());
        }

        let vis_choices = app.host_choices(1);
        let vis_idx = vis_choices
            .iter()
            .position(|(v, _)| v == app.host_value(1))
            .unwrap_or(0);
        let vis_options = choice_labels(&vis_choices);
        sync_rows(&self.mp_models.host_visibility, &vis_options);
        mp.set_host_visibility_index(vis_idx as i32);

        let map_choices = app.host_choices(2);
        let map_idx = map_choices
            .iter()
            .position(|(v, _)| v == app.host_value(2))
            .unwrap_or(0);
        let map_options = choice_labels(&map_choices);
        sync_rows(&self.mp_models.host_map, &map_options);
        mp.set_host_map_index(map_idx as i32);

        let side_choices = app.host_choices(3);
        let side_idx = side_choices
            .iter()
            .position(|(v, _)| v == app.host_value(3))
            .unwrap_or(0);
        let side_options = choice_labels(&side_choices);
        sync_rows(&self.mp_models.host_side, &side_options);
        mp.set_host_side_index(side_idx as i32);

        if !mp.get_host_players_focused() && mp.get_host_players().as_str() != app.host_form.players
        {
            mp.set_host_players(app.host_form.players.clone().into());
        }

        let conn_choices = app.host_choices(5);
        let conn_idx = conn_choices
            .iter()
            .position(|(v, _)| v == app.host_value(5))
            .unwrap_or(0);
        let conn_options = choice_labels(&conn_choices);
        sync_rows(&self.mp_models.host_connection, &conn_options);
        mp.set_host_connection_index(conn_idx as i32);

        let (status_text, status_err) = match &app.host_form.error {
            Some(err) => (err.as_str(), true),
            None => (
                "Hosting uses this computer and your imported game. It keeps running when minimized.",
                false,
            ),
        };
        mp.set_host_status_text(status_text.into());
        mp.set_host_status_error(status_err);
        mp.set_host_busy(false);
        mp.set_host_focus(app.host_form.focus as i32);
    }

    fn sync_mp_direct(&mut self, shell: &ShellHost) {
        let mp = self.root.global::<MpModel>();
        if !mp.get_direct_addr_focused()
            && mp.get_direct_address().as_str() != shell.app.direct_connect_draft
        {
            mp.set_direct_address(shell.app.direct_connect_draft.clone().into());
        }
        if !mp.get_direct_port_focused()
            && mp.get_direct_port().as_str() != shell.app.direct_port_draft
        {
            mp.set_direct_port(shell.app.direct_port_draft.clone().into());
        }
        mp.set_direct_status_text("".into());
        mp.set_direct_status_error(false);
        mp.set_direct_focus(shell.app.direct_field as i32);
    }

    pub(super) fn drain_mp_action(
        &mut self,
        action: MpAction,
        sh: &mut ShellHost,
        settings: &mut AppSettingsStore,
        audio_router: &SharedAudioRouter,
    ) -> Option<ShellTransition> {
        let was_notice = sh.app.mp_needs_notice;
        let prev_screen = sh.app.current_screen();

        match action {
            MpAction::TabCreate => {
                sh.app.request_host();
            }
            MpAction::SourceSelected(idx) => {
                let source = if idx == 1 {
                    Source::Lan
                } else {
                    Source::Internet
                };
                sh.browser.set_source(source);
                sh.sync_page_browser();
            }
            MpAction::SearchEdited(val) | MpAction::SearchSubmitted(val) => {
                sh.browser.set_filter(FilterUpdate::Name(val));
                sh.sync_page_browser();
            }
            MpAction::MapSelected(idx) => {
                let map_id = if idx <= 0 {
                    "all".to_string()
                } else {
                    sh.app
                        .data
                        .maps
                        .iter()
                        .filter(|m| m.extracted != Some(false))
                        .nth((idx - 1) as usize)
                        .map(|m| m.id.clone())
                        .unwrap_or_else(|| "all".into())
                };
                sh.browser.set_filter(FilterUpdate::Map(map_id));
                sh.sync_page_browser();
            }
            MpAction::EraSelected(idx) => {
                let era = match idx {
                    1 => "cw",
                    2 => "gcw",
                    _ => "all",
                };
                sh.browser.set_filter(FilterUpdate::Era(era.into()));
                sh.sync_page_browser();
            }
            MpAction::PingSelected(idx) => {
                let ping = PingFilter::ALL
                    .get(idx.max(0) as usize)
                    .copied()
                    .unwrap_or(PingFilter::Any);
                sh.browser.set_filter(FilterUpdate::Ping(ping));
                sh.sync_page_browser();
            }
            MpAction::ToggleHideFull => {
                let next = !sh.browser.filters().hide_full;
                sh.browser.set_filter(FilterUpdate::HideFull(next));
                sh.sync_page_browser();
            }
            MpAction::ToggleHideEmpty => {
                let next = !sh.browser.filters().hide_empty;
                sh.browser.set_filter(FilterUpdate::HideEmpty(next));
                sh.sync_page_browser();
            }
            MpAction::SortBy(col) => {
                if let Some(&column) = SortColumn::ALL.get(col.max(0) as usize) {
                    sh.browser.sort_by(column);
                    sh.sync_page_browser();
                }
            }
            MpAction::RowSelected(idx) => {
                let id = sh
                    .browser
                    .rows()
                    .get(idx.max(0) as usize)
                    .map(|s| s.id.clone());
                if let Some(id) = id {
                    sh.browser.select(Some(&id));
                }
                sh.page.browser.focus = 0;
                sh.sync_page_browser();
            }
            MpAction::RowDoubleClicked(idx) => {
                let server = sh
                    .browser
                    .rows()
                    .get(idx.max(0) as usize)
                    .map(|s| (*s).clone());
                if let Some(server) = server {
                    sh.browser.select(Some(&server.id));
                    sh.page.browser.focus = 0;
                    sh.sync_page_browser();
                    if compatible(&server, &ServerContract::current()) {
                        let kind = if sh.browser.source() == Source::Lan {
                            MultiplayerKind::Lan
                        } else {
                            MultiplayerKind::Online
                        };
                        sh.app.join_browser_server(server, kind, &mut sh.browser);
                    }
                }
            }
            MpAction::ReviewNotice => {
                sh.app.popup = Some(bfe_shell::screens::ShellApp::online_notice());
            }
            MpAction::Refresh => {
                sh.browser_footer_action(1);
            }
            MpAction::DirectConnect => {
                sh.browser_footer_action(2);
            }
            MpAction::JoinSelected => {
                sh.browser_footer_action(3);
            }
            MpAction::Back => {
                sh.browser_footer_action(0);
            }
            MpAction::OpenSponsorUrl(_url) => {}
            MpAction::HostNameEdited(val) => {
                sh.app.host_config.server_name = val;
                sh.app.host_form.error = None;
            }
            MpAction::HostVisibilitySelected(idx) => {
                sh.app.host_form.choice = Some(1);
                sh.app.host_control(HostControl::Choice(idx.max(0) as usize));
            }
            MpAction::HostMapSelected(idx) => {
                sh.app.host_form.choice = Some(2);
                sh.app.host_control(HostControl::Choice(idx.max(0) as usize));
            }
            MpAction::HostSideSelected(idx) => {
                sh.app.host_form.choice = Some(3);
                sh.app.host_control(HostControl::Choice(idx.max(0) as usize));
            }
            MpAction::HostPlayersEdited(val) => {
                sh.app.host_form.players = val;
                sh.app.host_form.error = None;
            }
            MpAction::HostConnectionSelected(idx) => {
                sh.app.host_form.choice = Some(5);
                sh.app.host_control(HostControl::Choice(idx.max(0) as usize));
            }
            MpAction::HostSubmit => {
                sh.app.host_control(HostControl::Start);
            }
            MpAction::HostClose => {
                sh.app.host_control(HostControl::Cancel);
            }
            MpAction::DirectAddressEdited(val) => {
                sh.app.direct_connect_draft = val;
                sh.app.direct_field = 0;
            }
            MpAction::DirectPortEdited(val) => {
                let digits: String = val
                    .chars()
                    .filter(|c| c.is_ascii_digit())
                    .take(5)
                    .collect();
                sh.app.direct_port_draft = digits;
                sh.app.direct_field = 1;
            }
            MpAction::DirectSubmit => {
                sh.app.accept(&mut sh.browser);
            }
            MpAction::DirectCancel => {
                sh.app.back();
            }
            MpAction::NoticeAccept(version) => {
                // The browser refuses a notice whose version differs from the one
                // it was opened for (`onlineNotice` in src/shell/online-notice.js).
                let mp = self.root.global::<MpModel>();
                if mp.get_notice_busy() {
                    return None;
                }
                if version.as_str() != ONLINE_NOTICE_VERSION {
                    mp.set_notice_status_text(
                        "Your choice could not be saved. The notice has changed. Close this dialog and open it again."
                            .into(),
                    );
                    mp.set_notice_status_error(true);
                    return None;
                }
                // Draw one frame of `Saving your choice…` with both buttons
                // disabled before the write, as the browser does while it awaits.
                mp.set_notice_status_text("Saving your choice\u{2026}".into());
                mp.set_notice_status_error(false);
                mp.set_notice_busy(true);
                self.actions
                    .borrow_mut()
                    .push(UiAction::Mp(MpAction::NoticeCommit));
                return None;
            }
            MpAction::NoticeCommit => {
                let mp = self.root.global::<MpModel>();
                let saved = default_consent_path()
                    .ok_or_else(|| "No application data folder is available.".to_string())
                    .and_then(|path| write_online_consent(&path));
                mp.set_notice_busy(false);
                match saved {
                    Ok(_) => {
                        mp.set_notice_status_text("".into());
                        if let Some(PopupState::OnlineNotice {
                            selected_accept, ..
                        }) = &mut sh.app.popup
                        {
                            *selected_accept = true;
                        }
                        sh.app.accept(&mut sh.browser);
                    }
                    Err(cause) => {
                        mp.set_notice_status_text(
                            format!("Your choice could not be saved. {cause}").into(),
                        );
                        mp.set_notice_status_error(true);
                        return None;
                    }
                }
            }
            MpAction::NoticeDecline => {
                if let Some(PopupState::OnlineNotice {
                    selected_accept, ..
                }) = &mut sh.app.popup
                {
                    *selected_accept = false;
                }
                sh.app.accept(&mut sh.browser);
            }
        }

        sh.browser.privacy_accepted = !sh.app.mp_needs_notice;
        let entered_list = matches!(
            sh.app.current_screen(),
            ScreenId::MultiplayerList(MultiplayerKind::Online) | ScreenId::Multiplayer
        ) && prev_screen != sh.app.current_screen();
        if (was_notice && !sh.app.mp_needs_notice)
            || (entered_list && !sh.browser.is_refresh_in_flight())
        {
            sh.browser.trigger_refresh();
            sh.sync_page_browser();
        }
        sh.flush_events(settings, audio_router)
    }
}
