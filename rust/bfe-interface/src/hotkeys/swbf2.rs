//! Battlefront II PC keyboard and mouse controls, in page order.

use super::{HotkeyRowSpec, HotkeyTab};
use std::collections::HashMap;

pub(super) const TABS: [HotkeyTab; 5] = [
    super::TABS[0], super::TABS[1], super::TABS[2],
    HotkeyTab { id: "jedi", label_key: "ifs.Controls.JediTab", label: "JEDI" },
    HotkeyTab { id: "turret", label_key: "ifs.Controls.TurretTab", label: "TURRET" },
];

// The two sides of a modifier can have different assignments.
pub(super) const MODIFIERS: [(&str, &str, &str); 6] = [
    ("AltLeft", "ifs.controls.lalt", "LEFT"),
    ("AltRight", "ifs.controls.ralt", "RIGHT"),
    ("ControlLeft", "ifs.controls.lcontrol", "LEFT"),
    ("ControlRight", "ifs.controls.rcontrol", "RIGHT"),
    ("ShiftLeft", "ifs.controls.lshift", "LEFT"),
    ("ShiftRight", "ifs.controls.rshift", "RIGHT"),
];

pub(super) fn key_name(code: &str, strings: &HashMap<String, String>) -> String {
    if let Some(label) = strings.get(code) { return label.trim().to_uppercase(); }
    swbf2_key_caption(code, |key| strings.get(key).map(String::as_str))
}

/// Display a physical key using the active 2005 game's prepared captions.
pub fn swbf2_key_caption<'a>(code: &str, label: impl FnOnce(&str) -> Option<&'a str>) -> String {
    if let Some(text) = label(&key_label_id(code)) { return text.trim().to_uppercase(); }
    if code == "MouseX" { return "MOUSE X AXIS".into(); }
    if code == "MouseY" { return "MOUSE Y AXIS".into(); }
    if let Some(&(_, _, side)) = MODIFIERS.iter().find(|&&(physical, _, _)| physical == code) {
        return format!("{side} {}", super::key_name(code));
    }
    super::key_name(code)
}

/// Physical key captions are separate from the action names in the prepared text.
pub fn swbf2_key_labels() -> impl Iterator<Item = (&'static str, String)> {
    super::INPUT_CODES.into_iter().chain(["MouseX", "MouseY"]).map(|code| (code, key_label_id(code)))
}

fn key_label_id(code: &str) -> String {
    if let Some(&(_, key, _)) = MODIFIERS.iter().find(|&&(physical, _, _)| physical == code) {
        return key.to_owned();
    }
    let suffix = match code {
        "Mouse0" => "mbtn0", "Mouse2" => "mbtn1", "Mouse1" => "mbtn2",
        "Mouse3" => "mbtn3", "Mouse4" => "mbtn4",
        "WheelUp" => "mwheelup", "WheelDown" => "mwheeldn",
        "ArrowRight" => "rightarrow", "ArrowLeft" => "leftarrow",
        "ArrowUp" => "uparrow", "ArrowDown" => "downarrow",
        "Enter" => "return", "Backquote" => "grave",
        "BracketLeft" => "lbracket", "BracketRight" => "rbracket",
        "Equal" => "equals", "Quote" => "apostrophe",
        "NumpadAdd" => "add", "NumpadSubtract" => "minus", "NumpadMultiply" => "multiply",
        "NumpadDivide" => "divide", "NumpadDecimal" => "decimal",
        other => other.strip_prefix("Key").or_else(|| other.strip_prefix("Digit")).unwrap_or(other),
    };
    format!("ifs.controls.{}", suffix.to_ascii_lowercase())
}

#[derive(Clone, Copy)]
pub(super) struct Control {
    pub row: HotkeyRowSpec,
    pub(super) keys: &'static [&'static str],
}

impl Control {
    const fn bound(action: &'static str, key: &'static str, label: &'static str, keys: &'static [&'static str]) -> Self {
        Self { row: HotkeyRowSpec::rebindable(action, key, label), keys }
    }

    const fn fixed(key: &'static str, label: &'static str, display: &'static [&'static str]) -> Self {
        Self { row: HotkeyRowSpec::fixed(key, label, display), keys: &[] }
    }

    const fn label(mut self, key: &'static str, label: &'static str) -> Self {
        self.row.label_key = key;
        self.row.label = label;
        self
    }

    const fn action(mut self, id: &'static str) -> Self {
        self.row.action = Some(id);
        self
    }
}

const PRIMARY: Control = Control::bound("holds.firePrimary", "ifs.controls.primaryfire", "Primary fire", &["Mouse0"]);
const SECONDARY: Control = Control::bound("holds.fireSecondary", "ifs.controls.secondaryfire", "Secondary fire", &["Mouse2"]);
const CYCLE: Control = Control::bound("presses.nextPrimary", "ifs.controls.primarynext", "Cycle primary", &["Digit1", "WheelDown"]);
const CYCLE_SECONDARY: Control = Control::bound("presses.nextSecondary", "ifs.controls.secondarynext", "Cycle secondary", &["Digit2", "WheelUp"]);
const LOCK: Control = Control::bound("presses.targetLock", "ifs.controls.locktarget", "Lock target", &["KeyQ", "Mouse3"]);
const SPRINT: Control = Control::bound("holds.sprint", "ifs.controls.sprint", "Sprint", &["ShiftLeft", "ShiftRight"]);
const JUMP: Control = Control::bound("presses.jump", "ifs.controls.jump", "Jump", &["Space", "Numpad0"]);
const CROUCH: Control = Control::bound("presses.crouch", "ifs.controls.crouch", "Crouch", &["KeyC", "ControlRight"]);
const ROLL: Control = Control::bound("presses.roll", "ifs.controls.roll", "Roll", &["AltLeft", "Numpad1"]);
const ZOOM: Control = Control::bound("presses.zoom", "ifs.controls.zoom", "Zoom", &["KeyZ", "Mouse1"]);
const VIEW: Control = Control::bound("presses.perspective", "ifs.controls.view", "Change view", &["KeyV", "End"]);
const RELOAD: Control = Control::bound("presses.reload", "ifs.controls.reload", "Reload", &["KeyR", "Delete"]);
const USE: Control = Control::bound("presses.use", "ifs.controls.General.Enter", "Enter", &["KeyE", "Enter"]);
const SEAT: Control = Control::bound("presses.nextPrimary", "ifs.controls.nextposition", "Next seat", &["Digit1", "WheelUp"]);
const PREV_SEAT: Control = Control::bound("presses.previousPrimary", "ifs.controls.prevposition", "Previous seat", &["Digit2", "WheelDown"]);
const BOOST: Control = SPRINT.action("holds.boost").label("ifs.controls.boost", "Boost");
const AIM_X: Control = Control::fixed("ifs.controls.looklraxis", "Horizontal aim", &["MouseX"]);
const AIM_Y: Control = Control::fixed("ifs.controls.lookudaxis", "Vertical aim", &["MouseY"]);
const RIGHT: Control = Control::bound("axes.right+", "ifs.controls.moveright", "Move right", &["KeyD", "ArrowRight"]);
const LEFT: Control = Control::bound("axes.right-", "ifs.controls.moveleft", "Move left", &["KeyA", "ArrowLeft"]);
const FORWARD: Control = Control::bound("axes.forward+", "ifs.controls.moveforward", "Move forward", &["KeyW", "ArrowUp"]);
const BACK: Control = Control::bound("axes.forward-", "ifs.controls.moveback", "Move back", &["KeyS", "ArrowDown"]);
const LOOK_RIGHT: Control = Control::bound("axes.turn+", "ifs.controls.lookright", "Look right", &[]);
const LOOK_LEFT: Control = Control::bound("axes.turn-", "ifs.controls.lookleft", "Look left", &[]);
const ACCEPT: Control = Control::bound("presses.accept", "ifs.controls.accept", "Accept", &["PageUp", "F1"]);
const DECLINE: Control = Control::bound("presses.decline", "ifs.controls.decline", "Decline", &["PageDown", "F2"]);
const ROSTER: Control = Control::bound("ui.roster", "ifs.controls.playerlist", "Player list", &["Tab", "Backslash"]);
const CHAT: Control = Control::bound("ui.chat", "ifs.controls.chat", "Chat", &["KeyT"]);
const TEAM: Control = Control::bound("ui.teamChat", "ifs.controls.teamchat", "Team chat", &["KeyY"]);
const SQUAD: Control = Control::bound("presses.squadCommand", "ifs.controls.squadcom", "Squad command", &["F4", "Numpad4"]);
const MAP: Control = Control::bound("presses.map", "ifs.controls.map", "Map", &["KeyM"]);

const INFANTRY: &[Control] = &[
    PRIMARY, CYCLE, SECONDARY, CYCLE_SECONDARY, LOCK, SPRINT, JUMP, CROUCH, ROLL, ZOOM, VIEW, RELOAD, USE,
    AIM_X, AIM_Y, RIGHT, LEFT, FORWARD, BACK, LOOK_RIGHT, LOOK_LEFT,
    ACCEPT, DECLINE, ROSTER, CHAT, TEAM, SQUAD, MAP,
];
const VEHICLE: &[Control] = &[
    PRIMARY, SECONDARY, LOCK, BOOST, JUMP, CROUCH.label("ifs.controls.droidekadeploy", "Deploy"),
    ZOOM, VIEW.label("ifs.controls.droidekaview", "Change view"), RELOAD, USE.label("ifs.controls.exitvehicle", "Exit vehicle"),
    SEAT, PREV_SEAT, AIM_X, AIM_Y, RIGHT, LEFT, FORWARD, BACK, LOOK_RIGHT, LOOK_LEFT,
    ACCEPT, DECLINE, ROSTER, CHAT, TEAM, SQUAD, MAP,
];
const FLIGHT: &[Control] = &[
    PRIMARY.action("firePrimary"), SECONDARY.action("fireSecondary"), LOCK.action("targetLock"), BOOST.action("boost"),
    JUMP.action("takeoff").label("ifs.controls.General.Land_Takeoff", "Take off / land"),
    Control::bound("trick", "ifs.controls.trick", "Evasive maneuver", &["AltLeft", "ControlRight"]),
    Control::bound("immelmann", "ifs.controls.immelmann", "Immelmann", &["KeyC", "Numpad1"]),
    Control::bound("rollLock", "ifs.controls.rolllock", "Roll lock", &["ControlLeft", "Mouse1"]),
    VIEW.action("perspective"), RELOAD, USE.action("use").label("ifs.controls.exitstarfighter", "Exit starfighter"),
    SEAT.action("nextPrimary"), PREV_SEAT.action("previousPrimary"),
    AIM_X.label("ifs.controls.turnaxis", "Yaw"), AIM_Y.label("ifs.controls.pitchaxis", "Pitch"),
    RIGHT.action("rollRight").label("ifs.controls.rollcw", "Roll right"),
    LEFT.action("rollLeft").label("ifs.controls.rollccw", "Roll left"),
    FORWARD.action("throttle").label("ifs.controls.throttleinc", "Increase throttle"),
    BACK.action("brake").label("ifs.controls.throttledec", "Decrease throttle"),
    LOOK_RIGHT.label("ifs.controls.turnright", "Turn right"), LOOK_LEFT.label("ifs.controls.turnleft", "Turn left"),
    ACCEPT, DECLINE, ROSTER, CHAT, TEAM, SQUAD, MAP,
];
const JEDI: &[Control] = &[
    PRIMARY.label("ifs.controls.lightsaberattack", "Saber attack"),
    Control::bound("holds.block", "ifs.controls.lightsaberblock", "Block", &["KeyF", "Mouse1"]),
    SECONDARY.label("ifs.controls.useforcepower", "Use power"),
    Control::bound("presses.nextSecondary", "ifs.controls.switchforcepower", "Cycle power", &["Digit1", "WheelUp"]),
    LOCK, SPRINT.label("ifs.controls.jedisprint", "Sprint"), JUMP.label("ifs.controls.forcejump", "Force jump"),
    CROUCH, ROLL, USE, AIM_X, AIM_Y, RIGHT, LEFT, FORWARD, BACK, LOOK_RIGHT, LOOK_LEFT,
    ACCEPT, DECLINE, ROSTER, CHAT, TEAM, SQUAD, MAP,
];
const TURRET: &[Control] = &[
    PRIMARY.label("ifs.controls.attack", "Fire"), LOCK, ZOOM,
    FORWARD.action("holds.zoomIn").label("ifs.controls.zoomin", "Zoom in"),
    BACK.action("holds.zoomOut").label("ifs.controls.zoomout", "Zoom out"),
    USE.label("ifs.controls.exitturret", "Exit turret"), SEAT, PREV_SEAT, AIM_X, AIM_Y,
    RIGHT.action("axes.turn+").label("ifs.controls.lookright", "Look right"),
    LEFT.action("axes.turn-").label("ifs.controls.lookleft", "Look left"),
    ACCEPT, DECLINE, ROSTER, CHAT, TEAM, SQUAD, MAP,
];

pub(super) fn controls(tab: &str) -> &'static [Control] {
    match tab {
        "infantry" => INFANTRY,
        "vehicle" => VEHICLE,
        "starfighter" => FLIGHT,
        "jedi" => JEDI,
        "turret" => TURRET,
        _ => &[],
    }
}

pub(super) fn row(spec: &HotkeyRowSpec, bindings: &super::BindingMap, strings: &HashMap<String, String>) -> super::HotkeyRow {
    let title = crate::options::say(strings, Some(spec.label_key), spec.label).to_uppercase();
    let keys = match spec.action {
        Some(action) => bindings.get(action).into_iter().flatten()
            .map(|code| key_name(code, strings)).collect::<Vec<_>>().join(", "),
        None => spec.fixed.iter().map(|code| key_name(code, strings)).collect::<Vec<_>>().join(", "),
    };
    super::HotkeyRow { action: spec.action.map(str::to_owned), label: format!("{}:", title.trim_end_matches(':')),
        keys, disabled: spec.action.is_none() }
}
