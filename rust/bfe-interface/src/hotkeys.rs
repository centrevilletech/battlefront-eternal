//! Control binding tables, key names, and `CONTROL OPTIONS` tab rows
//! (`src/menus/hotkeys.js`, `src/input.js`, `src/flight/controls.js`).
//!
//! Ports `TABS`, `NAMES`, `keyName`, `keysText`, `ROWS` (`infantry`, `vehicle`,
//! `starfighter`), `bindingCodes`, `hotkeyRows`, `AXES`, `PRESSES`, `HOLDS`,
//! `BINDINGS`, `WORLD_AXES`, `RESERVED`, `INPUT_CODES`, `bindable`,
//! `FLIGHT_BINDINGS`, `flatBindings`, `playerBindings`, `flightBindings`, and `rebind`.

use std::collections::{BTreeMap, HashMap};

/// Control tab definition (`TABS` in `src/menus/hotkeys.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyTab {
    pub id: &'static str,
    pub label: &'static str,
    pub label_key: &'static str,
}

/// The three `CONTROL OPTIONS` binding tabs (`TABS` in `src/menus/hotkeys.js`).
pub const TABS: [HotkeyTab; 3] = [
    HotkeyTab {
        id: "infantry",
        label: "INFANTRY",
        label_key: "ifs.controls.Infantry",
    },
    HotkeyTab {
        id: "vehicle",
        label: "VEHICLE",
        label_key: "ifs.controls.Vehicle",
    },
    HotkeyTab {
        id: "starfighter",
        label: "STARFIGHTER",
        label_key: "ifs.controls.Flyer",
    },
];

/// Axis actions that are split into `<action>-` and `<action>+` in `flatBindings()`
/// (`WORLD_AXES` in `src/input.js`).
pub const WORLD_AXES: [&str; 4] = ["moveX", "moveY", "yaw", "pitch"];

/// Keys reserved for global shell/HUD shortcuts that cannot be rebound
/// (`RESERVED` in `src/input.js`).
pub const RESERVED: [&str; 6] = ["Escape", "Tab", "KeyT", "KeyY", "F11", "F12"];

/// All 89 recognized keyboard and mouse input codes (`INPUT_CODES` in `src/input.js`).
pub const INPUT_CODES: [&str; 90] = [
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
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
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
    "ShiftLeft",
    "ShiftRight",
    "ControlLeft",
    "ControlRight",
    "AltLeft",
    "AltRight",
    "CapsLock",
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
#[inline]
pub fn bindable(code: &str) -> bool {
    INPUT_CODES.contains(&code) && !RESERVED.contains(&code)
}

/// Converts a key/mouse code (`"KeyW"`, `"MouseLeft"`, `"WheelUp"`) into its uppercase
/// display label (`"W"`, `"MOUSE 1"`, `"MOUSE WHEEL UP"`), matching `keyName(code)` in
/// `src/menus/hotkeys.js`.
pub fn key_name(code: &str) -> String {
    if code.is_empty() {
        return String::new();
    }
    if let Some(letter) = code.strip_prefix("Key") {
        if letter.len() == 1 && letter.as_bytes()[0].is_ascii_uppercase() {
            return letter.to_string();
        }
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        if digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit() {
            return digit.to_string();
        }
    }
    if let Some(num) = code.strip_prefix("Numpad") {
        if num.len() == 1 && num.as_bytes()[0].is_ascii_digit() {
            return format!("NUM {num}");
        }
    }
    match code {
        "ArrowUp" => "UP".to_string(),
        "ArrowDown" => "DOWN".to_string(),
        "ArrowLeft" => "LEFT".to_string(),
        "ArrowRight" => "RIGHT".to_string(),
        "Space" => "SPACE".to_string(),
        "Enter" => "ENTER".to_string(),
        "Backspace" => "BACKSPACE".to_string(),
        "Delete" => "DELETE".to_string(),
        "Insert" => "INSERT".to_string(),
        "Home" => "HOME".to_string(),
        "End" => "END".to_string(),
        "PageUp" => "PAGE UP".to_string(),
        "PageDown" => "PAGE DOWN".to_string(),
        "ShiftLeft" => "LEFT SHIFT".to_string(),
        "ShiftRight" => "RIGHT SHIFT".to_string(),
        "ControlLeft" => "LEFT CTRL".to_string(),
        "ControlRight" => "RIGHT CTRL".to_string(),
        "AltLeft" => "LEFT ALT".to_string(),
        "AltRight" => "RIGHT ALT".to_string(),
        "CapsLock" => "CAPS LOCK".to_string(),
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
        "MouseLeft" => "MOUSE 1".to_string(),
        "MouseRight" => "MOUSE 2".to_string(),
        "MouseMiddle" => "MOUSE 3".to_string(),
        "MouseBack" => "MOUSE 4".to_string(),
        "MouseForward" => "MOUSE 5".to_string(),
        "WheelUp" => "MOUSE WHEEL UP".to_string(),
        "WheelDown" => "MOUSE WHEEL DOWN".to_string(),
        "MouseX" => "MOUSE X".to_string(),
        "MouseY" => "MOUSE Y".to_string(),
        other => other.to_ascii_uppercase(),
    }
}

/// Formats a list of bound codes as `"KEY1, KEY2"` or `"---"` when empty
/// (`keysText(codes)` in `src/menus/hotkeys.js`).
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
    pub action: &'static str,
    pub label: &'static str,
    pub keys: &'static [&'static str],
}

/// All 16 starfighter actions and their default keys (`FLIGHT_BINDINGS` in `src/flight/controls.js`).
pub const FLIGHT_BINDINGS: [FlightBindingDef; 16] = [
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
/// `PRESSES`, `HOLDS`, and `BINDINGS`.
pub const DEFAULT_FLAT_BINDINGS: [(&str, &[&str]); 37] = [
    ("moveX-", &["KeyA", "ArrowLeft"]),
    ("moveX+", &["KeyD", "ArrowRight"]),
    ("moveY-", &["KeyW", "ArrowUp"]),
    ("moveY+", &["KeyS", "ArrowDown"]),
    ("yaw-", &[]),
    ("yaw+", &[]),
    ("pitch-", &[]),
    ("pitch+", &[]),
    ("jump", &["Space"]),
    ("crouch", &["KeyC"]),
    ("prone", &["KeyX", "ControlLeft"]),
    ("reload", &["KeyR"]),
    ("switchPrimary", &["KeyF", "MouseMiddle"]),
    ("switchSecondary", &["KeyQ", "ControlRight"]),
    ("nextPrimary", &["WheelUp"]),
    ("nextSecondary", &["WheelDown"]),
    ("enter", &["KeyE", "Enter"]),
    ("exit", &["KeyE"]),
    ("view", &["KeyV"]),
    ("dropFlag", &["KeyZ"]),
    ("map", &["KeyM"]),
    ("zoom", &["ShiftRight"]),
    ("target", &["KeyG"]),
    ("fire", &["MouseLeft"]),
    ("secondary", &["MouseRight"]),
    ("sprint", &["ShiftLeft"]),
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

/// Additional command bindings in `BINDINGS` from `src/input.js`.
pub const EXTRA_DEFAULT_FLAT_BINDINGS: [(&str, &[&str]); 5] = [
    ("defend", &["Digit2"]),
    ("follow", &["Digit3"]),
    ("hold", &["Digit4"]),
    ("getIn", &["Digit5"]),
    ("getOut", &["Digit6"]),
];

/// Builds the complete default flat bindings map (`flatBindings()` in `src/input.js`).
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
    if !defaults.contains_key(action) {
        return saved.cloned().unwrap_or_default();
    }
    if let Some(c) = code {
        if !bindable(c) {
            return saved.cloned().unwrap_or_default();
        }
    }
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
    if let Some(c) = code {
        for (other, codes) in merged.iter_mut() {
            if other != action {
                codes.retain(|existing| existing != c);
            }
        }
        merged.insert(action.to_string(), vec![c.to_string()]);
    } else {
        merged.insert(action.to_string(), Vec::new());
    }

    let mut diff = BTreeMap::new();
    for (k, codes) in merged {
        let def_codes = defaults.get(&k).map(Vec::as_slice).unwrap_or(&[]);
        if codes.as_slice() != def_codes {
            diff.insert(k, codes);
        }
    }
    diff
}

/// Schema entry for one row in `ROWS.infantry`, `ROWS.vehicle`, or `starfighter`
/// (`src/menus/hotkeys.js`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyRowSpec {
    pub action: Option<&'static str>,
    pub fixed: &'static [&'static str],
    pub label: &'static str,
    pub label_key: Option<&'static str>,
}

impl HotkeyRowSpec {
    const fn act(action: &'static str, label: &'static str, label_key: Option<&'static str>) -> Self {
        Self {
            action: Some(action),
            fixed: &[],
            label,
            label_key,
        }
    }

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
    HotkeyRowSpec::act("fire", "PRIMARY ATTACK", Some("ifs.controls.Fire")),
    HotkeyRowSpec::act(
        "secondary",
        "SECONDARY ATTACK",
        Some("ifs.controls.Secondary"),
    ),
    HotkeyRowSpec::act(
        "switchPrimary",
        "SWITCH PRIMARY WEAPON",
        Some("ifs.controls.Change"),
    ),
    HotkeyRowSpec::act(
        "nextPrimary",
        "NEXT PRIMARY WEAPON",
        Some("ifs.controls.Next"),
    ),
    HotkeyRowSpec::act(
        "switchSecondary",
        "SWITCH SECONDARY",
        Some("ifs.controls.ChangeSecondary"),
    ),
    HotkeyRowSpec::act(
        "nextSecondary",
        "NEXT SECONDARY",
        Some("ifs.controls.NextSecondary"),
    ),
    HotkeyRowSpec::act("zoom", "ZOOM", Some("ifs.controls.Zoom")),
    HotkeyRowSpec::act("jump", "JUMP", Some("ifs.controls.Jump")),
    HotkeyRowSpec::act("crouch", "CROUCH", Some("ifs.controls.Crouch")),
    HotkeyRowSpec::act("prone", "PRONE", Some("ifs.controls.Prone")),
    HotkeyRowSpec::act("sprint", "SPRINT", None),
    HotkeyRowSpec::act("reload", "RELOAD", Some("ifs.controls.Reload")),
    HotkeyRowSpec::act("enter", "USE / ENTER VEHICLE", Some("ifs.controls.Enter")),
    HotkeyRowSpec::act("moveY-", "MOVE FORWARD", Some("ifs.controls.Move_Up")),
    HotkeyRowSpec::act("moveY+", "MOVE BACK", Some("ifs.controls.Move_Down")),
    HotkeyRowSpec::act("moveX-", "STRAFE LEFT", Some("ifs.controls.Move_Left")),
    HotkeyRowSpec::act("moveX+", "STRAFE RIGHT", Some("ifs.controls.Move_Right")),
    HotkeyRowSpec::act("yaw-", "TURN LEFT", None),
    HotkeyRowSpec::act("yaw+", "TURN RIGHT", None),
    HotkeyRowSpec::act("pitch-", "LOOK UP", None),
    HotkeyRowSpec::act("pitch+", "LOOK DOWN", None),
    HotkeyRowSpec::fix(&["MouseX"], "LOOK HORIZONTAL", None),
    HotkeyRowSpec::fix(&["MouseY"], "LOOK VERTICAL", None),
    HotkeyRowSpec::act(
        "view",
        "FIRST / THIRD PERSON VIEW",
        Some("ifs.controls.View"),
    ),
    HotkeyRowSpec::act("map", "VIEW MAP", Some("ifs.controls.ViewMap")),
    HotkeyRowSpec::act("dropFlag", "DROP ITEM", None),
    HotkeyRowSpec::act("target", " LOCK TARGET", Some("ifs.controls.TargetLock")),
    HotkeyRowSpec::act("attack", "ORDER: ATTACK", None),
    HotkeyRowSpec::act("defend", "ORDER: DEFEND", None),
    HotkeyRowSpec::act("follow", "ORDER: FOLLOW", None),
    HotkeyRowSpec::act("hold", "ORDER: HOLD", None),
    HotkeyRowSpec::act("backup", "CALL FOR BACKUP", None),
];

/// The 19 vehicle binding rows (`ROWS.vehicle` in `src/menus/hotkeys.js`).
pub const VEHICLE_ROWS: [HotkeyRowSpec; 19] = [
    HotkeyRowSpec::act("fire", "PRIMARY ATTACK", Some("ifs.controls.Fire")),
    HotkeyRowSpec::act(
        "secondary",
        "SECONDARY ATTACK",
        Some("ifs.controls.Secondary"),
    ),
    HotkeyRowSpec::act("jump", "BOOST", None),
    HotkeyRowSpec::act("exit", "EXIT VEHICLE", Some("ifs.controls.Exit")),
    HotkeyRowSpec::act("landTakeoff", "LAND / TAKE OFF", None),
    HotkeyRowSpec::act("moveY-", "THROTTLE UP", None),
    HotkeyRowSpec::act("moveY+", "THROTTLE DOWN", None),
    HotkeyRowSpec::act("moveX-", "STRAFE / ROLL LEFT", None),
    HotkeyRowSpec::act("moveX+", "STRAFE / ROLL RIGHT", None),
    HotkeyRowSpec::act("yaw-", "STEER LEFT", None),
    HotkeyRowSpec::act("yaw+", "STEER RIGHT", None),
    HotkeyRowSpec::act("pitch-", "PITCH UP", None),
    HotkeyRowSpec::act("pitch+", "PITCH DOWN", None),
    HotkeyRowSpec::fix(&["MouseX"], "TURN HORIZONTAL", None),
    HotkeyRowSpec::fix(&["MouseY"], "PITCH VERTICAL", None),
    HotkeyRowSpec::act(
        "view",
        "FIRST / THIRD PERSON VIEW",
        Some("ifs.controls.View"),
    ),
    HotkeyRowSpec::act("target", "LOCK TARGET", Some("ifs.controls.TargetLock")),
    HotkeyRowSpec::act("getIn", "ORDER: GET IN", None),
    HotkeyRowSpec::act("getOut", "ORDER: GET OUT", None),
];

/// Returns the active key codes for `row` (`bindingCodes(row, bindings)` in `src/menus/hotkeys.js`).
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
    pub action: Option<String>,
    pub label: String,
    pub keys: String,
    pub disabled: bool,
}

/// Builds the display rows for `tab` (`"infantry"`, `"vehicle"`, `"starfighter"`)
/// (`hotkeyRows(tab, bindings, strings)` in `src/menus/hotkeys.js`).
pub fn hotkey_rows(
    tab: &str,
    bindings: &BTreeMap<String, Vec<String>>,
    strings: &HashMap<String, String>,
) -> Vec<HotkeyRow> {
    let say = |key: Option<&str>, fallback: &str| -> String {
        let raw = key
            .and_then(|k| strings.get(k))
            .map(String::as_str)
            .unwrap_or(fallback);
        raw.replace("\r\n", " ").replace('\n', " ").trim().to_string()
    };

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
