//! Control binding tables, key names, and `CONTROL OPTIONS` tab rows
//! (`src/menus/hotkeys.js`, `src/input.js`, `src/flight/controls.js`).
//!
//! Provides the keyboard and mouse input code registry (`INPUT_CODES`, `RESERVED`,
//! `bindable`), human-readable key label formatting (`key_name`, `keys_text`),
//! default and player-overridden binding maps for ground/vehicle (`flat_bindings`,
//! `player_bindings`) and starfighter controls (`default_flight_bindings`,
//! `flight_bindings`), single-key exclusive rebinding (`rebind`), and row table
//! builders (`hotkey_rows`) for the `INFANTRY`, `VEHICLE`, and `STARFIGHTER` tabs
//! on the `CONTROL OPTIONS` menu page.

use std::collections::{BTreeMap, HashMap};

/// Control tab definition (`TABS` in `src/menus/hotkeys.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyTab {
    /// Internal tab identifier (`"infantry"`, `"vehicle"`, or `"starfighter"`).
    pub id: &'static str,
    /// Fallback uppercase English label shown on the tab button.
    pub label: &'static str,
    /// Localization string key looked up in `common.json` (`strings`).
    pub label_key: &'static str,
}

/// The three `CONTROL OPTIONS` binding tabs (`TABS` in `src/menus/hotkeys.js`).
///
/// The player switches between these tabs in `CONTROL OPTIONS` using the tab bar
/// or the Left/Right arrow keys while the bindings list is focused.
pub const TABS: [HotkeyTab; 3] = [
    // On-foot soldier movement, combat, view, and squad order bindings.
    HotkeyTab {
        id: "infantry",
        label: "INFANTRY",
        label_key: "ifs.controls.Infantry",
    },
    // Ground vehicle and walker movement, turret, and boarding bindings.
    HotkeyTab {
        id: "vehicle",
        label: "VEHICLE",
        label_key: "ifs.controls.Vehicle",
    },
    // Starfighter attitude, throttle, boost, weapon, and landing bindings.
    HotkeyTab {
        id: "starfighter",
        label: "STARFIGHTER",
        label_key: "ifs.controls.Flyer",
    },
];

/// Axis actions that are split into `<action>-` and `<action>+` in `flat_bindings()`.
///
/// Mirrors the two-sided directional axes in `src/input.js` (`AXES` / `WORLD_AXES`),
/// where negative and positive directions are bound as separate rows on the
/// `CONTROL OPTIONS` screen.
pub const WORLD_AXES: [&str; 4] = ["moveX", "moveY", "yaw", "pitch"];

/// Keys reserved for global shell/HUD shortcuts that cannot be rebound
/// (`RESERVED` in `src/input.js`).
///
/// Includes `Escape` (pause menu), `Tab` (player list), `KeyT` / `KeyY` (chat and
/// team chat), `F11` (fullscreen toggle), and `F12` (developer tools).
pub const RESERVED: [&str; 6] = ["Escape", "Tab", "KeyT", "KeyY", "F11", "F12"];

/// All 90 recognized keyboard and mouse input codes (`INPUT_CODES` in
/// `src/runtime/network-entity-input.js`, re-exported by `src/input.js`).
///
/// Only codes present in this list (and not in [`RESERVED`]) are accepted by
/// [`bindable`] when capturing a new key on the `CONTROL OPTIONS` page.
pub const INPUT_CODES: [&str; 90] = [
    // Alphabetic keys (A..Z).
    "KeyA",
    "KeyB",
    "KeyC",
    "KeyD",
    "KeyE",
    "KeyF",
    "KeyG",
    "KeyH",
    "KeyI",
    "KeyJ",
    "KeyK",
    "KeyL",
    "KeyM",
    "KeyN",
    "KeyO",
    "KeyP",
    "KeyQ",
    "KeyR",
    "KeyS",
    "KeyT",
    "KeyU",
    "KeyV",
    "KeyW",
    "KeyX",
    "KeyY",
    "KeyZ",
    // Top-row number keys (0..9).
    "Digit0",
    "Digit1",
    "Digit2",
    "Digit3",
    "Digit4",
    "Digit5",
    "Digit6",
    "Digit7",
    "Digit8",
    "Digit9",
    // Function keys (F1..F12).
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    // Arrow navigation keys.
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    // Whitespace, control, and navigation cluster keys.
    "Space",
    "Enter",
    "Escape",
    "Tab",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    // Left and right modifier keys plus Caps Lock.
    "ShiftLeft",
    "ShiftRight",
    "ControlLeft",
    "ControlRight",
    "AltLeft",
    "AltRight",
    "CapsLock",
    // Punctuation and symbol keys.
    "Minus",
    "Equal",
    "BracketLeft",
    "BracketRight",
    "Backslash",
    "Semicolon",
    "Quote",
    "Backquote",
    "Comma",
    "Period",
    "Slash",
    // Mouse buttons, scroll wheel directions, and look axes.
    "MouseLeft",
    "MouseRight",
    "MouseMiddle",
    "MouseBack",
    "MouseForward",
    "WheelUp",
    "WheelDown",
    "MouseX",
    "MouseY",
];

/// Returns true if `code` can be bound by the player (`bindable(code)` in `src/input.js`).
///
/// A code is bindable when it is a recognized network/input code in [`INPUT_CODES`]
/// and is not reserved for shell or HUD shortcuts in [`RESERVED`].
#[inline]
pub fn bindable(code: &str) -> bool {
    INPUT_CODES.contains(&code) && !RESERVED.contains(&code)
}

/// Converts a key/mouse code (`"KeyW"`, `"MouseLeft"`, `"WheelUp"`) into its uppercase
/// display label (`"W"`, `"MOUSE 1"`, `"MOUSE WHEEL UP"`), matching `keyName(code)` in
/// `src/menus/hotkeys.js`.
pub fn key_name(code: &str) -> String {
    // Empty input codes format to an empty string so `keys_text` can skip them.
    if code.is_empty() {
        return String::new();
    }
    // Single-letter `KeyA`..`KeyZ` codes strip the `"Key"` prefix.
    if let Some(letter) = code.strip_prefix("Key") {
        if letter.len() == 1 && letter.as_bytes()[0].is_ascii_uppercase() {
            return letter.to_string();
        }
    }
    // Top-row `Digit0`..`Digit9` codes strip the `"Digit"` prefix.
    if let Some(digit) = code.strip_prefix("Digit") {
        if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() {
            return digit.to_string();
        }
    }
    // Numeric keypad digits format as `"NUM 0"`..`"NUM 9"`.
    if let Some(num) = code.strip_prefix("Numpad") {
        if num.len() == 1 && num.as_bytes()[0].is_ascii_digit() {
            return format!("NUM {num}");
        }
    }
    match code {
        // Directional arrow keys.
        "ArrowUp" => "UP".to_string(),
        "ArrowDown" => "DOWN".to_string(),
        "ArrowLeft" => "LEFT".to_string(),
        "ArrowRight" => "RIGHT".to_string(),
        // Editing and navigation cluster keys.
        "Space" => "SPACE".to_string(),
        "Enter" => "ENTER".to_string(),
        "Backspace" => "BACKSPACE".to_string(),
        "Delete" => "DELETE".to_string(),
        "Insert" => "INSERT".to_string(),
        "Home" => "HOME".to_string(),
        "End" => "END".to_string(),
        "PageUp" => "PAGE UP".to_string(),
        "PageDown" => "PAGE DOWN".to_string(),
        // Modifier keys.
        "ShiftLeft" => "LEFT SHIFT".to_string(),
        "ShiftRight" => "RIGHT SHIFT".to_string(),
        "ControlLeft" => "LEFT CTRL".to_string(),
        "ControlRight" => "RIGHT CTRL".to_string(),
        "AltLeft" => "LEFT ALT".to_string(),
        "AltRight" => "RIGHT ALT".to_string(),
        "CapsLock" => "CAPS LOCK".to_string(),
        // Punctuation keys displayed as their literal symbol characters.
        "Minus" => "-".to_string(),
        "Equal" => "=".to_string(),
        "BracketLeft" => "[".to_string(),
        "BracketRight" => "]".to_string(),
        "Backslash" => "\\".to_string(),
        "Semicolon" => ";".to_string(),
        "Quote" => "'".to_string(),
        "Backquote" => "`".to_string(),
        "Comma" => ",".to_string(),
        "Period" => ".".to_string(),
        "Slash" => "/".to_string(),
        // Mouse buttons, wheel directions, and continuous look axes.
        "MouseLeft" => "MOUSE 1".to_string(),
        "MouseRight" => "MOUSE 2".to_string(),
        "MouseMiddle" => "MOUSE 3".to_string(),
        "MouseBack" => "MOUSE 4".to_string(),
        "MouseForward" => "MOUSE 5".to_string(),
        "WheelUp" => "MOUSE WHEEL UP".to_string(),
        "WheelDown" => "MOUSE WHEEL DOWN".to_string(),
        "MouseX" => "MOUSE X".to_string(),
        "MouseY" => "MOUSE Y".to_string(),
        // Any other code (such as `F1`..`F12`) is uppercased directly.
        other => other.to_ascii_uppercase(),
    }
}

/// Formats a list of bound codes as `"KEY1, KEY2"` or `"---"` when empty
/// (`keysText(codes)` in `src/menus/hotkeys.js`).
///
/// Empty key codes are ignored; when no valid bound keys remain, returns `"---"`
/// to indicate an unbound action row in the `CONTROL OPTIONS` list.
pub fn keys_text<S: AsRef<str>>(codes: &[S]) -> String {
    let mut parts = Vec::new();
    for c in codes {
        let name = key_name(c.as_ref());
        if !name.is_empty() {
            parts.push(name);
        }
    }
    if parts.is_empty() {
        "---".to_string()
    } else {
        parts.join(", ")
    }
}

/// Starfighter binding definition (`FLIGHT_BINDINGS` in `src/flight/controls.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlightBindingDef {
    /// Action key stored in the `flightBindings` setting map.
    pub action: &'static str,
    /// Human-readable action label displayed on the `STARFIGHTER` tab.
    pub label: &'static str,
    /// Default input codes bound to this starfighter action.
    pub keys: &'static [&'static str],
}

/// All 16 starfighter actions and their default keys (corresponding to
/// `FLIGHT_BINDINGS` in `src/flight/controls.js`).
pub const FLIGHT_BINDINGS: [FlightBindingDef; 16] = [
    // Pitch and roll attitude controls.
    FlightBindingDef {
        action: "pitchUp",
        label: "Pitch Up",
        keys: &["KeyS", "ArrowDown"],
    },
    FlightBindingDef {
        action: "pitchDown",
        label: "Pitch Down",
        keys: &["KeyW", "ArrowUp"],
    },
    FlightBindingDef {
        action: "rollLeft",
        label: "Roll Left",
        keys: &["KeyA", "ArrowLeft"],
    },
    FlightBindingDef {
        action: "rollRight",
        label: "Roll Right",
        keys: &["KeyD", "ArrowRight"],
    },
    // Rudder yaw controls.
    FlightBindingDef {
        action: "yawLeft",
        label: "Yaw Left",
        keys: &["KeyQ"],
    },
    FlightBindingDef {
        action: "yawRight",
        label: "Yaw Right",
        keys: &["KeyC"],
    },
    // Engine throttle, boost afterburner, and airbrake controls.
    FlightBindingDef {
        action: "throttleUp",
        label: "Throttle Up",
        keys: &["ShiftLeft", "ShiftRight"],
    },
    FlightBindingDef {
        action: "throttleDown",
        label: "Throttle Down",
        keys: &["ControlLeft", "ControlRight"],
    },
    FlightBindingDef {
        action: "boost",
        label: "Boost",
        keys: &["Space"],
    },
    FlightBindingDef {
        action: "airbrake",
        label: "Airbrake",
        keys: &["KeyX"],
    },
    // Starfighter weapon fire, secondary cycling, and reload controls.
    FlightBindingDef {
        action: "firePrimary",
        label: "Primary Weapon",
        keys: &["MouseLeft", "Enter"],
    },
    FlightBindingDef {
        action: "fireSecondary",
        label: "Secondary Weapon",
        keys: &["MouseRight"],
    },
    FlightBindingDef {
        action: "switchWeapon",
        label: "Switch Secondary",
        keys: &["KeyF"],
    },
    FlightBindingDef {
        action: "reload",
        label: "Reload",
        keys: &["KeyR"],
    },
    // Hangar landing gear and cockpit exit controls.
    FlightBindingDef {
        action: "landTakeoff",
        label: "Land / Take Off",
        keys: &["KeyL"],
    },
    FlightBindingDef {
        action: "exit",
        label: "Exit Craft",
        keys: &["KeyE"],
    },
];

/// Default flat bindings (`flatBindings()` in `src/input.js`), including split axis keys
/// (`moveX-`, `moveX+`, `moveY-`, `moveY+`, `yaw-`, `yaw+`, `pitch-`, `pitch+`),
/// discrete press actions, continuous hold actions, and squad orders.
pub const DEFAULT_FLAT_BINDINGS: [(&str, &[&str]); 37] = [
    // Split movement and keyboard turn/look axis directions.
    ("moveX-", &["KeyA", "ArrowLeft"]),
    ("moveX+", &["KeyD", "ArrowRight"]),
    ("moveY-", &["KeyW", "ArrowUp"]),
    ("moveY+", &["KeyS", "ArrowDown"]),
    ("yaw-", &[]),
    ("yaw+", &[]),
    ("pitch-", &[]),
    ("pitch+", &[]),
    // Stance and movement actions.
    ("jump", &["Space"]),
    ("crouch", &["KeyC"]),
    ("prone", &["KeyX", "ControlLeft"]),
    // Weapon reload and primary/secondary cycling.
    ("reload", &["KeyR"]),
    ("switchPrimary", &["KeyF", "MouseMiddle"]),
    ("switchSecondary", &["KeyQ", "ControlRight"]),
    ("nextPrimary", &["WheelUp"]),
    ("nextSecondary", &["WheelDown"]),
    // Vehicle boarding, camera view, tactical map, zoom, and target lock.
    ("enter", &["KeyE", "Enter"]),
    ("exit", &["KeyE"]),
    ("view", &["KeyV"]),
    ("dropFlag", &["KeyZ"]),
    ("map", &["KeyM"]),
    ("zoom", &["ShiftRight"]),
    ("target", &["KeyG"]),
    // Primary/secondary weapon triggers and sprint hold.
    ("fire", &["MouseLeft"]),
    ("secondary", &["MouseRight"]),
    ("sprint", &["ShiftLeft"]),
    // Function-key squad selection and quick radio calls.
    ("squad1", &["F1"]),
    ("squad2", &["F2"]),
    ("squad3", &["F3"]),
    ("squad4", &["F4"]),
    ("spot", &["F4"]),
    ("medic", &["F6"]),
    ("repair", &["F7"]),
    ("ammo", &["F8"]),
    ("pickup", &["F9"]),
    ("backup", &["F10"]),
    ("attack", &["Digit1"]),
];

/// Additional squad command bindings merged into [`flat_bindings`].
///
/// Covers `defend`, `follow`, `hold`, `getIn`, and `getOut` on `Digit2`..`Digit6`.
pub const EXTRA_DEFAULT_FLAT_BINDINGS: [(&str, &[&str]); 5] = [
    ("defend", &["Digit2"]),
    ("follow", &["Digit3"]),
    ("hold", &["Digit4"]),
    ("getIn", &["Digit5"]),
    ("getOut", &["Digit6"]),
];

/// Builds the complete default flat bindings map (`flatBindings()` in `src/input.js`).
///
/// Combines [`DEFAULT_FLAT_BINDINGS`] and [`EXTRA_DEFAULT_FLAT_BINDINGS`] into an
/// owned `BTreeMap` keyed by action name.
pub fn flat_bindings() -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for &(action, codes) in DEFAULT_FLAT_BINDINGS
        .iter()
        .chain(EXTRA_DEFAULT_FLAT_BINDINGS.iter())
    {
        out.insert(
            action.to_string(),
            codes.iter().map(|c| (*c).to_string()).collect(),
        );
    }
    out
}

/// Merges saved player bindings onto `flat_bindings()`, filtering through [`bindable`]
/// (`playerBindings(saved)` in `src/input.js`).
///
/// Unknown action keys in `saved` are ignored, and any unbindable or reserved key
/// codes inside a saved action's list are stripped out.
pub fn player_bindings(
    saved: Option<&BTreeMap<String, Vec<String>>>,
) -> BTreeMap<String, Vec<String>> {
    let mut out = flat_bindings();
    let Some(saved_map) = saved else {
        return out;
    };
    for (action, codes) in saved_map {
        if out.contains_key(action) {
            out.insert(
                action.clone(),
                codes
                    .iter()
                    .filter(|c| bindable(c.as_str()))
                    .cloned()
                    .collect(),
            );
        }
    }
    out
}

/// Builds the default starfighter bindings map from [`FLIGHT_BINDINGS`].
pub fn default_flight_bindings() -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for entry in FLIGHT_BINDINGS {
        out.insert(
            entry.action.to_string(),
            entry.keys.iter().map(|c| (*c).to_string()).collect(),
        );
    }
    out
}

/// Merges saved starfighter bindings onto [`default_flight_bindings`], filtering through [`bindable`]
/// (`flightBindings(saved)` in `src/flight/controls.js`).
pub fn flight_bindings(
    saved: Option<&BTreeMap<String, Vec<String>>>,
) -> BTreeMap<String, Vec<String>> {
    let mut out = default_flight_bindings();
    let Some(saved_map) = saved else {
        return out;
    };
    for entry in FLIGHT_BINDINGS {
        if let Some(codes) = saved_map.get(entry.action) {
            out.insert(
                entry.action.to_string(),
                codes
                    .iter()
                    .filter(|c| bindable(c.as_str()))
                    .cloned()
                    .collect(),
            );
        }
    }
    out
}

/// Rebinds `action` to `code` (`Some(code)` binds `[code]` and removes `code` from any other
/// action; `None` clears `action` to `[]`), returning only the diff against `defaults`
/// (`rebind(defaults, saved, action, code)` in `src/input.js`).
pub fn rebind(
    defaults: &BTreeMap<String, Vec<String>>,
    saved: Option<&BTreeMap<String, Vec<String>>>,
    action: &str,
    code: Option<&str>,
) -> BTreeMap<String, Vec<String>> {
    // Ignore unknown actions so stray identifiers never corrupt saved bindings.
    if !defaults.contains_key(action) {
        return saved.cloned().unwrap_or_default();
    }
    // Reject reserved or unrecognized key codes without mutating existing bindings.
    if let Some(c) = code {
        if !bindable(c) {
            return saved.cloned().unwrap_or_default();
        }
    }
    // Materialize the full current binding map by overlaying `saved` onto `defaults`.
    let mut merged: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, def_codes) in defaults {
        let cur = saved.and_then(|s| s.get(k)).unwrap_or(def_codes);
        merged.insert(
            k.clone(),
            cur.iter()
                .filter(|c| bindable(c.as_str()))
                .cloned()
                .collect(),
        );
    }
    // Enforce one-key-per-action exclusivity by removing `code` from all other actions.
    if let Some(c) = code {
        for (other, codes) in merged.iter_mut() {
            if other != action {
                codes.retain(|existing| existing != c);
            }
        }
        merged.insert(action.to_string(), vec![c.to_string()]);
    } else {
        // Passing `None` (e.g. Delete/Backspace in the key capture popup) clears the binding.
        merged.insert(action.to_string(), Vec::new());
    }

    // Return only the entries whose bound key list differs from `defaults`.
    let mut diff = BTreeMap::new();
    for (k, codes) in merged {
        let def_codes = defaults.get(&k).map(Vec::as_slice).unwrap_or(&[]);
        if codes.as_slice() != def_codes {
            diff.insert(k, codes);
        }
    }
    diff
}

/// Schema entry for one row in `INFANTRY_ROWS` or `VEHICLE_ROWS`
/// (`ROWS.infantry` and `ROWS.vehicle` in `src/menus/hotkeys.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyRowSpec {
    /// Mutable binding action key (`Some`), or `None` for read-only fixed-key rows.
    pub action: Option<&'static str>,
    /// Fixed input codes displayed when `action` is `None` (such as `MouseX` / `MouseY`).
    pub fixed: &'static [&'static str],
    /// Fallback uppercase English action label.
    pub label: &'static str,
    /// Optional localization key looked up in `common.json` (`strings`).
    pub label_key: Option<&'static str>,
}

impl HotkeyRowSpec {
    /// Constructs a rebindable row tied to `action` in the flat bindings table.
    const fn act(action: &'static str, label: &'static str, label_key: Option<&'static str>) -> Self {
        Self {
            action: Some(action),
            fixed: &[],
            label,
            label_key,
        }
    }

    /// Constructs a read-only row showing fixed hardware axes or keys (`action: None`).
    const fn fix(
        fixed: &'static [&'static str],
        label: &'static str,
        label_key: Option<&'static str>,
    ) -> Self {
        Self {
            action: None,
            fixed,
            label,
            label_key,
        }
    }
}

/// The 32 infantry binding rows (`ROWS.infantry` in `src/menus/hotkeys.js`).
pub const INFANTRY_ROWS: [HotkeyRowSpec; 32] = [
    // Primary and secondary weapon triggers and cycling.
    // Primary weapon trigger (`fire`, default `MouseLeft`).
    HotkeyRowSpec::act("fire", "PRIMARY ATTACK", Some("ifs.controls.Fire")),
    // Secondary weapon / grenade / mine trigger (`secondary`, default `MouseRight`).
    HotkeyRowSpec::act(
        "secondary",
        "SECONDARY ATTACK",
        Some("ifs.controls.Secondary"),
    ),
    // Cycle between carried primary weapons (`switchPrimary`, default `KeyF` / `MouseMiddle`).
    HotkeyRowSpec::act(
        "switchPrimary",
        "SWITCH PRIMARY WEAPON",
        Some("ifs.controls.Change"),
    ),
    // Scroll forward through primary weapons (`nextPrimary`, default `WheelUp`).
    HotkeyRowSpec::act(
        "nextPrimary",
        "NEXT PRIMARY WEAPON",
        Some("ifs.controls.Next"),
    ),
    // Cycle between carried secondary items (`switchSecondary`, default `KeyQ` / `ControlRight`).
    HotkeyRowSpec::act(
        "switchSecondary",
        "SWITCH SECONDARY",
        Some("ifs.controls.ChangeSecondary"),
    ),
    // Scroll forward through secondary items (`nextSecondary`, default `WheelDown`).
    HotkeyRowSpec::act(
        "nextSecondary",
        "NEXT SECONDARY",
        Some("ifs.controls.NextSecondary"),
    ),
    // Scope zoom, stances, sprint, reload, and vehicle entry.
    // Toggle weapon scope magnification (`zoom`, default `ShiftRight`).
    HotkeyRowSpec::act("zoom", "ZOOM", Some("ifs.controls.Zoom")),
    // Jump, combat roll (with strafe), or jetpack thrust in air (`jump`, default `Space`).
    HotkeyRowSpec::act("jump", "JUMP", Some("ifs.controls.Jump")),
    // Toggle crouched stance (`crouch`, default `KeyC`).
    HotkeyRowSpec::act("crouch", "CROUCH", Some("ifs.controls.Crouch")),
    // Toggle prone stance (`prone`, default `KeyX` / `ControlLeft`).
    HotkeyRowSpec::act("prone", "PRONE", Some("ifs.controls.Prone")),
    // Hold to sprint (`sprint`, default `ShiftLeft`).
    HotkeyRowSpec::act("sprint", "SPRINT", None),
    // Reload active weapon (`reload`, default `KeyR`).
    HotkeyRowSpec::act("reload", "RELOAD", Some("ifs.controls.Reload")),
    // Board vehicle, man turret, or interact with command post (`enter`, default `KeyE` / `Enter`).
    HotkeyRowSpec::act("enter", "USE / ENTER VEHICLE", Some("ifs.controls.Enter")),
    // Foot movement, keyboard turning/pitching, and fixed mouse look axes.
    // Negative Y movement axis (`moveY-`, default `KeyW` / `ArrowUp`).
    HotkeyRowSpec::act("moveY-", "MOVE FORWARD", Some("ifs.controls.Move_Up")),
    // Positive Y movement axis (`moveY+`, default `KeyS` / `ArrowDown`).
    HotkeyRowSpec::act("moveY+", "MOVE BACK", Some("ifs.controls.Move_Down")),
    // Negative X strafe axis (`moveX-`, default `KeyA` / `ArrowLeft`).
    HotkeyRowSpec::act("moveX-", "STRAFE LEFT", Some("ifs.controls.Move_Left")),
    // Positive X strafe axis (`moveX+`, default `KeyD` / `ArrowRight`).
    HotkeyRowSpec::act("moveX+", "STRAFE RIGHT", Some("ifs.controls.Move_Right")),
    // Optional keyboard yaw-left axis (`yaw-`, unbound by default).
    HotkeyRowSpec::act("yaw-", "TURN LEFT", None),
    // Optional keyboard yaw-right axis (`yaw+`, unbound by default).
    HotkeyRowSpec::act("yaw+", "TURN RIGHT", None),
    // Optional keyboard pitch-up axis (`pitch-`, unbound by default).
    HotkeyRowSpec::act("pitch-", "LOOK UP", None),
    // Optional keyboard pitch-down axis (`pitch+`, unbound by default).
    HotkeyRowSpec::act("pitch+", "LOOK DOWN", None),
    // Read-only horizontal mouse look axis (`MouseX`).
    HotkeyRowSpec::fix(&["MouseX"], "LOOK HORIZONTAL", None),
    // Read-only vertical mouse look axis (`MouseY`).
    HotkeyRowSpec::fix(&["MouseY"], "LOOK VERTICAL", None),
    // Camera toggle, map overlay, flag drop, target lock, and squad orders.
    HotkeyRowSpec::act(
        "view",
        "FIRST / THIRD PERSON VIEW",
        Some("ifs.controls.View"),
    ),
    // Toggle tactical overview map (`map`, default `KeyM`).
    HotkeyRowSpec::act("map", "VIEW MAP", Some("ifs.controls.ViewMap")),
    // Drop carried objective item or flag (`dropFlag`, default `KeyZ`).
    HotkeyRowSpec::act("dropFlag", "DROP ITEM", None),
    // Lock onto highlighted enemy target (`target`, default `KeyG`).
    HotkeyRowSpec::act("target", " LOCK TARGET", Some("ifs.controls.TargetLock")),
    // Squad command: attack target (`attack`, default `Digit1`).
    HotkeyRowSpec::act("attack", "ORDER: ATTACK", None),
    // Squad command: defend position (`defend`, default `Digit2`).
    HotkeyRowSpec::act("defend", "ORDER: DEFEND", None),
    // Squad command: follow player (`follow`, default `Digit3`).
    HotkeyRowSpec::act("follow", "ORDER: FOLLOW", None),
    // Squad command: hold position (`hold`, default `Digit4`).
    HotkeyRowSpec::act("hold", "ORDER: HOLD", None),
    // Radio call for nearby friendly reinforcements (`backup`, default `F10`).
    HotkeyRowSpec::act("backup", "CALL FOR BACKUP", None),
];

/// The 19 vehicle binding rows (`ROWS.vehicle` in `src/menus/hotkeys.js`).
pub const VEHICLE_ROWS: [HotkeyRowSpec; 19] = [
    // Vehicle weapon triggers, boost, boarding exit, and landing toggle.
    HotkeyRowSpec::act("fire", "PRIMARY ATTACK", Some("ifs.controls.Fire")),
    HotkeyRowSpec::act(
        "secondary",
        "SECONDARY ATTACK",
        Some("ifs.controls.Secondary"),
    ),
    // Speeder/vehicle boost (`jump`, default `Space`).
    HotkeyRowSpec::act("jump", "BOOST", None),
    // Disembark from current vehicle or turret (`exit`, default `KeyE`).
    HotkeyRowSpec::act("exit", "EXIT VEHICLE", Some("ifs.controls.Exit")),
    // Toggle transport landing gear (`landTakeoff`, default `KeyL`).
    HotkeyRowSpec::act("landTakeoff", "LAND / TAKE OFF", None),
    // Throttle, strafe/roll, steering, pitch, and fixed mouse turret axes.
    // Forward drive / throttle increase (`moveY-`, default `KeyW` / `ArrowUp`).
    HotkeyRowSpec::act("moveY-", "THROTTLE UP", None),
    // Reverse drive / throttle decrease (`moveY+`, default `KeyS` / `ArrowDown`).
    HotkeyRowSpec::act("moveY+", "THROTTLE DOWN", None),
    // Hovercraft strafe left or banking roll left (`moveX-`, default `KeyA` / `ArrowLeft`).
    HotkeyRowSpec::act("moveX-", "STRAFE / ROLL LEFT", None),
    // Hovercraft strafe right or banking roll right (`moveX+`, default `KeyD` / `ArrowRight`).
    HotkeyRowSpec::act("moveX+", "STRAFE / ROLL RIGHT", None),
    // Keyboard steering left (`yaw-`).
    HotkeyRowSpec::act("yaw-", "STEER LEFT", None),
    // Keyboard steering right (`yaw+`).
    HotkeyRowSpec::act("yaw+", "STEER RIGHT", None),
    // Keyboard turret/nose pitch up (`pitch-`).
    HotkeyRowSpec::act("pitch-", "PITCH UP", None),
    // Keyboard turret/nose pitch down (`pitch+`).
    HotkeyRowSpec::act("pitch+", "PITCH DOWN", None),
    // Read-only horizontal turret/craft aim axis (`MouseX`).
    HotkeyRowSpec::fix(&["MouseX"], "TURN HORIZONTAL", None),
    // Read-only vertical turret/craft aim axis (`MouseY`).
    HotkeyRowSpec::fix(&["MouseY"], "PITCH VERTICAL", None),
    // Camera perspective, target lock, and passenger boarding orders.
    // Toggle between cockpit and exterior chase camera (`view`, default `KeyV`).
    HotkeyRowSpec::act(
        "view",
        "FIRST / THIRD PERSON VIEW",
        Some("ifs.controls.View"),
    ),
    // Lock onto enemy vehicle target (`target`, default `KeyG`).
    HotkeyRowSpec::act("target", "LOCK TARGET", Some("ifs.controls.TargetLock")),
    // Order friendly bots to board vehicle (`getIn`, default `Digit5`).
    HotkeyRowSpec::act("getIn", "ORDER: GET IN", None),
    // Order friendly bots to disembark vehicle (`getOut`, default `Digit6`).
    HotkeyRowSpec::act("getOut", "ORDER: GET OUT", None),
];

/// Returns the active key codes for `row` (`bindingCodes(row, bindings)` in `src/menus/hotkeys.js`).
///
/// Fixed rows return their static `row.fixed` slice; rebindable rows look up
/// `row.action` in `bindings`.
pub fn binding_codes(
    row: &HotkeyRowSpec,
    bindings: &BTreeMap<String, Vec<String>>,
) -> Vec<String> {
    if !row.fixed.is_empty() {
        return row.fixed.iter().map(|s| (*s).to_string()).collect();
    }
    if let Some(act) = row.action {
        if let Some(codes) = bindings.get(act) {
            return codes.clone();
        }
    }
    Vec::new()
}

/// Formatted hotkey row returned by [`hotkey_rows`] (`hotkeyRows` in `src/menus/hotkeys.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyRow {
    /// Action identifier for rebinding (`None` when the row is a fixed read-only axis).
    pub action: Option<String>,
    /// Localized uppercase action label displayed in the left column.
    pub label: String,
    /// Comma-separated uppercase key names (`"W, UP"`) or `"---"` displayed in the right column.
    pub keys: String,
    /// True when the row cannot be rebound (`action.is_none()`).
    pub disabled: bool,
}

/// Builds the display rows for `tab` (`"infantry"`, `"vehicle"`, `"starfighter"`)
/// (`hotkeyRows(tab, bindings, strings)` in `src/menus/hotkeys.js`).
pub fn hotkey_rows(
    tab: &str,
    bindings: &BTreeMap<String, Vec<String>>,
    strings: &HashMap<String, String>,
) -> Vec<HotkeyRow> {
    // Resolve localized string keys and collapse embedded line breaks to single spaces.
    let say = |key: Option<&str>, fallback: &str| -> String {
        let raw = key
            .and_then(|k| strings.get(k))
            .map(String::as_str)
            .unwrap_or(fallback);
        raw.replace("\r\n", " ").replace('\n', " ").trim().to_string()
    };

    // Starfighter rows come from `FLIGHT_BINDINGS` with uppercase labels and all rows rebindable.
    if tab == "starfighter" {
        return FLIGHT_BINDINGS
            .iter()
            .map(|entry| {
                let codes = bindings
                    .get(entry.action)
                    .cloned()
                    .unwrap_or_else(|| entry.keys.iter().map(|s| (*s).to_string()).collect());
                HotkeyRow {
                    action: Some(entry.action.to_string()),
                    label: entry.label.to_ascii_uppercase(),
                    keys: keys_text(&codes),
                    disabled: false,
                }
            })
            .collect();
    }

    // Infantry and vehicle rows read from their static `HotkeyRowSpec` tables.
    let source: &[HotkeyRowSpec] = if tab == "vehicle" {
        &VEHICLE_ROWS
    } else {
        &INFANTRY_ROWS
    };
    source
        .iter()
        .map(|row| {
            let codes = binding_codes(row, bindings);
            HotkeyRow {
                action: row.action.map(str::to_string),
                label: say(row.label_key, row.label),
                keys: keys_text(&codes),
                disabled: row.action.is_none(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verifies tab counts, row counts, key label formatting, reserved keys, and exclusive rebinding.
    #[test]
    fn hotkeys_and_bindings_match_js() {
        assert_eq!(TABS.len(), 3);
        assert_eq!(INFANTRY_ROWS.len(), 32);
        assert_eq!(VEHICLE_ROWS.len(), 19);
        assert_eq!(FLIGHT_BINDINGS.len(), 16);

        assert_eq!(key_name("KeyW"), "W");
        assert_eq!(key_name("Digit4"), "4");
        assert_eq!(key_name("Numpad7"), "NUM 7");
        assert_eq!(key_name("MouseLeft"), "MOUSE 1");
        assert_eq!(key_name("WheelUp"), "MOUSE WHEEL UP");
        assert_eq!(keys_text(&["KeyW", "ArrowUp"]), "W, UP");
        assert_eq!(keys_text::<&str>(&[]), "---");

        assert!(bindable("KeyW"));
        assert!(!bindable("Escape"));
        assert!(!bindable("Tab"));
        assert!(!bindable("F12"));

        let defaults = flat_bindings();
        let diff = rebind(&defaults, None, "jump", Some("KeyJ"));
        assert_eq!(diff.get("jump"), Some(&vec!["KeyJ".to_string()]));
    }
}
