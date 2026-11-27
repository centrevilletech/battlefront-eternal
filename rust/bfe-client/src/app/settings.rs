//! Player settings persistence backed by [`bfe_interface::Settings`].

use bfe_desktop::{
    paths::{resolve_desktop_paths, DesktopPaths, DesktopPathsOptions},
    settings::{DesktopSettings, SettingsPatch, SettingsStore},
};
use bfe_interface::{hotkeys::ControlContext, settings::{DefaultVal, SWBF2_BINDINGS, SWBF2_FLIGHT_BINDINGS, SWBF2_GROUND_BINDINGS}, DesktopSettingsSnapshot, SettingValue, Settings as InterfaceSettings};
use bfe_shell::{
    launch::PlaylistEntry,
    screens::{ShellApp, ShellSettings},
};
use serde::{Deserialize, Serialize};

/// The saved record's revision: 1 fixed the old mouse sensitivity default, 2 gave Minimum the SHORT view distance.
const NATIVE_SETTINGS_VERSION: u32 = 2;
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

/// Returns the platform string matching `bfe-desktop` conventions (`"linux"`, `"darwin"`, `"win32"`).
///
/// Used when constructing [`DesktopPathsOptions`], [`ToolchainOptions`], and
/// [`SelectTargetOptions`]: the platform names the release feed and the user
/// folders use (`win32`, `darwin`, `linux`), the same as earlier releases wrote.
pub fn current_platform_str() -> &'static str {
    // The release feed's platform names.
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
    // The release feed's architecture names.
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x64"
    }
}

/// Resolves the repository or packaged root directory relative to the running binary
/// or current working directory.
///
/// A packaged executable resolves resources beside itself, independently of cwd.
pub fn application_resources() -> Option<PathBuf> {
    let executable = env::current_exe().ok()?;
    let resources = executable.parent()?.join("resources");
    resources
        .join("tools/prepare.py")
        .is_file()
        .then_some(resources)
}

/// Find packaged resources, a working-directory checkout, or the checkout
/// this development binary was built from. Launching from a file manager must
/// not change which preparation tools or content-version file the client uses.
pub fn resolve_project_root() -> PathBuf {
    if let Some(resources) = application_resources() {
        return resources.parent().unwrap_or(&resources).to_path_buf();
    }
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
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent);
    if let Some(root) = checkout.filter(|p| p.join("tools/content-version.json").is_file()) {
        return root.to_path_buf();
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
    #[serde(default = "default_profile_name")]
    pub profile_name: String,
    /// Whether the player has accepted the online multiplayer privacy notice.
    #[serde(default)]
    pub privacy_notice_accepted: bool,
    /// Native projection revision; absent in earlier builds' records and older native files.
    #[serde(default)]
    pub native_settings_version: u32,
    /// Whether `values` holds a complete 59-key [`bfe_interface::DesktopSettingsSnapshot`].
    #[serde(default)]
    pub complete: bool,
    /// Serialized 59-key [`bfe_interface::Settings`] map (`masterVolume`, `mouseSensitivity`, keybindings, etc.).
    #[serde(default, deserialize_with = "bfe_storage::settings_json::object")]
    pub values: Map<String, Value>,
    /// Only this top-level version admits the Battlefront II maps in `values`.
    #[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "bfe_storage::settings_json::optional")]
    pub swbf2_controls_version: Option<Value>,
    /// Optional accessibility preference for a dedicated Battlefront II block control.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swbf2_hold_to_block: Option<bool>,
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

fn default_profile_name() -> String {
    bfe_shell::profile::DEFAULT_PLAYER_NAME.to_string()
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
            native_settings_version: NATIVE_SETTINGS_VERSION,
            complete: snap.complete,
            values: snap.values,
            swbf2_controls_version: None,
            swbf2_hold_to_block: None,
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
        if self.swbf2_controls_version.as_ref().and_then(Value::as_u64) != Some(1) {
            self.sync_released_from_shell();
            return;
        }
        let controls = SWBF2_BINDINGS.map(|key| self.values.remove(key));
        self.sync_released_from_shell();
        for (key, value) in SWBF2_BINDINGS.into_iter().zip(controls) {
            if let Some(value) = value {
                self.values.insert(key.into(), value);
            }
        }
    }

    fn sync_released_from_shell(&mut self) {
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
    /// Snapshot of `desktop.extra["games"]` as last loaded from or saved to disk,
    /// so ordinary 2004 saves leave `games` on disk completely untouched while
    /// deliberate in-memory edits or clears to `games` are persisted accurately.
    saved_games: Option<Value>,
    /// Canonical 59-key [`bfe_interface::Settings`] model driving the `bfe-interface` options pages.
    pub interface: InterfaceSettings,
    /// Persisted player profile, playlist, and synchronized shell/pause settings.
    pub player: PersistedPlayerSettings,
}

// Battle view selectors are optional interface extensions, rather than new
// canonical graphics/control defaults. Keep their saved values beside those
// defaults so a shell theme change cannot silently remove a Classic override.
const BATTLE_VIEW_KEYS: [&str; 2] = ["spawnScreenView", "scoreboardView"];

impl AppSettingsStore {
    fn snapshot_with_battle_views(&self) -> DesktopSettingsSnapshot {
        let mut snapshot = self.interface.to_desktop_snapshot();
        for key in BATTLE_VIEW_KEYS {
            if let Some(value) = self.interface.values.get(key) {
                snapshot.values.insert(key.into(), value.to_json());
            }
        }
        snapshot
    }
    /// Loads `DesktopSettings`, `bfe_interface::Settings`, and `PersistedPlayerSettings`
    /// from `paths` (falling back to defaults when missing or corrupt).
    pub fn load(paths: DesktopPaths) -> Self {
        let desktop_store = SettingsStore::new(&paths.settings_file);
        let desktop = desktop_store.load();
        let saved_games = desktop.extra.get("games").cloned();
        let (player, interface, migrated) = Self::load_player_file(&paths.player_settings_file);
        let mut store = Self {
            paths,
            desktop_store,
            desktop,
            saved_games,
            interface,
            player,
        };
        // Persist the revision once so choosing 0.5 after upgrading stays a
        // deliberate preference. A read-only file still uses the corrected value.
        if migrated {
            if let Err(error) = store.save_player() {
                eprintln!("could not save migrated player settings: {error}");
            }
        }
        store
    }

    /// Reads `player-settings.json` and reconstructs both [`PersistedPlayerSettings`]
    /// and [`bfe_interface::Settings`].
    fn load_player_file(path: &Path) -> (PersistedPlayerSettings, InterfaceSettings, bool) {
        let (player, mut interface, migrated) = Self::load_released_player_file(path);
        if player.swbf2_controls_version.as_ref().and_then(Value::as_u64) == Some(1) {
            for context in ControlContext::ALL {
                let value = match context {
                    ControlContext::Starfighter => player.values.get(SWBF2_FLIGHT_BINDINGS),
                    _ => player.values.get(SWBF2_GROUND_BINDINGS).and_then(|map| map.get(context.tab())),
                };
                if let Some(SettingValue::Bindings(map)) = value
                    .and_then(|v| SettingValue::from_json_typed(v, DefaultVal::Bindings))
                {
                    let map = context.saved_overrides(map);
                    if !map.is_empty() {
                        interface.swbf2_bindings.insert(context, map);
                    }
                }
            }
        }
        (player, interface, migrated)
    }

    fn load_released_player_file(path: &Path) -> (PersistedPlayerSettings, InterfaceSettings, bool) {
        let Ok(bytes) = fs::read(path) else {
            let mut interface = InterfaceSettings::new();
            let player = PersistedPlayerSettings::default();
            PersistedPlayerSettings::apply_shell_to_interface(&player.shell, &mut interface);
            interface.fresh = true;
            return (player, interface, false);
        };
        let Ok((mut parsed, native_record, native_profile)) = Self::parse_player_record(&bytes) else {
            let mut interface = InterfaceSettings::new();
            let player = PersistedPlayerSettings::default();
            PersistedPlayerSettings::apply_shell_to_interface(&player.shell, &mut interface);
            interface.fresh = true;
            return (player, interface, false);
        };
        // Settings files written by earlier releases share the complete/values
        // record. Only files carrying an old native shell projection can
        // contain the erroneous 0.5 default; a 0.5 the player chose in an
        // earlier release and other saved slider values remain intentional.
        let fresh = !native_record && parsed.values.is_empty();
        let migrated = native_record && parsed.native_settings_version == 0;
        if migrated {
            let sensitivity = parsed
                .values
                .get("mouseSensitivity")
                .and_then(Value::as_f64)
                .unwrap_or(parsed.shell.mouse_sensitivity as f64);
            if sensitivity == 0.5 {
                parsed.shell.mouse_sensitivity = ShellSettings::default().mouse_sensitivity;
                if !parsed.values.is_empty() {
                    parsed.values.insert(
                        "mouseSensitivity".into(),
                        Value::from(parsed.shell.mouse_sensitivity),
                    );
                }
            }
        }
        if parsed.native_settings_version == 0 {
            parsed.native_settings_version = 1;
        }
        // Version 2: the Minimum preset's view distance became SHORT. A player still on Minimum gets it; a
        // player who set a view distance by hand is on "custom" and keeps theirs.
        let minimum_view = parsed.native_settings_version < 2 && Self::adopt_minimum_view(&mut parsed.values);
        if parsed.native_settings_version < NATIVE_SETTINGS_VERSION {
            parsed.native_settings_version = NATIVE_SETTINGS_VERSION;
        }
        let migrated = migrated || minimum_view;
        // Rehydrate `InterfaceSettings` from `parsed.values` when present, or seed from `parsed.shell`.
        let mut interface = if !parsed.values.is_empty() {
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
        // Browser records have no native profile field; playerName in the
        // shared values is authoritative when importing that record.
        if !native_profile {
            parsed.profile_name = interface.get_string("playerName");
        }
        // The canonical loader intentionally accepts only DEFAULTS. Admit the
        // two battle selectors explicitly; arbitrary saved keys stay excluded.
        for key in BATTLE_VIEW_KEYS {
            if let Some(value) = parsed.values.get(key).and_then(Value::as_str) {
                interface.set(key, value.to_owned());
            }
        }
        parsed
            .pause_kv
            .extend(PersistedPlayerSettings::interface_to_pause_kv(
                &interface,
                &parsed.shell,
            ));
        interface.fresh = fresh;
        (parsed, interface, migrated)
    }

    fn parse_player_record(bytes: &[u8]) -> serde_json::Result<(PersistedPlayerSettings, bool, bool)> {
        let record = bfe_storage::settings_json::from_slice(bytes)?;
        let native_record = record.get("shell").is_some_and(Value::is_object);
        let native_profile = record.get("profileName").is_some();
        Ok((serde_json::from_value(record)?, native_record, native_profile))
    }

    /// Version 2's change to a saved record: on the Minimum preset with the old Minimum view distance, SHORT.
    fn adopt_minimum_view(values: &mut Map<String, Value>) -> bool {
        let minimum = values.get("performancePreset").and_then(Value::as_str) == Some("minimum");
        let old = values.get("viewDistance").and_then(Value::as_str).is_none_or(|v| v == "original");
        if minimum && old {
            values.insert("viewDistance".into(), Value::from("short"));
        }
        minimum && old
    }

    /// The first run's quality preset: Minimum for a pre-Ivy Bridge Intel GPU (as the
    /// browser build), else by the measured fill rate (`gpu_probe`), else Default.
    pub fn adopt_gpu_preset(&mut self, renderer: &str, vendor: &str, fill_ms_per_mpix: Option<f64>) -> Result<(), String> {
        if !self.interface.fresh { return Ok(()); }
        let gpu = bfe_interface::settings::classify_gpu(renderer, vendor);
        self.interface.adopt_hardware_defaults(Some(&gpu), None);
        // A software renderer draws on the CPU the game also needs: always the floor.
        let software = ["llvmpipe", "softpipe", "swiftshader", "software rasterizer", "microsoft basic", "gdi generic"]
            .iter().any(|name| renderer.to_ascii_lowercase().contains(name));
        if software {
            self.interface.set("performancePreset", "minimum");
        } else if !gpu.is_old_intel {
            if let Some(preset) = fill_ms_per_mpix.and_then(measured_preset) {
                self.interface.set("performancePreset", preset);
            } else if cfg!(target_os = "macos") {
                // A Mac's own display has two to three times the pixels of a common PC monitor and an
                // integrated GPU to fill them: a first start there keeps 60 frames a second by drawing
                // the scene smaller when it has to (an iMac's 4480x2520 at full size draws about 40).
                self.interface.set("dynamicResolution", "60");
            }
        }
        self.interface.fresh = false;
        eprintln!("first-run graphics: GPU={renderer} fill={} ms/Mpix preset={}",
            fill_ms_per_mpix.map_or("unmeasured".into(), |v| format!("{v:.3}")), self.interface.get_string("performancePreset"));
        self.player.shell = PersistedPlayerSettings::extract_shell_from_interface(&self.interface, &self.player.shell);
        self.save_player()?;
        Ok(())
    }

    /// Synchronizes `self.player` into `self.interface` when `self.player.shell` was mutated directly.
    pub fn sync_interface_from_player(&mut self) {
        if !self.interface.swbf2_bindings.is_empty() {
            self.interface.complete = true;
        }
        self.sync_released_interface_from_player();
        let controls = &self.interface.swbf2_bindings;
        self.player.swbf2_controls_version = (!controls.is_empty()).then(|| Value::from(1));
        let mut ground = Map::new();
        for (context, bindings) in controls {
            let value = serde_json::to_value(bindings).expect("string bindings serialize");
            if *context == ControlContext::Starfighter {
                self.player.values.insert(SWBF2_FLIGHT_BINDINGS.into(), value);
            } else {
                ground.insert(context.tab().into(), value);
            }
        }
        if !ground.is_empty() {
            self.player.values.insert(SWBF2_GROUND_BINDINGS.into(), Value::Object(ground));
        }
    }

    fn sync_released_interface_from_player(&mut self) {
        PersistedPlayerSettings::apply_shell_to_interface(&self.player.shell, &mut self.interface);
        self.interface
            .set("playerName", self.player.profile_name.clone());
        let snap = self.snapshot_with_battle_views();
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
        if self.interface.swbf2_bindings != interface.swbf2_bindings
            && self.interface.complete == interface.complete && self.interface.values == interface.values
        {
            self.interface = interface.clone();
            self.save_player()?;
            return Ok(true);
        }
        self.capture_released_interface_settings(interface, app)
    }

    fn capture_released_interface_settings(
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
        let snap = self.snapshot_with_battle_views();
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
        static SAVE_SEQUENCE: AtomicU32 = AtomicU32::new(0);
        self.sync_interface_from_player();
        let path = &self.paths.player_settings_file;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut text = serde_json::to_string_pretty(&self.player).map_err(|e| e.to_string())?;
        text.push('\n');
        let (tmp, mut file) = loop {
            let sequence = SAVE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let tmp = path.with_extension(format!("json.native-{}-{sequence}.tmp", std::process::id()));
            match fs::File::create_new(&tmp) {
                Ok(file) => break (tmp, file),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        };
        let written = file.write_all(text.as_bytes()).and_then(|()| file.sync_all());
        drop(file);
        let published = written
            .and_then(|()| bfe_storage::preserve_unreadable(path, |bytes| Self::parse_player_record(bytes).is_ok()))
            .and_then(|_| fs::rename(&tmp, path));
        if published.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        published.map_err(|e| e.to_string())
    }

    /// Replaces `self.desktop` from a completed background search while keeping
    /// the last-saved `games` snapshot synchronized.
    pub fn adopt_desktop(&mut self, desktop: DesktopSettings) {
        self.saved_games = desktop.extra.get("games").cloned();
        self.desktop = desktop;
    }

    /// Updates and atomically persists a single game family's install directory
    /// without touching unrelated settings fields or sibling families.
    pub fn save_game_dir_for(
        &mut self,
        family: bfe_family::GameFamily,
        dir: Option<String>,
    ) -> Result<(), String> {
        let mut patch = SettingsPatch::default();
        if family == bfe_family::GameFamily::Swbf1 {
            patch.game_dir = Some(dir);
        } else {
            patch.game_dirs.insert(family, dir);
        }
        self.desktop = self.desktop_store.save(&patch)?;
        self.saved_games = self.desktop.extra.get("games").cloned();
        Ok(())
    }

    /// Saves `DesktopSettings` (`desktop-settings.json`) atomically.
    ///
    /// Leaves `extra["games"]` on disk completely untouched unless `self.desktop.extra["games"]`
    /// was explicitly mutated or cleared in memory since load/last save.
    pub fn save_desktop(&mut self) -> Result<(), String> {
        let current_games = self.desktop.extra.get("games").cloned();
        let games_extra = if current_games != self.saved_games {
            Some(current_games)
        } else {
            None
        };
        let patch = SettingsPatch {
            enhanced_sound: self.desktop.enhanced_sound,
            content_dir: Some(
                self.desktop
                    .chosen_content_dir
                    .clone()
                    .or_else(|| self.desktop.content_dir.clone()),
            ),
            game_dir: Some(self.desktop.game_dir.clone()),
            game_dirs: BTreeMap::new(),
            update_channel: Some(self.desktop.update_channel.clone()),
            auto_check_updates: Some(self.desktop.auto_check_updates),
            discord_presence: Some(self.desktop.discord_presence),
            presence: self.desktop.presence.clone(),
            steam: self.desktop.steam.clone(),
            preparations: Some(self.desktop.preparations.clone()),
            game_preparations: BTreeMap::new(),
            games_extra,
        };
        self.desktop = self.desktop_store.save(&patch)?;
        self.saved_games = self.desktop.extra.get("games").cloned();
        Ok(())
    }
}

/// The preset a measured fill rate (milliseconds per million blended, textured
/// pixels, `gpu_probe`) starts at. Measured: Radeon 890M 0.063, Radeon R5 (A8-6410
/// laptop, Default 19-25 fps at 1600x900) 1.66; GPUs two to three times slower than
/// that one (Sandy Bridge's HD 2000/3000) land above 2.5 and start at Minimum.
pub fn measured_preset(fill_ms_per_mpix: f64) -> Option<&'static str> {
    if !fill_ms_per_mpix.is_finite() { return None; }
    if fill_ms_per_mpix > 2.5 { Some("minimum") } else if fill_ms_per_mpix > 0.4 { Some("performance") } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_paths() -> DesktopPaths {
        let dir = std::env::temp_dir().join(format!(
            "bfe-player-migration-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        DesktopPaths {
            user_data_dir: dir.clone(),
            user_data: dir.clone(),
            managed_content_dir: dir.join("game-data"),
            content_root: dir.join("game-data"),
            settings_file: dir.join("desktop-settings.json"),
            player_settings_file: dir.join("player-settings.json"),
            timings_file: dir.join("prepare-timings.json"),
            prepare_log_file: dir.join("logs/prepare.log"),
            update_root: None,
            project_root: dir,
        }
    }

    #[test]
    fn profile_extension_keys_survive_or_get_the_original_file_backup() {
        let paths = test_paths();
        let bytes = br#"{"profileName":"Amber","nativeSettingsVersion":2,"iaRandom":true,"iaPlaylist":[{"family":"swbf1","map":"moss","era":"cw","mode":"conquest"}],"extra":{"$serde_json::private::RawValue":7}}"#;
        fs::write(&paths.player_settings_file, bytes).unwrap();
        let mut store = AppSettingsStore::load(paths.clone());
        let backup = paths.user_data.join("player-settings.json.bak");
        store.save_player().unwrap();
        let reopened = AppSettingsStore::load(paths.clone());
        if bfe_storage::settings_json::from_slice(bytes).is_ok() {
            assert_eq!(reopened.player.profile_name, "Amber");
            assert_eq!(reopened.player.ia_playlist[0].map, "moss");
            assert!(reopened.player.ia_random);
            assert!(!backup.exists());
        } else {
            assert_eq!(fs::read(backup).unwrap(), bytes);
            assert_eq!(reopened.player.profile_name, default_profile_name());
            assert!(reopened.player.ia_playlist.is_empty());
        }
        fs::remove_dir_all(paths.user_data).unwrap();
    }

    #[test]
    fn first_run_gpu_choice_matches_browser_and_keeps_saved_settings() {
        let paths = test_paths();
        let mut store = AppSettingsStore::load(paths.clone());
        store.adopt_gpu_preset("Intel(R) HD Graphics 3000", "Intel", None).unwrap();
        assert_eq!(store.interface.get_string("performancePreset"), "minimum");
        store.interface.set("performancePreset", "quality");
        store.player.shell = PersistedPlayerSettings::extract_shell_from_interface(&store.interface, &store.player.shell);
        store.save_player().unwrap();
        let mut saved = AppSettingsStore::load(paths.clone());
        saved.adopt_gpu_preset("Intel(R) HD Graphics 3000", "Intel", None).unwrap();
        assert_eq!(saved.interface.get_string("performancePreset"), "quality");
        fs::remove_dir_all(&paths.user_data_dir).unwrap();
        let mut fresh = AppSettingsStore::load(paths.clone());
        fresh.adopt_gpu_preset("AMD Radeon R5 Graphics (radeonsi, kabini)", "AMD", None).unwrap();
        let record: Value = serde_json::from_slice(&fs::read(&paths.player_settings_file).unwrap()).unwrap();
        assert_eq!(record["values"]["performancePreset"], "custom");
        assert_eq!(record["values"]["renderScale"], 1.);
        let mut reloaded = AppSettingsStore::load(paths.clone());
        reloaded.adopt_gpu_preset("Intel(R) HD Graphics 3000", "Intel", Some(9.)).unwrap();
        assert_eq!(reloaded.interface.get_string("performancePreset"), "custom");
        for name in ["AMD Radeon R5 Graphics (radeonsi, kabini)", "Intel(R) HD Graphics 4000", "llvmpipe (Intel(R) HD Graphics 3000)"] {
            assert!(!bfe_interface::settings::classify_gpu(name, "").is_old_intel, "{name}");
        }
        assert!(bfe_interface::settings::classify_gpu("Mesa Intel(R) HD Graphics 2000 (SNB GT1)", "").is_old_intel);
        fs::remove_dir_all(paths.user_data_dir).unwrap();
    }

    #[test]
    fn first_run_preset_follows_the_measured_fill_rate() {
        assert_eq!(super::measured_preset(0.063), None);
        assert_eq!(super::measured_preset(1.66), Some("performance"));
        assert_eq!(super::measured_preset(4.0), Some("minimum"));
        assert_eq!(super::measured_preset(f64::NAN), None);
        let paths = test_paths();
        let mut store = AppSettingsStore::load(paths.clone());
        store.adopt_gpu_preset("llvmpipe (LLVM 20.1.2, 256 bits)", "Mesa", Some(0.1)).unwrap();
        assert_eq!(store.interface.get_string("performancePreset"), "minimum");
        fs::remove_dir_all(paths.user_data_dir).unwrap();
    }

    #[test]
    fn minimum_players_get_the_short_view_and_hand_set_distances_stay() {
        for (preset, saved, expected) in [("minimum", "original", "short"), ("custom", "original", "original"), ("minimum", "far", "far"), ("balanced", "original", "original")] {
            let paths = test_paths();
            let mut old = PersistedPlayerSettings::default();
            old.native_settings_version = 1;
            old.values.insert("performancePreset".into(), Value::from(preset));
            old.values.insert("viewDistance".into(), Value::from(saved));
            fs::write(&paths.player_settings_file, serde_json::to_vec(&old).unwrap()).unwrap();
            let store = AppSettingsStore::load(paths.clone());
            assert_eq!(store.interface.get_string("viewDistance"), expected, "{preset} {saved}");
            // A changed record is saved once, so the menus show the preset's own value.
            let written: Value = serde_json::from_slice(&fs::read(&paths.player_settings_file).unwrap()).unwrap();
            assert_eq!(written["values"]["viewDistance"], expected);
            fs::remove_dir_all(paths.user_data_dir).unwrap();
        }
    }

    #[test]
    fn native_mouse_default_is_the_neutral_browser_slider_value() {
        let player = PersistedPlayerSettings::default();
        assert_eq!(player.shell.mouse_sensitivity, 5.);
        assert_eq!(player.values["mouseSensitivity"], 5.);
        let mut interface = InterfaceSettings::new();
        PersistedPlayerSettings::apply_shell_to_interface(&player.shell, &mut interface);
        assert_eq!(interface.mouse_scale(), 1.);
    }

    #[test]
    fn old_native_mouse_default_migrates_once_without_losing_preferences() {
        for shell_only in [false, true] {
            let paths = test_paths();
            let mut old = PersistedPlayerSettings::default();
            old.shell.mouse_sensitivity = 0.5;
            old.shell.invert_y = true;
            old.profile_name = "SCOUT".into();
            old.sync_from_shell();
            let mut record = serde_json::to_value(old).unwrap();
            record["values"]["spawnScreenView"] = Value::from("classic");
            record["values"]["scoreboardView"] = Value::from("page");
            record
                .as_object_mut()
                .unwrap()
                .remove("nativeSettingsVersion");
            if shell_only {
                record.as_object_mut().unwrap().remove("values");
            }
            fs::write(
                &paths.player_settings_file,
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
            let mut store = AppSettingsStore::load(paths.clone());
            assert_eq!(store.interface.mouse_scale(), 1.);
            assert_eq!(store.player.shell.mouse_sensitivity, 5.);
            assert_eq!(store.player.profile_name, "SCOUT");
            if !shell_only {
                assert_eq!(store.interface.get_string("spawnScreenView"), "classic");
                assert_eq!(store.interface.get_string("scoreboardView"), "page");
            }
            assert!(store.player.shell.invert_y);
            assert_eq!(store.player.pause_kv["mouseSensitivity"], "5.00");
            let saved: Value =
                serde_json::from_slice(&fs::read(&paths.player_settings_file).unwrap()).unwrap();
            assert_eq!(saved["nativeSettingsVersion"], 2);
            assert_eq!(saved["values"]["mouseSensitivity"], 5.);
            // A subsequent intentional minimum slider value must survive reload.
            store.player.shell.mouse_sensitivity = 0.5;
            store.save_player().unwrap();
            let reloaded = AppSettingsStore::load(paths.clone());
            assert_eq!(reloaded.player.shell.mouse_sensitivity, 0.5);
            fs::remove_dir_all(paths.user_data).unwrap();
        }
    }

    #[test]
    fn browser_records_preserve_sensitivity_profile_and_binding_overrides() {
        for sensitivity in [0.5, 3.2, 5.] {
            let paths = test_paths();
            let record = serde_json::json!({"complete": true, "values": {
                "mouseSensitivity": sensitivity, "playerName": "PILOT", "masterVolume": 0.3,
                "keyBindings": {"presses.jump": ["KeyJ"]}
            }});
            fs::write(
                &paths.player_settings_file,
                serde_json::to_vec(&record).unwrap(),
            )
            .unwrap();
            let mut store = AppSettingsStore::load(paths.clone());
            assert_eq!(store.interface.get_f64("mouseSensitivity"), sensitivity);
            assert_eq!(store.player.profile_name, "PILOT");
            assert_eq!(store.interface.get_f64("masterVolume"), 0.3);
            assert_eq!(
                store.interface.get_bindings_ref("keyBindings").unwrap()["presses.jump"],
                ["KeyJ"]
            );
            store.save_player().unwrap();
            // Earlier releases read only complete/values from the same file.
            let saved: Value =
                serde_json::from_slice(&fs::read(&paths.player_settings_file).unwrap()).unwrap();
            assert!(
                (saved["values"]["mouseSensitivity"].as_f64().unwrap() - sensitivity).abs() < 1e-6
            );
            assert_eq!(saved["values"]["playerName"], "PILOT");
            fs::remove_dir_all(paths.user_data).unwrap();
        }
    }

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
        store.interface.set("spawnScreenView", "classic");
        store.interface.set("scoreboardView", "page");
        store.save_player().expect("save player settings");

        // Each offer answer must survive the atomic store's read/patch/write,
        // otherwise a fresh client shows the question again on every launch.
        for detail in ["off", "game", "full"] {
            store
                .desktop
                .presence
                .get_or_insert_with(Default::default)
                .discord = Some(detail.into());
            store.desktop.discord_presence = detail != "off";
            store.save_desktop().expect("save presence choice");
            let saved = AppSettingsStore::load(paths.clone());
            assert_eq!(
                saved.desktop.presence.unwrap().discord.as_deref(),
                Some(detail)
            );
            assert_eq!(saved.desktop.discord_presence, detail != "off");
        }

        // NOT NOW on the Steam offer must also survive a relaunch.
        store.desktop.steam.get_or_insert_with(Default::default).declined = Some(true);
        store.save_desktop().expect("save steam answer");
        let saved = AppSettingsStore::load(paths.clone());
        assert_eq!(saved.desktop.steam.and_then(|s| s.declined), Some(true));

        let reloaded = AppSettingsStore::load(paths);
        assert_eq!(reloaded.interface.get_string("spawnScreenView"), "classic");
        assert_eq!(reloaded.interface.get_string("scoreboardView"), "page");
        assert_eq!(reloaded.player.profile_name, "Commander Cody");
        assert!(reloaded.player.privacy_notice_accepted);
        assert!((reloaded.player.shell.master_volume - 0.6).abs() < 1e-5);
        assert!(reloaded.player.shell.invert_y);
        assert!((reloaded.interface.get_f64("masterVolume") - 0.6).abs() < 1e-5);
        assert!(reloaded.interface.get_bool("invertMouse"));
        assert_eq!(
            reloaded
                .player
                .pause_kv
                .get("masterVolume")
                .map(String::as_str),
            Some("60")
        );
        assert_eq!(
            reloaded.player.pause_kv.get("invertY").map(String::as_str),
            Some("true")
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_desktop_preserves_swbf2_extensions_and_persists_explicit_clear() {
        let paths = test_paths();
        let initial = serde_json::json!({
            "gameDir": "/games/bf1",
            "games": {
                "swbf2": {
                    "gameDir": "/keep",
                    "preparations": {
                        "/a": {
                            "gameDir": "/g",
                            "addOns": [],
                            "updatedAt": "2026-01-01T00:00:00Z",
                            "extension": ["keep"]
                        },
                        "future-record": "keep-as-is"
                    }
                }
            }
        });
        fs::write(
            &paths.settings_file,
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        // 1. Ordinary 2004 startup save must NOT strip extensions or non-record entries under games.swbf2.
        let mut store = AppSettingsStore::load(paths.clone());
        store.desktop.found_content_dir = Some("/found".to_string());
        store.save_desktop().unwrap();
        let after_2004_save: Value =
            serde_json::from_str(&fs::read_to_string(&paths.settings_file).unwrap()).unwrap();
        assert_eq!(
            after_2004_save["games"]["swbf2"]["preparations"]["/a"]["extension"],
            serde_json::json!(["keep"]),
            "ordinary 2004 save_desktop stripped games.swbf2.preparations extension"
        );
        assert_eq!(
            after_2004_save["games"]["swbf2"]["preparations"]["future-record"],
            serde_json::json!("keep-as-is"),
            "ordinary 2004 save_desktop dropped games.swbf2.preparations future-record"
        );

        // 2. Explicitly clearing swbf2 gameDir and preparations in memory then calling save_desktop
        //    must actually clear them on disk rather than resurrecting "/keep".
        store
            .desktop
            .set_game_dir_for(bfe_family::GameFamily::Swbf2, None);
        store
            .desktop
            .set_preparations_for(bfe_family::GameFamily::Swbf2, BTreeMap::new());
        store.save_desktop().unwrap();
        let after_clear: Value =
            serde_json::from_str(&fs::read_to_string(&paths.settings_file).unwrap()).unwrap();
        assert!(
            after_clear.get("games").and_then(|g| g.get("swbf2")).is_none(),
            "clearing swbf2 then calling save_desktop resurrected old data: {after_clear}"
        );

        let _ = fs::remove_dir_all(&paths.user_data_dir);
    }
}
