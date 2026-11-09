//! Desktop settings view over the shared native options controller.
use super::{settings::AppSettingsStore, shell::ShellHost};
use bfe_desktop::{
    discover::steam_roots,
    presence::discord::{candidate_ipc_paths, present_sockets},
};
use bfe_interface::{
    options::OptionKind,
    screen::MenuScreen,
    widgets::{
        ButtonWidget, OptionsItem, OptionsLayoutKind, OptionsWidget, TextWidget, Widget,
        WidgetAction,
    },
};
impl ShellHost {
    pub(super) fn open_integrations(&mut self, settings: &AppSettingsStore) {
        let env = std::env::vars().collect();
        let platform = if cfg!(target_os = "windows") {
            "win32"
        } else if cfg!(target_os = "macos") {
            "darwin"
        } else {
            "linux"
        };
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(std::path::PathBuf::from);
        let steam = steam_roots(platform, home.as_deref(), &env)
            .iter()
            .any(|p| p.exists());
        let discord = !present_sockets(&candidate_ipc_paths(platform, &env, None)).is_empty();
        let detail = settings
            .desktop
            .presence
            .as_ref()
            .and_then(|p| p.discord.as_deref())
            .unwrap_or(if settings.desktop.discord_presence {
                "full"
            } else {
                "off"
            });
        let item = |id: &str, label: &str, value: &str, kind, disabled, action| OptionsItem {
            id: id.into(),
            label: label.into(),
            value_text: value.into(),
            kind,
            ratio: 0.,
            min: 0.,
            max: 1.,
            step: 1.,
            disabled,
            dim: false,
            hidden: false,
            note: String::new(),
            action,
        };
        let activity = match detail {
            "full" => "MAP AND SERVER",
            "game" => "GAME ONLY",
            _ => "OFF",
        };
        let items = vec![
            item(
                "discord-activity",
                "DISCORD ACTIVITY",
                activity,
                OptionKind::Action,
                false,
                Some(WidgetAction::Named("discord-cycle".into())),
            ),
            item(
                "discord-status",
                "DISCORD",
                if detail == "off" {
                    "OFF"
                } else if discord {
                    "AVAILABLE"
                } else {
                    "DISCORD NOT RUNNING"
                },
                OptionKind::Fixed,
                true,
                None,
            ),
            // The Electron shortcut launcher is not a native executable target.
            // Keep that service unavailable until the desktop host supplies one.
            item(
                "steam",
                "STEAM LIBRARY",
                if steam {
                    "NOT IN THIS BUILD"
                } else {
                    "STEAM NOT FOUND"
                },
                OptionKind::Fixed,
                true,
                None,
            ),
        ];
        let widgets = vec![
            Widget::Text(TextWidget::title("title", "STEAM & DISCORD", 10.)),
            Widget::Options(OptionsWidget::with_layout(
                "rows",
                OptionsLayoutKind::Game,
                items,
                self.interface_surface.height,
            )),
            Widget::Button(ButtonWidget::new("back", "back", "BACK", None)),
        ];
        if self
            .options_ctrl
            .stack
            .top()
            .is_some_and(|s| s.id == "options-integrations")
        {
            self.options_ctrl.stack.pop();
        }
        self.options_ctrl
            .stack
            .push(MenuScreen::new("options-integrations", widgets));
    }
    pub(super) fn step_discord_detail(&mut self, settings: &mut AppSettingsStore, back: bool) {
        let record = settings
            .desktop
            .presence
            .get_or_insert_with(Default::default);
        let i = match record.discord.as_deref() {
            Some("full") => 0,
            Some("game") => 1,
            _ => 2,
        };
        let next = ["full", "game", "off"][(i + if back { 2 } else { 1 }) % 3];
        record.discord = Some(next.into());
        settings.desktop.discord_presence = next != "off";
        if let Err(error) = settings.save_desktop() {
            eprintln!("cannot save presence preference: {error}");
        }
        self.open_integrations(settings);
    }
}
