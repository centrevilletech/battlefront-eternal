//! Player settings persistence and single swap point for `bfe-interface`.
//!
//! While lane `ui` (`bfe-interface`) is in flight, this module bridges
//! [`bfe_shell::screens::ShellSettings`] (front-end `OPTIONS` pages) and the
//! key-value map consumed by [`bfe_battle_ui::PauseMenu`] (in-battle `OPTIONS`
//! and `CONTROLS` views), persisting them alongside the active player profile
//! and Instant Action playlist in `DesktopPaths::player_settings_file`.
//!
//! When `bfe-interface` lands, only [`AppSettingsStore`] in this file needs
//! to swap its backing schema to `bfe_interface::Settings`.

use bfe_desktop::{
    paths::{resolve_desktop_paths, DesktopPaths, DesktopPathsOptions},
    settings::{DesktopSettings, SettingsPatch, SettingsStore},
};
use bfe_shell::{
    launch::PlaylistEntry,
    screens::{ShellApp, ShellSettings},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Returns the platform string matching `bfe-desktop` conventions (`"linux"`, `"darwin"`, `"win32"`).
pub fn current_platform_str() -> &'static str {
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

/// Returns the CPU architecture string matching `bfe-desktop` conventions (`"x64"`, `"arm64"`).
pub fn current_arch_str() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Resolves the repository or packaged root directory relative to the running binary
/// or current working directory.
pub fn resolve_project_root() -> PathBuf {
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("tools").join("content-version.json").exists() {
        return cwd;
    }
    if let Some(parent) = cwd.parent() {
        if parent.join("tools").join("content-version.json").exists() {
            return parent.to_path_buf();
        }
    }
    cwd
}

/// Resolves [`DesktopPaths`] from the current environment.
pub fn current_desktop_paths() -> DesktopPaths {
    let env_map: BTreeMap<String, String> = env::vars().collect();
    let home = env_map
        .get("HOME")
        .or_else(|| env_map.get("USERPROFILE"))
        .map(PathBuf::from);
    let cwd = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    resolve_desktop_paths(&DesktopPathsOptions {
        platform: current_platform_str().to_string(),
        home,
        cwd,
        project_root: resolve_project_root(),
        exec_dir: env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf())),
        resources_path: None,
        packaged: false,
        env: env_map,
    })
}

/// Persisted player settings file (`player-settings.json` in `DesktopPaths::user_data`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedPlayerSettings {
    pub profile_name: String,
    pub privacy_notice_accepted: bool,
    pub shell: ShellSettings,
    #[serde(default)]
    pub pause_kv: BTreeMap<String, String>,
    #[serde(default)]
    pub ia_playlist: Vec<PlaylistEntry>,
    #[serde(default)]
    pub ia_random: bool,
}

impl Default for PersistedPlayerSettings {
    fn default() -> Self {
        let shell = ShellSettings::default();
        let pause_kv = Self::shell_to_pause_kv(&shell);
        Self {
            profile_name: "Player 1".to_string(),
            privacy_notice_accepted: false,
            shell,
            pause_kv,
            ia_playlist: vec![
                PlaylistEntry::new("nab2", Some("cw".to_string())),
                PlaylistEntry::new("tat1", Some("cw".to_string())),
                PlaylistEntry::new("bes1", Some("cw".to_string())),
            ],
            ia_random: false,
        }
    }
}

impl PersistedPlayerSettings {
    /// Synchronizes `ShellSettings` fields into the string map expected by
    /// `bfe_battle_ui::PauseMenu`.
    pub fn shell_to_pause_kv(shell: &ShellSettings) -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        map.insert(
            "masterVolume".to_string(),
            format!("{}", (shell.master_volume * 100.0).round() as i32),
        );
        map.insert(
            "musicVolume".to_string(),
            format!("{}", (shell.music_volume * 100.0).round() as i32),
        );
        map.insert(
            "effectsVolume".to_string(),
            format!("{}", (shell.effects_volume * 100.0).round() as i32),
        );
        map.insert(
            "interfaceVolume".to_string(),
            format!("{}", (shell.interface_volume * 100.0).round() as i32),
        );
        map.insert("menuTheme".to_string(), shell.menu_theme.clone());
        map.insert("battleSize".to_string(), shell.battle_size.clone());
        map.insert("aiDifficulty".to_string(), shell.difficulty.clone());
        map.insert(
            "mouseSensitivity".to_string(),
            format!("{:.2}", shell.mouse_sensitivity),
        );
        map.insert(
            "invertY".to_string(),
            if shell.invert_y { "true" } else { "false" }.to_string(),
        );
        map
    }

    /// Re-synchronizes `pause_kv` from `self.shell`.
    pub fn sync_from_shell(&mut self) {
        let fresh = Self::shell_to_pause_kv(&self.shell);
        for (k, v) in fresh {
            self.pause_kv.insert(k, v);
        }
    }
}

/// Unified settings store owning both [`DesktopSettings`] (`desktop-settings.json`)
/// and [`PersistedPlayerSettings`] (`player-settings.json`).
#[derive(Clone, Debug)]
pub struct AppSettingsStore {
    pub paths: DesktopPaths,
    pub desktop_store: SettingsStore,
    pub desktop: DesktopSettings,
    pub player: PersistedPlayerSettings,
}

impl AppSettingsStore {
    /// Loads both `DesktopSettings` and `PersistedPlayerSettings` from `paths`
    /// (falling back to defaults when missing or corrupt).
    pub fn load(paths: DesktopPaths) -> Self {
        let desktop_store = SettingsStore::new(&paths.settings_file);
        let desktop = desktop_store.load();
        let player = Self::load_player_file(&paths.player_settings_file);
        Self {
            paths,
            desktop_store,
            desktop,
            player,
        }
    }

    fn load_player_file(path: &Path) -> PersistedPlayerSettings {
        let Ok(bytes) = fs::read(path) else {
            return PersistedPlayerSettings::default();
        };
        let Ok(mut parsed) = serde_json::from_slice::<PersistedPlayerSettings>(&bytes) else {
            return PersistedPlayerSettings::default();
        };
        if parsed.pause_kv.is_empty() {
            parsed.sync_from_shell();
        }
        parsed
    }

    /// Applies persisted player settings into a newly constructed [`ShellApp`].
    pub fn apply_to_shell_app(&self, app: &mut ShellApp) {
        app.profile.name = self.player.profile_name.clone();
        app.mp_needs_notice = !self.player.privacy_notice_accepted;
        app.settings = self.player.shell.clone();
        if !self.player.ia_playlist.is_empty() {
            app.ia_entries = self.player.ia_playlist.clone();
        }
        app.ia_random = self.player.ia_random;
    }

    /// Captures changes made in [`ShellApp`] and persists them to disk if dirty.
    pub fn capture_from_shell_app(&mut self, app: &ShellApp) -> Result<bool, String> {
        let notice_accepted = !app.mp_needs_notice;
        let changed = self.player.profile_name != app.profile.name
            || self.player.privacy_notice_accepted != notice_accepted
            || self.player.shell != app.settings
            || self.player.ia_playlist != app.ia_entries
            || self.player.ia_random != app.ia_random;
        if !changed {
            return Ok(false);
        }
        self.player.profile_name = app.profile.name.clone();
        self.player.privacy_notice_accepted = notice_accepted;
        self.player.shell = app.settings.clone();
        self.player.ia_playlist = app.ia_entries.clone();
        self.player.ia_random = app.ia_random;
        self.player.sync_from_shell();
        self.save_player()?;
        Ok(true)
    }

    /// Saves `player-settings.json` atomically.
    pub fn save_player(&self) -> Result<(), String> {
        let path = &self.paths.player_settings_file;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let mut text = serde_json::to_string_pretty(&self.player).map_err(|e| e.to_string())?;
        text.push('\n');
        {
            let mut file = fs::File::create(&tmp).map_err(|e| e.to_string())?;
            file.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
        }
        fs::rename(&tmp, path).map_err(|e| e.to_string())
    }

    /// Saves `DesktopSettings` (`desktop-settings.json`) atomically.
    pub fn save_desktop(&mut self) -> Result<(), String> {
        let patch = SettingsPatch {
            content_dir: Some(
                self.desktop
                    .chosen_content_dir
                    .clone()
                    .or_else(|| self.desktop.content_dir.clone()),
            ),
            game_dir: Some(self.desktop.game_dir.clone()),
            update_channel: Some(self.desktop.update_channel.clone()),
            auto_check_updates: Some(self.desktop.auto_check_updates),
            discord_presence: Some(self.desktop.discord_presence),
            preparations: Some(self.desktop.preparations.clone()),
        };
        self.desktop = self.desktop_store.save(&patch)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_persisted_player_settings() {
        let dir = std::env::temp_dir().join(format!(
            "bfe-settings-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let paths = DesktopPaths {
            user_data_dir: dir.clone(),
            user_data: dir.clone(),
            managed_content_dir: dir.join("game-data"),
            content_root: dir.join("game-data"),
            settings_file: dir.join("desktop-settings.json"),
            player_settings_file: dir.join("player-settings.json"),
            timings_file: dir.join("prepare-timings.json"),
            prepare_log_file: dir.join("logs/prepare.log"),
            update_root: None,
            project_root: dir.clone(),
        };
        let mut store = AppSettingsStore::load(paths.clone());
        store.player.profile_name = "Commander Cody".to_string();
        store.player.privacy_notice_accepted = true;
        store.player.shell.master_volume = 0.6;
        store.player.shell.invert_y = true;
        store.player.sync_from_shell();
        store.save_player().expect("save player settings");

        let reloaded = AppSettingsStore::load(paths);
        assert_eq!(reloaded.player.profile_name, "Commander Cody");
        assert!(reloaded.player.privacy_notice_accepted);
        assert!((reloaded.player.shell.master_volume - 0.6).abs() < 1e-5);
        assert!(reloaded.player.shell.invert_y);
        assert_eq!(
            reloaded.player.pause_kv.get("masterVolume").map(String::as_str),
            Some("60")
        );
        assert_eq!(
            reloaded.player.pause_kv.get("invertY").map(String::as_str),
            Some("true")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
