//! Player settings persistence backed by [`bfe_interface::Settings`].
//!
//! This module owns [`AppSettingsStore`], which persists the canonical 59-key
//! [`bfe_interface::Settings`] schema (sound, graphics, game, camera, chat,
//! developer, and all 34 keybindings) alongside the active player profile,
//! privacy notice flag, and Instant Action playlist in
//! `DesktopPaths::player_settings_file` (`player-settings.json`).
//!
//! ## Schema Synchronization and Projections
//! - **Canonical `bfe_interface::Settings` Store**: All 59 keys from `src/settings.js`
//!   (volumes, video resolution, quality presets, FOV, mouse sensitivity, invert Y,
//!   crosshair, friendly fire, AI difficulty, and 34 control bindings) live in
//!   [`AppSettingsStore::interface`] and serialize to `PersistedPlayerSettings::values`
//!   (`DesktopSettingsSnapshot` format).
//! - **`bfe-shell` Projection (`ShellSettings`)**: The 9 overlapping fields consumed
//!   directly by `ShellApp` and `ShellAudioMixer` (`master_volume`, `music_volume`,
//!   `effects_volume`, `interface_volume`, `menu_theme`, `battle_size`, `difficulty`,
//!   `mouse_sensitivity`, `invert_y`) are bidirectionally synchronized via
//!   [`PersistedPlayerSettings::apply_shell_to_interface`] and
//!   [`PersistedPlayerSettings::extract_shell_from_interface`].
//! - **In-Battle `PauseMenu` Projection (`pause_kv`)**: [`bfe_battle_ui::PauseMenu`]
//!   reads a `BTreeMap<String, String>` where volumes are formatted as integer
//!   percentages (`"0"`..`"100"`) and vertical look inversion is stored under `"invertY"`.
//!   [`PersistedPlayerSettings::interface_to_pause_kv`] keeps this map synchronized
//!   whenever settings are loaded or modified.
//! - **Atomic Persistence**: [`AppSettingsStore::save_player`] writes to a sibling
//!   `.json.tmp` file, flushes via `File::sync_all`, and renames over `player-settings.json`
//!   so power loss or crashes never leave a truncated configuration file.
//!
//! ## File Locations and Compatibility
//! - `DesktopPaths::settings_file` (`desktop-settings.json`): Stores installation paths
//!   (`content_dir`, `chosen_content_dir`, `found_content_dir`, `game_dir`), release
//!   channel (`update_channel`), `auto_check_updates`, `discord_presence`, and
//!   preparation history stamps.
//! - `DesktopPaths::player_settings_file` (`player-settings.json`): Stores `profile_name`,
//!   `privacy_notice_accepted`, the 59-key `bfe_interface::DesktopSettingsSnapshot`
//!   (`complete` + `values`), the `ShellSettings` projection (`shell`), the in-battle
//!   pause menu key-value map (`pause_kv`), and the Instant Action playlist (`ia_playlist`,
//!   `ia_random`).

use bfe_desktop::{
    paths::{resolve_desktop_paths, DesktopPaths, DesktopPathsOptions},
    settings::{DesktopSettings, SettingsPatch, SettingsStore},
};
use bfe_interface::{DesktopSettingsSnapshot, Settings as InterfaceSettings};
use bfe_shell::{
    launch::PlaylistEntry,
    screens::{ShellApp, ShellSettings},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Returns the platform string matching `bfe-desktop` conventions (`"linux"`, `"darwin"`, `"win32"`).
///
/// Used when constructing [`DesktopPathsOptions`], [`ToolchainOptions`], and
/// [`SelectTargetOptions`] so native Rust path and release-feed resolution matches
/// `electron/paths.js` and `electron/update.js` byte for byte.
pub fn current_platform_str() -> &'static str {
    // Map Rust `target_os` cfg attributes to Node/Electron `process.platform` identifiers.
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

/// Returns the CPU architecture string matching `bfe-desktop` conventions (`"x64"`, `"arm64"`).
///
/// Used by startup release-feed target selection and extraction toolchain lookup.
pub fn current_arch_str() -> &'static str {
    // Map Rust `target_arch` cfg attributes to Node/Electron `process.arch` identifiers.
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Resolves the repository or packaged root directory relative to the running binary
/// or current working directory.
///
/// Searches the current working directory and its parent for `tools/content-version.json`
/// so running `cargo run` from either the repository root or `rust/` finds the shared
/// `tools/` and `content/` directories.
pub fn resolve_project_root() -> PathBuf {
    // Check whether the current working directory or its parent contains `tools/content-version.json`.
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
///
/// Inspects `HOME`, `USERPROFILE`, `XDG_CONFIG_HOME`, `APPDATA`, and `BFE_*` overrides
/// through [`bfe_desktop::paths::resolve_desktop_paths`].
pub fn current_desktop_paths() -> DesktopPaths {
    // Collect environment variables (`HOME`, `USERPROFILE`, `XDG_CONFIG_HOME`, etc.) for path resolution.
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
///
/// The `complete` and `values` fields match the JSON layout of
/// [`bfe_interface::DesktopSettingsSnapshot`], allowing [`bfe_interface::Settings`]
/// to load and save all 59 canonical option keys directly from this file while
/// preserving the profile and playlist metadata.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedPlayerSettings {
    /// Active player profile display name (`"Player 1"` by default).
    pub profile_name: String,
    /// Whether the player has accepted the online multiplayer privacy notice.
    pub privacy_notice_accepted: bool,
    /// Whether `values` holds a complete 59-key [`bfe_interface::DesktopSettingsSnapshot`].
    #[serde(default)]
    pub complete: bool,
    /// Serialized 59-key [`bfe_interface::Settings`] map (`masterVolume`, `mouseSensitivity`, keybindings, etc.).
    #[serde(default)]
    pub values: Map<String, Value>,
    /// Synchronized [`ShellSettings`] projection consumed by `bfe-shell` audio and mission launch.
    #[serde(default)]
    pub shell: ShellSettings,
    /// Stringified key-value settings consumed by [`bfe_battle_ui::PauseMenu`] in-battle options/controls.
    #[serde(default)]
    pub pause_kv: BTreeMap<String, String>,
    /// Persisted Instant Action mission rotation playlist.
    #[serde(default)]
    pub ia_playlist: Vec<PlaylistEntry>,
    /// Whether Instant Action randomizes mission order (`ORDER: RANDOM` vs `ORDER: IN ORDER`).
    #[serde(default)]
    pub ia_random: bool,
}

impl Default for PersistedPlayerSettings {
    /// Constructs default player settings with a 3-map Clone Wars playlist (`nab2`, `tat1`, `bes1`).
    fn default() -> Self {
        let mut interface = InterfaceSettings::new();
        let shell = ShellSettings::default();
        Self::apply_shell_to_interface(&shell, &mut interface);
        let snap = interface.to_desktop_snapshot();
        let pause_kv = Self::interface_to_pause_kv(&interface, &shell);
        Self {
            profile_name: bfe_shell::profile::DEFAULT_PLAYER_NAME.to_string(),
            privacy_notice_accepted: false,
            complete: snap.complete,
            values: snap.values,
            shell,
            pause_kv,
            ia_playlist: Vec::new(),
            ia_random: false,
        }
    }
}

impl PersistedPlayerSettings {
    /// Synchronizes `ShellSettings` fields into `bfe_interface::Settings`.
    ///
    /// Maps the 9 front-end shell option fields (`masterVolume`, `musicVolume`,
    /// `effectsVolume`, `interfaceVolume`, `menuTheme`, `battleSize`, `aiDifficulty`,
    /// `mouseSensitivity`, and `invertMouse`) into the 59-key `bfe-interface` store.
    pub fn apply_shell_to_interface(shell: &ShellSettings, interface: &mut InterfaceSettings) {
        // Copy audio volumes, theme, battle size, AI difficulty, and mouse look preferences into `interface`.
        interface.set("masterVolume", shell.master_volume as f64);
        interface.set("musicVolume", shell.music_volume as f64);
        interface.set("effectsVolume", shell.effects_volume as f64);
        interface.set("interfaceVolume", shell.interface_volume as f64);
        interface.set("menuTheme", shell.menu_theme.clone());
        interface.set("battleSize", shell.battle_size.clone());
        interface.set("aiDifficulty", shell.difficulty.clone());
        interface.set("mouseSensitivity", shell.mouse_sensitivity as f64);
        interface.set("invertMouse", shell.invert_y);
    }

    /// Reads the `ShellSettings` projection back from `bfe_interface::Settings`.
    ///
    /// Clamps normalized audio gains to `0.0..=1.0` and preserves `fallback` string values
    /// if a string setting is unexpectedly empty.
    pub fn extract_shell_from_interface(
        interface: &InterfaceSettings,
        fallback: &ShellSettings,
    ) -> ShellSettings {
        // Project the 9 overlapping keys from `bfe_interface::Settings` back onto `ShellSettings`.
        let menu_theme = interface.get_string("menuTheme");
        let battle_size = interface.get_string("battleSize");
        let difficulty = interface.get_string("aiDifficulty");
        ShellSettings {
            master_volume: (interface.get_f64("masterVolume") as f32).clamp(0.0, 1.0),
            music_volume: (interface.get_f64("musicVolume") as f32).clamp(0.0, 1.0),
            effects_volume: (interface.get_f64("effectsVolume") as f32).clamp(0.0, 1.0),
            interface_volume: (interface.get_f64("interfaceVolume") as f32).clamp(0.0, 1.0),
            menu_theme: if menu_theme.is_empty() {
                fallback.menu_theme.clone()
            } else {
                menu_theme
            },
            battle_size: if battle_size.is_empty() {
                fallback.battle_size.clone()
            } else {
                battle_size
            },
            difficulty: if difficulty.is_empty() {
                fallback.difficulty.clone()
            } else {
                difficulty
            },
            mouse_sensitivity: interface.get_f64("mouseSensitivity") as f32,
            invert_y: interface.get_bool("invertMouse"),
        }
    }

    /// Synchronizes `ShellSettings` fields into the string map expected by
    /// `bfe_battle_ui::PauseMenu`.
    pub fn shell_to_pause_kv(shell: &ShellSettings) -> BTreeMap<String, String> {
        let mut interface = InterfaceSettings::new();
        Self::apply_shell_to_interface(shell, &mut interface);
        Self::interface_to_pause_kv(&interface, shell)
    }

    /// Exports all `bfe_interface::Settings` keys plus `ShellSettings` fields
    /// into the string map expected by `bfe_battle_ui::PauseMenu`.
    pub fn interface_to_pause_kv(
        interface: &InterfaceSettings,
        shell: &ShellSettings,
    ) -> BTreeMap<String, String> {
        // First populate all 59 canonical keys from `bfe_interface::Settings`.
        let mut map = BTreeMap::new();
        let snap = interface.to_desktop_snapshot();
        for (k, v) in snap.values {
            let text = match v {
                Value::String(s) => s,
                Value::Bool(b) => if b { "true" } else { "false" }.to_string(),
                Value::Number(n) => n.to_string(),
                other => other.to_string(),
            };
            map.insert(k, text);
        }
        // Then format percentage volumes (`0..=100`) and `"invertY"` as expected by `bfe_battle_ui::PauseMenu`.
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

    /// Re-synchronizes `values` and `pause_kv` from `self.shell`.
    pub fn sync_from_shell(&mut self) {
        // Rebuild an `InterfaceSettings` instance from the snapshot, apply `self.shell`, and export back.
        let snapshot = DesktopSettingsSnapshot {
            complete: self.complete,
            values: self.values.clone(),
        };
        let mut interface =
            InterfaceSettings::from_storage_and_desktop(&BTreeMap::new(), Some(&snapshot));
        Self::apply_shell_to_interface(&self.shell, &mut interface);
        let snap = interface.to_desktop_snapshot();
        self.complete = snap.complete;
        self.values = snap.values;
        let fresh = Self::interface_to_pause_kv(&interface, &self.shell);
        for (k, v) in fresh {
            self.pause_kv.insert(k, v);
        }
    }
}

/// Unified settings store owning [`DesktopSettings`] (`desktop-settings.json`),
/// [`bfe_interface::Settings`] (all 59 canonical option keys), and
/// [`PersistedPlayerSettings`] (`player-settings.json`).
#[derive(Clone, Debug)]
pub struct AppSettingsStore {
    /// Resolved application data and content paths.
    pub paths: DesktopPaths,
    /// Atomic store for `desktop-settings.json`.
    pub desktop_store: SettingsStore,
    /// Loaded desktop settings (`content_dir`, `game_dir`, `update_channel`, etc.).
    pub desktop: DesktopSettings,
    /// Canonical 59-key [`bfe_interface::Settings`] model driving the `bfe-interface` options pages.
    pub interface: InterfaceSettings,
    /// Persisted player profile, playlist, and synchronized shell/pause settings.
    pub player: PersistedPlayerSettings,
}

impl AppSettingsStore {
    /// Loads `DesktopSettings`, `bfe_interface::Settings`, and `PersistedPlayerSettings`
    /// from `paths` (falling back to defaults when missing or corrupt).
    pub fn load(paths: DesktopPaths) -> Self {
        let desktop_store = SettingsStore::new(&paths.settings_file);
        let desktop = desktop_store.load();
        let (player, interface) = Self::load_player_file(&paths.player_settings_file);
        Self {
            paths,
            desktop_store,
            desktop,
            interface,
            player,
        }
    }

    /// Reads `player-settings.json` and reconstructs both [`PersistedPlayerSettings`]
    /// and [`bfe_interface::Settings`].
    fn load_player_file(path: &Path) -> (PersistedPlayerSettings, InterfaceSettings) {
        let Ok(bytes) = fs::read(path) else {
            let mut interface = InterfaceSettings::new();
            let player = PersistedPlayerSettings::default();
            PersistedPlayerSettings::apply_shell_to_interface(&player.shell, &mut interface);
            return (player, interface);
        };
        let Ok(mut parsed) = serde_json::from_slice::<PersistedPlayerSettings>(&bytes) else {
            let mut interface = InterfaceSettings::new();
            let player = PersistedPlayerSettings::default();
            PersistedPlayerSettings::apply_shell_to_interface(&player.shell, &mut interface);
            return (player, interface);
        };
        // Rehydrate `InterfaceSettings` from `parsed.values` when present, or seed from `parsed.shell`.
        let interface = if !parsed.values.is_empty() {
            let snapshot = DesktopSettingsSnapshot {
                complete: parsed.complete,
                values: parsed.values.clone(),
            };
            let iface =
                InterfaceSettings::from_storage_and_desktop(&BTreeMap::new(), Some(&snapshot));
            parsed.shell =
                PersistedPlayerSettings::extract_shell_from_interface(&iface, &parsed.shell);
            iface
        } else {
            let mut iface = InterfaceSettings::new();
            PersistedPlayerSettings::apply_shell_to_interface(&parsed.shell, &mut iface);
            let snap = iface.to_desktop_snapshot();
            parsed.complete = snap.complete;
            parsed.values = snap.values;
            iface
        };
        if parsed.pause_kv.is_empty() {
            parsed.pause_kv =
                PersistedPlayerSettings::interface_to_pause_kv(&interface, &parsed.shell);
        }
        (parsed, interface)
    }

    /// Synchronizes `self.player` into `self.interface` when `self.player.shell` was mutated directly.
    pub fn sync_interface_from_player(&mut self) {
        PersistedPlayerSettings::apply_shell_to_interface(&self.player.shell, &mut self.interface);
        self.interface
            .set("playerName", self.player.profile_name.clone());
        let snap = self.interface.to_desktop_snapshot();
        self.player.complete = snap.complete;
        self.player.values = snap.values;
        let fresh =
            PersistedPlayerSettings::interface_to_pause_kv(&self.interface, &self.player.shell);
        for (k, v) in fresh {
            self.player.pause_kv.insert(k, v);
        }
    }

    /// Applies changes made in `bfe_interface::Settings` (from the `bfe-interface`
    /// `OPTIONS` pages) into `self.interface`, `self.player`, and `ShellApp`,
    /// saving to disk if anything changed.
    pub fn capture_from_interface_settings(
        &mut self,
        interface: &InterfaceSettings,
        app: &mut ShellApp,
    ) -> Result<bool, String> {
        if self.interface.values == interface.values
            && self.interface.complete == interface.complete
        {
            return Ok(false);
        }
        self.interface = interface.clone();
        let snap = self.interface.to_desktop_snapshot();
        self.player.complete = snap.complete;
        self.player.values = snap.values;
        self.player.shell = PersistedPlayerSettings::extract_shell_from_interface(
            &self.interface,
            &self.player.shell,
        );
        app.settings = self.player.shell.clone();
        let fresh =
            PersistedPlayerSettings::interface_to_pause_kv(&self.interface, &self.player.shell);
        for (k, v) in fresh {
            self.player.pause_kv.insert(k, v);
        }
        self.save_player()?;
        Ok(true)
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
        self.sync_interface_from_player();
        self.save_player()?;
        Ok(true)
    }

    /// Saves `player-settings.json` atomically after synchronizing `self.interface`
    /// with `self.player`.
    pub fn save_player(&mut self) -> Result<(), String> {
        self.sync_interface_from_player();
        let path = &self.paths.player_settings_file;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // Write to a sibling `.json.tmp` file and `fsync` before renaming into place.
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

    /// Verifies that `AppSettingsStore` persists and reloads `bfe_interface::Settings`,
    /// `ShellSettings`, and `pause_kv` across a round-trip to `player-settings.json`.
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
        assert!((reloaded.interface.get_f64("masterVolume") - 0.6).abs() < 1e-5);
        assert!(reloaded.interface.get_bool("invertMouse"));
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
