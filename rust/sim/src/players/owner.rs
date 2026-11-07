//! The connected players a dedicated match keeps (src/runtime/
//! multiplayer-world.js `players`): each one's command queues, connection
//! facts and the per-body facts the JavaScript kept on its Unit objects.
//! Bodies are pooled, so those facts are kept by body slot, as the objects
//! were, and outlive the player who set them exactly as the source's did.
use super::controls::Controls;
use crate::flight::model::FlightInput;
use crate::gen::{F3PhaseInput, F3PhaseNetwork, F4Handle, HsCommand, IaState, IfResult, Intent, PlayerShots, PlayerView, Unit};
use crate::store::{block::Block, handles::Pool, Ref, StrId, Strings};

/// How many players a match admits at most (world-worker's 64 connections
/// plus the dedicated world's own viewpoint), the size of every view block.
pub const PLAYER_CAPACITY: usize = 65;
/// MAX_INPUT_LEAD_TICKS and ONLINE_PLAYOUT_MAX_TICKS (src/runtime/network-hit-limits.js).
pub const MAX_INPUT_LEAD_TICKS: f64 = 24.0;
pub const ONLINE_PLAYOUT_MAX_TICKS: f64 = 10.0;

/// One of his trigger presses this tick (FireCommands.take's shots).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Shot {
    pub id: f64,
    pub tick: f64,
    pub late: f64,
    pub aim_yaw: Option<f64>,
    pub aim_pitch: Option<f64>,
    pub view_tick: Option<f64>,
}

/// Unit.networkShots and the network facts beside it: present once a
/// player's controls or his stop have written them to the body.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NetworkFacts {
    pub shots: [Option<Shot>; 2],
    pub view: Option<f64>,
    pub lead: Option<f64>,
    pub playout: Option<f64>,
}

/// Unit.intent.flight: never written, `{}` after his input stopped, or
/// the seat's keys as the hull takes them (boarding.js seatInput).
#[derive(Clone, Copy, Debug, Default)]
pub enum Flight {
    #[default]
    Absent,
    Empty,
    Keys(FlightInput),
}

/// What the JavaScript kept on a body's Unit object between ticks.
#[derive(Clone, Debug, Default)]
pub struct UnitShadow {
    pub network: Option<NetworkFacts>,
    pub flight: Flight,
    /// His intent as his controls or his stop last wrote it, over the
    /// body's published one; `fresh` until the unit phase's row reads it.
    pub intent: Option<Intent>,
    pub fresh: bool,
}

/// The string ids the controls are read with: the world's key tables
/// (src/input.js WORLD_AXES, HOLDS, PRESSES) and the flight package's
/// default bindings (src/flight/controls.js FLIGHT_BINDINGS).
pub struct Names {
    pub forward: [[StrId; 2]; 2],
    pub right: [[StrId; 2]; 2],
    pub turn: [[StrId; 1]; 2],
    pub fire_primary: [StrId; 1],
    pub fire_secondary: [StrId; 3],
    pub flight_fire_primary: [StrId; 1],
    pub flight_fire_secondary: [StrId; 1],
    pub space: StrId,
    pub control_left: StrId,
    pub mouse0: StrId,
    pub mouse2: StrId,
    pub squads: [StrId; 4],
    pub next_spawn: StrId,
    /// jump, crouch, prone, use, reload, nextPrimary, previousPrimary,
    /// nextSecondary, zoom: applyNetworkControls' order.
    pub actions: [StrId; 9],
    /// Flight bindings by flightInput's names: throttle, brake, rollLeft,
    /// rollRight, boost, trick, immelmann, rollLock, takeoff, targetLock.
    pub flight: [(StrId, [Option<StrId>; 2]); 10],
    pub empty: StrId,
}

impl Names {
    pub fn new(strings: &mut Strings) -> Self {
        let mut s = |text: &str| strings.intern_str(text);
        let key_w = s("KeyW"); let arrow_up = s("ArrowUp"); let key_s = s("KeyS"); let arrow_down = s("ArrowDown");
        let key_d = s("KeyD"); let arrow_right = s("ArrowRight"); let key_a = s("KeyA"); let arrow_left = s("ArrowLeft");
        let mouse0 = s("Mouse0"); let mouse2 = s("Mouse2"); let space = s("Space");
        Self {
            forward: [[key_w, arrow_up], [key_s, arrow_down]],
            right: [[key_d, arrow_right], [key_a, arrow_left]],
            turn: [[s("NumpadMultiply")], [s("NumpadDivide")]],
            fire_primary: [mouse0],
            fire_secondary: [mouse2, s("AltLeft"), s("AltRight")],
            flight_fire_primary: [mouse0],
            flight_fire_secondary: [mouse2],
            space,
            control_left: s("ControlLeft"),
            mouse0,
            mouse2,
            squads: [s("squadMoveOut"), s("squadHold"), s("squadFollow"), s("squadAtEase")],
            next_spawn: s("nextSpawn"),
            actions: [s("jump"), s("crouch"), s("prone"), s("use"), s("reload"), s("nextPrimary"), s("previousPrimary"), s("nextSecondary"), s("zoom")],
            flight: [
                (s("throttle"), [Some(key_w), None]),
                (s("brake"), [Some(key_s), None]),
                (s("rollLeft"), [Some(key_a), None]),
                (s("rollRight"), [Some(key_d), None]),
                (s("boost"), [Some(s("ShiftLeft")), Some(s("ShiftRight"))]),
                (s("trick"), [Some(s("AltLeft")), Some(s("ControlRight"))]),
                (s("immelmann"), [Some(s("CapsLock")), None]),
                (s("rollLock"), [Some(s("KeyZ")), Some(s("Mouse1"))]),
                (s("takeoff"), [Some(space), None]),
                (s("targetLock"), [Some(s("KeyQ")), Some(s("Mouse3"))]),
            ],
            empty: s(""),
        }
    }
    /// A flight binding's codes by flightInput's name.
    pub fn flight_codes(&self, name: &str) -> (StrId, &[Option<StrId>; 2]) {
        let index = match name {
            "throttle" => 0, "brake" => 1, "rollLeft" => 2, "rollRight" => 3, "boost" => 4,
            "trick" => 5, "immelmann" => 6, "rollLock" => 7, "takeoff" => 8, _ => 9,
        };
        (self.flight[index].0, &self.flight[index].1)
    }
}

/// One connection (multiplayer-world.js `p`): its lanes and flags, the
/// Character the roster gave it and when it last spoke.
pub struct Player {
    pub mailbox: usize,
    pub character: Option<F4Handle>,
    pub spectator: bool,
    pub reliable_fire: bool,
    pub separate_actions: bool,
    pub team_select: bool,
    pub auto_respawn: bool,
    pub ordinal: f64,
    pub team: f64,
    pub recon_key: Option<u32>,
    pub disconnected_at: Option<f64>,
    pub last_input_at: f64,
    pub inputs_stale: bool,
    pub input_lead_ticks: Option<f64>,
    pub controls: Controls,
    /// FireCommands: its state, with the take's shots beside it.
    pub fire: IfResult,
    /// InputActions.
    pub actions: IaState,
    pub actions_taken: Vec<StrId>,
    pub human_taken: Vec<HsCommand>,
    /// This tick's mouse samples (command.look), copied out of the mailbox.
    pub looks: Vec<[f64; 2]>,
    /// Whether this tick changed what the host mirrors of him, and whether
    /// his controls or his stop wrote his body's intent.
    pub changed: bool,
    pub applied: bool,
    pub stopped: bool,
}

impl Player {
    /// addPlayer's fresh record: awake now, lanes empty, team select on.
    pub fn new(mailbox: usize, at: f64) -> Self {
        Self {
            mailbox, character: None, spectator: false, reliable_fire: false, separate_actions: false, team_select: true,
            auto_respawn: false, ordinal: 0.0, team: 0.0, recon_key: None, disconnected_at: None, last_input_at: at,
            inputs_stale: false, input_lead_ticks: None, controls: Controls::new(), fire: Self::fire_lane(), actions: Self::actions_lane(),
            actions_taken: Vec::with_capacity(64), human_taken: Vec::with_capacity(16), looks: Vec::with_capacity(2048), changed: true, applied: false, stopped: false,
        }
    }
    /// `new FireCommands()`: a take yields at most one shot a button.
    pub fn fire_lane() -> IfResult {
        let mut lane = IfResult::default();
        lane.state.queue.reserve(32);
        lane.shots.reserve(2);
        lane
    }
    /// `new InputActions()`: epoch 1, 128 queued messages.
    pub fn actions_lane() -> IaState {
        let mut lane = IaState { epoch: 1.0, capacity: 128.0, ..Default::default() };
        lane.queue.reserve(128);
        lane
    }
    /// His input arrived: he is awake, and the world notes when.
    pub fn heard(&mut self, at: f64) {
        self.last_input_at = at;
        self.inputs_stale = false;
    }
}

/// The match's players, in the order they joined (the source's Map).
pub struct Players {
    pub pool: Pool<Player>,
    pub order: Vec<usize>,
    pub shadows: Vec<UnitShadow>,
    pub names: Option<Names>,
    /// A match owns this world: its tick captures hit history and runs the
    /// players' phase. Standalone unit fixtures never set it.
    pub active: bool,
    pub views: Block,
    pub changed: Vec<PlayerView>,
    /// This tick's presses a body fired, read by the host before its drain.
    pub shots: Vec<PlayerShots>,
    pub shots_block: Block,
    pub lanes: Block,
    /// The bodies in the factory's order, and this tick's rows for them.
    pub scratch: Vec<Ref<Unit>>,
    pub rows: Vec<F3PhaseInput>,
    pub tails: Vec<F3PhaseNetwork>,
    /// Inputs the source would have thrown on (a full press ring, an
    /// overflowing mailbox batch, a receipt backlog): dropped and counted.
    pub faults: u32,
}

impl Default for Players {
    fn default() -> Self { Self::new() }
}

impl Players {
    pub const fn new() -> Self {
        Self { pool: Pool::new(16), order: Vec::new(), shadows: Vec::new(), names: None, active: false, views: Block::new(), changed: Vec::new(),
            shots: Vec::new(), shots_block: Block::new(), lanes: Block::new(), scratch: Vec::new(), rows: Vec::new(), tails: Vec::new(), faults: 0 }
    }
    /// createMultiplayerWorld: reserve every per-player list once.
    pub fn activate(&mut self, strings: &mut Strings, units: usize) {
        if self.names.is_none() { self.names = Some(Names::new(strings)); }
        self.active = true;
        self.order.reserve(PLAYER_CAPACITY);
        self.changed.reserve(PLAYER_CAPACITY);
        self.views.words.reserve(crate::store::block::HEADER + crate::gen::PlayerViews::WORDS);
        self.views.heap.reserve(PLAYER_CAPACITY * PlayerView::WORDS);
        self.shots.reserve(PLAYER_CAPACITY);
        self.shots_block.words.reserve(crate::store::block::HEADER + crate::gen::PlayerShotList::WORDS);
        self.shots_block.heap.reserve(PLAYER_CAPACITY * PlayerShots::WORDS);
        self.lanes.words.reserve(crate::store::block::HEADER + crate::gen::PlayerLanes::WORDS + 256 * crate::gen::IaOutcome::WORDS);
        self.scratch.reserve(units);
        self.rows.reserve(PLAYER_CAPACITY);
        self.tails.reserve(PLAYER_CAPACITY);
        self.reserve_shadows(units);
    }
    /// A body slot the registry grew to: its facts start blank.
    pub fn reserve_shadows(&mut self, units: usize) {
        if self.shadows.len() < units { self.shadows.resize_with(units, UnitShadow::default); }
    }
    pub fn shadow(&mut self, slot: u32) -> &mut UnitShadow {
        self.reserve_shadows(slot as usize + 1);
        &mut self.shadows[slot as usize]
    }
    pub fn names(&self) -> &Names { self.names.as_ref().expect("activated players") }
    /// addPlayer: refused past the match's capacity, as `server full` was.
    pub fn admit(&mut self, player: Player) -> Option<usize> {
        if self.order.len() >= PLAYER_CAPACITY { return None; }
        let key = self.pool.insert(player)?;
        self.order.push(key);
        Some(key)
    }
    /// A retired connection leaves the order; its body facts stay by slot.
    pub fn remove(&mut self, key: usize) -> bool {
        if !self.pool.remove(key) { return false; }
        if let Some(at) = self.order.iter().position(|k| *k == key) { self.order.remove(at); }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_keep_join_order_across_removal_and_body_facts_outlive_them() {
        let mut players = Players::new();
        let mut strings = Strings::default();
        players.activate(&mut strings, 4);
        let a = players.admit(Player::new(1, 0.0)).unwrap();
        let b = players.admit(Player::new(2, 0.0)).unwrap();
        let c = players.admit(Player::new(3, 0.0)).unwrap();
        players.shadow(9).flight = Flight::Empty;
        assert!(players.remove(b));
        assert!(!players.remove(b));
        assert_eq!(players.order, vec![a, c]);
        assert!(matches!(players.shadow(9).flight, Flight::Empty));
        assert_eq!(players.names().flight_codes("takeoff").1[0], Some(players.names().space));
        let d = players.admit(Player::new(4, 0.0)).unwrap();
        assert_eq!(players.order, vec![a, c, d]);
    }
}
