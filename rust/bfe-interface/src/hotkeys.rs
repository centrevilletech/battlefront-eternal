//! Control tabs, default bindings, key label formatting, and exclusive rebinding
//! (`src/menus/hotkeys.js`, `src/input.js`, `src/flight/controls.js`,
//! `src/runtime/network-entity-input.js`).
//!
//! Ports:
//! - [`TABS`]: the three control tabs (`infantry`, `vehicle`, `starfighter`).
//! - [`INFANTRY_ROWS`] (32 rows), [`VEHICLE_ROWS`] (19 rows), and [`FLIGHT_BINDINGS`] (16 rows).
//! - [`INPUT_CODES`] and [`RESERVED`] (`bindable(code)`).
//! - [`key_name`] and [`keys_text`] for formatting key codes into uppercase Battlefront UI labels.
//! - [`flat_bindings`], [`player_bindings`], [`default_flight_bindings`], [`flight_bindings`],
//!   and [`rebind`] for exclusive key reassignment (`rebind` removes `code` from any other
//!   action in the same table and returns only the diff from `defaults`).

use crate::options::say;
use std::collections::{BTreeMap, HashMap};

/// Metadata for one of the three `CONTROL OPTIONS` tabs (`TABS` in `src/menus/hotkeys.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyTab {
    /// Internal tab identifier (`"infantry"`, `"vehicle"`, or `"starfighter"`).
    pub id: &'static str,
    /// Localization string key (`"ifs.Controls.SoldierTab"`, etc.).
    pub label_key: &'static str,
    /// Fallback English tab label (`"INFANTRY"`, `"VEHICLES"`, `"STARFIGHTERS"`).
    pub label: &'static str,
}

/// The three `CONTROL OPTIONS` tabs (`TABS` in `src/menus/hotkeys.js`).
pub const TABS: [HotkeyTab; 3] = [
    // Ground soldier controls (`INFANTRY`).
    HotkeyTab {
        id: "infantry",
        label_key: "ifs.Controls.SoldierTab",
        label: "INFANTRY",
    },
    // Ground and walker vehicle controls (`VEHICLES`).
    HotkeyTab {
        id: "vehicle",
        label_key: "ifs.Controls.VehicleTab",
        label: "VEHICLE",
    },
    // Starfighter and gunship flight controls (`STARFIGHTERS`).
    HotkeyTab {
        id: "starfighter",
        label_key: "ifs.Controls.FlyerTab",
        label: "STARFIGHTER",
    },
];

/// Fixed mouse-axis movement bindings (`WORLD_AXES` in `src/menus/hotkeys.js`),
/// displayed as read-only rows (`disabled: true`) in the infantry and vehicle tables.
pub const WORLD_AXES: [(&str, &[&str]); 6] = [
    ("Look Up", &["MouseY"]),
    ("Look Down", &["MouseY"]),
    ("Turn Left", &["MouseX"]),
    ("Turn Right", &["MouseX"]),
    ("Pitch Up", &["MouseY"]),
    ("Pitch Down", &["MouseY"]),
];

/// Reserved keys that cannot be bound to gameplay actions (`RESERVED` in `src/input.js`).
///
/// `Escape` opens the pause menu, `Tab` opens the scoreboard, `F11` toggles fullscreen,
/// `T`/`Y` open chat, and `F12` is reserved for screenshots/dev tools.
pub const RESERVED: [&str; 6] = ["Escape", "Tab", "KeyT", "KeyY", "F11", "F12"];

/// Canonical set of 104 bindable DOM `KeyboardEvent.code` and mouse/wheel tokens
/// (`INPUT_CODES` in `src/runtime/network-entity-input.js` and `BINDABLE` in `src/input.js`).
#[path = "input_codes.rs"]
mod input_codes;
pub use input_codes::INPUT_CODES;

/// Returns `true` if `code` is in [`INPUT_CODES`] and not in [`RESERVED`]
/// (`bindable(code)` in `src/input.js`).
pub fn bindable(code: &str) -> bool {
    // Reject reserved UI keys and chat keys first, then verify
    // that `code` appears in the canonical `INPUT_CODES` allowlist.
    !RESERVED.contains(&code) && INPUT_CODES.contains(&code)
}

/// Converts a raw input code (`"KeyW"`, `"Digit1"`, `"Mouse0"`, `"WheelUp"`) into its
/// uppercase Battlefront UI display string (`keyName(code)` in `src/menus/hotkeys.js`).
pub fn key_name(code: &str) -> String {
    // Strip `"Key"` prefix for letter keys (`"KeyA"` -> `"A"`).
    if let Some(letter) = code.strip_prefix("Key") {
        if letter.len() == 1 {
            return letter.to_string();
        }
    }
    // Strip `"Digit"` prefix for top-row numbers (`"Digit1"` -> `"1"`).
    if let Some(digit) = code.strip_prefix("Digit") {
        if digit.len() == 1 {
            return digit.to_string();
        }
    }
    // Format numpad digits as `"KP_INSERT"` (`Numpad0`) or `"KP_1"`..`"KP_9"` (`src/menus/hotkeys.js`).
    if let Some(numpad) = code.strip_prefix("Numpad") {
        if numpad.len() == 1 && numpad.chars().all(|c| c.is_ascii_digit()) {
            return if numpad == "0" {
                "KP_INSERT".to_string()
            } else {
                format!("KP_{numpad}")
            };
        }
    }
    // Map named special keys, modifiers, punctuation, mouse buttons, and wheel axes (`NAMES` in `src/menus/hotkeys.js`).
    match code {
        // Whitespace & Enter/Escape
        "Space" => "SPACE".to_string(),
        "Enter" => "RETURN".to_string(),
        "NumpadEnter" => "KP_ENTER".to_string(),
        "Escape" => "ESC".to_string(),
        "Tab" => "TAB".to_string(),
        // Left/Right modifier keys collapse to `"SHIFT"`, `"CONTROL"`, `"ALT"` (`src/menus/hotkeys.js`).
        "ShiftLeft" | "ShiftRight" => "SHIFT".to_string(),
        "ControlLeft" | "ControlRight" => "CONTROL".to_string(),
        "AltLeft" | "AltRight" => "ALT".to_string(),
        // Lock & editing keys
        "Backquote" => "`".to_string(),
        "Backspace" => "BACKSPACE".to_string(),
        "Delete" => "DELETE".to_string(),
        "Insert" => "INSERT".to_string(),
        "Home" => "HOME".to_string(),
        "End" => "END".to_string(),
        "PageUp" => "PAGE UP".to_string(),
        "PageDown" => "PAGE DOWN".to_string(),
        // Arrow keys
        "ArrowUp" => "UP".to_string(),
        "ArrowDown" => "DOWN".to_string(),
        "ArrowLeft" => "LEFT".to_string(),
        "ArrowRight" => "RIGHT".to_string(),
        // Punctuation keys
        "Minus" => "-".to_string(),
        "Equal" => "=".to_string(),
        "BracketLeft" => "[".to_string(),
        "BracketRight" => "]".to_string(),
        "Backslash" => "\\".to_string(),
        "Semicolon" => ";".to_string(),
        "Quote" => "'".to_string(),
        "Comma" => ",".to_string(),
        "Period" => ".".to_string(),
        "Slash" => "/".to_string(),
        // Mouse buttons, wheel, and fixed mouse look axes (`NAMES` in `src/menus/hotkeys.js`)
        "Mouse0" | "MouseLeft" => "MOUSE BUTTON 1".to_string(),
        "Mouse2" | "MouseRight" => "MOUSE BUTTON 2".to_string(),
        "Mouse1" | "MouseMiddle" => "MOUSE BUTTON 3".to_string(),
        "Mouse3" | "MouseBack" => "MOUSE BUTTON 4".to_string(),
        "Mouse4" | "MouseForward" => "MOUSE BUTTON 5".to_string(),
        "WheelUp" => "MOUSE WHEEL UP".to_string(),
        "WheelDown" => "MOUSE WHEEL DOWN".to_string(),
        // Fallback: uppercase the raw code (`F1`..`F12`, etc.).
        other => other.to_ascii_uppercase(),
    }
}

/// Formats a slice of key codes as a comma-separated display string
/// (`keysText(codes)` in `src/menus/hotkeys.js`).
///
/// Deduplicates mapped key names while preserving order (`[...new Set(codes.map(keyName))].join(", ")`),
/// and returns `""` when `codes` is empty.
pub fn keys_text<S: AsRef<str>>(codes: &[S]) -> String {
    if codes.is_empty() {
        return String::new();
    }
    let mut unique: Vec<String> = Vec::with_capacity(codes.len());
    for c in codes {
        let name = key_name(c.as_ref());
        if !unique.contains(&name) {
            unique.push(name);
        }
    }
    unique.join(", ")
}

/// Starfighter binding definition (`FLIGHT_BINDINGS` in `src/flight/controls.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlightBindingDef {
    /// Action identifier in `flightBindings` (`"throttle"`, `"brake"`, `"rollLeft"`, etc.).
    pub id: &'static str,
    /// Localization string key (`"ifs.controls.General.ThrottleUp"`, etc.).
    pub label_key: &'static str,
    /// Fallback English label (`"Throttle"`, `"Brake"`, etc.).
    pub label: &'static str,
    /// Default key codes bound to this flight action.
    pub keys: &'static [&'static str],
}

/// All 16 starfighter control rows and their default bindings
/// (`FLIGHT_BINDINGS` in `src/flight/controls.js`).
pub const FLIGHT_BINDINGS: [FlightBindingDef; 16] = [
    // Increase starfighter engine throttle (`throttle`).
    FlightBindingDef {
        id: "throttle",
        label_key: "ifs.controls.General.ThrottleUp",
        label: "Throttle",
        keys: &["KeyW"],
    },
    // Decrease starfighter engine throttle / brake (`brake`).
    FlightBindingDef {
        id: "brake",
        label_key: "ifs.controls.General.ThrottleDown",
        label: "Brake",
        keys: &["KeyS"],
    },
    // Roll starfighter counter-clockwise (`rollLeft`).
    FlightBindingDef {
        id: "rollLeft",
        label_key: "ifs.controls.Flyer.RollLeft",
        label: "Roll Left",
        keys: &["KeyA"],
    },
    // Roll starfighter clockwise (`rollRight`).
    FlightBindingDef {
        id: "rollRight",
        label_key: "ifs.controls.Flyer.RollRight",
        label: "Roll Right",
        keys: &["KeyD"],
    },
    // Engage afterburner boost (`boost`).
    FlightBindingDef {
        id: "boost",
        label_key: "ifs.controls.Flyer.Boost",
        label: "Boost",
        keys: &["ShiftLeft", "ShiftRight"],
    },
    // Perform evasive roll / flip maneuver (`trick`).
    FlightBindingDef {
        id: "trick",
        label_key: "ifs.controls.Flyer.Trick",
        label: "Trick",
        keys: &["AltLeft", "ControlRight"],
    },
    // Perform Immelmann turn (`immelmann`).
    FlightBindingDef {
        id: "immelmann",
        label_key: "ifs.controls.Flyer.Immelmann",
        label: "Immelmann",
        keys: &["CapsLock"],
    },
    // Hold roll lock (`rollLock`).
    FlightBindingDef {
        id: "rollLock",
        label_key: "ifs.controls.Flyer.RollLock",
        label: "Roll Lock",
        keys: &["KeyZ", "Mouse1"],
    },
    // Vertical takeoff or landing in hangar (`takeoff`).
    FlightBindingDef {
        id: "takeoff",
        label_key: "ifs.controls.General.LandTakeoff",
        label: "Take Off / Land",
        keys: &["Space"],
    },
    // Lock onto enemy target (`targetLock`).
    FlightBindingDef {
        id: "targetLock",
        label_key: "ifs.controls.Flyer.TargetLock",
        label: "Target Lock",
        keys: &["KeyQ", "Mouse3"],
    },
    // Fire primary laser cannons (`firePrimary`).
    FlightBindingDef {
        id: "firePrimary",
        label_key: "ifs.controls.General.Primary",
        label: "Fire Primary",
        keys: &["Mouse0"],
    },
    // Fire proton torpedoes / concussion missiles (`fireSecondary`).
    FlightBindingDef {
        id: "fireSecondary",
        label_key: "ifs.controls.General.Secondary",
        label: "Fire Secondary",
        keys: &["Mouse2"],
    },
    // Toggle 1st-person cockpit / 3rd-person chase camera (`perspective`).
    FlightBindingDef {
        id: "perspective",
        label_key: "ifs.controls.General.FirstThirdPerson",
        label: "Perspective",
        keys: &["KeyV"],
    },
    // Enter or exit landed starfighter (`use`).
    FlightBindingDef {
        id: "use",
        label_key: "ifs.controls.General.Action",
        label: "Use",
        keys: &["KeyE"],
    },
    // Cycle to next primary weapon (`nextPrimary`).
    FlightBindingDef {
        id: "nextPrimary",
        label_key: "ifs.controls.Soldier.NextPrimary",
        label: "Next Primary",
        keys: &["Digit1", "WheelUp"],
    },
    // Cycle to previous primary weapon (`previousPrimary`).
    FlightBindingDef {
        id: "previousPrimary",
        label_key: "ifs.controls.Soldier.PrevPrimary",
        label: "Previous Primary",
        keys: &["Digit2", "WheelDown"],
    },
];

/// Default flat ground/vehicle bindings (`flatBindings(DEFAULT_BINDINGS)` in `src/input.js`).
///
/// Contains all 24 canonical `axes.*`, `presses.*`, and `holds.*` entries from `DEFAULT_BINDINGS`
/// in `src/input.js`.
pub const DEFAULT_FLAT_BINDINGS: [(&str, &[&str]); 24] = [
    // Movement axes (`DEFAULT_BINDINGS.axes`)
    ("axes.forward+", &["KeyW", "ArrowUp"]),
    ("axes.forward-", &["KeyS", "ArrowDown"]),
    ("axes.right+", &["KeyD", "ArrowRight"]),
    ("axes.right-", &["KeyA", "ArrowLeft"]),
    ("axes.turn+", &[]),
    ("axes.turn-", &[]),
    // Discrete press actions (`DEFAULT_BINDINGS.presses`)
    ("presses.jump", &["Space"]),
    ("presses.crouch", &["KeyC", "ControlLeft", "ControlRight"]),
    ("presses.prone", &["KeyX"]),
    ("presses.reload", &["KeyR", "Enter"]),
    ("presses.use", &["KeyE", "Numpad0"]),
    ("presses.nextPrimary", &["KeyF", "WheelUp"]),
    ("presses.previousPrimary", &["WheelDown"]),
    ("presses.nextSecondary", &["KeyG"]),
    ("presses.zoom", &["KeyZ", "Mouse1"]),
    ("presses.perspective", &["KeyQ", "Mouse3"]),
    ("presses.map", &["KeyM"]),
    ("presses.stats", &["Backquote"]),
    ("presses.squadMoveOut", &["F3"]),
    ("presses.squadHold", &["F4"]),
    ("presses.squadFollow", &["F2"]),
    ("presses.squadAtEase", &["F1"]),
    // Continuous hold actions (`DEFAULT_BINDINGS.holds`)
    ("holds.firePrimary", &["Mouse0"]),
    ("holds.fireSecondary", &["Mouse2", "AltLeft", "AltRight"]),
];

/// Additional default flat bindings (empty in `src/input.js`; retained as a const slice for API compatibility).
pub const EXTRA_DEFAULT_FLAT_BINDINGS: [(&str, &[&str]); 0] = [];

/// Builds the complete default ground/vehicle bindings map (`flatBindings()` in `src/input.js`).
///
/// Converts [`DEFAULT_FLAT_BINDINGS`] into an owned `BTreeMap<String, Vec<String>>`
/// used as the baseline for [`player_bindings`] and [`rebind`].
pub fn flat_bindings() -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    for &(action, codes) in DEFAULT_FLAT_BINDINGS
        .iter()
        .chain(EXTRA_DEFAULT_FLAT_BINDINGS.iter())
    {
        map.insert(
            action.to_string(),
            codes.iter().map(|s| (*s).to_string()).collect(),
        );
    }
    map
}

/// Merges saved player bindings onto `flat_bindings()`, filtering through [`bindable`]
/// (`playerBindings(saved)` in `src/input.js`).
///
/// Only keys present in [`flat_bindings`] are overridden, and any reserved or unknown
/// key codes in `saved` are stripped out.
pub fn player_bindings(
    saved: Option<&BTreeMap<String, Vec<String>>>,
) -> BTreeMap<String, Vec<String>> {
    let mut out = flat_bindings();
    let Some(saved) = saved else {
        return out;
    };
    // Overlay each known action's saved key list after filtering through `bindable(code)`.
    for (action, default_codes) in &mut out {
        if let Some(list) = saved.get(action) {
            *default_codes = list
                .iter()
                .filter(|c| bindable(c.as_str()))
                .cloned()
                .collect();
        }
    }
    out
}

/// Builds the default starfighter bindings map from [`FLIGHT_BINDINGS`]
/// (`defaultFlightBindings()` in `src/flight/controls.js`).
pub fn default_flight_bindings() -> BTreeMap<String, Vec<String>> {
    let mut map = BTreeMap::new();
    for entry in FLIGHT_BINDINGS {
        map.insert(
            entry.id.to_string(),
            entry.keys.iter().map(|s| (*s).to_string()).collect(),
        );
    }
    map
}

/// Merges saved starfighter bindings onto [`default_flight_bindings`], filtering through [`bindable`]
/// (`flightBindings(saved)` in `src/flight/controls.js`).
pub fn flight_bindings(
    saved: Option<&BTreeMap<String, Vec<String>>>,
) -> BTreeMap<String, Vec<String>> {
    let mut out = default_flight_bindings();
    let Some(saved) = saved else {
        return out;
    };
    for entry in FLIGHT_BINDINGS {
        if let Some(list) = saved.get(entry.id) {
            out.insert(
                entry.id.to_string(),
                list.iter()
                    .filter(|c| bindable(c.as_str()))
                    .cloned()
                    .collect(),
            );
        }
    }
    out
}

/// Rebinds `action` to `code` (or clears `action` when `code` is `None`), removing `code`
/// from any other action in the table and returning only the entries that differ from `defaults`
/// (`rebind(defaults, saved, action, code)` in `src/input.js`).
pub fn rebind(
    defaults: &BTreeMap<String, Vec<String>>,
    saved: Option<&BTreeMap<String, Vec<String>>>,
    action: &str,
    code: Option<&str>,
) -> BTreeMap<String, Vec<String>> {
    // Reject unknown actions or non-bindable key codes without modifying the saved diff.
    if !defaults.contains_key(action) {
        return saved.cloned().unwrap_or_default();
    }
    if let Some(c) = code {
        if !bindable(c) {
            return saved.cloned().unwrap_or_default();
        }
    }

    // 1. Reconstruct the full current binding table by overlaying `saved` onto `defaults`.
    let mut full: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, def_list) in defaults {
        let cur = saved.and_then(|s| s.get(k)).unwrap_or(def_list);
        let filtered: Vec<String> = cur
            .iter()
            .filter(|c| bindable(c.as_str()))
            .cloned()
            .collect();
        full.insert(k.clone(), filtered);
    }

    // 2. Remove `code` from every action so a key is never bound to two actions at once.
    if let Some(c) = code {
        for list in full.values_mut() {
            list.retain(|item| item != c);
        }
    }

    // 3. Assign `[code]` (or `[]` when clearing) to the target `action`.
    let assigned = match code {
        Some(c) => vec![c.to_string()],
        None => Vec::new(),
    };
    full.insert(action.to_string(), assigned);

    // 4. Return only the sparse diff of actions whose key lists differ from `defaults`.
    let mut diff = BTreeMap::new();
    for (k, def_list) in defaults {
        if let Some(cur_list) = full.get(k) {
            if cur_list != def_list {
                diff.insert(k.clone(), cur_list.clone());
            }
        }
    }
    diff
}

/// Schema entry for one row in `INFANTRY_ROWS` or `VEHICLE_ROWS`
/// (`ROWS.infantry` and `ROWS.vehicle` in `src/menus/hotkeys.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyRowSpec {
    /// Action key in [`flat_bindings`], or `None` for fixed read-only rows (`Mouse X-Axis`, `ESC`, etc.).
    pub action: Option<&'static str>,
    /// Localization key in `common.json` (`"ifs.controls.General.Forward"`, etc.).
    pub label_key: &'static str,
    /// Fallback English label (`"Move Forward"`, `"Look Up"`, etc.).
    pub label: &'static str,
    /// Fixed key codes or literal display string for read-only rows (`action.is_none()`).
    pub fixed: &'static [&'static str],
}

impl HotkeyRowSpec {
    /// Constructs a rebindable row tied to `action` in the flat bindings table.
    pub const fn rebindable(
        action: &'static str,
        label_key: &'static str,
        label: &'static str,
    ) -> Self {
        Self {
            action: Some(action),
            label_key,
            label,
            fixed: &[],
        }
    }

    /// Constructs a fixed (read-only) row displaying `fixed` key codes or text.
    pub const fn fixed(
        label_key: &'static str,
        label: &'static str,
        fixed: &'static [&'static str],
    ) -> Self {
        Self {
            action: None,
            label_key,
            label,
            fixed,
        }
    }
}

/// The 32 infantry binding rows (`ROWS.infantry` in `src/menus/hotkeys.js`).
pub const INFANTRY_ROWS: [HotkeyRowSpec; 32] = [
    HotkeyRowSpec::rebindable(
        "holds.firePrimary",
        "ifs.controls.General.Primary",
        "PRIMARY FIRE",
    ),
    HotkeyRowSpec::rebindable(
        "holds.fireSecondary",
        "bfe.controls.GrenadeSecondaryFire",
        "GRENADE / SECONDARY FIRE",
    ),
    HotkeyRowSpec::rebindable(
        "presses.nextSecondary",
        "ifs.controls.General.SecondaryNext",
        "SECONDARY NEXT",
    ),
    HotkeyRowSpec::rebindable(
        "presses.nextPrimary",
        "ifs.controls.General.PrimaryNext",
        "PRIMARY NEXT",
    ),
    HotkeyRowSpec::rebindable("presses.previousPrimary", "", "PRIMARY PREVIOUS"),
    HotkeyRowSpec::rebindable("presses.jump", "ifs.controls.General.Jump", "JUMP"),
    HotkeyRowSpec::fixed("", "DIVE", &["SPACE WITH A OR D"]),
    HotkeyRowSpec::fixed("", "JETPACK", &["SPACE IN THE AIR"]),
    HotkeyRowSpec::rebindable("presses.crouch", "ifs.controls.General.Crouch", "CROUCH"),
    HotkeyRowSpec::rebindable("presses.prone", "", "PRONE"),
    HotkeyRowSpec::rebindable("presses.zoom", "ifs.controls.General.Zoom", "ZOOM"),
    HotkeyRowSpec::rebindable(
        "presses.perspective",
        "ifs.controls.General.FirstThirdPerson",
        "1ST/3RD VIEW",
    ),
    HotkeyRowSpec::rebindable("presses.reload", "ifs.controls.General.Reload", "RELOAD"),
    HotkeyRowSpec::rebindable("presses.use", "", "ENTER/EXIT"),
    HotkeyRowSpec::rebindable("presses.squadMoveOut", "", "SQUAD CMD-MOVE OUT"),
    HotkeyRowSpec::rebindable("presses.squadHold", "", "SQUAD CMD-HOLD"),
    HotkeyRowSpec::rebindable("presses.squadFollow", "", "SQUAD CMD-FOLLOW"),
    HotkeyRowSpec::rebindable("presses.squadAtEase", "", "SQUAD CMD-AT EASE"),
    HotkeyRowSpec::rebindable("axes.forward+", "", "MOVE FORWARD"),
    HotkeyRowSpec::rebindable("axes.forward-", "", "MOVE BACK"),
    HotkeyRowSpec::rebindable("axes.right-", "", "MOVE LEFT"),
    HotkeyRowSpec::rebindable("axes.right+", "", "MOVE RIGHT"),
    HotkeyRowSpec::rebindable("axes.turn-", "", "TURN LEFT"),
    HotkeyRowSpec::rebindable("axes.turn+", "", "TURN RIGHT"),
    HotkeyRowSpec::fixed("", "LOOK LEFT/RIGHT AXIS", &["MOUSE X AXIS"]),
    HotkeyRowSpec::fixed("", "LOOK UP/DOWN AXIS", &["MOUSE Y AXIS"]),
    HotkeyRowSpec::rebindable("presses.map", "", "SHOW MAP"),
    HotkeyRowSpec::fixed("ifs.controls.playerlist", "PLAYER LIST", &["TAB"]),
    HotkeyRowSpec::fixed("", "CHAT", &["T"]),
    HotkeyRowSpec::fixed("", "TEAM CHAT", &["Y"]),
    HotkeyRowSpec::fixed("ifs.controls.General.Pause", "PAUSE MENU", &["ESC"]),
    HotkeyRowSpec::rebindable("presses.stats", "", "STATS LINE"),
];

/// The 19 vehicle binding rows (`ROWS.vehicle` in `src/menus/hotkeys.js`).
pub const VEHICLE_ROWS: [HotkeyRowSpec; 19] = [
    HotkeyRowSpec::rebindable(
        "holds.firePrimary",
        "ifs.controls.General.Primary",
        "PRIMARY FIRE",
    ),
    HotkeyRowSpec::rebindable(
        "holds.fireSecondary",
        "ifs.controls.General.Secondary",
        "SECONDARY FIRE",
    ),
    HotkeyRowSpec::rebindable("axes.forward+", "", "ACCELERATE"),
    HotkeyRowSpec::rebindable("axes.forward-", "", "REVERSE"),
    HotkeyRowSpec::rebindable("axes.right-", "", "STRAFE LEFT"),
    HotkeyRowSpec::rebindable("axes.right+", "", "STRAFE RIGHT"),
    HotkeyRowSpec::rebindable("axes.turn-", "", "TURN LEFT"),
    HotkeyRowSpec::rebindable("axes.turn+", "", "TURN RIGHT"),
    HotkeyRowSpec::fixed("", "AIM", &["MOUSE"]),
    HotkeyRowSpec::fixed("", "BRAKE", &["SPACE"]),
    HotkeyRowSpec::rebindable(
        "presses.nextPrimary",
        "ifs.controls.General.NextPosition",
        "NEXT POSITION",
    ),
    HotkeyRowSpec::rebindable("presses.use", "", "ENTER/EXIT"),
    HotkeyRowSpec::rebindable("presses.reload", "ifs.controls.General.Reload", "RELOAD"),
    HotkeyRowSpec::rebindable(
        "presses.perspective",
        "ifs.controls.General.FirstThirdPerson",
        "1ST/3RD VIEW",
    ),
    HotkeyRowSpec::rebindable("presses.map", "", "SHOW MAP"),
    HotkeyRowSpec::fixed("ifs.controls.playerlist", "PLAYER LIST", &["TAB"]),
    HotkeyRowSpec::fixed("", "CHAT", &["T"]),
    HotkeyRowSpec::fixed("", "TEAM CHAT", &["Y"]),
    HotkeyRowSpec::fixed("ifs.controls.General.Pause", "PAUSE MENU", &["ESC"]),
];

/// Returns the active key codes for `row` (`bindingCodes(row, bindings)` in `src/menus/hotkeys.js`).
///
/// Fixed rows return their static `row.fixed` slice; rebindable rows look up
/// `row.action` in `bindings` (or return an empty vector when unbound).
pub fn binding_codes(row: &HotkeyRowSpec, bindings: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    if let Some(action) = row.action {
        bindings.get(action).cloned().unwrap_or_default()
    } else {
        row.fixed.iter().map(|s| (*s).to_string()).collect()
    }
}

/// Formatted hotkey row returned by [`hotkey_rows`] (`hotkeyRows` in `src/menus/hotkeys.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HotkeyRow {
    /// Action identifier for rebinding (`None` when the row is a fixed read-only axis).
    pub action: Option<String>,
    /// Localized uppercase row label with trailing colon (`"MOVE FORWARD:"`, etc.).
    pub label: String,
    /// Formatted comma-separated key names (`"W, UP ARROW"`, `"MOUSE BUTTON 1"`, `""`).
    pub keys: String,
    /// True when the row is a fixed axis and cannot be rebound.
    pub disabled: bool,
}

/// Builds the formatted [`HotkeyRow`] list for `tab` (`"infantry"`, `"vehicle"`, or `"starfighter"`)
/// (`hotkeyRows(tab, bindings, strings)` in `src/menus/hotkeys.js`).
pub fn hotkey_rows(
    tab: &str,
    bindings: &BTreeMap<String, Vec<String>>,
    strings: &HashMap<String, String>,
) -> Vec<HotkeyRow> {
    // Starfighter rows come from `FLIGHT_BINDINGS` with uppercase labels and all rows rebindable.
    if tab == "starfighter" {
        return FLIGHT_BINDINGS
            .iter()
            .map(|entry| {
                // Flight rows use action names, independent of localized ground labels.
                let raw_label = entry.label.to_uppercase();
                let label = format!("{}:", raw_label.trim_end_matches(':'));
                let codes = bindings
                    .get(entry.id)
                    .cloned()
                    .unwrap_or_else(|| entry.keys.iter().map(|s| (*s).to_string()).collect());
                HotkeyRow {
                    action: Some(entry.id.to_string()),
                    label,
                    keys: keys_text(&codes),
                    disabled: false,
                }
            })
            .collect();
    }

    // Select the 19-row vehicle table or 32-row infantry table.
    let spec_list: &[HotkeyRowSpec] = if tab == "vehicle" {
        &VEHICLE_ROWS
    } else {
        &INFANTRY_ROWS
    };
    spec_list
        .iter()
        .map(|spec| {
            let raw_label = say(strings, Some(spec.label_key), spec.label).to_uppercase();
            let label = format!("{}:", raw_label.trim_end_matches(':'));
            let keys = if spec.action.is_some() {
                let codes = binding_codes(spec, bindings);
                keys_text(&codes)
            } else {
                spec.fixed.join(", ")
            };
            HotkeyRow {
                action: spec.action.map(str::to_string),
                label,
                keys,
                disabled: spec.action.is_none(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Frozen hotkeyRows output with default bindings and no localized strings.
    // This covers order, fixed text and the action keys the input dispatcher uses.
    #[test]
    fn all_control_rows_match_the_reference_snapshot() {
        let expected = include_str!("../tests/fixtures/control-rows.tsv");
        let mut actual = String::new();
        for tab in ["infantry", "vehicle", "starfighter"] {
            let bindings = if tab == "starfighter" {
                default_flight_bindings()
            } else {
                flat_bindings()
            };
            // Flight labels are action names even if ground localization has a
            // colliding key; the browser deliberately does not translate them.
            let strings =
                HashMap::from([("ifs.controls.General.ThrottleUp".into(), "IGNORED".into())]);
            for row in hotkey_rows(tab, &bindings, &strings) {
                assert_eq!(row.disabled, row.action.is_none());
                actual.push_str(&format!(
                    "{tab}\t{}\t{}\t{}\n",
                    row.label,
                    row.keys,
                    row.action.as_deref().unwrap_or("-")
                ));
            }
        }
        assert_eq!(actual, expected);
    }

    // Verifies tab counts, row counts, key label formatting, reserved keys, and exclusive rebinding.
    #[test]
    fn hotkeys_and_bindings_match_js() {
        assert_eq!(TABS.len(), 3);
        assert_eq!(TABS[0].label_key, "ifs.Controls.SoldierTab");
        assert_eq!(TABS[1].label_key, "ifs.Controls.VehicleTab");
        assert_eq!(TABS[2].label_key, "ifs.Controls.FlyerTab");
        assert_eq!(INFANTRY_ROWS.len(), 32);
        assert_eq!(VEHICLE_ROWS.len(), 19);
        assert_eq!(FLIGHT_BINDINGS.len(), 16);
        assert_eq!(INPUT_CODES.len(), 104);
        assert_eq!(DEFAULT_FLAT_BINDINGS.len(), 24);

        assert_eq!(key_name("KeyW"), "W");
        assert_eq!(key_name("Digit4"), "4");
        assert_eq!(key_name("Numpad0"), "KP_INSERT");
        assert_eq!(key_name("Numpad7"), "KP_7");
        assert_eq!(key_name("Mouse0"), "MOUSE BUTTON 1");
        assert_eq!(key_name("Mouse2"), "MOUSE BUTTON 2");
        assert_eq!(key_name("Mouse1"), "MOUSE BUTTON 3");
        assert_eq!(key_name("Mouse3"), "MOUSE BUTTON 4");
        assert_eq!(key_name("Mouse4"), "MOUSE BUTTON 5");
        assert_eq!(key_name("WheelUp"), "MOUSE WHEEL UP");
        assert_eq!(keys_text(&["KeyW", "ArrowUp"]), "W, UP");
        assert_eq!(keys_text(&["ShiftLeft", "ShiftRight"]), "SHIFT");
        assert_eq!(keys_text::<&str>(&[]), "");

        assert!(!bindable("Escape"));
        assert!(!bindable("Tab"));
        assert!(!bindable("F11"));
        assert!(!bindable("F12"));
        assert!(bindable("KeyW"));
        assert!(bindable("Mouse0"));
        assert!(bindable("Delete"));
        assert!(bindable("Backspace"));
        assert!(bindable("Numpad0"));

        let defaults = flat_bindings();
        let diff = rebind(&defaults, None, "presses.jump", Some("KeyJ"));
        assert_eq!(
            diff.get("presses.jump").cloned(),
            Some(vec!["KeyJ".to_string()])
        );

        // Rebinding KeyW to jump should remove KeyW from axes.forward+
        let diff2 = rebind(&defaults, None, "presses.jump", Some("KeyW"));
        assert_eq!(
            diff2.get("presses.jump").cloned(),
            Some(vec!["KeyW".to_string()])
        );
        assert_eq!(
            diff2.get("axes.forward+").cloned(),
            Some(vec!["ArrowUp".to_string()])
        );
    }
}
