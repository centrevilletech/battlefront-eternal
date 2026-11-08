//! Click/key gate (`PRESS ANY KEY OR CLICK TO BEGIN`, `src/shell/gate.js`).
//!
//! Ports `src/shell/gate.js`:
//! - `GATE_TEXT = "PRESS ANY KEY OR CLICK TO BEGIN"`, `IW = 1024`, `PULSE = 0.7`,
//!   `LOW = 0.3`, `AT = 0.82`.
//! - `countsAsPress`: non-keydown events (pointerdown, touchstart) always count;
//!   keydown events with `ctrlKey`, `metaKey`, or `altKey` held do not count,
//!   nor do keys matching `/^(F\d+|Tab|Unidentified|Control|Meta|Alt|OS)$/`.
//! - `pulse(t)`: triangle wave over `PULSE = 0.7` seconds from `1.0` down to
//!   `LOW = 0.3` at `t = 0.35` and back to `1.0` at `t = 0.7`.

/// Interface coordinate width (`1024`).
/// Mirrors `IW` in `src/shell/gate.js`.
pub const IW: f64 = 1024.0;

/// Prompt text displayed on the gate screen.
/// Mirrors `GATE_TEXT` in `src/shell/gate.js`.
pub const GATE_TEXT: &str = "PRESS ANY KEY OR CLICK TO BEGIN";

/// Period in seconds (`0.7s`) of one full alpha pulse cycle.
/// Mirrors `PULSE` in `src/shell/gate.js`.
pub const PULSE: f64 = 0.7;

/// Minimum alpha reached halfway through the pulse (`0.3`).
/// Mirrors `LOW` in `src/shell/gate.js`.
pub const LOW: f64 = 0.3;

/// Vertical fraction (`0.82`) within the safe area (`0.05..0.95` of `H`).
/// Mirrors `AT` in `src/shell/gate.js`.
pub const AT: f64 = 0.82;

/// Checks whether an input event unlocks the click gate.
///
/// Mirrors `countsAsPress(event)` in `src/shell/gate.js`:
/// - Non-`"keydown"` events (`"pointerdown"`, `"touchstart"`) return `true`.
/// - `"keydown"` events with `ctrl_key`, `meta_key`, or `alt_key` held return `false`.
/// - Keys matching `/^(F\d+|Tab|Unidentified|Control|Meta|Alt|OS)$/` return `false`.
pub fn counts_as_press(
    event_type: &str,
    key: &str,
    ctrl_key: bool,
    meta_key: bool,
    alt_key: bool,
) -> bool {
    if event_type != "keydown" {
        return true;
    }
    if ctrl_key || meta_key || alt_key {
        return false;
    }
    !is_ignored_key(key)
}

fn is_ignored_key(key: &str) -> bool {
    if matches!(
        key,
        "Tab"
            | "Unidentified"
            | "Control"
            | "ControlLeft"
            | "ControlRight"
            | "Meta"
            | "MetaLeft"
            | "MetaRight"
            | "Alt"
            | "AltLeft"
            | "AltRight"
            | "OS"
    ) {
        return true;
    }
    if let Some( digits) = key.strip_prefix('F') {
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
            return true;
        }
    }
    false
}

/// Computes the gate prompt's alpha `t` seconds in: starts at `1.0`, dips to
/// `0.3` at `t = 0.35`, and returns to `1.0` at `t = 0.7`.
///
/// Mirrors `pulse(t)` in `src/shell/gate.js`.
pub fn pulse(t: f64) -> f64 {
    let phase = (t / PULSE) % 1.0;
    let phase = if phase < 0.0 { phase + 1.0 } else { phase };
    let tri = if phase < 0.5 {
        phase * 2.0
    } else {
        2.0 - phase * 2.0
    };
    LOW + (1.0 - LOW) * (1.0 - tri)
}

/// Computes the interface coordinate height `H = 768 + (h / w - 0.75) * 768`
/// and the gate prompt anchor `(IW / 2, H * (0.05 + 0.9 * AT))` for a viewport
/// of `(w, h)` pixels.
///
/// Mirrors the coordinate calculation in `showGate`'s `draw` in `src/shell/gate.js`.
pub fn gate_layout(w: f64, h: f64) -> (f64, f64, f64) {
    let w = w.max(1.0);
    let h = h.max(1.0);
    let interface_h = 768.0 + (h / w - 0.75) * 768.0;
    let x = IW * 0.5;
    let y = interface_h * (0.05 + 0.9 * AT);
    (x, y, interface_h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_accepts_clicks_and_keys_except_modifiers_and_browser_keys_and_pulses_1_to_0_3_to_1() {
        assert!(counts_as_press("pointerdown", "", false, false, false));
        assert!(counts_as_press("keydown", "Enter", false, false, false));
        assert!(counts_as_press("keydown", " ", false, false, false));
        assert!(!counts_as_press("keydown", "F12", false, false, false));
        assert!(!counts_as_press("keydown", "r", true, false, false));
        assert!(!counts_as_press("keydown", "Tab", false, false, false));

        assert!((pulse(0.0) - 1.0).abs() < 1e-9);
        assert!((pulse(0.35) - 0.3).abs() < 1e-9);
        assert!((pulse(0.7) - 1.0).abs() < 1e-9);
        assert!((pulse(0.175) - 0.65).abs() < 1e-9);
    }
}
