//! input.js::Controls and flight/controls.js bindings, expressed with the generated prediction schema.
//! Presses coalesce into ClientCommand action bits while a frame stalls. Key
//! repeat cannot enqueue another action, and focus loss clears held fire.
use crate::generated_input::{
    ClientAction as Action, ClientCommand, ClientFlightAction as FlightAction, ClientFlightCommand,
    ClientFlightHeld as FlightHeld, ClientHeld as Held, ClientLook,
};
use std::collections::BTreeMap;
use winit::{event::MouseButton, keyboard::KeyCode};

// KeyCode discriminants are local device identifiers, never ABI offsets.
// Unsupported physical keys are ignored with checked access below.
const KEYS: usize = 256;
// Match the protocol fire-edge bound when events arrive without a tick.\n// Counts saturate; the latest edge id still identifies the observed press.
const MAX_FIRE: u32 = 32;

/// Maps a physical [`KeyCode`] to its DOM `KeyboardEvent.code` string (`"KeyW"`, `"Space"`,
/// `"Numpad0"`, etc.) across all keyboard keys in `INPUT_CODES`.
pub fn keycode_to_dom_code(code: KeyCode) -> Option<&'static str> {
    use KeyCode::*;
    Some(match code {
        KeyA => "KeyA",
        KeyB => "KeyB",
        KeyC => "KeyC",
        KeyD => "KeyD",
        KeyE => "KeyE",
        KeyF => "KeyF",
        KeyG => "KeyG",
        KeyH => "KeyH",
        KeyI => "KeyI",
        KeyJ => "KeyJ",
        KeyK => "KeyK",
        KeyL => "KeyL",
        KeyM => "KeyM",
        KeyN => "KeyN",
        KeyO => "KeyO",
        KeyP => "KeyP",
        KeyQ => "KeyQ",
        KeyR => "KeyR",
        KeyS => "KeyS",
        KeyT => "KeyT",
        KeyU => "KeyU",
        KeyV => "KeyV",
        KeyW => "KeyW",
        KeyX => "KeyX",
        KeyY => "KeyY",
        KeyZ => "KeyZ",
        Digit0 => "Digit0",
        Digit1 => "Digit1",
        Digit2 => "Digit2",
        Digit3 => "Digit3",
        Digit4 => "Digit4",
        Digit5 => "Digit5",
        Digit6 => "Digit6",
        Digit7 => "Digit7",
        Digit8 => "Digit8",
        Digit9 => "Digit9",
        Space => "Space",
        Enter => "Enter",
        Escape => "Escape",
        Tab => "Tab",
        Backquote => "Backquote",
        CapsLock => "CapsLock",
        ShiftLeft => "ShiftLeft",
        ShiftRight => "ShiftRight",
        ControlLeft => "ControlLeft",
        ControlRight => "ControlRight",
        AltLeft => "AltLeft",
        AltRight => "AltRight",
        ArrowUp => "ArrowUp",
        ArrowDown => "ArrowDown",
        ArrowLeft => "ArrowLeft",
        ArrowRight => "ArrowRight",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        Backspace => "Backspace",
        Delete => "Delete",
        Insert => "Insert",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        Minus => "Minus",
        Equal => "Equal",
        BracketLeft => "BracketLeft",
        BracketRight => "BracketRight",
        Backslash => "Backslash",
        Semicolon => "Semicolon",
        Quote => "Quote",
        Comma => "Comma",
        Period => "Period",
        Slash => "Slash",
        Numpad0 => "Numpad0",
        Numpad1 => "Numpad1",
        Numpad2 => "Numpad2",
        Numpad3 => "Numpad3",
        Numpad4 => "Numpad4",
        Numpad5 => "Numpad5",
        Numpad6 => "Numpad6",
        Numpad7 => "Numpad7",
        Numpad8 => "Numpad8",
        Numpad9 => "Numpad9",
        NumpadEnter => "NumpadEnter",
        NumpadAdd => "NumpadAdd",
        NumpadSubtract => "NumpadSubtract",
        NumpadMultiply => "NumpadMultiply",
        NumpadDivide => "NumpadDivide",
        NumpadDecimal => "NumpadDecimal",
        _ => return None,
    })
}

/// Maps a DOM `KeyboardEvent.code` string (e.g. `"KeyW"`, `"Space"`, `"Numpad0"`) to a
/// [`KeyCode`] across all keyboard keys in `INPUT_CODES`.
pub fn dom_code_to_keycode(code: &str) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match code {
        "KeyA" => KeyA,
        "KeyB" => KeyB,
        "KeyC" => KeyC,
        "KeyD" => KeyD,
        "KeyE" => KeyE,
        "KeyF" => KeyF,
        "KeyG" => KeyG,
        "KeyH" => KeyH,
        "KeyI" => KeyI,
        "KeyJ" => KeyJ,
        "KeyK" => KeyK,
        "KeyL" => KeyL,
        "KeyM" => KeyM,
        "KeyN" => KeyN,
        "KeyO" => KeyO,
        "KeyP" => KeyP,
        "KeyQ" => KeyQ,
        "KeyR" => KeyR,
        "KeyS" => KeyS,
        "KeyT" => KeyT,
        "KeyU" => KeyU,
        "KeyV" => KeyV,
        "KeyW" => KeyW,
        "KeyX" => KeyX,
        "KeyY" => KeyY,
        "KeyZ" => KeyZ,
        "Digit0" => Digit0,
        "Digit1" => Digit1,
        "Digit2" => Digit2,
        "Digit3" => Digit3,
        "Digit4" => Digit4,
        "Digit5" => Digit5,
        "Digit6" => Digit6,
        "Digit7" => Digit7,
        "Digit8" => Digit8,
        "Digit9" => Digit9,
        "Space" => Space,
        "Enter" => Enter,
        "Escape" => Escape,
        "Tab" => Tab,
        "Backquote" => Backquote,
        "CapsLock" => CapsLock,
        "ShiftLeft" => ShiftLeft,
        "ShiftRight" => ShiftRight,
        "ControlLeft" => ControlLeft,
        "ControlRight" => ControlRight,
        "AltLeft" => AltLeft,
        "AltRight" => AltRight,
        "ArrowUp" => ArrowUp,
        "ArrowDown" => ArrowDown,
        "ArrowLeft" => ArrowLeft,
        "ArrowRight" => ArrowRight,
        "F1" => F1,
        "F2" => F2,
        "F3" => F3,
        "F4" => F4,
        "F5" => F5,
        "F6" => F6,
        "F7" => F7,
        "F8" => F8,
        "F9" => F9,
        "F10" => F10,
        "F11" => F11,
        "F12" => F12,
        "Backspace" => Backspace,
        "Delete" => Delete,
        "Insert" => Insert,
        "Home" => Home,
        "End" => End,
        "PageUp" => PageUp,
        "PageDown" => PageDown,
        "Minus" => Minus,
        "Equal" => Equal,
        "BracketLeft" => BracketLeft,
        "BracketRight" => BracketRight,
        "Backslash" => Backslash,
        "Semicolon" => Semicolon,
        "Quote" => Quote,
        "Comma" => Comma,
        "Period" => Period,
        "Slash" => Slash,
        "Numpad0" => Numpad0,
        "Numpad1" => Numpad1,
        "Numpad2" => Numpad2,
        "Numpad3" => Numpad3,
        "Numpad4" => Numpad4,
        "Numpad5" => Numpad5,
        "Numpad6" => Numpad6,
        "Numpad7" => Numpad7,
        "Numpad8" => Numpad8,
        "Numpad9" => Numpad9,
        "NumpadEnter" => NumpadEnter,
        "NumpadAdd" => NumpadAdd,
        "NumpadSubtract" => NumpadSubtract,
        "NumpadMultiply" => NumpadMultiply,
        "NumpadDivide" => NumpadDivide,
        "NumpadDecimal" => NumpadDecimal,
        _ => return None,
    })
}

/// Physical key, mouse button (`0..=4`), or wheel direction bound to a control slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputTrigger {
    #[default]
    None,
    Key(KeyCode),
    Mouse(u8),
    WheelUp,
    WheelDown,
}

impl InputTrigger {
    /// Parses a DOM input code (`"KeyW"`, `"Mouse0"`..`"Mouse4"`, `"WheelUp"`, `"WheelDown"`)
    /// into a stack-allocated [`InputTrigger`].
    pub fn from_dom_code(code: &str) -> Option<Self> {
        match code {
            "Mouse0" => Some(Self::Mouse(0)),
            "Mouse1" => Some(Self::Mouse(1)),
            "Mouse2" => Some(Self::Mouse(2)),
            "Mouse3" => Some(Self::Mouse(3)),
            "Mouse4" => Some(Self::Mouse(4)),
            "WheelUp" => Some(Self::WheelUp),
            "WheelDown" => Some(Self::WheelDown),
            other => dom_code_to_keycode(other).map(Self::Key),
        }
    }
}

type TriggerSlot = Vec<InputTrigger>;

fn parse_slot(own: &BTreeMap<String, Vec<String>>, key: &str, default: TriggerSlot) -> TriggerSlot {
    own.get(key).map_or(default, |codes| {
        codes
            .iter()
            .filter_map(|code| InputTrigger::from_dom_code(code))
            .collect()
    })
}
#[inline]
fn slot_matches(slot: &TriggerSlot, trigger: InputTrigger) -> bool {
    trigger != InputTrigger::None && slot.contains(&trigger)
}

/// Cold-resolved table of all 24 infantry/vehicle bindings (`BINDINGS` in `src/input.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroundBindings {
    pub forward_plus: TriggerSlot,
    pub forward_minus: TriggerSlot,
    pub right_plus: TriggerSlot,
    pub right_minus: TriggerSlot,
    pub turn_plus: TriggerSlot,
    pub turn_minus: TriggerSlot,
    pub jump: TriggerSlot,
    pub crouch: TriggerSlot,
    pub prone: TriggerSlot,
    pub reload: TriggerSlot,
    pub use_key: TriggerSlot,
    pub next_primary: TriggerSlot,
    pub prev_primary: TriggerSlot,
    pub next_secondary: TriggerSlot,
    pub zoom: TriggerSlot,
    pub perspective: TriggerSlot,
    pub map: TriggerSlot,
    pub stats: TriggerSlot,
    pub squad_move_out: TriggerSlot,
    pub squad_hold: TriggerSlot,
    pub squad_follow: TriggerSlot,
    pub squad_at_ease: TriggerSlot,
    pub fire_primary: TriggerSlot,
    pub fire_secondary: TriggerSlot,
}

impl Default for GroundBindings {
    fn default() -> Self {
        let source = bfe_interface::flat_bindings();
        let slot = |name| parse_slot(&source, name, Vec::new());
        Self {
            forward_plus: slot("axes.forward+"),
            forward_minus: slot("axes.forward-"),
            right_plus: slot("axes.right+"),
            right_minus: slot("axes.right-"),
            turn_plus: slot("axes.turn+"),
            turn_minus: slot("axes.turn-"),
            jump: slot("presses.jump"),
            crouch: slot("presses.crouch"),
            prone: slot("presses.prone"),
            reload: slot("presses.reload"),
            use_key: slot("presses.use"),
            next_primary: slot("presses.nextPrimary"),
            prev_primary: slot("presses.previousPrimary"),
            next_secondary: slot("presses.nextSecondary"),
            zoom: slot("presses.zoom"),
            perspective: slot("presses.perspective"),
            map: slot("presses.map"),
            stats: slot("presses.stats"),
            squad_move_out: slot("presses.squadMoveOut"),
            squad_hold: slot("presses.squadHold"),
            squad_follow: slot("presses.squadFollow"),
            squad_at_ease: slot("presses.squadAtEase"),
            fire_primary: slot("holds.firePrimary"),
            fire_secondary: slot("holds.fireSecondary"),
        }
    }
}

impl GroundBindings {
    /// Applies user overrides (`keyBindings` in `src/settings.js`) on top of [`Self::default`].
    pub fn from_overrides(own: &BTreeMap<String, Vec<String>>) -> Self {
        let def = Self::default();
        if own.is_empty() {
            return def;
        }
        Self {
            forward_plus: parse_slot(own, "axes.forward+", def.forward_plus),
            forward_minus: parse_slot(own, "axes.forward-", def.forward_minus),
            right_plus: parse_slot(own, "axes.right+", def.right_plus),
            right_minus: parse_slot(own, "axes.right-", def.right_minus),
            turn_plus: parse_slot(own, "axes.turn+", def.turn_plus),
            turn_minus: parse_slot(own, "axes.turn-", def.turn_minus),
            jump: parse_slot(own, "presses.jump", def.jump),
            crouch: parse_slot(own, "presses.crouch", def.crouch),
            prone: parse_slot(own, "presses.prone", def.prone),
            reload: parse_slot(own, "presses.reload", def.reload),
            use_key: parse_slot(own, "presses.use", def.use_key),
            next_primary: parse_slot(own, "presses.nextPrimary", def.next_primary),
            prev_primary: parse_slot(own, "presses.previousPrimary", def.prev_primary),
            next_secondary: parse_slot(own, "presses.nextSecondary", def.next_secondary),
            zoom: parse_slot(own, "presses.zoom", def.zoom),
            perspective: parse_slot(own, "presses.perspective", def.perspective),
            map: parse_slot(own, "presses.map", def.map),
            stats: parse_slot(own, "presses.stats", def.stats),
            squad_move_out: parse_slot(own, "presses.squadMoveOut", def.squad_move_out),
            squad_hold: parse_slot(own, "presses.squadHold", def.squad_hold),
            squad_follow: parse_slot(own, "presses.squadFollow", def.squad_follow),
            squad_at_ease: parse_slot(own, "presses.squadAtEase", def.squad_at_ease),
            fire_primary: parse_slot(own, "holds.firePrimary", def.fire_primary),
            fire_secondary: parse_slot(own, "holds.fireSecondary", def.fire_secondary),
        }
    }
}

/// Cold-resolved table of all 16 starfighter bindings (`FLIGHT_BINDINGS` in `src/flight/controls.js`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlightBindingsTable {
    pub throttle: TriggerSlot,
    pub brake: TriggerSlot,
    pub roll_left: TriggerSlot,
    pub roll_right: TriggerSlot,
    pub boost: TriggerSlot,
    pub trick: TriggerSlot,
    pub immelmann: TriggerSlot,
    pub roll_lock: TriggerSlot,
    pub takeoff: TriggerSlot,
    pub target_lock: TriggerSlot,
    pub fire_primary: TriggerSlot,
    pub fire_secondary: TriggerSlot,
    pub perspective: TriggerSlot,
    pub use_key: TriggerSlot,
    pub next_primary: TriggerSlot,
    pub prev_primary: TriggerSlot,
}

impl Default for FlightBindingsTable {
    fn default() -> Self {
        let source = bfe_interface::default_flight_bindings();
        let slot = |name| parse_slot(&source, name, Vec::new());
        Self {
            throttle: slot("throttle"),
            brake: slot("brake"),
            roll_left: slot("rollLeft"),
            roll_right: slot("rollRight"),
            boost: slot("boost"),
            trick: slot("trick"),
            immelmann: slot("immelmann"),
            roll_lock: slot("rollLock"),
            takeoff: slot("takeoff"),
            target_lock: slot("targetLock"),
            fire_primary: slot("firePrimary"),
            fire_secondary: slot("fireSecondary"),
            perspective: slot("perspective"),
            use_key: slot("use"),
            next_primary: slot("nextPrimary"),
            prev_primary: slot("previousPrimary"),
        }
    }
}

impl FlightBindingsTable {
    /// Applies user overrides (`flightBindings` in `src/settings.js`) on top of [`Self::default`].
    pub fn from_overrides(own: &BTreeMap<String, Vec<String>>) -> Self {
        let def = Self::default();
        if own.is_empty() {
            return def;
        }
        Self {
            throttle: parse_slot(own, "throttle", def.throttle),
            brake: parse_slot(own, "brake", def.brake),
            roll_left: parse_slot(own, "rollLeft", def.roll_left),
            roll_right: parse_slot(own, "rollRight", def.roll_right),
            boost: parse_slot(own, "boost", def.boost),
            trick: parse_slot(own, "trick", def.trick),
            immelmann: parse_slot(own, "immelmann", def.immelmann),
            roll_lock: parse_slot(own, "rollLock", def.roll_lock),
            takeoff: parse_slot(own, "takeoff", def.takeoff),
            target_lock: parse_slot(own, "targetLock", def.target_lock),
            fire_primary: parse_slot(own, "firePrimary", def.fire_primary),
            fire_secondary: parse_slot(own, "fireSecondary", def.fire_secondary),
            perspective: parse_slot(own, "perspective", def.perspective),
            use_key: parse_slot(own, "use", def.use_key),
            next_primary: parse_slot(own, "nextPrimary", def.next_primary),
            prev_primary: parse_slot(own, "previousPrimary", def.prev_primary),
        }
    }
}

// This state belongs to one window. It survives redraws, but a focus change
// clears it before any resumed World step can observe stale held buttons.
pub struct Input {
    keys: KeyState,
    mouse_buttons: [bool; 5],
    actions: u32,
    look: [f64; 2],
    // Lifetime admitted displacement survives sampling and overlay clears.
    // The pause snapshot can compare real device counts with displayed turn.
    pointer_total: [f64; 2],
    mouse_scale: f64,
    fire: [u32; 2],
    fire_id: [u32; 2],
    sequence: u32,
    flight_actions: u32,
    pub view_actions: ViewActions,
    pub ground: GroundBindings,
    pub flight: FlightBindingsTable,
    pub flight_mode: bool,
    pub flight_scale: f64,
    pub flight_curve: f64,
    pub flight_invert: bool,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            keys: KeyState::default(),
            mouse_buttons: [false; 5],
            actions: 0,
            look: [0.0; 2],
            pointer_total: [0.; 2],
            mouse_scale: 1.,
            fire: [0; 2],
            fire_id: [0; 2],
            sequence: 0,
            flight_actions: 0,
            view_actions: ViewActions::default(),
            ground: GroundBindings::default(),
            flight: FlightBindingsTable::default(),
            flight_mode: false,
            flight_scale: 1.0,
            flight_curve: 1.35,
            flight_invert: false,
        }
    }
}

/// Window/UI intentions have no simulation memory offsets. The native menu
/// owner can consume these coalesced values alongside generated gameplay input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ViewActions {
    pub perspective: bool,
    pub map: bool,
    pub stats: bool,
}

struct KeyState([bool; KEYS]);
impl Default for KeyState {
    fn default() -> Self {
        Self([false; KEYS])
    }
}

impl Input {
    // Full-window device admission always includes a real lens and the shared
    // mouse multiplier. An overlay clears input but keeps the view record valid.
    pub fn sample(
        &mut self,
        tick: u32,
        generation: u32,
        active: bool,
        aspect: f64,
        mouse_scale: f64,
        invert: bool,
    ) -> (ClientCommand, ClientLook, ClientFlightCommand, ViewActions) {
        if !active {
            self.clear();
        }
        let (mut command, look) = self.take(tick, generation);
        command.aspect = aspect;
        command.has_aspect = true;
        self.mouse_scale = mouse_scale;
        command.mouse_scale = mouse_scale;
        command.has_mouse_scale = true;
        command.invert = Some(invert);
        (command, look, self.take_flight(), self.take_view())
    }

    /// Pointer counts and the last sampled multiplier, for cold control snapshots.
    pub fn pointer_metrics(&self) -> ([f64; 2], f64) {
        (self.pointer_total, self.mouse_scale)
    }

    pub fn clear(&mut self) {
        self.keys.0.fill(false);
        self.mouse_buttons.fill(false);
        self.actions = 0;
        self.look = [0.0; 2];
        self.fire = [0; 2];
        self.flight_actions = 0;
        self.view_actions = ViewActions::default();
    }

    /// Updates ground and starfighter key bindings from `keyBindings` and `flightBindings`.
    pub fn set_bindings(
        &mut self,
        key_bindings: &BTreeMap<String, Vec<String>>,
        flight_bindings: &BTreeMap<String, Vec<String>>,
    ) {
        self.ground = GroundBindings::from_overrides(key_bindings);
        self.flight = FlightBindingsTable::from_overrides(flight_bindings);
    }

    // Read aliases independently: releasing W must not release ArrowUp.
    // The canonical held mask is reconstructed only at command admission.
    fn down(&self, key: KeyCode) -> bool {
        self.keys.0.get(key as usize).copied().unwrap_or(false)
    }

    #[inline]
    fn trigger_down(&self, trig: InputTrigger) -> bool {
        match trig {
            InputTrigger::Key(k) => self.down(k),
            InputTrigger::Mouse(b) => self.mouse_buttons.get(b as usize).copied().unwrap_or(false),
            InputTrigger::None | InputTrigger::WheelUp | InputTrigger::WheelDown => false,
        }
    }

    #[inline]
    fn slot_down(&self, slot: &TriggerSlot) -> bool {
        slot.iter().any(|trigger| self.trigger_down(*trigger))
    }

    #[inline]
    fn primary_down(&self) -> bool {
        if self.flight_mode {
            self.slot_down(&self.flight.fire_primary)
        } else {
            self.slot_down(&self.ground.fire_primary)
        }
    }

    #[inline]
    fn secondary_down(&self) -> bool {
        if self.flight_mode {
            self.slot_down(&self.flight.fire_secondary)
        } else {
            self.slot_down(&self.ground.fire_secondary)
        }
    }

    fn action(&mut self, action: Action) {
        self.actions |= 1 << action as u8;
    }

    fn apply_edge(&mut self, trig: InputTrigger, suppress_stats_chord: bool) {
        if self.flight_mode {
            if slot_matches(&self.flight.use_key, trig) {
                self.action(Action::Use);
            }
            if slot_matches(&self.flight.next_primary, trig) {
                self.action(Action::NextPrimary);
            }
            if slot_matches(&self.flight.prev_primary, trig) {
                self.action(Action::PreviousPrimary);
            }
            if slot_matches(&self.flight.perspective, trig) {
                self.view_actions.perspective = true;
            }
        } else {
            if slot_matches(&self.ground.jump, trig) {
                self.action(Action::Jump);
            }
            if slot_matches(&self.ground.crouch, trig) {
                self.action(Action::Crouch);
            }
            if slot_matches(&self.ground.prone, trig) {
                self.action(Action::Prone);
            }
            if slot_matches(&self.ground.reload, trig) {
                self.action(Action::Reload);
            }
            if slot_matches(&self.ground.use_key, trig) {
                self.action(Action::Use);
            }
            if slot_matches(&self.ground.next_primary, trig) {
                self.action(Action::NextPrimary);
            }
            if slot_matches(&self.ground.prev_primary, trig) {
                self.action(Action::PreviousPrimary);
            }
            if slot_matches(&self.ground.next_secondary, trig) {
                self.action(Action::NextSecondary);
            }
            if slot_matches(&self.ground.zoom, trig) {
                self.action(Action::Zoom);
            }
            if slot_matches(&self.ground.perspective, trig) {
                self.view_actions.perspective = true;
            }
        }
        if !self.flight_mode {
            if slot_matches(&self.ground.map, trig) {
                self.view_actions.map = true;
            }
            if slot_matches(&self.ground.stats, trig)
                && !(suppress_stats_chord && trig == InputTrigger::Key(KeyCode::Backquote))
            {
                self.view_actions.stats = true;
            }
            // Squad orders go to the world in the command, as every other
            // press does (Controls.command, src/input.js): the server takes
            // them from the player's controls and answers with the order.
            if slot_matches(&self.ground.squad_at_ease, trig) {
                self.action(Action::SquadAtEase);
            }
            if slot_matches(&self.ground.squad_follow, trig) {
                self.action(Action::SquadFollow);
            }
            if slot_matches(&self.ground.squad_move_out, trig) {
                self.action(Action::SquadMoveOut);
            }
            if slot_matches(&self.ground.squad_hold, trig) {
                self.action(Action::SquadHold);
            }
        }
        if self.flight_mode {
            if slot_matches(&self.flight.takeoff, trig) {
                self.flight_actions |= 1 << FlightAction::Takeoff as u8;
            }
            if slot_matches(&self.flight.trick, trig) {
                self.flight_actions |= 1 << FlightAction::Trick as u8;
            }
            if slot_matches(&self.flight.immelmann, trig) {
                self.flight_actions |= 1 << FlightAction::Immelmann as u8;
            }
            if slot_matches(&self.flight.target_lock, trig) {
                self.flight_actions |= 1 << FlightAction::TargetLock as u8;
            }
        }
    }

    // Controls.keyDown queues on a physical rising edge, even when several
    // aliases name one action. Repeated OS keydown events stay coalesced.
    pub fn key(&mut self, key: KeyCode, pressed: bool) {
        self.key_with_modifiers(key, pressed, false);
    }

    /// Processes a physical key press/release while suppressing `Backquote` stats toggle
    /// when `shift_or_ctrl` is true (`overlayChord` in `src/input.js`).
    pub fn key_with_modifiers(&mut self, key: KeyCode, pressed: bool, shift_or_ctrl: bool) {
        let was_primary = self.primary_down();
        let was_secondary = self.secondary_down();
        let Some(was) = self.keys.0.get_mut(key as usize) else {
            return;
        };
        let edge = pressed && !*was;
        *was = pressed;
        if edge {
            self.apply_edge(InputTrigger::Key(key), shift_or_ctrl);
        }
        if !was_primary && self.primary_down() {
            self.press_fire(0);
        }
        // Alt aliases share the secondary-fire hold. Switching between them
        // while one remains down must not manufacture another fire edge.
        if !was_secondary && self.secondary_down() {
            self.press_fire(1);
        }
    }

    // Reliable fire identities are independent from movement sequence.
    // A press and release between ticks still reaches the next command.
    fn press_fire(&mut self, button: usize) {
        self.fire[button] = (self.fire[button] + 1).min(MAX_FIRE);
        self.fire_id[button] = self.fire_id[button].wrapping_add(1);
        self.action(if button == 0 {
            Action::FirePrimary
        } else {
            Action::FireSecondary
        });
    }

    // DOM Mouse0/1/2/3/4 are left/middle/right/back/forward. Preserve Controls'
    // physical defaults: middle zooms (flight roll-lock), right fires the secondary.
    // The network encoder names those physical codes in the shared profile.
    pub fn mouse(&mut self, button: MouseButton, pressed: bool) {
        let idx = match button {
            MouseButton::Left => 0usize,
            MouseButton::Middle => 1usize,
            MouseButton::Right => 2usize,
            MouseButton::Back => 3usize,
            MouseButton::Forward => 4usize,
            MouseButton::Other(_) => return,
        };
        let was_primary = self.primary_down();
        let was_secondary = self.secondary_down();
        let edge = pressed && !self.mouse_buttons[idx];
        self.mouse_buttons[idx] = pressed;
        if edge {
            self.apply_edge(InputTrigger::Mouse(idx as u8), false);
        }
        if !was_primary && self.primary_down() {
            self.press_fire(0);
        }
        if !was_secondary && self.secondary_down() {
            self.press_fire(1);
        }
    }

    // Controls treats a wheel event as one cycling intention. Magnitude does
    // not enqueue hundreds of weapon changes from a high-resolution wheel.
    pub fn wheel(&mut self, y: f64) {
        let trig = if y > 0.0 {
            InputTrigger::WheelUp
        } else if y < 0.0 {
            InputTrigger::WheelDown
        } else {
            return;
        };
        self.apply_edge(trig, false);
        let fire_pri = if self.flight_mode {
            &self.flight.fire_primary
        } else {
            &self.ground.fire_primary
        };
        let fire_sec = if self.flight_mode {
            &self.flight.fire_secondary
        } else {
            &self.ground.fire_secondary
        };
        let primary = slot_matches(fire_pri, trig);
        let secondary = slot_matches(fire_sec, trig);
        if primary {
            self.press_fire(0);
        }
        if secondary {
            self.press_fire(1);
        }
    }

    // Controls.look accumulates raw relative pixels; scaling belongs to the
    // game, not the OS cursor. Saturate stalled-device input without infinity.
    pub fn motion(&mut self, x: f64, y: f64) {
        for (at, delta) in [x, y].into_iter().enumerate() {
            if delta.is_finite() {
                self.look[at] = (self.look[at] + delta).clamp(-1e6, 1e6);
                self.pointer_total[at] += delta;
            }
        }
    }

    // flight/controls.js uses a separate scheme over the same physical keys.
    // Publish its generated record beside the infantry command, so the World
    // chooses its current seat mode without asking the window for key names.
    pub fn take_flight(&mut self) -> ClientFlightCommand {
        let mut held = 0;
        for (on, flag) in [
            (self.slot_down(&self.flight.throttle), FlightHeld::Throttle),
            (self.slot_down(&self.flight.brake), FlightHeld::Brake),
            (
                self.slot_down(&self.flight.roll_right),
                FlightHeld::RollRight,
            ),
            (self.slot_down(&self.flight.roll_left), FlightHeld::RollLeft),
            (self.slot_down(&self.flight.boost), FlightHeld::Boost),
            (self.slot_down(&self.flight.roll_lock), FlightHeld::RollLock),
        ] {
            if on {
                held |= 1 << flag as u8;
            }
        }
        let mut default_held = 0;
        for (on, flag) in [
            (self.down(KeyCode::KeyW), FlightHeld::Throttle),
            (self.down(KeyCode::KeyS), FlightHeld::Brake),
            (self.down(KeyCode::KeyD), FlightHeld::RollRight),
            (self.down(KeyCode::KeyA), FlightHeld::RollLeft),
            (
                self.down(KeyCode::ShiftLeft) || self.down(KeyCode::ShiftRight),
                FlightHeld::Boost,
            ),
            (
                self.down(KeyCode::KeyZ) || self.mouse_buttons[1],
                FlightHeld::RollLock,
            ),
        ] {
            if on {
                default_held |= 1 << flag as u8;
            }
        }
        let result = ClientFlightCommand {
            held,
            default_held,
            actions: self.flight_actions,
            scale: self.flight_scale,
            curve: self.flight_curve,
            invert: self.flight_invert,
            ..Default::default()
        };
        self.flight_actions = 0;
        result
    }

    pub fn take_view(&mut self) -> ViewActions {
        std::mem::take(&mut self.view_actions)
    }

    // worldKeys passes through raw codes only when neither the active map
    // nor its canonical world keys consume them. Ground Space is a brake;
    // flight remaps takeoff to the canonical Space hold instead.
    fn raw_world_key(&self, key: KeyCode) -> bool {
        let trigger = InputTrigger::Key(key);
        let mapped = if self.flight_mode {
            [
                &self.flight.throttle,
                &self.flight.brake,
                &self.flight.roll_left,
                &self.flight.roll_right,
                &self.flight.boost,
                &self.flight.trick,
                &self.flight.immelmann,
                &self.flight.roll_lock,
                &self.flight.takeoff,
                &self.flight.target_lock,
                &self.flight.fire_primary,
                &self.flight.fire_secondary,
                &self.flight.perspective,
                &self.flight.use_key,
                &self.flight.next_primary,
                &self.flight.prev_primary,
            ]
            .iter()
            .any(|slot| slot_matches(slot, trigger))
        } else {
            [
                &self.ground.forward_plus,
                &self.ground.forward_minus,
                &self.ground.right_plus,
                &self.ground.right_minus,
                &self.ground.turn_plus,
                &self.ground.turn_minus,
                &self.ground.fire_primary,
                &self.ground.fire_secondary,
            ]
            .iter()
            .any(|slot| slot_matches(slot, trigger))
        };
        self.down(key) && !mapped
    }

    // ClientCommand carries only generated numeric fields. look_at is a word
    // offset in a packed block, while the native World receives a typed borrow.
    // Taking a command drains edges and look, but held keys survive until release.
    pub fn take(&mut self, tick: u32, generation: u32) -> (ClientCommand, ClientLook) {
        let mut held = 0;
        let (forward, backward, right, left) = if self.flight_mode {
            (
                self.slot_down(&self.flight.throttle),
                self.slot_down(&self.flight.brake),
                self.slot_down(&self.flight.roll_right),
                self.slot_down(&self.flight.roll_left),
            )
        } else {
            (
                self.slot_down(&self.ground.forward_plus),
                self.slot_down(&self.ground.forward_minus),
                self.slot_down(&self.ground.right_plus),
                self.slot_down(&self.ground.right_minus),
            )
        };
        for (on, flag) in [
            (forward, Held::Forward),
            (backward, Held::Backward),
            (right, Held::Right),
            (left, Held::Left),
            (
                if self.flight_mode {
                    self.raw_world_key(KeyCode::NumpadMultiply)
                } else {
                    self.slot_down(&self.ground.turn_plus)
                },
                Held::TurnRight,
            ),
            (
                if self.flight_mode {
                    self.raw_world_key(KeyCode::NumpadDivide)
                } else {
                    self.slot_down(&self.ground.turn_minus)
                },
                Held::TurnLeft,
            ),
            (self.primary_down(), Held::Primary),
            (self.secondary_down(), Held::Secondary),
            (
                if self.flight_mode {
                    self.slot_down(&self.flight.takeoff)
                } else {
                    self.raw_world_key(KeyCode::Space)
                },
                Held::Space,
            ),
        ] {
            if on {
                held |= 1 << flag as u8;
            }
        }
        self.sequence = self.sequence.wrapping_add(1);
        let command = ClientCommand {
            tick: tick as f64,
            generation: generation as f64,
            sequence: self.sequence as f64,
            held,
            actions: self.actions,
            look_at: ClientCommand::WORDS as u32,
            look_count: 1,
            mouse_scale: 1.0,
            has_mouse_scale: true,
            invert: Some(false),
            primary_count: self.fire[0],
            primary_id: self.fire_id[0] as f64,
            secondary_count: self.fire[1],
            secondary_id: self.fire_id[1] as f64,
            ..Default::default()
        };
        let look = ClientLook {
            x: self.look[0],
            y: self.look[1],
        };
        self.actions = 0;
        self.look = [0.0; 2];
        self.fire = [0; 2];
        (command, look)
    }
}
