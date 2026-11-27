//! delivery boundary.
//! Packet callbacks consume leases before the next datagram can recycle them;
//! only resource identities and the own camera checkpoint are retained here.
use super::feedback::{Ids, Results};
use bfe_presentation::{
    network::{Session, Shared},
    network_layout::{net_header as h, net_kind as k, net_limits as l, net_record as r},
};
use bfe_sim::prediction::{
    corrections::{decode_compact_soldier_fixed, decode_droideka, decode_own_player_header},
    reconcile::EstimatorState,
};
use serde::Deserialize;
use std::{cell::RefCell, rc::Rc};
use bfe_sim::wire::PositionBounds;
#[derive(Default, Clone, Copy)]
pub struct Own {
    pub id: f64,
    pub generation: f64,
    pub class: usize,
    pub team: u8,
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub yaw: f64,
    pub pitch: f64,
    pub alive: bool,
    pub dead: bool,
    pub removed: bool,
    pub tick: f64,
    pub body_schema: u8,
    /// The own body is a droideka. Its state shares schema 10 with a ground
    /// vehicle's (DROIDEKA_BODY_SCHEMA and GROUND_CORRECTION_SCHEMA); the
    /// header's vehicle id tells them apart, as network-entity-player.js does.
    pub walker: bool,
    pub health: f64,
    pub vehicle: f64,
    pub vehicle_generation: f64,
    pub posture: usize,
    pub air: u8,
    pub zoom: f64,
    pub zoom_step: u8,
    pub dive: f64,
    pub fuel: f64,
    pub lock: f64,
    /// A 2005 hero's power lock target (unit slot and life) while one holds,
    /// from his own soldier body.
    pub power_lock: Option<(u16, u32)>,
    pub walker_mode: u8,
    pub walker_reverse: bool,
    pub walker_time: f64,
    pub walker_legs: f64,
    pub walker_roll: f64,
    pub walker_lean: f64,
    // The droideka's gun counter as the world last had it (its next round
    // comes from gun counter % guns).
    pub walker_gun: f64,
    pub shield: Option<f64>,
    pub shield_on: bool,
    pub mount: Option<(u32, u32, usize)>,
    pub mount_kind: u32,
    pub flight: Option<u8>,
    pub flight_warning: u8,
    pub flight_can_land: bool,
    pub remote: Option<(u16, u32, usize)>,
    pub remote_position: [f64; 3],
    pub remote_velocity: [f64; 3],
    pub remote_yaw: f64,
    pub remote_pitch: f64,
    pub remote_health: f64,
    pub remote_countdown: f64,
    pub remote_zoom_step: u8,
    pub remote_weapons: [Option<([f64; 20], u8, u8)>; 2],
}
impl Own {
    fn apply_compact_soldier(&mut self, bytes: Option<&[u8]>) {
        if let Ok(body) = decode_compact_soldier_fixed(bytes, &[true; 6]) {
            self.position = body.vectors[0];
            self.velocity = body.vectors[1];
            self.fuel = body.numbers[0];
            self.yaw = body.numbers[1];
            self.pitch = body.numbers[2];
            self.posture = body.posture as usize;
            self.air = body.air;
            self.dive = body.numbers[3];
            self.lock = body.numbers[5];
        }
    }
}
// Server diagnostics are terminal observations, not retained JSON objects.
// Keep at most the client's 240 characters, including escaped JSON text;
// four UTF-8 bytes per scalar fit the cold diagnostic reservation exactly.
struct Notice {
    bytes: [u8; 960],
    length: usize,
}
impl Notice {
    fn text(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.length]).unwrap_or("")
    }
}
impl<'de> Deserialize<'de> for Notice {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Text;
        impl<'de> serde::de::Visitor<'de> for Text {
            type Value = Notice;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("server diagnostic text")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Notice, E> {
                let mut out = Notice { bytes: [0; 960], length: 0 };
                for c in value.chars().take(240) {
                    let mut encoded = [0; 4];
                    let text = c.encode_utf8(&mut encoded);
                    out.bytes[out.length..out.length + text.len()].copy_from_slice(text.as_bytes());
                    out.length += text.len();
                }
                Ok(out)
            }
        }
        d.deserialize_str(Text)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Message<'a> {
    #[serde(default, rename = "records", deserialize_with = "super::events::ignore")]
    _records: (),
    #[serde(rename = "type")]
    kind: &'a str,
    #[serde(default)]
    epoch: Option<f64>,
    #[serde(default)]
    generation: Option<f64>,
    #[serde(default)]
    player_id: Option<f64>,
    #[serde(default)]
    tick: Option<f64>,
    #[serde(default)]
    server_tick: Option<f64>,
    #[serde(default)]
    ack: Option<f64>,
    #[serde(default)]
    ack_tick: Option<f64>,
    #[serde(default)]
    dropped: Option<f64>,
    #[serde(default)]
    spectator: bool,
    #[serde(default)]
    team_select: bool,
    #[serde(default)]
    message: Option<Notice>,
    #[serde(default)]
    time: Option<f64>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    delay_ms: Option<f64>,
    #[serde(default)]
    received: Ids,
    #[serde(default)]
    results: Results,
}
/// A 2005 welcome's `flightCeiling`: a JSON number that is a finite IEEE
/// binary32 (magnitude at most 3.4028234663852886e38) widened exactly, as a
/// mission script supplies it; none when absent or anything else.
fn welcome_ceiling(bytes: &[u8]) -> Option<f64> {
    #[derive(Deserialize)]
    struct Welcome {
        #[serde(rename = "flightCeiling")]
        flight_ceiling: Option<serde_json::Value>,
    }
    let ceiling = serde_json::from_slice::<Welcome>(bytes).ok()?.flight_ceiling?.as_f64()?;
    (ceiling.is_finite() && f64::from(ceiling as f32) == ceiling).then_some(ceiling)
}
#[derive(Clone, Copy)]
struct Early {
    bytes: [u8; l::PAYLOAD],
    length: usize,
    now: f64,
}
fn swbf2_display_schema() -> u16 { bfe_sim::movement::soldier::swbf2::body::DISPLAY_SCHEMA }
type Displays = std::collections::HashMap<u32, (u32, bfe_sim::movement::soldier::swbf2::body::Display)>;
// A 2005 soldier's display field, read at the match's revision and kept for
// the unit's life: id 100,000 plus the unit's slot.
fn install_display(displays: &mut Displays, plan: Option<&bfe_sim::wire::revisions::Plan>, id: u32, life: u32, bytes: &[u8])
    -> Result<(), &'static str>
{
    use bfe_sim::wire::revisions::{split_revision, Use, SOLDIER_DISPLAY};
    let Some(Use::Revision(revision)) = plan.map(|plan| plan.of(SOLDIER_DISPLAY)) else { return Ok(()) };
    let slot = id.checked_sub(100_000).filter(|slot| (*slot as usize) < l::SLOTS).ok_or("invalid Battlefront II soldier field")?;
    // An empty field is its removal: the soldier has departed.
    if bytes.is_empty() { displays.remove(&slot); return Ok(()); }
    let (written, body) = split_revision(bytes)?;
    if written != revision { return Err("invalid Battlefront II soldier field"); }
    let display = bfe_sim::movement::soldier::swbf2::body::read_display(body)?;
    if displays.get(&slot).is_none_or(|&(known, _)| life >= known) {
        displays.insert(slot, (life, display));
    }
    Ok(())
}
pub struct Receiver {
    pub(crate) space: Option<crate::local::space::History>,
    pub(crate) mission_view: Option<Box<crate::local::mission_view::Observation>>,
    pub(crate) mission_score: Option<bfe_sim::battle::script::MissionScore>,
    pub(crate) infantry_view: Option<super::own_view::LocalInfantry>,
    pub(crate) infantry: Option<bfe_sim::units::infantry::Sample>,
    pub(crate) contacts: Option<Box<bfe_sim::units::infantry::contact::Contacts>>,
    pub(crate) powers: Option<Box<bfe_sim::combat::powers::PowerFrame>>,
    pub(crate) hero_audio: Option<Box<bfe_sim::units::infantry::hero_audio::HeroAudioSnapshot>>,
    /// Game whose pilot and passenger controls apply to this battle.
    pub control_family: bfe_family::GameFamily,
    pub space_online: Option<Box<super::space_receiver::OnlineSpace>>,
    pub(crate) doors: crate::local::door_frames::DoorFrames,
    pub chat: bfe_battle_ui::ChatLog,
    flight: bfe_sim::prediction::corrections::FlightState,
    pub next_match: Option<super::rotation::NextMatch>,
    /// The checked plan of the 2005 match this client plays; none in a 2004 battle.
    pub swbf2_plan: Option<bfe_sim::wire::revisions::Plan>,
    /// A 2005 match's flight ceiling in metres, from its welcome: what
    /// prediction uses for the whole epoch in place of the client's files.
    pub flight_ceiling: Option<f64>,
    /// A 2005 match's combat checkpoint parts, reassembled against the
    /// accepted own correction and committed only together with it.
    pub combat_parts: Box<bfe_sim::wire::checkpoint::Assembly>,
    /// The mission of a scripted 2005 battle, as its authority replicates it;
    /// none in every other battle.
    pub mission: Option<Box<super::mission::Mission>>,
    /// Whether the welcome lets this player choose his team.
    pub team_select: bool,
    /// Each 2005 soldier's shared display by unit slot: the life it belongs
    /// to and what the field says. A display of an older life never replaces
    /// a newer one, and a consumer reads it only for the life it draws.
    pub swbf2_displays: Displays,
    pub roster: super::roster::Roster,
    pub publications: Box<super::publications::Publications>,
    pub network: Shared,
    pub clock: super::clock::Clock,
    pub facts: Vec<[f64; r::STRIDE]>,
    indices: super::index::Index,
    free: Vec<usize>,
    pub own_fact: [f64; r::STRIDE],
    pub own: Own,
    pub welcomed: bool,
    pub spectator: bool,
    pub epoch: f64,
    pub latest: f64,
    pub complete: bool,
    pub registry: bool,
    expected: serde_json::Value,
    bounds: PositionBounds,
    pub estimator: EstimatorState,
    pub ack: f64,
    /// The newest command sequence carried by an `input-ack` message (which
    /// supplies `ack_tick` and `ack_server_tick`). Snapshots advance `ack`
    /// without those timings, so lateness tracks `ack_measured` separately.
    pub ack_measured: f64,
    /// The tick the server ran the newest acknowledged command at, and its tick when it said so.
    pub ack_tick: f64,
    pub ack_server_tick: f64,
    /// Ticks the server's world has given up so far (it fell behind real time), from its input-ack.
    pub server_dropped: f64,
    pub packets: u64,
    pub corrections: u64,
    pub corrected_sequence: f64,
    pub refused: u64,
    pub body_bytes: Box<[u8; l::PAYLOAD]>,
    pub body_length: usize,
    pub checkpoint: Box<[u8; l::PAYLOAD]>,
    pub checkpoint_length: usize,
    pub fault: Option<&'static str>,
    fault_detail: String,
    pongs: [Option<f64>; 32],
    pong_at: usize,
    pong_count: usize,
    pong_bytes: [u8; 96],

    pub received: Ids,
    pub outcomes: Ids,
    early: Vec<Early>,
    early_at: usize,
    early_count: usize,
}
impl Receiver {
    pub(crate) fn enable_local_infantry_view(&mut self) {
        self.infantry_view = Some(super::own_view::LocalInfantry::new());
    }

    // Reserve both receiver baselines and the early-transport queue before
    // any live callback can arrive. Resource facts retain no payload pointer.
    pub fn new(expected: serde_json::Value) -> Self {
        Self::with_bounds(expected, PositionBounds::LEGACY)
    }

    pub(crate) fn with_bounds(expected: serde_json::Value, bounds: PositionBounds) -> Self {
        let mut session = Session::new();
        session.reserve_receive();
        Self {
            infantry: None,
            space: None,
            mission_view: None,
            mission_score: None, contacts: None, powers: None, hero_audio: None,
            infantry_view: None,
            control_family: bfe_family::GameFamily::Swbf1,
            chat: bfe_battle_ui::ChatLog::new(),
            flight: bfe_sim::prediction::corrections::FlightState {
                numbers: Vec::with_capacity(37),
                vectors: Vec::with_capacity(7),
                quaternions: Vec::with_capacity(6),
                ..Default::default()
            },
            next_match: None,
            swbf2_plan: None,
            flight_ceiling: None,
            combat_parts: Box::default(),
            mission: None,
            team_select: false,
            swbf2_displays: std::collections::HashMap::with_capacity(64),
            roster: Default::default(),
            publications: Box::new(Default::default()),
            network: Rc::new(RefCell::new(session)),
            clock: Default::default(),
            facts: vec![[0.; r::STRIDE]; l::SLOTS],
            indices: super::index::Index::new(l::SLOTS),
            free: (0..l::SLOTS).rev().collect(),
            own_fact: [0.; r::STRIDE],
            own: Own::default(),
            welcomed: false,
            spectator: false,
            epoch: 1.,
            doors: Default::default(),
            space_online: None,
            latest: 0.,
            complete: false,
            registry: false,
            expected,
            bounds,
            estimator: EstimatorState { hz: 60., window: 64., at: 0., count: 0., offset: None, dropped: None, offsets: vec![0.; 64] },
            ack: 0.,
            ack_measured: 0.,
            ack_tick: 0.,
            ack_server_tick: 0.,
            server_dropped: 0.,
            packets: 0,
            corrections: 0,
            corrected_sequence: 0.,
            refused: 0,
            body_bytes: vec![0u8; l::PAYLOAD].into_boxed_slice().try_into().unwrap(),
            body_length: 0,
            checkpoint: vec![0u8; l::PAYLOAD].into_boxed_slice().try_into().unwrap(),
            checkpoint_length: 0,
            fault: None,
            fault_detail: String::with_capacity(960),
            pongs: [None; 32],
            pong_at: 0,
            pong_count: 0,
            pong_bytes: [0; 96],
            received: Ids::default(),
            outcomes: Ids::default(),
            early: (0..256).map(|_| Early { bytes: [0; l::PAYLOAD], length: 0, now: 0. }).collect(),
            early_at: 0,
            early_count: 0,
        }
    }
    pub fn message(&mut self, text: bool, bytes: &[u8], now: f64) {
        if self.fault.is_some() {
            return;
        }
        if text {
            self.text(bytes, now);
            // State and reliable bootstrap have independent transport order.
            // Keep bounded early state until registry/welcome admission, just
            // as NetworkSession's initial entity queue did in the client.
            if self.registry && self.welcomed {
                while self.early_count != 0 && self.fault.is_none() {
                    let early = self.early[self.early_at];
                    self.early_at = (self.early_at + 1) % self.early.len();
                    self.early_count -= 1;
                    self.message(false, &early.bytes[..early.length], early.now);
                }
            }
            return;
        }
        if !self.registry || !self.welcomed {
            if !(20..=l::PAYLOAD).contains(&bytes.len()) {
                self.fault = Some("invalid early snapshot");
                return;
            }
            if self.early_count == self.early.len() {
                self.early_at = (self.early_at + 64) % self.early.len();
                self.early_count -= 64;
                self.refused += 64;
            }
            let at = (self.early_at + self.early_count) % self.early.len();
            self.early[at].bytes[..bytes.len()].copy_from_slice(bytes);
            self.early[at].length = bytes.len();
            self.early[at].now = now;
            self.early_count += 1;
            return;
        }
        if self.swbf2_plan.is_none()
            && self.control_family != bfe_family::GameFamily::Swbf2
        {
            self.released_state(bytes, now);
        } else {
            self.swbf2_state(bytes, now);
        }
    }
    fn released_state(&mut self, bytes: &[u8], now: f64) {
        // Session.receive is the same decoder used by SnapshotClient. Its
        // publication lease lasts only until this callback receives again.
        let mut owner = self.network.borrow_mut();
        if let Err(e) = owner.receive_with_bounds(bytes, &self.bounds) {
            self.fault = Some(e);
            return;
        }
        // Count delivered datagrams separately from valid publications:
        // duplicate or incomplete parts may contribute no changed records.
        self.packets += 1;
        if owner.header[h::VALID] == 0. {
            return;
        }
        // An epoch invalidates every resource identity and arrival estimate.
        // Keep reservations, but never pair a new map with an old pose.
        if owner.header[h::EPOCH_CHANGED] != 0. {
            self.indices.clear();
            self.free.clear();
            self.free.extend((0..l::SLOTS).rev());
            self.facts.fill([0.; r::STRIDE]);
            self.complete = false;
            self.estimator.reset();
            self.clock.reset();
            self.doors.clear();
            self.publications.reset();
            self.roster.reset();
        }
        self.epoch = owner.header[h::EPOCH];
        let tick = owner.header[h::TICK];
        if tick > self.latest {
            self.latest = tick;
            self.estimator.arrive(tick, now / 1000., 0.);
        }
        if let Err(error) = self.clock.arrive(tick, now / 1000., owner.header[h::INTERVAL]) {
            self.fault = Some(error);
            return;
        }
        self.complete |= owner.header[h::COMPLETE] != 0.;
        self.ack = self.ack.max(owner.header[h::ACK_INPUT]);
        for at in 0..(owner.header[h::RECORDS] as usize).min(owner.leases.publications.len()) {
            let lease = owner.leases.publications[at] as usize;
            let row = &owner.leases.records[lease * r::STRIDE..][..r::STRIDE];
            let kind = row[r::KIND] as u32;
            let id = row[r::ID] as u32;
            if matches!(kind, k::SUMMARY | k::COMBAT) {
                let offset = row[r::PAYLOAD_OFFSET] as usize;
                let length = row[r::PAYLOAD_LENGTH] as usize;
                if let Some(bytes) = owner.leases.payloads.get(offset..offset + length) {
                    if kind == k::SUMMARY && self.roster.receive(row[r::SCHEMA] as u32, id, bytes).is_none() {
                        self.fault = Some("invalid roster summary");
                        return;
                    }
                    self.publications.push(row, bytes);
                }
                continue;
            }
            if kind == k::MOUNT {
                self.publications.push(row, &[]);
                continue;
            }
            if kind == k::SEAT {
                if let Some(&index) = self.indices.get(&(k::SOLDIER, id)).or_else(|| self.indices.get(&(k::DROIDEKA, id))) {
                    if self.facts[index][r::GENERATION] == row[r::GENERATION] {
                        self.facts[index][r::SEAT_PRESENT..=r::SEAT_INDEX].copy_from_slice(&row[r::SEAT_PRESENT..=r::SEAT_INDEX]);
                    }
                }
                continue;
            }
            // Read only the shared correction prefix needed for the own
            // camera. Opaque arsenal state remains in the native receiver.
            if kind == k::CORRECTION && (!self.spectator || row[r::ID] == self.own.id) {
                self.own.id = row[r::ID];
                let offset = row[r::PAYLOAD_OFFSET] as usize;
                let length = row[r::PAYLOAD_LENGTH] as usize;
                let Some(bytes) = owner.leases.payloads.get(offset..offset + length) else {
                    continue;
                };
                let Ok(head) = decode_own_player_header(bytes) else {
                    self.fault = Some("invalid own correction");
                    return;
                };
                self.checkpoint_length = bytes.len();
                self.checkpoint[..bytes.len()].copy_from_slice(bytes);
                let mount = bfe_sim::prediction::corrections::own_mounted(bytes).ok().flatten();
                // Vehicle and emplacement ids occupy independent namespaces.
                // Passenger leases also name a hull when the body header does not.
                self.own.mount_kind = mount.as_ref().map_or(0, |mount| {
                    if mount.mode == 3 {
                        k::TURRET
                    } else {
                        k::VEHICLE
                    }
                });
                self.own.mount = mount.map(|mount| (mount.id, mount.generation, mount.index as usize));
                self.own.remote = None;
                self.own.remote_weapons = [None, None];
                if let Ok(own_state) = bfe_sim::prediction::corrections::decode_own_player(Some(bytes), true) {
                    if let Some(rem) = own_state.remote {
                        if let Some(rem_bytes) = rem.payload.get(bytes) {
                            if let Ok(recon) = bfe_sim::prediction::corrections::decode_recon_header(Some(rem_bytes)) {
                                if recon.bits & 2 != 0 && recon.bits & 1 == 0 {
                                    self.own.remote = Some((rem.id, rem.generation, rem.definition_id as usize));
                                    self.own.remote_yaw = recon.numbers[0];
                                    self.own.remote_pitch = recon.numbers[1];
                                    self.own.remote_health = recon.numbers[2];
                                    self.own.remote_countdown = recon.numbers[3];
                                    self.own.remote_zoom_step = recon.zoom_step;
                                    self.own.remote_position = recon.position;
                                    self.own.remote_velocity = recon.velocity;
                                    let mut cursor = recon.cursor;
                                    for slot in 0..(recon.weapon_count as usize).min(2) {
                                        if let Ok(wp) =
                                            bfe_sim::prediction::corrections::decode_recon_payload(rem_bytes, cursor)
                                        {
                                            cursor = wp.cursor;
                                            if let Ok(w) = bfe_sim::prediction::corrections::decode_weapon(
                                                wp.payload.get(rem_bytes),
                                            ) {
                                                self.own.remote_weapons[slot] = Some((w.numbers, w.flags, w.level));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                self.own.health = head.health;
                self.own.vehicle = head.vehicle_id as f64;
                self.own.vehicle_generation = head.vehicle_generation as f64;
                self.own.body_schema = head.body_schema;
                self.own.walker = head.body_schema == 10 && head.vehicle_id == 0;
                self.own.flight = None;
                self.own.flight_warning = 0;
                self.own.flight_can_land = false;
                if let Some(body) = head.body.get(bytes) {
                    if body.len() > self.body_bytes.len() {
                        self.fault = Some("own body exceeds prediction reservation");
                        return;
                    }
                    self.body_length = body.len();
                    self.body_bytes[..body.len()].copy_from_slice(body);
                }
                self.own.generation = row[r::GENERATION];
                self.own.class = head.class_id as usize;
                self.own.team = head.team;
                self.own.dead = head.dead != 0;
                self.own.removed = head.removed != 0;
                self.own.alive = head.dead == 0 && head.removed == 0;
                self.own.tick = tick;
                self.own.zoom = head.zoom;
                self.own.zoom_step = head.zoom_step;
                self.own.shield = None;
                self.own.shield_on = false;
                if head.body_schema == 6 {
                    self.own.apply_compact_soldier(head.body.get(bytes));
                } else if self.own.walker {
                    if let Ok(body) = decode_droideka(head.body.get(bytes)) {
                        self.own.position = body.vectors[0];
                        self.own.velocity = body.vectors[1];
                        self.own.yaw = body.numbers[0];
                        self.own.pitch = body.numbers[1];
                        self.own.walker_mode = body.mode;
                        self.own.walker_reverse = body.flags & 2 != 0;
                        self.own.walker_time = body.numbers[3];
                        self.own.walker_legs = body.numbers[4];
                        self.own.walker_roll = body.numbers[6];
                        self.own.walker_lean = body.numbers[7];
                        self.own.walker_gun = body.numbers[11];
                        self.own.shield = body.shield;
                        self.own.shield_on = body.flags & 16 != 0;
                        // Droideka.posture: "crouch" (its rolled camera section) from FOLD on.
                        self.own.posture = (body.mode >= 11) as usize;
                        self.own.air = 0;
                        self.own.dive = 0.;
                    }
                } else if head.body_schema == 7 {
                    if bfe_sim::prediction::corrections::decode_compact_flight_into(
                        head.body.get(bytes),
                        true,
                        [true; 9],
                        [true; 4],
                        &mut self.flight,
                    )
                    .is_ok()
                    {
                        self.own.position = self.flight.vectors[0];
                        self.own.velocity = self.flight.vectors[1];
                        self.own.yaw = self.flight.seat[0].value;
                        self.own.pitch = self.flight.seat[1].value;
                        use bfe_sim::gen::ClientFlightEnumField as field;
                        self.own.flight = Some(self.flight.enums[field::State as usize]);
                        self.own.flight_warning = self.flight.enums[field::Warning as usize];
                        self.own.flight_can_land = self.flight.enums[field::Reason as usize]
                            == bfe_sim::flight::Message::PressSpace as u8;
                    }
                } else if head.body_schema == 10 {
                    if let Ok(body) = bfe_sim::prediction::corrections::decode_ground(head.body.get(bytes), true, [true; 2]) {
                        self.own.position = body.position;
                        self.own.velocity = body.velocity;
                        self.own.yaw = body.seat[0].value;
                        self.own.pitch = body.seat[1].value;
                    }
                }
                self.own_fact.fill(0.);
                let fact = &mut self.own_fact;
                fact[r::KIND] = if self.own.walker { k::DROIDEKA } else { k::SOLDIER } as f64;
                fact[r::ID] = self.own.id;
                fact[r::GENERATION] = self.own.generation;
                fact[r::CLASS_ID] = self.own.class as f64;
                fact[r::TEAM] = self.own.team as f64;
                fact[r::POSITION..r::POSITION + 3].copy_from_slice(&self.own.position);
                fact[r::VELOCITY..r::VELOCITY + 3].copy_from_slice(&self.own.velocity);
                fact[r::YAW] = self.own.yaw;
                fact[r::PITCH] = self.own.pitch;
                fact[r::POSTURE] = self.own.posture as f64;
                fact[r::STATE] = self.own.air as f64;
                fact[r::HEALTH] = self.own.health;
                fact[r::DEAD] = (!self.own.alive) as u8 as f64;
                if self.own.walker {
                    fact[r::MODE] = self.own.walker_mode as f64;
                    fact[r::REVERSE] = self.own.walker_reverse as u8 as f64;
                    fact[r::TIME] = self.own.walker_time;
                    fact[r::LEGS] = self.own.walker_legs;
                    fact[r::ROLL] = self.own.walker_roll;
                    fact[r::LEAN] = self.own.walker_lean;
                    fact[r::SHIELD] = self.own.shield.unwrap_or(-1.);
                    fact[r::SHIELD_ON] = self.own.shield_on as u8 as f64;
                }
                // This row survives corrections. An exit must release the
                // old rider binding as well as the own control checkpoint.
                fact[r::SEAT_PRESENT..=r::SEAT_INDEX].fill(0.);
                if let Some((id, generation, index)) = self.own.mount {
                    fact[r::SEAT_PRESENT] = 1.;
                    fact[r::SEAT_VEHICLE_ID] = id as f64;
                    fact[r::SEAT_VEHICLE_GENERATION] = generation as f64;
                    fact[r::SEAT_INDEX] = index as f64;
                }
                if let Ok((_, mut weapons)) = bfe_sim::prediction::corrections::own_weapons(bytes) {
                    if let Some(Ok(weapon)) = weapons.next() {
                        fact[r::WEAPON] = weapon.id as f64;
                    }
                }
                self.corrected_sequence = owner.header[h::ACK_INPUT];
                self.corrections += 1;
                continue;
            }
            if kind == k::TOMBSTONE {
                self.publications.push(row, &[]);
                // Identity is shared across entity kinds, but a late tombstone
                // cannot remove the newer generation that reused that identity.
                for removed in k::SOLDIER..=k::REMOTE {
                    if let Some(&index) = self.indices.get(&(removed, id)) {
                        if self.facts[index][r::GENERATION] == row[r::GENERATION] {
                            self.indices.remove(&(removed, id));
                            self.facts[index].fill(0.);
                            self.free.push(index);
                        }
                    }
                }
                continue;
            }
            // Summary/combat leases are not model identities. Resource
            // admission below retains only the body/hull identity facts.
            if kind > k::REMOTE {
                continue;
            }
            let index = if let Some(&index) = self.indices.get(&(kind, id)) {
                index
            } else {
                let Some(index) = self.free.pop() else {
                    self.refused += 1;
                    continue;
                };
                self.indices.insert((kind, id), index);
                index
            };
            let seat = if self.facts[index][r::GENERATION] == row[r::GENERATION] {
                Some([
                    self.facts[index][r::SEAT_PRESENT],
                    self.facts[index][r::SEAT_VEHICLE_ID],
                    self.facts[index][r::SEAT_VEHICLE_GENERATION],
                    self.facts[index][r::SEAT_INDEX],
                ])
            } else {
                None
            };
            self.facts[index].copy_from_slice(row);
            if let Some(seat) = seat {
                self.facts[index][r::SEAT_PRESENT..=r::SEAT_INDEX].copy_from_slice(&seat);
            }
        }
        // Seat leases publish in their own bank, after changed body rows.
        // This ordering admits a rider whose hull arrived later in the part.
        for at in 0..(owner.header[h::SEATS] as usize).min(owner.leases.seats.len()) {
            let lease = owner.leases.seats[at] as usize;
            let row = &owner.leases.records[lease * r::STRIDE..][..r::STRIDE];
            let id = row[r::ID] as u32;
            if let Some(&index) = self.indices.get(&(k::SOLDIER, id)).or_else(|| self.indices.get(&(k::DROIDEKA, id))) {
                if self.facts[index][r::GENERATION] == row[r::GENERATION] {
                    self.facts[index][r::SEAT_PRESENT..=r::SEAT_INDEX].copy_from_slice(&row[r::SEAT_PRESENT..=r::SEAT_INDEX]);
                }
            }
        }
    }
    fn swbf2_state(&mut self, bytes: &[u8], now: f64) {
        // A 2005 match's checkpoint parts are datagrams of their own, read by
        // the match's plan.
        if bytes.get(..4) == Some(&bfe_sim::wire::checkpoint::MAGIC.to_le_bytes()[..]) {
            self.checkpoint_part(bytes);
            return;
        }
        let mut owner = self.network.borrow_mut();
        if let Err(e) = owner.receive_with_bounds(bytes, &self.bounds) {
            self.fault = Some(e);
            return;
        }
        self.packets += 1;
        if owner.header[h::VALID] == 0. {
            return;
        }
        if owner.header[h::EPOCH_CHANGED] != 0. {
            self.swbf2_displays.clear();
            if let Some(mission) = &mut self.mission { mission.reset(); }
            if let Some(space) = &mut self.space_online { space.reset(); }
            if let Some(history) = &mut self.space { history.reset(); }
            self.indices.clear();
            self.free.clear();
            self.free.extend((0..l::SLOTS).rev());
            self.facts.fill([0.; r::STRIDE]);
            self.complete = false;
            self.estimator.reset();
            self.clock.reset();
            self.doors.clear();
            self.publications.reset();
            self.roster.reset();
        }
        self.epoch = owner.header[h::EPOCH];
        let tick = owner.header[h::TICK];
        if tick > self.latest {
            self.latest = tick;
            self.estimator.arrive(tick, now / 1000., 0.);
        }
        if let Err(error) = self.clock.arrive(tick, now / 1000., owner.header[h::INTERVAL]) {
            self.fault = Some(error);
            return;
        }
        self.complete |= owner.header[h::COMPLETE] != 0.;
        self.ack = self.ack.max(owner.header[h::ACK_INPUT]);
        for at in 0..(owner.header[h::RECORDS] as usize).min(owner.leases.publications.len()) {
            let lease = owner.leases.publications[at] as usize;
            let row = &owner.leases.records[lease * r::STRIDE..][..r::STRIDE];
            let kind = row[r::KIND] as u32;
            let id = row[r::ID] as u32;
            if matches!(kind, k::SUMMARY | k::COMBAT) {
                let offset = row[r::PAYLOAD_OFFSET] as usize;
                let length = row[r::PAYLOAD_LENGTH] as usize;
                if let Some(bytes) = owner.leases.payloads.get(offset..offset + length) {
                    if kind == k::SUMMARY && self.roster.receive(row[r::SCHEMA] as u32, id, bytes).is_none() {
                        self.fault = Some("invalid roster summary");
                        return;
                    }
                    if kind == k::SUMMARY && row[r::SCHEMA] == f64::from(swbf2_display_schema()) {
                        if let Err(error) = install_display(&mut self.swbf2_displays, self.swbf2_plan.as_ref(), id, row[r::GENERATION] as u32, bytes) {
                            self.fault = Some(error);
                            return;
                        }
                    }
                    if kind == k::SUMMARY && super::mission::Mission::carries(row[r::SCHEMA] as u16) {
                        // Only a battle whose registry named a mission has one.
                        // An empty field is its removal.
                        let installed = self.mission.as_mut().ok_or("a mission record in a battle without a mission")
                            .and_then(|mission| if bytes.is_empty() { mission.remove(row[r::SCHEMA] as u16, id) } else { mission.install(row[r::SCHEMA] as u16, id, bytes) });
                        if let Err(error) = installed {
                            self.fault = Some(error);
                            return;
                        }
                    }
                    if kind == k::SUMMARY && row[r::SCHEMA] == 7. {
                        if let Some(space) = self.space_online.as_mut() {
                            space.receive_building_summary(id, bytes);
                        }
                    }
                    if kind == k::SUMMARY && super::space_receiver::OnlineSpace::carries(row[r::SCHEMA] as u16) {
                        let installed = self.space_online.as_mut().ok_or("a space record in a battle without space")
                            .and_then(|space| space.receive_field_at(tick as u32, row[r::SCHEMA] as u16, id, bytes));
                        match installed {
                            Ok(true) => {
                                if let Some(space) = &self.space_online {
                                    self.doors.publish(tick, &space.door_states);
                                }
                            }
                            Ok(false) => {}
                            Err(error) => {
                                self.fault = Some(error);
                                return;
                            }
                        }
                    }
                    self.publications.push(row, bytes);
                }
                continue;
            }
            if kind == k::MOUNT {
                self.publications.push(row, &[]);
                continue;
            }
            if kind == k::SEAT {
                if let Some(&index) = self.indices.get(&(k::SOLDIER, id)).or_else(|| self.indices.get(&(k::DROIDEKA, id))) {
                    if self.facts[index][r::GENERATION] == row[r::GENERATION] {
                        self.facts[index][r::SEAT_PRESENT..=r::SEAT_INDEX].copy_from_slice(&row[r::SEAT_PRESENT..=r::SEAT_INDEX]);
                    }
                }
                continue;
            }
            if kind == k::CORRECTION && (!self.spectator || row[r::ID] == self.own.id) {
                self.own.id = row[r::ID];
                let offset = row[r::PAYLOAD_OFFSET] as usize;
                let length = row[r::PAYLOAD_LENGTH] as usize;
                let Some(bytes) = owner.leases.payloads.get(offset..offset + length) else {
                    continue;
                };
                let Ok(head) = decode_own_player_header(bytes) else {
                    self.fault = Some("invalid own correction");
                    return;
                };
                // A 2005 match commits a combat checkpoint only with the own
                // correction of its epoch, tick, unit and life.
                self.combat_parts.accept(bfe_sim::wire::checkpoint::Anchor { epoch: self.epoch as u16, tick: row[r::TICK] as u32,
                    subject: id, life: row[r::GENERATION] as u32 });
                self.checkpoint_length = bytes.len();
                self.checkpoint[..bytes.len()].copy_from_slice(bytes);
                let mount = bfe_sim::prediction::corrections::own_mounted(bytes).ok().flatten();
                self.own.mount_kind = mount.as_ref().map_or(0, |mount| {
                    if mount.mode == 3 {
                        k::TURRET
                    } else {
                        k::VEHICLE
                    }
                });
                self.own.mount = mount.map(|mount| (mount.id, mount.generation, mount.index as usize));
                self.own.remote = None;
                self.own.remote_weapons = [None, None];
                if let Ok(own_state) = bfe_sim::prediction::corrections::decode_own_player(Some(bytes), true) {
                    if let Some(rem) = own_state.remote {
                        if let Some(rem_bytes) = rem.payload.get(bytes) {
                            if let Ok(recon) = bfe_sim::prediction::corrections::decode_recon_header(Some(rem_bytes)) {
                                if recon.bits & 2 != 0 && recon.bits & 1 == 0 {
                                    self.own.remote = Some((rem.id, rem.generation, rem.definition_id as usize));
                                    self.own.remote_yaw = recon.numbers[0];
                                    self.own.remote_pitch = recon.numbers[1];
                                    self.own.remote_health = recon.numbers[2];
                                    self.own.remote_countdown = recon.numbers[3];
                                    self.own.remote_zoom_step = recon.zoom_step;
                                    self.own.remote_position = recon.position;
                                    self.own.remote_velocity = recon.velocity;
                                    let mut cursor = recon.cursor;
                                    for slot in 0..(recon.weapon_count as usize).min(2) {
                                        if let Ok(wp) =
                                            bfe_sim::prediction::corrections::decode_recon_payload(rem_bytes, cursor)
                                        {
                                            cursor = wp.cursor;
                                            if let Ok(w) = bfe_sim::prediction::corrections::decode_weapon(
                                                wp.payload.get(rem_bytes),
                                            ) {
                                                self.own.remote_weapons[slot] = Some((w.numbers, w.flags, w.level));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                self.own.health = head.health;
                self.own.vehicle = head.vehicle_id as f64;
                self.own.vehicle_generation = head.vehicle_generation as f64;
                self.own.body_schema = head.body_schema;
                self.own.walker = head.body_schema == 10 && head.vehicle_id == 0;
                self.own.flight = None;
                self.own.flight_warning = 0;
                self.own.flight_can_land = false;
                if let Some(body) = head.body.get(bytes) {
                    if body.len() > self.body_bytes.len() {
                        self.fault = Some("own body exceeds prediction reservation");
                        return;
                    }
                    self.body_length = body.len();
                    self.body_bytes[..body.len()].copy_from_slice(body);
                }
                self.own.generation = row[r::GENERATION];
                self.own.class = head.class_id as usize;
                self.own.team = head.team;
                self.own.dead = head.dead != 0;
                self.own.removed = head.removed != 0;
                self.own.alive = head.dead == 0 && head.removed == 0;
                self.own.tick = tick;
                self.own.zoom = head.zoom;
                self.own.zoom_step = head.zoom_step;
                self.own.shield = None;
                self.own.shield_on = false;
                self.own.power_lock = None;
                use bfe_sim::movement::soldier::swbf2::body as swbf2_body;
                if head.body_schema == 6 || head.body_schema == swbf2_body::SCHEMA {
                    // A 2005 soldier's body holds the same compact body after its revision.
                    let body = head.body.get(bytes);
                    let compact = if head.body_schema == 6 { body } else {
                        body.and_then(|b| swbf2_body::revision_body(b).ok()).and_then(|b| b.get(..swbf2_body::compact_len(b)?))
                    };
                    self.own.apply_compact_soldier(compact);
                    // After the movement state: his power lock, while one holds.
                    self.own.power_lock = body.filter(|_| head.body_schema == swbf2_body::SCHEMA)
                        .and_then(|b| swbf2_body::revision_body(b).ok())
                        .and_then(|b| b.get(swbf2_body::compact_len(b)?..))
                        .and_then(|suffix| swbf2_body::split_lock(suffix).ok())
                        .and_then(|(_, lock)| lock);
                } else if head.body_schema == bfe_sim::wire::space::OWN_BODY_SCHEMA {
                    let ack_seq = owner.header[h::ACK_INPUT] as u32;
                    if let (Some(body_raw), Some(space)) = (head.body.get(bytes), self.space_online.as_mut()) {
                        match space.receive_own(body_raw, ack_seq, tick as u32) {
                            Ok(bfe_sim::wire::space::OwnSpaceBody::Pilot(p)) => {
                                self.own.position = p.position;
                                self.own.velocity = p.velocity;
                                self.own.flight = Some(p.state);
                                self.own.flight_can_land = p.can_land;
                                self.own.flight_warning = u8::from(p.oob_warning);
                                if let Some(history) = &mut self.space {
                                    space.apply_own_to_latest(self.own.team, self.mission.as_deref(), history);
                                }
                            }
                            Ok(bfe_sim::wire::space::OwnSpaceBody::Chair(c)) => {
                                self.own.position = c.feet;
                                self.own.velocity = [0.; 3];
                                if let Some(history) = &mut self.space {
                                    space.apply_own_to_latest(self.own.team, self.mission.as_deref(), history);
                                }
                            }
                            Err(error) => {
                                self.fault = Some(error);
                                return;
                            }
                        }
                    }
                } else if self.own.walker {
                    if let Ok(body) = decode_droideka(head.body.get(bytes)) {
                        self.own.position = body.vectors[0];
                        self.own.velocity = body.vectors[1];
                        self.own.yaw = body.numbers[0];
                        self.own.pitch = body.numbers[1];
                        self.own.walker_mode = body.mode;
                        self.own.walker_reverse = body.flags & 2 != 0;
                        self.own.walker_time = body.numbers[3];
                        self.own.walker_legs = body.numbers[4];
                        self.own.walker_roll = body.numbers[6];
                        self.own.walker_lean = body.numbers[7];
                        self.own.walker_gun = body.numbers[11];
                        self.own.shield = body.shield;
                        self.own.shield_on = body.flags & 16 != 0;
                        self.own.posture = (body.mode >= 11) as usize;
                        self.own.air = 0;
                        self.own.dive = 0.;
                    }
                } else if head.body_schema == 7 {
                    if bfe_sim::prediction::corrections::decode_compact_flight_into(
                        head.body.get(bytes),
                        true,
                        [true; 9],
                        [true; 4],
                        &mut self.flight,
                    )
                    .is_ok()
                    {
                        self.own.position = self.flight.vectors[0];
                        self.own.velocity = self.flight.vectors[1];
                        self.own.yaw = self.flight.seat[0].value;
                        self.own.pitch = self.flight.seat[1].value;
                        use bfe_sim::gen::ClientFlightEnumField as field;
                        self.own.flight = Some(self.flight.enums[field::State as usize]);
                        self.own.flight_warning = self.flight.enums[field::Warning as usize];
                        self.own.flight_can_land = self.flight.enums[field::Reason as usize]
                            == bfe_sim::flight::Message::PressSpace as u8;
                    }
                } else if head.body_schema == 10 {
                    if let Ok(body) = bfe_sim::prediction::corrections::decode_ground(head.body.get(bytes), true, [true; 2]) {
                        self.own.position = body.position;
                        self.own.velocity = body.velocity;
                        self.own.yaw = body.seat[0].value;
                        self.own.pitch = body.seat[1].value;
                    }
                }
                self.own_fact.fill(0.);
                let fact = &mut self.own_fact;
                fact[r::KIND] = if self.own.walker { k::DROIDEKA } else { k::SOLDIER } as f64;
                fact[r::ID] = self.own.id;
                fact[r::GENERATION] = self.own.generation;
                fact[r::CLASS_ID] = self.own.class as f64;
                fact[r::TEAM] = self.own.team as f64;
                fact[r::POSITION..r::POSITION + 3].copy_from_slice(&self.own.position);
                fact[r::VELOCITY..r::VELOCITY + 3].copy_from_slice(&self.own.velocity);
                fact[r::YAW] = self.own.yaw;
                fact[r::PITCH] = self.own.pitch;
                fact[r::POSTURE] = self.own.posture as f64;
                fact[r::STATE] = self.own.air as f64;
                fact[r::HEALTH] = self.own.health;
                fact[r::DEAD] = (!self.own.alive) as u8 as f64;
                if self.own.walker {
                    fact[r::MODE] = self.own.walker_mode as f64;
                    fact[r::REVERSE] = self.own.walker_reverse as u8 as f64;
                    fact[r::TIME] = self.own.walker_time;
                    fact[r::LEGS] = self.own.walker_legs;
                    fact[r::ROLL] = self.own.walker_roll;
                    fact[r::LEAN] = self.own.walker_lean;
                    fact[r::SHIELD] = self.own.shield.unwrap_or(-1.);
                    fact[r::SHIELD_ON] = self.own.shield_on as u8 as f64;
                }
                fact[r::SEAT_PRESENT..=r::SEAT_INDEX].fill(0.);
                if let Some((id, generation, index)) = self.own.mount {
                    fact[r::SEAT_PRESENT] = 1.;
                    fact[r::SEAT_VEHICLE_ID] = id as f64;
                    fact[r::SEAT_VEHICLE_GENERATION] = generation as f64;
                    fact[r::SEAT_INDEX] = index as f64;
                }
                if let Ok((_, mut weapons)) = bfe_sim::prediction::corrections::own_weapons(bytes) {
                    if let Some(Ok(weapon)) = weapons.next() {
                        fact[r::WEAPON] = weapon.id as f64;
                    }
                }
                self.corrected_sequence = owner.header[h::ACK_INPUT];
                self.corrections += 1;
                if head.body_schema != bfe_sim::wire::space::OWN_BODY_SCHEMA {
                    if let Some(space) = self.space_online.as_mut() {
                        space.clear_own();
                        if let Some(history) = &mut self.space {
                            space.apply_own_to_latest(self.own.team, self.mission.as_deref(), history);
                        }
                    }
                }
                continue;
            }
            if kind == k::TOMBSTONE {
                self.publications.push(row, &[]);
                for removed in k::SOLDIER..=k::REMOTE {
                    if let Some(&index) = self.indices.get(&(removed, id)) {
                        if self.facts[index][r::GENERATION] == row[r::GENERATION] {
                            self.indices.remove(&(removed, id));
                            self.facts[index].fill(0.);
                            self.free.push(index);
                        }
                    }
                }
                continue;
            }
            if kind > k::REMOTE {
                continue;
            }
            let index = if let Some(&index) = self.indices.get(&(kind, id)) {
                index
            } else {
                let Some(index) = self.free.pop() else {
                    self.refused += 1;
                    continue;
                };
                self.indices.insert((kind, id), index);
                index
            };
            let seat = if self.facts[index][r::GENERATION] == row[r::GENERATION] {
                Some([
                    self.facts[index][r::SEAT_PRESENT],
                    self.facts[index][r::SEAT_VEHICLE_ID],
                    self.facts[index][r::SEAT_VEHICLE_GENERATION],
                    self.facts[index][r::SEAT_INDEX],
                ])
            } else {
                None
            };
            self.facts[index].copy_from_slice(row);
            if let Some(seat) = seat {
                self.facts[index][r::SEAT_PRESENT..=r::SEAT_INDEX].copy_from_slice(&seat);
            }
        }
        for at in 0..(owner.header[h::SEATS] as usize).min(owner.leases.seats.len()) {
            let lease = owner.leases.seats[at] as usize;
            let row = &owner.leases.records[lease * r::STRIDE..][..r::STRIDE];
            let id = row[r::ID] as u32;
            if let Some(&index) = self.indices.get(&(k::SOLDIER, id)).or_else(|| self.indices.get(&(k::DROIDEKA, id))) {
                if self.facts[index][r::GENERATION] == row[r::GENERATION] {
                    self.facts[index][r::SEAT_PRESENT..=r::SEAT_INDEX].copy_from_slice(&row[r::SEAT_PRESENT..=r::SEAT_INDEX]);
                }
            }
        }
        if owner.header[h::COMPLETE] != 0. {
            if let (Some(space), Some(history)) = (self.space_online.as_mut(), self.space.as_mut()) {
                space.publish_snapshot(tick as u32, self.own.team, self.mission.as_deref(), history);
            }
        }
    }
    // Reliable bootstrap is cold. Repeated input receipts instead borrow
    // strings and fixed arrays directly from their current transport message.
    fn text(&mut self, bytes: &[u8], now: f64) {
        if self.swbf2_plan.is_none() {
            self.released_text(bytes, now);
        } else {
            self.swbf2_text(bytes, now);
        }
    }
    fn released_text(&mut self, bytes: &[u8], now: f64) {
        if bytes.len() > 262144 {
            self.fault = Some("server message exceeds bounds");
            return;
        }
        let Ok(m) = serde_json::from_slice::<Message>(bytes) else {
            self.fault = Some("invalid server message");
            return;
        };
        match m.kind {
            "chat" => {
                if let Err(error) = super::chat::receive(bytes, &mut self.chat, now) {
                    self.fault = Some(error);
                }
            }
            "entity-registry" => {
                if self.registry {
                    self.fault = Some("duplicate entity registry");
                    return;
                }
                let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
                    return;
                };
                let mut wanted = self.expected.clone();
                // Epoch is an authority lease, not authored content. All table
                // names and input profiles still have to match local content.
                let epoch = value["registry"]["contentEpoch"].as_u64().unwrap_or(0);
                if epoch == 0 || epoch > u32::MAX as u64 {
                    self.fault = Some("invalid content epoch");
                    return;
                }
                wanted["contentEpoch"] = epoch.into();
                if value["registry"] != wanted {
                    self.fault = Some("entity content/input registry mismatch");
                    return;
                }
                self.registry = true;
            }
            // Registry ordering is a protocol prerequisite. An authenticated
            // connection is not yet subscribed until Client sends start.
            "welcome" => {
                if !self.registry || self.welcomed {
                    self.fault = Some("unexpected battle welcome");
                    return;
                }
                self.welcomed = true;
                self.team_select = m.team_select;
                self.spectator = m.spectator;
                self.own.id = m.player_id.unwrap_or(-1.);
                self.epoch = m.epoch.unwrap_or(1.);
                if let Some(t) = m.tick {
                    self.latest = t;
                    self.estimator.arrive(t, now / 1000., 0.);
                }
            }
            "gameplay-events" => {
                if let Err(error) = super::events::receive(bytes, &mut self.publications) {
                    self.fault = Some(error);
                }
            }
            "input-ack" => {
                if m.epoch == Some(self.epoch) && m.generation == Some(self.own.generation) {
                    if m.ack.unwrap_or(0.) > self.ack {
                        self.ack_tick = m.ack_tick.unwrap_or(0.);
                        self.ack_server_tick = m.server_tick.unwrap_or(0.);
                    }
                    self.server_dropped = self.server_dropped.max(m.dropped.unwrap_or(0.));
                    self.ack = self.ack.max(m.ack.unwrap_or(0.));
                    if let Some(t) = m.server_tick {
                        self.latest = self.latest.max(t);
                        self.estimator.arrive(t, now / 1000., 0.);
                    }
                }
            }
            "entity-edge-results" => {
                if m.epoch == Some(self.epoch) && m.generation == Some(self.own.generation) {
                    for &id in &m.received.values[..m.received.count] {
                        self.received.add(id);
                    }
                    for &id in &m.results.0.values[..m.results.0.count] {
                        self.outcomes.add(id);
                    }
                }
            }
            "match" => {
                // A refused announcement also ends the one it replaces: nothing pending stays.
                match super::rotation::NextMatch::new(m.query.unwrap_or_default(), m.delay_ms, now) {
                    Ok(next) => self.next_match = Some(next),
                    Err(error) => {
                        self.next_match = None;
                        self.fault = Some(error);
                    }
                }
            }
            "ping" => {
                // A paused receiver retains a bounded heartbeat batch. A full
                // bank replaces the oldest, just like replaceable receipts.
                if self.pong_count == self.pongs.len() {
                    self.pong_sent();
                }
                self.pongs[(self.pong_at + self.pong_count) % self.pongs.len()] = m.time;
                self.pong_count += 1;
            }
            "gameplay-error" | "version-error" => {
                self.fault_detail.clear();
                self.fault_detail.extend(m.message.as_ref().map_or("server rejected battle admission", Notice::text).chars().take(240));
                self.fault = Some("server rejected battle admission");
            }
            _ => {}
        }
    }
    fn swbf2_text(&mut self, bytes: &[u8], now: f64) {
        if bytes.len() > 262144 {
            self.fault = Some("server message exceeds bounds");
            return;
        }
        let Ok(m) = serde_json::from_slice::<Message>(bytes) else {
            self.fault = Some("invalid server message");
            return;
        };
        match m.kind {
            "chat" => {
                if let Err(error) = super::chat::receive(bytes, &mut self.chat, now) {
                    self.fault = Some(error);
                }
            }
            "entity-registry" => {
                if self.registry {
                    self.fault = Some("duplicate entity registry");
                    return;
                }
                let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
                    return;
                };
                let mut wanted = self.expected.clone();
                let epoch = value["registry"]["contentEpoch"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
                if epoch == 0 {
                    self.fault = Some("invalid content epoch");
                    return;
                }
                wanted["contentEpoch"] = epoch.into();
                if value["registry"] != wanted {
                    self.fault = Some("entity content/input registry mismatch");
                    return;
                }
                // A 2005 match's dictionaries travel as records beside the registry.
                if let (Some(plan), Some(records)) = (&self.swbf2_plan, value.get("records")) {
                    let Some(records) = records.as_array().filter(|r| r.len() <= bfe_sim::wire::revisions::MAX_KINDS) else {
                        self.fault = Some("invalid entity registry");
                        return;
                    };
                    for record in records {
                        use bfe_sim::wire::{mission, revisions};
                        match revisions::recorded(plan, record) {
                            Ok(None) => {}
                            // The mission's teams and their classes, by the ids of
                            // the registry just matched; one record names them.
                            Ok(Some((revisions::REGISTRY, body))) if self.mission.is_none() => {
                                match mission::Roster::read(body, |id| self.registered("classes", id).is_some()) {
                                    Ok(roster) => {
                                        self.mission = Some(Box::new(super::mission::Mission::new(&roster, |id| self.registered("classes", id))));
                                    }
                                    Err(error) => { self.fault = Some(error); return; }
                                }
                            }
                            Ok(Some(_)) => { self.fault = Some("a Battlefront II registry record this build does not read"); return; }
                            Err(error) => { self.fault = Some(error); return; }
                        }
                    }
                }
                self.registry = true;
            }
            "welcome" => {
                if !self.registry || self.welcomed {
                    self.fault = Some("unexpected battle welcome");
                    return;
                }
                // A 2005 match's welcome carries the epoch's flight ceiling, which
                // prediction needs before its first tick.
                match welcome_ceiling(bytes) {
                    Some(ceiling) => self.flight_ceiling = Some(ceiling),
                    None => {
                        self.fault = Some("a Battlefront II welcome without a usable flight ceiling");
                        return;
                    }
                }
                self.welcomed = true;
                self.team_select = m.team_select;
                self.spectator = m.spectator;
                self.own.id = m.player_id.unwrap_or(-1.);
                self.epoch = m.epoch.unwrap_or(1.);
                if let Some(t) = m.tick {
                    self.latest = t;
                    self.estimator.arrive(t, now / 1000., 0.);
                }
            }
            "gameplay-events" => {
                if let Err(error) = super::events::receive(bytes, &mut self.publications) {
                    self.fault = Some(error);
                }
            }
            "input-ack" => {
                if m.epoch == Some(self.epoch) && m.generation == Some(self.own.generation) {
                    let ack = m.ack.unwrap_or(0.);
                    if ack > self.ack_measured {
                        self.ack_measured = ack;
                        self.ack_tick = m.ack_tick.unwrap_or(0.);
                        self.ack_server_tick = m.server_tick.unwrap_or(0.);
                    }
                    self.server_dropped = self.server_dropped.max(m.dropped.unwrap_or(0.));
                    self.ack = self.ack.max(ack);
                    if let Some(t) = m.server_tick {
                        self.latest = self.latest.max(t);
                        self.estimator.arrive(t, now / 1000., 0.);
                    }
                }
            }
            "entity-edge-results" => {
                if m.epoch == Some(self.epoch) && m.generation == Some(self.own.generation) {
                    for &id in &m.received.values[..m.received.count] {
                        self.received.add(id);
                    }
                    for &id in &m.results.0.values[..m.results.0.count] {
                        self.outcomes.add(id);
                    }
                }
            }
            // A 2005 record on the reliable lane, read by the match's plan: an
            // optional one this build does not implement is skipped.
            "record" => {
                let Some(plan) = &self.swbf2_plan else { return };
                let Ok(value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
                    self.fault = Some("invalid server message");
                    return;
                };
                match bfe_sim::wire::revisions::recorded(plan, &value) {
                    Ok(None) => {}
                    Ok(Some((bfe_sim::wire::revisions::NOTICE, body))) => {
                        let Some(mut mission) = self.mission.take() else {
                            self.fault = Some("a mission record in a battle without a mission");
                            return;
                        };
                        if let Err(error) = mission.notice(body, |id| self.registered("weapons", id)) { self.fault = Some(error); }
                        self.mission = Some(mission);
                    }
                    Ok(Some(_)) => self.fault = Some("a Battlefront II record a client in the battle does not read"),
                    Err(error) => self.fault = Some(error),
                }
            }
            "match" => {
                match super::rotation::NextMatch::new(m.query.unwrap_or_default(), m.delay_ms, now) {
                    Ok(next) => self.next_match = Some(next),
                    Err(error) => {
                        self.next_match = None;
                        self.fault = Some(error);
                    }
                }
            }
            "ping" => {
                if self.pong_count == self.pongs.len() {
                    self.pong_sent();
                }
                self.pongs[(self.pong_at + self.pong_count) % self.pongs.len()] = m.time;
                self.pong_count += 1;
            }
            "gameplay-error" | "version-error" => {
                self.fault_detail.clear();
                self.fault_detail.extend(m.message.as_ref().map_or("server rejected battle admission", Notice::text).chars().take(240));
                self.fault = Some("server rejected battle admission");
            }
            _ => {}
        }
    }
    // The name a registry id has in one table of the matched registry.
    fn registered(&self, table: &str, id: u16) -> Option<String> {
        let rows = self.expected.get(table)?.as_array()?;
        rows.iter().find(|row| row[0].as_u64() == Some(id.into()))?.get(1)?.as_str().map(str::to_owned)
    }
    /// What a scripted 2005 battle's screens read of this moment; nothing in
    /// every other battle.
    pub fn observe_mission(&mut self, hud: &impl super::mission::Panel, replay: &super::replay::Replay) {
        let Some(mut mission) = self.mission.take() else { return };
        let mut view = self.mission_view.take();
        mission.observe(self, hud, replay, &mut view);
        self.mission_score = view.as_ref().map(|view| view.score);
        (self.mission, self.mission_view) = (Some(mission), view);
    }
    pub fn error(&self) -> Option<String> {
        self.fault.map(|error| if self.fault_detail.is_empty() { error.into() } else { self.fault_detail.clone() })
    }
    // NetworkSession echoes the application clock separately from UDP PING.
    // Serialize into the admitted buffer and keep it until reliable admission.
    pub fn pong(&mut self) -> Option<&[u8]> {
        if self.pong_count == 0 {
            return None;
        }
        use std::io::Write;
        let mut out = std::io::Cursor::new(&mut self.pong_bytes[..]);
        out.write_all(br#"{"type":"pong","time":"#).ok()?;
        serde_json::to_writer(&mut out, &self.pongs[self.pong_at]).ok()?;
        out.write_all(b"}").ok()?;
        let length = out.position() as usize;
        Some(&self.pong_bytes[..length])
    }
    pub fn pong_sent(&mut self) {
        if self.pong_count != 0 {
            self.pong_at = (self.pong_at + 1) % self.pongs.len();
            self.pong_count -= 1;
        }
    }
    pub fn active_slots(&self) -> impl Iterator<Item = (usize, &[f64; r::STRIDE])> {
        self.indices.values().map(|&index| (index, &self.facts[index]))
    }
    pub fn is_space_seated(&self, unit_slot: u32) -> bool {
        self.space_online.as_ref().is_some_and(|s| s.is_seated(unit_slot))
    }
    pub fn space_mounted(&self) -> bool {
        self.space_online.as_ref().is_some_and(|s| s.own_body.is_some())
            || self.space.as_ref().and_then(|h| h.latest()).is_some_and(|f| f.mount.is_some() || f.chair.is_some())
    }
    pub fn in_space_chair(&self) -> bool {
        self.space_online.as_ref().is_some_and(|s| matches!(s.own_body, Some(bfe_sim::wire::space::OwnSpaceBody::Chair(_))))
            || self.space.as_ref().and_then(|h| h.latest()).is_some_and(|f| f.chair.is_some())
    }
    pub fn in_space_gunner(&self) -> bool {
        self.space_online.as_ref().is_some_and(|s| matches!(&s.own_body, Some(bfe_sim::wire::space::OwnSpaceBody::Pilot(p)) if p.seat != 0))
            || self.space.as_ref().and_then(|h| h.latest()).and_then(|f| f.cockpit).is_some_and(|c| c.seat != 0)
    }
    pub fn in_space_pilot(&self) -> bool {
        self.space_online.as_ref().is_some_and(|s| matches!(&s.own_body, Some(bfe_sim::wire::space::OwnSpaceBody::Pilot(p)) if p.seat == 0))
            || self.space.as_ref().and_then(|h| h.latest()).and_then(|f| f.cockpit).is_some_and(|c| c.seat == 0)
    }
    /// Aircraft bindings and saved perspective also apply in gunner seats.
    /// Only the pilot receives a flight body; passengers name their hull by lease.
    pub fn in_aircraft(&self) -> bool {
        if self.own.flight.is_some() || self.in_space_pilot() || self.in_space_gunner() {
            return true;
        }
        let Some((id, generation, _)) = self.own.mount.filter(|_| self.own.mount_kind == k::VEHICLE) else {
            return false;
        };
        let Some(&index) = self.indices.get(&(k::VEHICLE, id)) else { return false };
        let row = &self.facts[index];
        // Live aircraft publish Landed through Landing (0..3); ground hulls
        // publish 5. Destroyed and dying hulls no longer carry their crew.
        row[r::GENERATION] == generation as f64 && (0. ..4.).contains(&row[r::STATE])
    }
    pub(crate) fn in_gunner_seat(&self) -> bool {
        self.in_space_gunner()
            || self.in_space_chair()
            || self.own.mount.is_some_and(|(_, _, seat)| seat != 0 || self.own.mount_kind == k::TURRET)
    }
    /// Whether this seat drives flight input and uses the saved flight camera.
    pub fn uses_flight_controls(&self) -> bool {
        match self.control_family {
            bfe_family::GameFamily::Swbf1 => self.in_aircraft(),
            bfe_family::GameFamily::Swbf2 => self.space.as_ref().and_then(|s| s.latest()?.mount).is_some() || (!self.in_gunner_seat() && self.in_aircraft()),
        }
    }
    // One part of a multipart 2005 record, dispatched on the kind and
    // revision its own header names, read and checked first: refused when the
    // match does not carry that kind or carries another revision, ignored
    // when this client skips an optional kind, and taken only for a kind this
    // build reassembles (the combat checkpoint).
    fn checkpoint_part(&mut self, bytes: &[u8]) {
        use bfe_sim::wire::{checkpoint, revisions::COMBAT};
        let Some(plan) = &self.swbf2_plan else { return };
        // The part's header (kind, revision, index, count and extent) is
        // checked whole before anything is decided, then the kind and
        // revision against the match's plan, skipped parts included.
        let identity = match checkpoint::identity(bytes) { Ok(identity) => identity, Err(error) => { self.fault = Some(error); return } };
        match plan.skips(identity.kind, identity.revision) {
            Err(why) => self.fault = Some(why),
            Ok(true) => {}
            Ok(false) if identity.kind == COMBAT => {
                if let Err(error) = self.combat_parts.offer(bytes, COMBAT, identity.revision) { self.fault = Some(error); }
            }
            Ok(false) => self.fault = Some("a multipart Battlefront II record this build does not reassemble"),
        }
    }
    /// The combat checkpoint committed with the accepted own correction: its
    /// identity (that correction's epoch, tick, unit and life) and bytes.
    pub fn combat(&self) -> Option<(bfe_sim::wire::checkpoint::Identity, &[u8])> { self.combat_parts.complete() }
    /// The display of a 2005 soldier's life, if the latest one is of that life.
    pub fn swbf2_display(&self, slot: u32, life: u32) -> Option<&bfe_sim::movement::soldier::swbf2::body::Display> {
        self.swbf2_displays.get(&slot).filter(|(known, _)| *known == life).map(|(_, display)| display)
    }
    /// The live record of one kind with this identity and generation. Ids are
    /// unique only within a kind: a dropped power-up's slot number and life can
    /// equal a hull's id and generation in the same snapshot.
    pub fn record(&self, kind: u32, id: f64, generation: f64) -> Option<&[f64; r::STRIDE]> {
        let &index = self.indices.get(&(kind, id as u32))?;
        let row = &self.facts[index];
        (row[r::GENERATION] == generation).then_some(row)
    }
    #[cfg(test)]
    pub(crate) fn insert_fact(&mut self, kind: u32, row: [f64; r::STRIDE]) {
        let index = self.free.pop().unwrap();
        self.indices.insert((kind, row[r::ID] as u32), index);
        self.facts[index] = row;
    }
    pub fn vehicle_seats(&self, id: u32, generation: u32) -> u16 {
        let mut seats = 0;
        for row in self.active().filter(|row| row[r::ID] != self.own.id && matches!(row[r::KIND] as u32, k::SOLDIER | k::DROIDEKA)) {
            if row[r::SEAT_PRESENT] != 0.
                && row[r::SEAT_VEHICLE_ID] == id as f64
                && row[r::SEAT_VEHICLE_GENERATION] == generation as f64
            {
                seats |= 1u16.checked_shl(row[r::SEAT_INDEX] as u32).unwrap_or(0);
            }
        }
        // The own correction supersedes its delayed public seat lease.
        if let Some((_, _, index)) = self.own.mount.filter(|lease| {
            self.own.mount_kind == k::VEHICLE && lease.0 == id && lease.1 == generation
        }) {
            seats |= 1u16.checked_shl(index as u32).unwrap_or(0);
        }
        seats
    }
    pub fn active(&self) -> impl Iterator<Item = &[f64; r::STRIDE]> {
        self.indices.values().map(|&index| &self.facts[index]).chain(std::iter::once(&self.own_fact).filter(|_| {
            self.corrections != 0
                && self.indices.get(&(k::SOLDIER, self.own.id as u32)).is_none()
                && self.indices.get(&(k::DROIDEKA, self.own.id as u32)).is_none()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vehicle_seat_occupancy_follows_qualified_leases() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.own.id = 1.;
        receiver.own.mount_kind = k::VEHICLE;
        receiver.own.mount = Some((9, 3, 0));
        receiver.indices.insert((k::SOLDIER, 2), 0);
        let row = &mut receiver.facts[0];
        row[r::KIND] = k::SOLDIER as f64;
        row[r::ID] = 2.;
        row[r::SEAT_PRESENT] = 1.;
        row[r::SEAT_VEHICLE_ID] = 9.;
        row[r::SEAT_VEHICLE_GENERATION] = 3.;
        row[r::SEAT_INDEX] = 2.;
        assert_eq!(receiver.vehicle_seats(9, 3), 0b101);
        assert_eq!(receiver.vehicle_seats(9, 4), 0);
        receiver.own.mount = Some((9, 3, 1));
        assert_eq!(receiver.vehicle_seats(9, 3), 0b110);
        receiver.own.mount_kind = k::TURRET;
        assert_eq!(receiver.vehicle_seats(9, 3), 0b100);
        receiver.facts[0][r::SEAT_PRESENT] = 0.;
        assert_eq!(receiver.vehicle_seats(9, 3), 0);
    }

    #[test]
    fn aircraft_controls_follow_passenger_leases() {
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.own.mount_kind = k::VEHICLE;
        receiver.own.mount = Some((12, 3, 1));
        receiver.indices.insert((k::VEHICLE, 12), 0);
        receiver.facts[0][r::GENERATION] = 3.;
        for state in 0..4 {
            receiver.facts[0][r::STATE] = state as f64;
            assert!(receiver.in_aircraft());
        }
        receiver.facts[0][r::STATE] = 5.;
        assert!(!receiver.in_aircraft());
        receiver.facts[0][r::STATE] = 0.;
        receiver.facts[0][r::GENERATION] = 4.;
        assert!(!receiver.in_aircraft());
        receiver.facts[0][r::GENERATION] = 3.;
        receiver.own.mount_kind = k::TURRET;
        assert!(!receiver.in_aircraft());
        receiver.own.mount_kind = k::VEHICLE;
        receiver.own.mount = None;
        assert!(!receiver.in_aircraft());
        receiver.own.flight = Some(0);
        assert!(receiver.in_aircraft());
    }

    #[test]
    fn changing_aircraft_seats_selects_the_2005_gunner_controls() {
        use bfe_family::GameFamily;
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.own.mount_kind = k::VEHICLE;
        receiver.indices.insert((k::VEHICLE, 12), 0);
        receiver.facts[0][r::GENERATION] = 3.;
        receiver.facts[0][r::STATE] = 1.;
        for family in [GameFamily::Swbf1, GameFamily::Swbf2] {
            receiver.control_family = family;
            for (seat, pilot_body) in [(0, None), (2, Some(0)), (2, None), (0, Some(0))] {
                receiver.own.mount = Some((12, 3, seat));
                receiver.own.flight = pilot_body;
                assert_eq!(receiver.uses_flight_controls(), seat == 0 || family == GameFamily::Swbf1);
            }
        }
        receiver.own.mount_kind = k::TURRET;
        assert!(!receiver.uses_flight_controls());
        receiver.own.mount = None;
        assert!(receiver.uses_flight_controls());
    }

    #[test]
    fn a_power_up_with_the_hull_id_and_generation_is_not_the_hull() {
        // A dropped power-up's slot 2, life 1 beside hull 2, generation 1: the
        // item's bucket comes first, so a lookup by id and generation alone
        // took the item for the hull the player drives.
        let mut receiver = Receiver::new(serde_json::Value::Null);
        for (kind, class) in [(k::VEHICLE, 12.), (k::ITEM, 0.)] {
            let mut row = [0.; r::STRIDE];
            row[r::KIND] = kind as f64;
            row[r::ID] = 2.;
            row[r::GENERATION] = 1.;
            row[r::CLASS_ID] = class;
            receiver.insert_fact(kind, row);
        }
        let first = receiver.active().find(|row| row[r::ID] == 2. && row[r::GENERATION] == 1.).unwrap();
        assert_eq!(first[r::KIND], k::ITEM as f64);
        let hull = receiver.record(k::VEHICLE, 2., 1.).unwrap();
        assert_eq!((hull[r::KIND], hull[r::CLASS_ID]), (k::VEHICLE as f64, 12.));
        assert!(receiver.record(k::VEHICLE, 2., 2.).is_none());
    }

    #[test]
    fn compact_soldier_body_decodes_fuel_from_slot_zero_not_state_time() {
        let mut bytes = Vec::with_capacity(124);
        bytes.extend_from_slice(&[0u8, 0, 4, 0]);
        for n in [0.875f64, 1.25, -0.25, 0.0, 4.5, 0.5] {
            bytes.extend_from_slice(&n.to_le_bytes());
        }
        for v in [[1.0f64, 2.0, 3.0], [4.0, 5.0, 6.0], [0.0, 1.0, 0.0]] {
            for c in v {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        let mut own = Own::default();
        own.apply_compact_soldier(Some(&bytes));
        assert_eq!(own.fuel, 0.875);
        assert_eq!(own.yaw, 1.25);
        assert_eq!(own.pitch, -0.25);
        assert_eq!(own.dive, 0.0);
        assert_eq!(own.lock, 0.5);
        assert_eq!(own.air, 4);
        assert_eq!(own.position, [1.0, 2.0, 3.0]);
        assert_eq!(own.velocity, [4.0, 5.0, 6.0]);
    }
    // A complete own correction of unit 7, life 1 (a standing soldier, an
    // empty arsenal), in one own datagram of epoch 1 at `tick`.
    fn own_datagram(tick: f64) -> Vec<u8> {
        use bfe_sim::wire::codec::{own_correction_into, own_datagram_into, OwnParts, Writer};
        let mut compact = vec![0, 0, 0, 1];
        for n in [0f64; 6].into_iter().chain([1., 2., 3., 0., 0., 0., 0., 1., 0.]) { compact.extend(n.to_le_bytes()); }
        let mut b = Vec::new();
        b.extend(1u32.to_le_bytes()); b.extend(1u32.to_le_bytes()); b.extend(1u16.to_le_bytes()); b.push(1); b.push(6);
        b.extend(0u32.to_le_bytes()); b.extend(0u32.to_le_bytes());
        b.extend((compact.len() as u16).to_le_bytes()); b.extend_from_slice(&compact);
        b.extend(100f64.to_le_bytes()); b.push(0); b.extend(0f64.to_le_bytes()); b.extend(0f64.to_le_bytes()); b.push(0);
        for _ in 0..3 { b.extend(0f64.to_le_bytes()); }
        b.push(0); b.extend(1f64.to_le_bytes()); b.push(0); b.extend([255, 255]);
        b.push(0);
        b.extend([0; 16]); b.extend(0u16.to_le_bytes()); b.extend(0u16.to_le_bytes()); b.extend(0u32.to_le_bytes()); b.push(0);
        let mut section = Writer::default();
        own_correction_into(&mut section, 7., 1., 11., &b, None).unwrap();
        let mut own = Writer::default();
        own_datagram_into(&mut own, OwnParts { epoch: 1., tick, interval: 3., baseline: 0., ack: 0., second: false, count: 1.,
            correction: Some(section.0.as_slice()), summaries: None, cosmetics: None }).unwrap();
        own.0
    }

    // A 2005 match's checkpoint parts commit only with the own correction the
    // receiver accepted for their epoch, tick, unit and life, whichever arrives
    // last; a delayed older checkpoint never replaces it; a match that does not
    // carry checkpoints refuses them; and a 2004 battle's receiver reads such a
    // datagram as released.
    #[test]
    fn checkpoint_parts_commit_with_the_accepted_correction() {
        use bfe_sim::wire::{checkpoint::{split, Anchor, Identity}, revisions::{Plan, Use, COMBAT, INPUT, SOLDIER}};
        let carried = Plan { kinds: vec![(INPUT, Use::Revision(1)), (SOLDIER, Use::Revision(1)), (COMBAT, Use::Revision(1))] };
        let mut receiver = Receiver::new(serde_json::Value::Null);
        (receiver.registry, receiver.welcomed, receiver.swbf2_plan) = (true, true, Some(carried));
        let identity = |tick: u32| Identity { anchor: Anchor { epoch: 1, tick, subject: 7, life: 1 }, kind: COMBAT, revision: 1 };
        let mut parts = Vec::new();
        split(identity(6), &[5; 1500], &mut parts).unwrap();
        for part in &parts { receiver.message(false, part, 0.); }
        assert!(receiver.fault.is_none() && receiver.combat().is_none());
        receiver.message(false, &own_datagram(6.), 100.);
        assert_eq!(receiver.fault, None);
        assert_eq!(receiver.combat(), Some((identity(6), &[5u8; 1500][..])));
        // The next tick's correction comes first, then its part.
        receiver.message(false, &own_datagram(9.), 150.);
        assert!(receiver.combat().is_none());
        split(identity(9), &[6; 40], &mut parts).unwrap();
        receiver.message(false, &parts[0], 0.);
        assert_eq!(receiver.combat(), Some((identity(9), &[6u8; 40][..])));
        // A delayed checkpoint of tick 6 changes nothing.
        split(identity(6), &[5; 20], &mut parts).unwrap();
        receiver.message(false, &parts[0], 0.);
        assert_eq!((receiver.fault, receiver.combat()), (None, Some((identity(9), &[6u8; 40][..]))));
        // A match whose table does not name checkpoints refuses a part.
        let mut receiver = Receiver::new(serde_json::Value::Null);
        (receiver.registry, receiver.welcomed) = (true, true);
        receiver.swbf2_plan = Some(Plan { kinds: vec![(INPUT, Use::Revision(1)), (SOLDIER, Use::Revision(1))] });
        receiver.message(false, &parts[0], 0.);
        assert_eq!(receiver.fault, Some("a Battlefront II record this match does not carry"));
        // A 2004 battle's receiver hands it to the released decoder.
        let mut released = Receiver::new(serde_json::Value::Null);
        (released.registry, released.welcomed) = (true, true);
        released.message(false, &parts[0], 0.);
        let mut session = Session::new();
        session.reserve_receive();
        assert_eq!(released.fault, session.receive_with_bounds(&parts[0], &PositionBounds::LEGACY).err());
    }

    // A multipart record of an optional kind this build does not implement,
    // split by the real writer, is dispatched on the kind its own header names
    // and skipped at the revision the match advertises for it; one at another
    // revision, one the match does not name, or a carried kind at another
    // revision than the match's, is refused.
    #[test]
    fn multipart_records_are_dispatched_on_their_own_kind() {
        use bfe_sim::wire::{checkpoint::{split, Anchor, Identity}, revisions::{Plan, Use, COMBAT, INPUT, SIT_OUT, SOLDIER}};
        let part = |kind: u8, revision: u16| {
            let mut parts = Vec::new();
            split(Identity { anchor: Anchor { epoch: 1, tick: 6, subject: 7, life: 1 }, kind, revision }, &[3; 50], &mut parts).unwrap();
            parts.remove(0)
        };
        let receiver = |plan: Plan| {
            let mut receiver = Receiver::new(serde_json::Value::Null);
            (receiver.registry, receiver.welcomed, receiver.swbf2_plan) = (true, true, Some(plan));
            receiver
        };
        let base = vec![(INPUT, Use::Revision(1)), (SOLDIER, Use::Revision(1)), (SIT_OUT, Use::Revision(1))];
        let advertised = Plan { kinds: [base.clone(), vec![(91, Use::Skip(2))]].concat() };
        let mut skipped = receiver(advertised.clone());
        skipped.message(false, &part(91, 2), 0.);
        assert_eq!((skipped.fault, skipped.combat()), (None, None));
        let mut later = receiver(advertised);
        later.message(false, &part(91, 3), 0.);
        assert_eq!(later.fault, Some("a Battlefront II record of another revision than the match's"));
        let mut unnamed = receiver(Plan { kinds: base.clone() });
        unnamed.message(false, &part(91, 2), 0.);
        assert_eq!(unnamed.fault, Some("a Battlefront II record this match does not carry"));
        let mut other = receiver(Plan { kinds: [base, vec![(COMBAT, Use::Revision(1))]].concat() });
        other.message(false, &part(COMBAT, 2), 0.);
        assert_eq!(other.fault, Some("a Battlefront II record of another revision than the match's"));
    }

    // A 2005 match's welcome names the epoch's flight ceiling, a finite
    // binary32 widened exactly; one without a usable ceiling is refused. A
    // 2004 welcome is read as released, whatever such a key holds.
    #[test]
    fn a_2005_welcome_names_its_flight_ceiling() {
        use bfe_sim::wire::revisions::{Plan, Use, INPUT, SIT_OUT, SOLDIER};
        let welcome = |plan: Option<Plan>, extra: &str| {
            let mut receiver = Receiver::new(serde_json::Value::Null);
            (receiver.registry, receiver.swbf2_plan) = (true, plan);
            receiver.message(true, format!(r#"{{"type":"welcome","playerId":4,"tick":30{extra}}}"#).as_bytes(), 0.);
            (receiver.welcomed, receiver.fault, receiver.flight_ceiling)
        };
        let plan = || Some(Plan { kinds: vec![(INPUT, Use::Revision(1)), (SOLDIER, Use::Revision(1)), (SIT_OUT, Use::Revision(1))] });
        for ceiling in [40., 1800., 200., -12.5, 3.4028234663852886e38] {
            assert_eq!(welcome(plan(), &format!(r#","flightCeiling":{ceiling:?}"#)), (true, None, Some(ceiling)), "{ceiling}");
        }
        let refused = (false, Some("a Battlefront II welcome without a usable flight ceiling"), None);
        for extra in ["", r#","flightCeiling":null"#, r#","flightCeiling":"40""#, r#","flightCeiling":200.1"#,
            r#","flightCeiling":3.5e38"#, r#","flightCeiling":1e400"#, r#","flightCeiling":40,"flightCeiling":40"#] {
            assert_eq!(welcome(plan(), extra), refused, "{extra}");
        }
        for extra in ["", r#","flightCeiling":"forty""#, r#","flightCeiling":40"#] {
            assert_eq!(welcome(None, extra), (true, None, None), "{extra}");
        }
    }

    // A 2005 soldier's shared display reaches the life it belongs to, never
    // an older life's in place of a newer one.
    #[test]
    fn a_soldier_display_is_kept_for_its_life() {
        use bfe_sim::gen::{WireField, WireSnapshot};
        use bfe_sim::movement::soldier::swbf2::body::DISPLAY_SCHEMA;
        use bfe_sim::wire::revisions::{Plan, Use, INPUT, SOLDIER, SOLDIER_DISPLAY};
        let mut receiver = Receiver::new(serde_json::Value::Null);
        (receiver.registry, receiver.welcomed) = (true, true);
        receiver.swbf2_plan = Some(Plan { kinds: vec![(INPUT, Use::Revision(1)), (SOLDIER, Use::Revision(1)), (SOLDIER_DISPLAY, Use::Revision(1))] });
        let mut encoder = bfe_sim::wire::buffered::Encoder::new(8);
        let mut send = |receiver: &mut Receiver, tick: f64, life: f64, flags: u8| {
            let field = WireField { id: 100_005., schema: f64::from(DISPLAY_SCHEMA), generation: life, payload: vec![1, 0, 0, 128, flags] };
            let snapshot = WireSnapshot { tick, epoch: 1., slots: vec![], world: vec![field] };
            for part in encoder.encode(&snapshot, None, tick, PositionBounds::LEGACY.width()).unwrap() { receiver.message(false, &part, tick * 50.); }
        };
        send(&mut receiver, 3., 2., 1);
        assert_eq!(receiver.fault, None);
        let shown = receiver.swbf2_display(5, 2).copied().unwrap();
        assert!(shown.sprint && !shown.exhausted && receiver.swbf2_display(5, 1).is_none());
        // A late field of the previous life does not replace it.
        send(&mut receiver, 6., 1., 8);
        assert!(receiver.swbf2_display(5, 2).is_some_and(|d| d.sprint) && receiver.swbf2_display(5, 1).is_none());
    }

    // A departed soldier's display field is removed by the authority: the
    // removal reaches the kept displays, and an id no battle can hold is
    // refused instead of stored.
    #[test]
    fn a_removed_soldier_display_is_forgotten() {
        use bfe_sim::gen::{WireField, WireSnapshot};
        use bfe_sim::movement::soldier::swbf2::body::DISPLAY_SCHEMA;
        use bfe_sim::wire::revisions::{Plan, Use, INPUT, SOLDIER, SOLDIER_DISPLAY};
        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.network.borrow_mut().admit_swbf2();
        (receiver.registry, receiver.welcomed) = (true, true);
        receiver.swbf2_plan = Some(Plan { kinds: vec![(INPUT, Use::Revision(1)), (SOLDIER, Use::Revision(1)), (SOLDIER_DISPLAY, Use::Revision(1))] });
        let mut encoder = bfe_sim::wire::buffered::Encoder::new(8);
        let field = |id: f64| WireField { id, schema: f64::from(DISPLAY_SCHEMA), generation: 1., payload: vec![1, 0, 0, 128, 1] };
        let at = |tick: f64, world: Vec<WireField>| WireSnapshot { tick, epoch: 1., slots: vec![], world };
        let first = at(3., vec![field(100_005.)]);
        for part in encoder.encode(&first, None, 3., PositionBounds::LEGACY.width()).unwrap() { receiver.message(false, &part, 150.); }
        // The own datagram holds tick 3 as the baseline a delta is read against.
        let mut own = bfe_sim::wire::codec::Writer::default();
        bfe_sim::wire::codec::own_datagram_into(&mut own, bfe_sim::wire::codec::OwnParts { epoch: 1., tick: 3., interval: 3., baseline: 0., ack: 0., second: false, count: 1., correction: None, summaries: None, cosmetics: None }).unwrap();
        receiver.message(false, &own.0, 150.);
        assert_eq!(receiver.fault, None);
        assert!(receiver.swbf2_display(5, 1).is_some());
        // The soldier departs: the next snapshot no longer carries his field.
        for part in encoder.encode(&at(6., vec![]), Some(&first), 3., PositionBounds::LEGACY.width()).unwrap() { receiver.message(false, &part, 300.); }
        assert_eq!(receiver.fault, None);
        assert!(receiver.swbf2_display(5, 1).is_none(), "a removed display is still kept");
        assert!(receiver.swbf2_displays.is_empty());
        // An id past every body a battle holds is not a soldier of it.
        let mut displays = Displays::default();
        let plan = Plan { kinds: vec![(SOLDIER_DISPLAY, Use::Revision(1))] };
        assert!(install_display(&mut displays, Some(&plan), 100_000 + 512, 1, &[1, 0, 0, 128, 1]).is_err());
        assert!(displays.is_empty());
    }

    #[test]
    fn local_swbf2_receiver_decodes_swbf2_own_soldier_body_without_network_plan() {
        use bfe_sim::movement::soldier::swbf2::body as swbf2_body;
        use bfe_sim::wire::codec::{own_correction_into, own_datagram_into, OwnParts, Writer};

        let mut receiver = Receiver::new(serde_json::Value::Null);
        receiver.control_family = bfe_family::GameFamily::Swbf2;
        (receiver.registry, receiver.welcomed) = (true, true);
        receiver.own.id = 91.0;

        // Compact soldier body (124 bytes: 4 flag/enum bytes + 6 f64 numbers + 3 f64 vectors).
        let mut compact = Vec::with_capacity(124);
        compact.extend_from_slice(&[0u8, 0, 0, 0]); // flags=0, posture=stand, air=0, reserved=0
        for v in [100.0f64, 0.75, -0.1, 0.0, 0.0, 0.0] {
            compact.extend_from_slice(&v.to_le_bytes());
        }
        for v in [-482.5f64, -1.92, -1250.25, 1.5, 0.0, 2.25, 0.0, 1.0, 0.0] {
            compact.extend_from_slice(&v.to_le_bytes());
        }
        assert_eq!(compact.len(), 124);
        let mut swbf2_payload = Vec::new();
        swbf2_payload.extend_from_slice(&swbf2_body::REVISION.to_le_bytes());
        swbf2_payload.extend_from_slice(&compact);
        swbf2_payload.extend_from_slice(&100.0f64.to_le_bytes()); // energy
        swbf2_payload.push(0); // moves = idle

        // Own-player envelope header (schema 11).
        let mut envelope = Vec::new();
        envelope.extend_from_slice(&1u32.to_le_bytes()); // epoch
        envelope.extend_from_slice(&1u32.to_le_bytes()); // content_epoch
        envelope.extend_from_slice(&1u16.to_le_bytes()); // class_id
        envelope.push(1); // team
        envelope.push(swbf2_body::SCHEMA);
        envelope.extend_from_slice(&0u32.to_le_bytes()); // vehicle_id
        envelope.extend_from_slice(&0u32.to_le_bytes()); // vehicle_generation
        envelope.extend_from_slice(&(swbf2_payload.len() as u16).to_le_bytes());
        envelope.extend_from_slice(&swbf2_payload);
        envelope.extend_from_slice(&100.0f64.to_le_bytes()); // health
        envelope.push(0); // dead
        envelope.extend_from_slice(&0.0f64.to_le_bytes()); // death_time
        envelope.extend_from_slice(&0.0f64.to_le_bytes()); // unit_dead_time
        envelope.push(0); // removed
        for _ in 0..3 {
            envelope.extend_from_slice(&0.0f64.to_le_bytes()); // kick
        }
        envelope.push(0); // zoom_step
        envelope.extend_from_slice(&1.0f64.to_le_bytes()); // zoom
        envelope.push(0); // first
        envelope.extend_from_slice(&[0u8, 0]); // selected

        let mut corr = Writer::default();
        own_correction_into(&mut corr, 91., 2., 11., &envelope, None).unwrap();
        let mut own = Writer::default();
        own_datagram_into(
            &mut own,
            OwnParts {
                epoch: 1.,
                tick: 60.,
                interval: 1.,
                baseline: 0.,
                ack: 1.,
                second: false,
                count: 1.,
                correction: Some(&corr.0),
                summaries: None,
                cosmetics: None,
            },
        )
        .unwrap();
        receiver.message(false, &own.0, 1000.);
        assert_eq!(receiver.fault, None);
        assert!(receiver.own.alive);
        assert!(
            (receiver.own.position[0] - (-482.5)).abs() < 0.01
                && (receiver.own.position[2] - (-1250.25)).abs() < 0.01,
            "local 2005 receiver must decode swbf2 soldier position without a network plan; got {:?}",
            receiver.own.position
        );
    }

    // The own correction's compact soldier prefix is the released one in both games: a flag byte above the
    // two released bits is refused whole, and the receiver keeps the pose it had.
    #[test]
    fn an_own_correction_with_a_flag_bit_above_the_released_two_is_refused_in_both_games() {
        use bfe_family::GameFamily;
        use bfe_sim::{
            combat::defs::WeaponDefs,
            gen::{Posture, Soldier, Unit, UnitBody, WireField, WireOwn, WireStoreName, WireStoreSetup},
            store::Strings,
            wire::{codec::own_datagram, store_capture::Capture as Maps, store_own::{Capture, Facts}},
        };
        let mut strings = Strings::new();
        let class = strings.intern_str("synthetic_hero");
        let setup = WireStoreSetup {
            item_capacity: 1, remote_capacity: 1, entity_capacity: 4, record_capacity: 4, seat_capacity: 1,
            classes: vec![WireStoreName { name: class, id: 1 }], ..Default::default()
        };
        let maps = Maps::new(&setup, &strings).unwrap();
        let defs = WeaponDefs::default();
        let mut cap = Capture::new(&defs);
        let facts = Facts::default();
        let mut packet = |tick: f64, position: [f64; 3], bit: u8| -> Vec<u8> {
            let body = UnitBody::Soldier(Soldier {
                position, settled: position, normal: [0., 1., 0.], posture: Posture::Stand, grounded: true, ..Default::default()
            });
            let schema = cap.foot(&body).unwrap();
            cap.body.value[0] |= bit;
            let mut unit = Unit { class_name: class, team: 1., alive: true, generation: 1., soldier: body, ..Default::default() };
            unit.combat.health = 100.;
            cap.envelope(&unit, schema, (0., 0.), 1., 1., &maps, &defs, &facts, None).unwrap();
            own_datagram(&WireOwn {
                epoch: 1., tick, interval: 1., baseline_tick: 0., ack_input: tick, own_parts: 1.,
                correction: Some(WireField { id: 7., generation: 1., schema: 11., payload: cap.payload.value.clone() }),
                ..Default::default()
            })
            .unwrap()
        };
        for family in [GameFamily::Swbf1, GameFamily::Swbf2] {
            let mut receiver = Receiver::new(serde_json::Value::Null);
            receiver.registry = true;
            receiver.welcomed = true;
            receiver.control_family = family;
            receiver.message(false, &packet(1., [1., 2., 3.], 0), 16.0);
            assert_eq!(receiver.own.position, [1., 2., 3.], "{family:?}: the plain body is taken");
            for bit in [4u8, 8, 0x80] {
                receiver.message(false, &packet(2., [9., 8., 7.], bit), 32.0);
                assert_eq!(receiver.own.position, [1., 2., 3.], "{family:?}: flag bit {bit:#x} is refused");
            }
        }
    }
}
