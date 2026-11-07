//! NetworkInputs accounting and wire-input encoding stay in their Rust owners.
//! This adapter supplies generated device records and transport receipts; it
//! keeps only the bounded command/edge metadata needed to retry publication.
use super::receiver::Receiver;
use crate::generated_input::{ClientCommand, ClientFlightCommand, ClientLook};
use bfe_presentation::{
    network::{input::encode, outbound::Outbound, Output, Words},
    network_layout::{net_edge as e, net_header as h, net_op as op, net_outbound as out, net_view as v},
};
use bfe_sim::{
    gen::{Air, Posture, Soldier},
    prediction::reconcile::{target_lead, LeadTargetInput},
};
use bfe_transport::{
    connection::Lane,
    endpoint::Endpoint,
    layout::{INPUT_ACTIONS, INPUT_CODES},
};
use serde::Serialize;
#[derive(Clone, Copy, Default)]
struct State {
    sequence: f64,
    tick: f64,
    held: u32,
    look: [f64; 2],
    revision: f64,
}
#[derive(Clone, Copy, Default)]
struct Pointer {
    sequence: f64,
    x: f64,
    y: f64,
    zoom: f64,
}
pub struct Sender {
    pub history: super::replay::History,
    owner: Outbound,
    life: [f64; 2],
    sequence: u32,
    edge_id: u32,
    edges: [[f64; e::STRIDE]; 32],
    edge_count: usize,
    latest: Option<State>,
    backup: Option<State>,
    use_backup: bool,
    held: [usize; 9],
    actions: [usize; 11],
    pointers: [Pointer; 128],
    pointer_at: usize,
    pointer_count: usize,
    pub yaw: f64,
    pub pitch: f64,
    pub sent: u64,
    pub refused: u64,
    pub fire_edges: u64,
    pub spawned: bool,
    last_tick: f64,
    last_ack: f64,
    turns: Vec<(f64, [f64; 2])>,
}
const KEYS: [&str; 9] = ["KeyW", "KeyS", "KeyD", "KeyA", "NumpadMultiply", "NumpadDivide", "Mouse0", "Mouse2", "Space"];
const ACTIONS: [&str; 11] =
    ["jump", "crouch", "prone", "use", "firePrimary", "fireSecondary", "reload", "nextPrimary", "previousPrimary", "nextSecondary", "zoom"];
fn put(x: &mut [f64], at: &mut usize, n: f64) {
    x[*at] = n;
    *at += 1;
}
// Generated physical codes name the wire bitmap, while ClientCommand uses
// the desktop held/action schema. Resolve that correspondence once at birth.
impl Default for Sender {
    fn default() -> Self {
        let held = KEYS.map(|name| INPUT_CODES.iter().position(|n| *n == name).expect("generated input code"));
        let actions =
            ACTIONS.map(|name| INPUT_ACTIONS.iter().position(|n| *n == name).unwrap_or(if name == "firePrimary" { 21 } else { 22 }));
        Self {
            history: Default::default(),
            owner: Outbound::default(),
            life: [0.; 2],
            sequence: 0,
            edge_id: 0,
            edges: [[0.; e::STRIDE]; 32],
            edge_count: 0,
            latest: None,
            backup: None,
            use_backup: false,
            held,
            actions,
            pointers: [Pointer::default(); 128],
            pointer_at: 0,
            pointer_count: 0,
            yaw: 0.,
            pitch: 0.,
            sent: 0,
            refused: 0,
            fire_edges: 0,
            spawned: false,
            last_tick: 0.,
            last_ack: f64::NEG_INFINITY,
            turns: Vec::new(),
        }
    }
}
impl Sender {
    pub fn new(turns: &[(f64, [f64; 2])]) -> Self {
        Self { turns: turns.to_vec(), ..Self::default() }
    }
    // EntityEdgeSender bounds outstanding retries. Preserve the newest tail
    // when a paused input burst exceeds the window, without growing a map.
    fn edge(&mut self, kind: f64, tick: f64) -> &mut [f64; e::STRIDE] {
        if self.edge_count == 32 {
            self.edges.copy_within(1..32, 0);
            self.edge_count -= 1;
            self.refused += 1;
        }
        self.edge_id = self.edge_id.wrapping_add(1).max(1);
        let at = self.edge_count;
        self.edge_count += 1;
        let row = &mut self.edges[at];
        row.fill(0.);
        row[e::LIFE_MATCHES] = 1.;
        row[e::ID] = self.edge_id as f64;
        row[e::TICK] = tick;
        row[e::KIND] = kind;
        row
    }
    // Receipt retires a retry, whereas an applied correction retires pointer
    // history. Those clocks must stay distinct during loss and server backlog.
    pub fn feedback(&mut self, receiver: &mut Receiver) {
        let life = [receiver.epoch, receiver.own.generation];
        if self.life != life {
            self.history.clear();
            self.life = life;
            self.edge_count = 0;
            self.edge_id = 0;
            self.latest = None;
            self.backup = None;
            self.pointer_count = 0;
            self.pointer_at = 0;
            self.last_tick = 0.;
        }
        let mut n = 0;
        for at in 0..self.edge_count {
            let row = self.edges[at];
            if !receiver.received.values[..receiver.received.count].contains(&(row[e::ID] as u32)) {
                self.edges[n] = row;
                n += 1;
            }
        }
        self.edge_count = n;
        receiver.received.count = 0;
        let mut ignored = [0.; 2];
        let _ = self.owner.run(op::OUTBOUND_ACK, &mut Words::new(&[receiver.ack]), &mut Output { values: &mut ignored, at: 0 });
        self.history.acknowledge(receiver.corrected_sequence);
        while self.pointer_count != 0 && self.pointers[self.pointer_at].sequence <= receiver.corrected_sequence {
            self.pointer_at = (self.pointer_at + 1) % 128;
            self.pointer_count -= 1;
        }
    }
    // NetworkInputs receives one generated command per fixed client step.
    // No entity facade or locally authoritative simulation participates.
    pub fn tick(&mut self, c: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand, receiver: &Receiver, rtt: f64, now: f64) {
        if !receiver.welcomed || receiver.spectator || receiver.corrections == 0 {
            return;
        }
        self.sequence = self.sequence.wrapping_add(1).max(1);
        // TargetLead uses the same transit estimate and future bound as the
        // page. Sequences remain distinct even when that bound holds a tick.
        let target = target_lead(LeadTargetInput {
            hz: 60.,
            margin: 2.,
            max_lead: 16.,
            worst_gap: 0.,
            extra: 0.,
            extrapolate: true,
            estimate: receiver.estimator.estimate(now / 1000.),
            latest: receiver.latest,
            round_trip_seconds: rtt / 1000.,
        })
        .bounded
        .floor();
        let tick = target.max(self.last_tick + 1.).min(receiver.latest + 30.);
        self.last_tick = tick;
        self.history.push(c, look, flight, tick, receiver.own.generation, self.sequence as f64);
        let mut x = [0.; 512];
        let mut at = 0;
        for n in [receiver.epoch, receiver.own.generation, self.sequence as f64, tick] {
            put(&mut x, &mut at, n);
        }
        let count = at;
        put(&mut x, &mut at, 0.);
        let start = at;
        for bit in 0..9 {
            if c.held & (1 << bit) != 0 {
                let name = KEYS[bit];
                put(&mut x, &mut at, name.len() as f64);
                for b in name.bytes() {
                    put(&mut x, &mut at, b as f64);
                }
            }
        }
        x[count] = (at - start) as f64;
        put(&mut x, &mut at, 1.);
        put(&mut x, &mut at, look.x);
        put(&mut x, &mut at, look.y);
        let count = at;
        put(&mut x, &mut at, 0.);
        let start = at;
        for bit in 0..11 {
            if c.actions & (1 << bit) != 0 && bit != 4 && bit != 5 {
                put(&mut x, &mut at, self.actions[bit] as f64);
            }
        }
        for (code, count) in [(21, c.primary_count.min(32)), (22, c.secondary_count.min(32))] {
            for _ in 0..count {
                put(&mut x, &mut at, code as f64);
            }
        }
        x[count] = (at - start) as f64;
        let mut values = [0.; out::STRIDE];
        if self.owner.run(op::OUTBOUND_COMMAND, &mut Words::new(&x[..at]), &mut Output { values: &mut values, at: 0 }).is_err() {
            self.refused += 1;
            return;
        }
        // OUTBOUND_COMMAND owns cumulative look and held revisions. These
        // small checkpoints retain only what the packed publication needs.
        let state = State {
            sequence: self.sequence as f64,
            tick,
            held: c.held,
            look: [values[out::LOOK_X], values[out::LOOK_Y]],
            revision: values[out::REVISION],
        };
        if values[out::RESET] != 0. {
            self.latest = None;
            self.backup = None;
        }
        if values[out::CHANGED] != 0. {
            self.backup = Some(state);
        }
        if values[out::TRANSITION] == 0. {
            self.backup = None;
        }
        self.use_backup = values[out::BACKUP] != 0.;
        self.latest = Some(state);
        // Copy callback scalars before mutating our retry queue. The outbound
        // owner retains its event bank until the next generated command.
        let mut actions = [0u8; 32];
        let ac = self.owner.actions().len().min(32);
        actions[..ac].copy_from_slice(&self.owner.actions()[..ac]);
        for &action in &actions[..ac] {
            self.edge(0., tick)[e::ACTION] = action as f64;
        }
        let mut fires = [bfe_presentation::network::outbound::Fire::default(); 128];
        let fc = self.owner.fires().len().min(128);
        fires[..fc].copy_from_slice(&self.owner.fires()[..fc]);
        if look.x != 0. || look.y != 0. {
            if self.pointer_count == 128 {
                self.pointer_at = (self.pointer_at + 1) % 128;
                self.pointer_count -= 1;
                self.refused += 1;
            }
            let to = (self.pointer_at + self.pointer_count) % 128;
            self.pointers[to] = Pointer { sequence: state.sequence, x: look.x, y: look.y, zoom: receiver.own.zoom.max(1.) };
            self.pointer_count += 1;
        }
        self.preview(receiver);
        for fire in &fires[..fc] {
            if !fire.held {
                let yaw = self.yaw;
                let pitch = self.pitch;
                let view = (receiver.latest - 6.).max(0.);
                let edge = self.edge(1., tick);
                edge[e::BUTTON] = fire.button as f64;
                edge[e::PRESSED] = fire.pressed as u8 as f64;
                edge[e::VIEW_TICK] = view;
                edge[e::AIM_YAW] = yaw;
                edge[e::AIM_PITCH] = pitch;
                self.fire_edges += 1;
            }
        }
    }
    // BodyPreview starts from the most recent applied correction, then
    // follows Soldier.lookRaw for each still-unapplied pointer event.
    pub fn preview(&mut self, receiver: &Receiver) {
        let own = &receiver.own;
        let (max_turn, pitch_limits) = self.turns.get(own.class).copied().unwrap_or((2., [-0.8, 1.1]));
        let mut body = Soldier {
            aim_yaw: own.yaw,
            aim_pitch: own.pitch,
            max_turn,
            pitch_limits,
            posture: Posture::from_u8(own.posture as u8).unwrap_or_default(),
            air: if own.air == 0 { None } else { Air::from_u8(own.air - 1) },
            dive: own.dive,
            lock: own.lock,
            ..Default::default()
        };
        for i in 0..self.pointer_count {
            let p = self.pointers[(self.pointer_at + i) % 128];
            body.look_raw(p.x / p.zoom, p.y / p.zoom, 1., false);
        }
        self.yaw = body.aim_yaw;
        self.pitch = body.aim_pitch;
    }
    pub fn spawn(&mut self, receiver: &Receiver) {
        if self.spawned || !receiver.complete || receiver.corrections == 0 || receiver.spectator {
            return;
        }
        // The correction names the server-selected roster class. Choosing a
        // model from the content table can select an unavailable special class.
        self.spawned = true;
        if receiver.own.alive {
            return;
        }
        let tick = self.last_tick.max(receiver.latest + 1.);
        let edge = self.edge(3., tick);
        edge[e::CLASS_ID] = receiver.own.class as f64;
        edge[e::POST_ID] = 0.;
    }
    pub fn send(&mut self, endpoint: &mut Endpoint, receiver: &mut Receiver) -> Result<(), String> {
        let network = receiver.network.borrow();
        let epoch = network.header[h::EPOCH];
        let ack = network.header[h::ACK_TICK];
        // Spawn screens and spectators still acknowledge complete snapshots.
        // A keepalive carries no synthetic movement command or new sequence.
        let Some(latest) = self.latest else {
            if receiver.welcomed && endpoint.now() - self.last_ack >= 100. {
                self.last_ack = endpoint.now();
                let mut bytes = [0; 1100];
                let n = encode(op::ACK, &mut Words::new(&[epoch, ack]), &mut bytes).map_err(str::to_string)?;
                endpoint.send(Lane::State, false, &bytes[..n]).map_err(|e| e.to_string())?;
            }
            return Ok(());
        };
        if !self.owner.allow(latest.tick, self.edge_count, 2) {
            return Ok(());
        }
        let mut count = 0;
        for at in 0..self.edge_count {
            if self.edges[at][e::TICK] >= latest.tick - 30. {
                self.edges[count] = self.edges[at];
                count += 1;
            }
        }
        self.edge_count = count;
        // A held transition retains one predecessor until acknowledged.
        // Both commands refer to a single validated view context on the wire.
        let backup = self.backup.filter(|s| self.use_backup && s.sequence < latest.sequence);
        let states = [backup.unwrap_or(latest), latest];
        let commands = if backup.is_some() { 2 } else { 1 };
        let mut edges = self.edge_count;
        loop {
            let mut x = [0.; 2048];
            let mut at = 0;
            for n in [self.life[0], self.life[1], 1., commands as f64, edges as f64, 1., epoch, ack] {
                put(&mut x, &mut at, n);
            }
            let start = at;
            at += v::STRIDE;
            x[start + v::FLAGS] = 2. + 32. + 64. + 128.;
            x[start + v::PROFILE_FOUND] = 1.;
            x[start + v::MOUSE_SCALE] = 1.;
            x[start + v::ASPECT] = 1280. / 720.;
            x[start + v::VIEW_TICK] = (receiver.latest - 6.).max(0.);
            for state in &states[..commands] {
                for n in [1., state.sequence, state.tick, 0.] {
                    put(&mut x, &mut at, n);
                }
                let count = at;
                put(&mut x, &mut at, 0.);
                let first = at;
                for bit in 0..9 {
                    if state.held & (1 << bit) != 0 {
                        put(&mut x, &mut at, self.held[bit] as f64);
                    }
                }
                x[count] = (at - first) as f64;
                put(&mut x, &mut at, state.look[0]);
                put(&mut x, &mut at, state.look[1]);
            }
            for row in &self.edges[..edges] {
                x[at..at + e::STRIDE].copy_from_slice(row);
                at += e::STRIDE;
            }
            for code in 0..128 {
                put(&mut x, &mut at, if code < INPUT_CODES.len() { 1. } else { 0. });
            }
            let mut bytes = [0; 1100];
            match encode(op::INPUT, &mut Words::new(&x[..at]), &mut bytes) {
                Ok(length) => {
                    if endpoint.send(Lane::State, false, &bytes[..length]).map_err(|e| e.to_string())? {
                        self.sent += 1;
                        let mut result = [0.; 4];
                        let words = [latest.sequence, latest.look[0], latest.look[1], latest.tick, latest.revision];
                        self.owner
                            .run(op::OUTBOUND_SENT, &mut Words::new(&words), &mut Output { values: &mut result, at: 0 })
                            .map_err(str::to_string)?;
                    }
                    break;
                }
                Err("input packet exceeds bounds") if edges != 0 => edges -= 1,
                Err(error) => return Err(error.into()),
            }
        }
        // Result ACKs retire server-side edge outcomes, independently of
        // input receipt and correction execution acknowledgements.
        if receiver.outcomes.count != 0 {
            #[derive(Serialize)]
            struct Ack<'a> {
                #[serde(rename = "type")]
                kind: &'static str,
                epoch: u32,
                generation: u32,
                ids: &'a [u32],
            }
            let mut bytes = Bytes { values: [0; 1100], at: 0 };
            serde_json::to_writer(
                &mut bytes,
                &Ack {
                    kind: "entity-edge-ack",
                    epoch: self.life[0] as u32,
                    generation: self.life[1] as u32,
                    ids: &receiver.outcomes.values[..receiver.outcomes.count],
                },
            )
            .map_err(|e| e.to_string())?;
            if endpoint.send(Lane::State, true, &bytes.values[..bytes.at]).map_err(|e| e.to_string())? {
                receiver.outcomes.count = 0;
            }
        }
        Ok(())
    }
}
struct Bytes {
    values: [u8; 1100],
    at: usize,
}
impl std::io::Write for Bytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let end = self.at + bytes.len();
        let Some(dest) = self.values.get_mut(self.at..end) else {
            return Err(std::io::ErrorKind::WriteZero.into());
        };
        dest.copy_from_slice(bytes);
        self.at = end;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
