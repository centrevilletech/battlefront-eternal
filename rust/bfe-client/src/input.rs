//! input.js::Controls defaults, expressed with the generated prediction schema.
//! Presses coalesce into ClientCommand action bits while a frame stalls. Key
//! repeat cannot enqueue another action, and focus loss clears held fire.
use crate::generated_input::{ClientAction as Action, ClientCommand, ClientHeld as Held, ClientLook, ClientFlightCommand, ClientFlightHeld as FlightHeld, ClientFlightAction as FlightAction};
use winit::{event::MouseButton, keyboard::KeyCode};

// KeyCode discriminants are local device identifiers, never ABI offsets.
// Unsupported physical keys are ignored with checked access below.
const KEYS: usize = 256;
// Match the protocol fire-edge bound when events arrive without a tick.
// Counts saturate; the latest edge id still identifies the observed press.
const MAX_FIRE: u32 = 32;
// This state belongs to one window. It survives redraws, but a focus change
// clears it before any resumed World step can observe stale held buttons.
#[derive(Default)]
pub struct Input {
    keys: KeyState,
    primary: bool,
    middle: bool,
    right_mouse: bool,
    actions: u32,
    look: [f64; 2],
    fire: [u32; 2],
    fire_id: [u32; 2],
    sequence: u32,
    flight_actions: u32,
    pub view_actions: ViewActions,
}
/// Window/UI intentions have no simulation memory offsets. The native menu
/// owner can consume these coalesced values alongside generated gameplay input.
#[derive(Default)]
pub struct ViewActions {
    pub perspective: bool,
    pub map: bool,
    pub stats: bool,
    pub squad: Option<SquadOrder>,
}
// Keep squad intent semantic until a World supplies its qualified source and
// target. The window has no battle ids with which to form a SquadCommand.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SquadOrder { MoveOut, AtEase, Hold, Follow }
struct KeyState([bool; KEYS]);
impl Default for KeyState { fn default() -> Self { Self([false; KEYS]) } }
impl Input {
    pub fn clear(&mut self) {
        self.keys.0.fill(false);
        self.primary = false;
        self.middle = false;
        self.right_mouse = false;
        self.actions = 0;
        self.look = [0.0; 2];
        self.fire = [0; 2];
        self.flight_actions = 0;
        self.view_actions = ViewActions::default();
    }
    // Read aliases independently: releasing W must not release ArrowUp.
    // The canonical held mask is reconstructed only at command admission.
    fn down(&self, key: KeyCode) -> bool { self.keys.0.get(key as usize).copied().unwrap_or(false) }
    fn action(&mut self, action: Action) { self.actions |= 1 << action as u8; }
    // Controls.keyDown queues on a physical rising edge, even when several
    // aliases name one action. Repeated OS keydown events stay coalesced.
    pub fn key(&mut self, key: KeyCode, pressed: bool) {
        let secondary = self.secondary();
        let Some(was) = self.keys.0.get_mut(key as usize) else { return; };
        let edge = pressed && !*was;
        *was = pressed;
        if edge {
            use KeyCode::*;
            match key {
                Space => { self.action(Action::Jump); self.flight_actions |= 1 << FlightAction::Takeoff as u8; },
                KeyQ => { self.view_actions.perspective = true; self.flight_actions |= 1 << FlightAction::TargetLock as u8; },
                KeyV => self.view_actions.perspective = true,
                KeyM => self.view_actions.map = true,
                Backquote => self.view_actions.stats = true,
                F1 => self.view_actions.squad = Some(SquadOrder::AtEase),
                F2 => self.view_actions.squad = Some(SquadOrder::Follow),
                F3 => self.view_actions.squad = Some(SquadOrder::MoveOut),
                F4 => self.view_actions.squad = Some(SquadOrder::Hold),
                CapsLock => self.flight_actions |= 1 << FlightAction::Immelmann as u8,
                AltLeft => self.flight_actions |= 1 << FlightAction::Trick as u8,
                KeyC | ControlLeft | ControlRight => self.action(Action::Crouch),
                KeyX => self.action(Action::Prone),
                KeyR | Enter => self.action(Action::Reload),
                KeyE | Numpad0 => self.action(Action::Use),
                KeyF => self.action(Action::NextPrimary),
                KeyG => self.action(Action::NextSecondary),
                KeyZ => self.action(Action::Zoom),
                _ => (),
            }
        }
        if edge && key == KeyCode::ControlRight { self.flight_actions |= 1 << FlightAction::Trick as u8; }
        // Alt aliases share the secondary-fire hold. Switching between them
        // while one remains down must not manufacture another fire edge.
        if !secondary && self.secondary() { self.press_fire(1); }
    }
    fn secondary(&self) -> bool {
        self.middle || self.down(KeyCode::AltLeft) || self.down(KeyCode::AltRight)
    }
    // Reliable fire identities are independent from movement sequence.
    // A press and release between ticks still reaches the next command.
    fn press_fire(&mut self, button: usize) {
        self.fire[button] = (self.fire[button] + 1).min(MAX_FIRE);
        self.fire_id[button] = self.fire_id[button].wrapping_add(1);
        self.action(if button == 0 { Action::FirePrimary } else { Action::FireSecondary });
    }
    // DOM Mouse0/1/2 map to left/right/middle, rather than left/middle/right.
    // Keep that distinction: the original controls zoom with the right button.
    pub fn mouse(&mut self, button: MouseButton, pressed: bool) {
        match button {
            MouseButton::Left => {
                if pressed && !self.primary { self.press_fire(0); }
                self.primary = pressed;
            }
            MouseButton::Middle => {
                let was = self.secondary();
                self.middle = pressed;
                if !was && self.secondary() { self.press_fire(1); }
            }
            MouseButton::Right => {
                if pressed && !self.right_mouse { self.action(Action::Zoom); }
                self.right_mouse = pressed;
            },
            MouseButton::Back if pressed => {
                self.view_actions.perspective = true;
                self.flight_actions |= 1 << FlightAction::TargetLock as u8;
            },
            _ => (),
        }
    }
    // Controls treats a wheel event as one cycling intention. Magnitude does
    // not enqueue hundreds of weapon changes from a high-resolution wheel.
    pub fn wheel(&mut self, y: f64) {
        if y > 0.0 { self.action(Action::NextPrimary); }
        if y < 0.0 { self.action(Action::PreviousPrimary); }
    }
    // Controls.look accumulates raw relative pixels; scaling belongs to the
    // game, not the OS cursor. Saturate stalled-device input without infinity.
    pub fn motion(&mut self, x: f64, y: f64) {
        for (at, delta) in [x, y].into_iter().enumerate() {
            if delta.is_finite() { self.look[at] = (self.look[at] + delta).clamp(-1e6, 1e6); }
        }
    }
    // flight/controls.js uses a separate scheme over the same physical keys.
    // Publish its generated record beside the infantry command, so the World
    // chooses its current seat mode without asking the window for key names.
    pub fn take_flight(&mut self) -> ClientFlightCommand {
        let mut held = 0;
        for (on, flag) in [
            (self.down(KeyCode::KeyW), FlightHeld::Throttle),
            (self.down(KeyCode::KeyS), FlightHeld::Brake),
            (self.down(KeyCode::KeyD), FlightHeld::RollRight),
            (self.down(KeyCode::KeyA), FlightHeld::RollLeft),
            (self.down(KeyCode::ShiftLeft) || self.down(KeyCode::ShiftRight), FlightHeld::Boost),
            (self.down(KeyCode::KeyZ) || self.right_mouse, FlightHeld::RollLock),
        ] { if on { held |= 1 << flag as u8; } }
        let result = ClientFlightCommand { held, default_held: held, actions: self.flight_actions,
            scale: 1.0, curve: 1.35, ..Default::default() };
        self.flight_actions = 0;
        result
    }
    pub fn take_view(&mut self) -> ViewActions { std::mem::take(&mut self.view_actions) }
    // ClientCommand carries only generated numeric fields. look_at is a word
    // offset in a packed block, while the native World receives a typed borrow.
    // Taking a command drains edges and look, but held keys survive until release.
    pub fn take(&mut self, tick: u32, generation: u32) -> (ClientCommand, ClientLook) {
        let mut held = 0;
        use KeyCode::*;
        for (on, flag) in [
            (self.down(KeyW) || self.down(ArrowUp), Held::Forward),
            (self.down(KeyS) || self.down(ArrowDown), Held::Backward),
            (self.down(KeyD) || self.down(ArrowRight), Held::Right),
            (self.down(KeyA) || self.down(ArrowLeft), Held::Left),
            (self.down(NumpadMultiply), Held::TurnRight),
            (self.down(NumpadDivide), Held::TurnLeft),
            (self.primary, Held::Primary), (self.secondary(), Held::Secondary),
            (self.down(Space), Held::Space),
        ] { if on { held |= 1 << flag as u8; } }
        self.sequence = self.sequence.wrapping_add(1);
        let command = ClientCommand {
            tick: tick as f64, generation: generation as f64, sequence: self.sequence as f64,
            held, actions: self.actions, look_at: ClientCommand::WORDS as u32,
            look_count: 1, mouse_scale: 1.0, has_mouse_scale: true, invert: Some(false),
            primary_count: self.fire[0], primary_id: self.fire_id[0] as f64,
            secondary_count: self.fire[1], secondary_id: self.fire_id[1] as f64,
            ..Default::default()
        };
        let look = ClientLook { x: self.look[0], y: self.look[1] };
        self.actions = 0;
        self.look = [0.0; 2];
        self.fire = [0; 2];
        (command, look)
    }
}
