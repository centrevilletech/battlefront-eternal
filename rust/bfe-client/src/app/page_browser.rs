//! Page browser interactions dispatch to the shell's existing service and actions.
use super::shell::ShellHost;
use bfe_online::browser::{BrowserKey, ServerBrowserService, Source};
use bfe_shell::screens::{MultiplayerKind, ScreenId};
impl ShellHost {
    pub(super) fn sync_page_browser(&mut self) {
        self.app.sync_from_browser(&self.browser);
        let previous = self
            .page
            .browser
            .info
            .as_ref()
            .map(|info| info.map_id.clone());
        self.page.browser.sync(&self.browser);
        let next = self
            .page
            .browser
            .info
            .as_ref()
            .map(|info| info.map_id.clone());
        if previous != next {
            let clip = next
                .as_ref()
                .and_then(|id| self.app.data.maps.iter().find(|map| &map.id == id))
                .and_then(|map| map.fly.clone());
            self.preview_movies.play(
                self.content_dir
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new(".")),
                bfe_shell::data::ScreenMovie {
                    intro: None,
                    loop_clip: clip,
                },
                false,
                bfe_shell::media::PREVIEW_DELAY,
            );
        }
    }
    pub(super) fn browser_page(&self) -> bool {
        self.app.popup.is_none()
            && !self.app.profile.editing
            && matches!(
                self.app.current_screen(),
                ScreenId::Multiplayer | ScreenId::MultiplayerList(_)
            )
    }
    pub(super) fn browser_footer_action(&mut self, index: usize) -> bool {
        match index {
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
            _ => return false,
        }
        true
    }
    pub(super) fn page_browser_key(&mut self, key: &str) -> bool {
        if key == "Tab" {
            self.page.browser.focus = (self.page.browser.focus + 1) % 5;
            return true;
        }
        if key == "Enter" || key == " " {
            let focus = self.page.browser.focus;
            return self.browser_footer_action(if focus == 0 { 3 } else { focus - 1 });
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
