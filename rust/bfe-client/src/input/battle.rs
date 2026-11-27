//! Select the battle's device bindings before sampling a command.
use bfe_wide::Wide;
use super::{
    Action, ClientCommand, ClientFlightCommand, ClientLook, FlightAction, FlightHeld, Held, Input, InputTrigger,
    KeyCode, MouseButton, ViewActions, KEYS,
};
use bfe_family::GameFamily;
use bfe_interface::{hotkeys::ControlContext, Settings};

pub type BattleCommands = (ClientCommand, ClientLook, ClientFlightCommand, ViewActions);

/// Battlefront II intentions without a corresponding released command field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Swbf2Actions {
    pub use_held: bool,
    pub offhand_pressed: bool,
    pub sprint: bool,
    pub roll: bool,
    pub block: bool,
    pub block_pressed: bool,
    pub melee_held: u8,
    pub melee_pressed: u8,
    pub target_lock: bool,
    pub boost: bool,
    pub zoom_in: bool,
    pub zoom_out: bool,
    pub accept: bool,
    pub decline: bool,
    pub squad_command: bool,
}

impl Swbf2Actions {
    /// Puts a 2005 soldier's sprint, block and roll into a network command,
    /// whose wire form and prediction read them there. Instant Action's world
    /// takes them beside its command instead.
    pub fn put(&self, command: &mut ClientCommand) {
        for (on, flag) in [(self.sprint, Held::Sprint), (self.block, Held::Block)] {
            if on {
                command.held |= 1 << flag as u8;
            }
        }
        if self.roll {
            command.actions |= 1 << Action::Roll as u8;
        }
    }

    /// The rest of what a network command's soldier holds and taps.
    pub fn soldier(&self, hold_to_block: bool) -> crate::input_family::SoldierInput {
        use bfe_sim::combat::combo::ComboButton;
        let held = |button: ComboButton| self.melee_held & button.bit() != 0;
        crate::input_family::SoldierInput {
            block_pressed: self.block_pressed,
            crouch: held(ComboButton::Crouch),
            reload: held(ComboButton::Reload),
            hold_to_block,
            lock_pressed: self.target_lock,
        }
    }
}

#[derive(Clone, Copy)]
enum Intent {
    Forward,
    Back,
    Right,
    Left,
    TurnRight,
    TurnLeft,
    Primary,
    Secondary,
    Jump,
    Crouch,
    Use,
    Reload,
    NextPrimary,
    PreviousPrimary,
    NextSecondary,
    Zoom,
    View,
    Map,
    Sprint,
    Roll,
    Block,
    GroundLock,
    Boost,
    Takeoff,
    Trick,
    Immelmann,
    PilotLock,
    RollLock,
    Throttle,
    Brake,
    RollRight,
    RollLeft,
    ZoomIn,
    ZoomOut,
    Accept,
    Decline,
    Squad,
    Roster,
    Chat,
    TeamChat,
}

impl Intent {
    fn for_action(action: &str) -> Self {
        match action {
            "axes.forward+" => Self::Forward,
            "axes.forward-" => Self::Back,
            "axes.right+" => Self::Right,
            "axes.right-" => Self::Left,
            "axes.turn+" => Self::TurnRight,
            "axes.turn-" => Self::TurnLeft,
            "holds.firePrimary" | "firePrimary" => Self::Primary,
            "holds.fireSecondary" | "fireSecondary" => Self::Secondary,
            "presses.jump" => Self::Jump,
            "presses.crouch" => Self::Crouch,
            "presses.use" | "use" => Self::Use,
            "presses.reload" => Self::Reload,
            "presses.nextPrimary" | "nextPrimary" => Self::NextPrimary,
            "presses.previousPrimary" | "previousPrimary" => Self::PreviousPrimary,
            "presses.nextSecondary" => Self::NextSecondary,
            "presses.zoom" => Self::Zoom,
            "presses.perspective" | "perspective" => Self::View,
            "presses.map" => Self::Map,
            "holds.sprint" => Self::Sprint,
            "presses.roll" => Self::Roll,
            "holds.block" => Self::Block,
            "presses.targetLock" => Self::GroundLock,
            "holds.boost" | "boost" => Self::Boost,
            "takeoff" => Self::Takeoff,
            "trick" => Self::Trick,
            "immelmann" => Self::Immelmann,
            "targetLock" => Self::PilotLock,
            "rollLock" => Self::RollLock,
            "throttle" => Self::Throttle,
            "brake" => Self::Brake,
            "rollRight" => Self::RollRight,
            "rollLeft" => Self::RollLeft,
            "holds.zoomIn" => Self::ZoomIn,
            "holds.zoomOut" => Self::ZoomOut,
            "presses.accept" => Self::Accept,
            "presses.decline" => Self::Decline,
            "presses.squadCommand" => Self::Squad,
            "ui.roster" => Self::Roster,
            "ui.chat" => Self::Chat,
            "ui.teamChat" => Self::TeamChat,
            _ => unreachable!("unmapped control row {action}"),
        }
    }

    fn bit(self) -> Wide {
        Wide::bit(self as u32)
    }
}

const DEVICES: usize = KEYS + 7;
type Bank = [Wide; DEVICES];

fn index(trigger: InputTrigger) -> Option<usize> {
    match trigger {
        InputTrigger::Key(key) if (key as usize) < KEYS => Some(key as usize),
        InputTrigger::Mouse(button @ 0..=4) => Some(KEYS + button as usize),
        InputTrigger::WheelUp => Some(KEYS + 5),
        InputTrigger::WheelDown => Some(KEYS + 6),
        _ => None,
    }
}

fn banks(settings: &Settings) -> Box<[Bank; 5]> {
    Box::new(ControlContext::ALL.map(|context| {
        let mut bank = [Wide::ZERO; DEVICES];
        for (action, codes) in context.bindings(settings) {
            let bit = Intent::for_action(&action).bit();
            for device in codes.iter().filter_map(|code| InputTrigger::from_dom_code(code).and_then(index)) {
                bank[device] = bank[device].or(bit);
            }
        }
        bank
    }))
}

struct Swbf2Input {
    banks: Box<[Bank; 5]>,
    context: ControlContext,
    down: [bool; DEVICES],
    // A roster release belongs to the overlay that received its press, even after a seat change.
    roster_down: [bool; DEVICES],
    held: Wide,
    edges: Wide,
    impulses: Wide,
}

impl Swbf2Input {
    fn held_keys(&self) -> Wide {
        self.banks[self.context as usize]
            .iter()
            .zip(&self.down)
            .filter(|(_, down)| **down)
            .fold(Wide::ZERO, |bits, (binding, _)| bits.or(*binding))
    }

    fn event(&mut self, device: &mut Input, trigger: InputTrigger, down: bool) {
        let Some(slot) = index(trigger) else { return };
        let bits = self.banks[self.context as usize][slot];
        let fire = if matches!(trigger, InputTrigger::WheelUp | InputTrigger::WheelDown) {
            self.impulses = self.impulses.or(bits);
            self.edges = self.edges.or(bits);
            bits
        } else {
            if self.down[slot] == down {
                return;
            }
            self.down[slot] = down;
            if !down {
                self.held = self.held_keys();
                return;
            }
            let rising = bits.and(self.held.not());
            self.edges = self.edges.or(rising);
            self.held = self.held.or(bits);
            rising
        };
        for (button, intent) in [Intent::Primary, Intent::Secondary].into_iter().enumerate() {
            if !fire.and(intent.bit()).is_zero() {
                device.press_fire(button);
            }
        }
    }

    fn sample(&mut self, commands: &mut BattleCommands) -> Swbf2Actions {
        use Intent::*;
        let edges = std::mem::take(&mut self.edges);
        let holds = self.held.or(std::mem::take(&mut self.impulses));
        let pressed = |intent: Intent| !edges.and(intent.bit()).is_zero();
        let held = |intent: Intent| !holds.and(intent.bit()).is_zero();
        let (command, _, flight, view) = commands;
        for (intent, action) in [
            (Jump, Action::Jump),
            (Crouch, Action::Crouch),
            (Use, Action::Use),
            (Reload, Action::Reload),
            (NextPrimary, Action::NextPrimary),
            (PreviousPrimary, Action::PreviousPrimary),
            (NextSecondary, Action::NextSecondary),
            (Zoom, Action::Zoom),
        ] {
            if pressed(intent) {
                command.actions |= 1 << action as u8;
            }
        }
        for (intent, flag) in [
            (Forward, Held::Forward),
            (Back, Held::Backward),
            (Right, Held::Right),
            (Left, Held::Left),
            (TurnRight, Held::TurnRight),
            (TurnLeft, Held::TurnLeft),
            (Primary, Held::Primary),
            (Secondary, Held::Secondary),
            (Throttle, Held::Forward),
            (Brake, Held::Backward),
            (RollRight, Held::Right),
            (RollLeft, Held::Left),
            (Jump, Held::Space),
            (Takeoff, Held::Space),
        ] {
            if held(intent) {
                command.held |= 1 << flag as u8;
            }
        }
        let pilot = self.context == ControlContext::Starfighter;
        if pilot {
            for (intent, flag) in [
                (Throttle, FlightHeld::Throttle),
                (Brake, FlightHeld::Brake),
                (RollLeft, FlightHeld::RollLeft),
                (RollRight, FlightHeld::RollRight),
                (Boost, FlightHeld::Boost),
                (RollLock, FlightHeld::RollLock),
            ] {
                if held(intent) {
                    flight.held |= 1 << flag as u8;
                }
            }
            for (intent, flag) in [
                (Takeoff, FlightAction::Takeoff),
                (Trick, FlightAction::Trick),
                (Immelmann, FlightAction::Immelmann),
                (PilotLock, FlightAction::TargetLock),
            ] {
                if pressed(intent) {
                    flight.actions |= 1 << flag as u8;
                }
            }
        }
        flight.default_held = flight.held;
        view.perspective = pressed(View);
        view.map = pressed(Map);
        use bfe_sim::combat::combo::ComboButton;
        let mut melee_held = 0;
        let mut melee_pressed = 0;
        for (intent, button) in [
            (Primary, ComboButton::Fire),
            (Jump, ComboButton::Jump),
            (Crouch, ComboButton::Crouch),
            (Sprint, ComboButton::Sprint),
            (Reload, ComboButton::Reload),
        ] {
            if held(intent) { melee_held |= button.bit(); }
            if pressed(intent) { melee_pressed |= button.bit(); }
        }
        Swbf2Actions {
            use_held: held(Use),
            offhand_pressed: pressed(Secondary),
            melee_held,
            melee_pressed,
            sprint: held(Sprint),
            block: held(Block), block_pressed: pressed(Block),
            roll: pressed(Roll),
            target_lock: pressed(GroundLock),
            boost: !pilot && held(Boost),
            zoom_in: held(ZoomIn),
            zoom_out: held(ZoomOut),
            accept: pressed(Accept),
            decline: pressed(Decline),
            squad_command: pressed(Squad),
        }
    }
}

/// One local or online battle. The device owns shared motion, fire identities and the sample clock.
#[derive(Default)]
pub struct BattleInput {
    pub device: Input,
    swbf2: Option<Swbf2Input>,
}

impl BattleInput {
    pub fn set_profile(&mut self, family: GameFamily, settings: &Settings) {
        match family {
            GameFamily::Swbf1 => {
                if self.swbf2.take().is_some() {
                    self.device.clear();
                    self.device.flight_mode = false;
                }
                let empty = Default::default();
                self.device.set_bindings(
                    settings.get_bindings_ref("keyBindings").unwrap_or(&empty),
                    settings.get_bindings_ref("flightBindings").unwrap_or(&empty),
                );
            }
            GameFamily::Swbf2 => {
                let banks = banks(settings);
                if let Some(input) = &mut self.swbf2 {
                    input.banks = banks;
                    input.held = input.held_keys();
                } else {
                    self.device.clear();
                    self.device.flight_mode = false;
                    self.swbf2 = Some(Swbf2Input {
                        banks,
                        context: ControlContext::Infantry,
                        down: [false; DEVICES],
                        roster_down: [false; DEVICES],
                        held: Wide::ZERO,
                        edges: Wide::ZERO,
                        impulses: Wide::ZERO,
                    });
                }
            }
        }
    }

    /// Held devices take on the new role immediately. A seat change never invents a press.
    pub fn set_context(&mut self, context: ControlContext) {
        let Some(input) = &mut self.swbf2 else { return };
        if input.context != context {
            input.context = context;
            input.held = input.held_keys();
            input.edges = Wide::ZERO;
            input.impulses = Wide::ZERO;
            self.device.actions = 0;
            self.device.fire = [0; 2];
        }
    }

    /// Overlay shortcuts are delivered to the local window, outside the command stream.
    pub fn overlay_event(&mut self, trigger: InputTrigger, down: bool, can_open: bool) -> Option<&'static str> {
        let input = self.swbf2.as_mut()?;
        let slot = index(trigger)?;
        if !down {
            if !matches!(trigger, InputTrigger::WheelUp | InputTrigger::WheelDown) {
                input.event(&mut self.device, trigger, false);
            }
            return std::mem::take(&mut input.roster_down[slot]).then_some("Tab");
        }
        if !can_open { return None; }
        let bits = input.banks[input.context as usize][slot];
        input.roster_down[slot] = !bits.and(Intent::Roster.bit()).is_zero();
        [(Intent::Roster, "Tab"), (Intent::Chat, "KeyT"), (Intent::TeamChat, "KeyY")]
            .into_iter()
            .find_map(|(intent, key)| (!bits.and(intent.bit()).is_zero()).then_some(key))
    }

    pub fn motion(&mut self, x: f64, y: f64) {
        self.device.motion(x, y);
    }

    pub fn pointer_metrics(&self) -> ([f64; 2], f64) {
        self.device.pointer_metrics()
    }

    pub fn key(&mut self, code: KeyCode, down: bool) {
        self.key_with_modifiers(code, down, false);
    }

    pub fn key_with_modifiers(&mut self, code: KeyCode, down: bool, chord: bool) {
        match &mut self.swbf2 {
            Some(input) => input.event(&mut self.device, InputTrigger::Key(code), down),
            None => self.device.key_with_modifiers(code, down, chord),
        }
    }

    pub fn mouse(&mut self, button: MouseButton, down: bool) {
        let Some(input) = &mut self.swbf2 else { return self.device.mouse(button, down) };
        let id = match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
            MouseButton::Back => 3,
            MouseButton::Forward => 4,
            MouseButton::Other(_) => return,
        };
        input.event(&mut self.device, InputTrigger::Mouse(id), down);
    }

    pub fn wheel(&mut self, y: f64) {
        let Some(input) = &mut self.swbf2 else { return self.device.wheel(y) };
        let trigger = if y > 0. {
            InputTrigger::WheelUp
        } else if y < 0. {
            InputTrigger::WheelDown
        } else {
            return;
        };
        input.event(&mut self.device, trigger, true);
    }

    pub fn clear(&mut self) {
        self.device.clear();
        if let Some(input) = &mut self.swbf2 {
            input.down.fill(false);
            input.held = Wide::ZERO;
            input.edges = Wide::ZERO;
            input.impulses = Wide::ZERO;
        }
    }

    pub fn sample_battle(
        &mut self,
        tick: u32,
        generation: u32,
        active: bool,
        aspect: f64,
        mouse_scale: f64,
        invert: bool,
    ) -> (BattleCommands, Swbf2Actions) {
        if !active && self.swbf2.is_some() {
            self.clear();
        }
        let mut commands = self.device.sample(tick, generation, active, aspect, mouse_scale, invert);
        let extra = self.swbf2.as_mut().map(|input| input.sample(&mut commands)).unwrap_or_default();
        (commands, extra)
    }
}
