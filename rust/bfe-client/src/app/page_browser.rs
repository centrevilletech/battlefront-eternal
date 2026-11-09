//! Page browser interactions dispatch to the shell's existing service and actions.
use super::shell::ShellHost;
use bfe_online::browser::{
    BrowserKey, FilterUpdate, PingFilter, ServerBrowserService, SortColumn, Source,
};
use bfe_shell::{
    page::Hit,
    screens::{MultiplayerKind, ScreenId},
};
impl ShellHost {
    /// Publish the shared text editor's search draft to the browser owner.
    pub(super) fn refresh_page_search(&mut self) {
        let name = self.page.browser.filters.name.clone();
        self.browser.set_filter(FilterUpdate::Name(name));
        self.sync_page_browser();
    }
    pub(super) fn sync_page_browser(&mut self) {
        self.app.sync_from_browser(&self.browser);
        self.page.browser.sync(&self.browser);
    }
    pub(super) fn browser_page(&self) -> bool {
        self.app.popup.is_none()
            && !self.app.profile.editing
            && matches!(
                self.app.current_screen(),
                ScreenId::Multiplayer | ScreenId::MultiplayerList(_)
            )
    }
    fn browser_choice(&mut self, field: usize, index: usize) {
        let Some((value, _)) = self.page.browser.choices.get(index) else {
            return;
        };
        match field {
            0 => {
                self.browser.set_source(if value == "lan" {
                    Source::Lan
                } else {
                    Source::Internet
                });
            }
            2 => {
                self.browser.set_filter(FilterUpdate::Map(value.clone()));
            }
            3 => {
                self.browser.set_filter(FilterUpdate::Era(value.clone()));
            }
            4 => {
                self.browser.set_filter(FilterUpdate::Ping(
                    PingFilter::from_id(value).unwrap_or_default(),
                ));
            }
            _ => {}
        }
        self.page.browser.choice = None;
        self.sync_page_browser();
    }
    pub(super) fn page_browser_hit(&mut self, hit: Hit) -> bool {
        match hit {
            Hit::BrowserTab(1) => {
                self.app.set_selected_row_index(ScreenId::Multiplayer, 0);
                self.app.accept(&mut self.browser);
            }
            Hit::BrowserTab(_) => {}
            Hit::BrowserFilter(field) => {
                self.page.browser.search = field == 1;
                self.page.browser.choice = None;
                if field == 5 || field == 6 {
                    let f = self.browser.filters();
                    let update = if field == 5 {
                        FilterUpdate::HideFull(!f.hide_full)
                    } else {
                        FilterUpdate::HideEmpty(!f.hide_empty)
                    };
                    self.browser.set_filter(update);
                    self.sync_page_browser();
                } else if field != 1 {
                    self.page.browser.choices = match field {
                        0 => vec![
                            ("internet".into(), "INTERNET".into()),
                            ("lan".into(), "LAN".into()),
                        ],
                        2 => std::iter::once(("all".into(), "ALL MAPS".into()))
                            .chain(
                                self.app
                                    .data
                                    .maps
                                    .iter()
                                    .map(|m| (m.id.clone(), m.name.clone())),
                            )
                            .collect(),
                        3 => vec![
                            ("all".into(), "ALL ERAS".into()),
                            ("cw".into(), "CLONE WARS".into()),
                            ("gcw".into(), "GALACTIC CIVIL WAR".into()),
                        ],
                        4 => PingFilter::ALL
                            .iter()
                            .map(|p| (p.id().into(), p.label().into()))
                            .collect(),
                        _ => Vec::new(),
                    };
                    self.page.browser.choice = Some(field);
                    self.page.browser.choice_cursor = 0;
                }
            }
            Hit::BrowserChoice(field, index) => self.browser_choice(field, index),
            Hit::BrowserSort(index) => {
                self.browser.sort_by(SortColumn::ALL[index]);
                self.sync_page_browser();
            }
            Hit::BrowserServer(index) => {
                if let Some(server) = self.app.mp_servers.get(index) {
                    self.browser.select(Some(&server.id));
                }
                self.page.browser.focus = 0;
                self.sync_page_browser();
            }
            Hit::BrowserFooter(index) => match index {
                0 => self.app.back(),
                1 => {
                    self.browser.trigger_refresh();
                    self.sync_page_browser();
                }
                2 => self.app.push_screen(ScreenId::DirectConnect),
                3 => {
                    if let Some(server) = self.browser.selected().cloned() {
                        let kind = if self.browser.source() == Source::Lan {
                            MultiplayerKind::Lan
                        } else {
                            MultiplayerKind::Online
                        };
                        self.app
                            .join_browser_server(server, kind, &mut self.browser);
                    }
                }
                _ => {}
            },
            Hit::BrowserNotice => {
                self.app.popup=Some(bfe_shell::screens::PopupState::OnlineNotice{
                    message:"Before you play online\n\nOnline services are operated by the directory operator.\n\nThe service sees your IP address and public player identity. Reports are retained for 30 days. TURN relay traffic is forwarded.\n\nAccept to contact https://online.battlefronteternal.com. Your choice is saved on this computer.".into(),selected_accept:false});
            }
            _ => return false,
        }
        true
    }
    pub(super) fn page_browser_key(&mut self, key: &str) -> bool {
        if let Some(field) = self.page.browser.choice {
            let view = &mut self.page.browser;
            let last = view.choices.len().saturating_sub(1);
            match key {
                "ArrowUp" => view.choice_cursor = view.choice_cursor.saturating_sub(1),
                "ArrowDown" => view.choice_cursor = (view.choice_cursor + 1).min(last),
                "Home" => view.choice_cursor = 0,
                "End" => view.choice_cursor = last,
                "Enter" | " " => {
                    let index = view.choice_cursor;
                    self.browser_choice(field, index);
                }
                "Escape" | "Tab" => view.choice = None,
                _ => {}
            }
            return true;
        }
        if self.page.browser.search {
            let mut value = self.browser.filters().name.clone();
            match key {
                "Escape" | "Enter" | "Tab" => self.page.browser.search = false,
                "Backspace" => {
                    value.pop();
                }
                k if k.chars().count() == 1 => value.push_str(k),
                _ => {}
            }
            self.browser.set_filter(FilterUpdate::Name(value));
            self.sync_page_browser();
            return true;
        }
        if key == "Tab" {
            self.page.browser.focus = (self.page.browser.focus + 1) % 5;
            return true;
        }
        if key == "Enter" || key == " " {
            let focus = self.page.browser.focus;
            return self.page_browser_hit(Hit::BrowserFooter(if focus == 0 {
                3
            } else {
                focus - 1
            }));
        }
        let navigation = match key {
            "ArrowUp" => Some(BrowserKey::ArrowUp),
            "ArrowDown" => Some(BrowserKey::ArrowDown),
            "Home" => Some(BrowserKey::Home),
            "End" => Some(BrowserKey::End),
            "PageUp" => Some(BrowserKey::PageUp),
            "PageDown" => Some(BrowserKey::PageDown),
            _ => None,
        };
        if let Some(key) = navigation {
            self.browser.key(key, 7);
            self.page.browser.focus = 0;
            self.sync_page_browser();
            return true;
        }
        false
    }
}
