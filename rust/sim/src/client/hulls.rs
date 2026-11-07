//! Private hull prediction: predictVehicle and predictionVehicleInput, with
//! every pending tick inside the page's single prediction-session boundary.
//! F2 supplies immutable construction data. Mutable solver rows, body flags,
//! command state and continuation history belong exclusively to this owner.

use crate::prediction::corrections;
use super::{axis, span, append_record, BACKWARD, FORWARD, LEFT, RIGHT, TURN_LEFT, TURN_RIGHT, INVALID};
use crate::collision::State;
use crate::flight::{Environment, Flight, FlightWorld, FlightClass, Member, Team, Tuning};
use crate::flight::config::Tunings;
use crate::flight::model::FlightInput;
use crate::flight::store::FlightTextIds;
use crate::gen::{ClientCommand, ClientFrame, ClientHull, ClientHullState, ClientHullSeat, ClientHullMember,
    ClientLook, ClientResult, ClientFlightCommand, ClientFlightHeld as Held, ClientFlightAction as Action, OrdnanceOwner, Vehicle, VehicleWheel};
use crate::seats::boarding::{LookAccess, LookField, SeatControl, seat_look_with};
use crate::store::Strings;
use crate::vehicles::{BodyId, Ground, Hit, Hull, Input, Polygon, SpringPolygons, VehicleWorld};
use crate::vehicles::native::{Collider, Collision, MapRules};
use three::{Matrix4, Quaternion, Vector3};

// predictVehicle retained 60 earlier poses plus every tick of a fresh replay.
// A 60-slot ring would lose usable corrections from a longer pending block.
const HISTORY: usize = 60;
const GROUND_BYTES: usize = 224;
const GROUND_SCHEMA: u32 = 10;
const FULL_FLIGHT_SCHEMA: u32 = 2;
const COMPACT_FLIGHT_SCHEMA: u32 = 7;
const STEP: f64 = 1.0 / 60.0;
const SUBSTEP: f64 = 1.0 / 120.0;
struct Checkpoint { tick: f64, position: [f64; 3], bytes: [u8; GROUND_BYTES] }

// This owner has battle lifetime but no authoritative lease. A page reconnect
// invalidates its identity without replacing immutable class/geometry handles.
// All mutable rows below are private, even when the canonical hull is borrowed
// during cold construction. Collision bodies themselves remain world-owned.
pub struct Session {
    hull: Hull,
    row: Vehicle,
    strings: Strings,
    texts: FlightTextIds,
    collision: Collision,
    map: MapRules,
    environment: Environment,
    tunings: Tunings,
    members: Vec<Member>,
    teams: Vec<Team>,
    history: Vec<Checkpoint>,
    life: Option<[f64; 4]>,
    authority: f64,
    tick: f64,
    sequence: f64,
    held: u32,
    from_walk: [f64; 2],
    checkpoint_flight_held: u32,
    checkpoint: Vec<u8>,
    incoming: Vec<u8>,
    flight_correction: corrections::FlightState,
}
impl Session {
    /// predictionVehicle's clone is cold. Subsequent frames retain the hull,
    /// decoded row, strings and collision scratch rather than cloning them.
    pub fn new(hull: Hull, row: Vehicle, mut strings: Strings, collision: Collision,
        map: MapRules, environment: Environment, tunings: Tunings) -> Self {
        let texts = FlightTextIds::new(&mut strings);
        Self { hull, row, strings, texts, collision, map, environment, tunings,
            members: Vec::new(), teams: Vec::new(), history: Vec::with_capacity(super::LIMIT as usize + HISTORY + 1),
            life: None, authority: 0.0, tick: 0.0, sequence: 0.0, held: 0, from_walk: [0.0; 2], checkpoint_flight_held: 0, checkpoint: Vec::with_capacity(1024), incoming: Vec::with_capacity(1024),
            flight_correction: corrections::FlightState { numbers: Vec::with_capacity(37), vectors: Vec::with_capacity(7),
                quaternions: Vec::with_capacity(6), ..Default::default() } }
    }
    pub(super) fn bind_native_collider(&mut self,body:u32,collider:Collider){self.collision.add_collider(body,collider);}
    pub(super) fn prepare_native_collision(&mut self,state:&mut State){self.collision.prepare(state);self.members.reserve(512);self.teams.reserve(8);self.hull.target_scratch.reserve(512);if let Some(legs)=&self.hull.legs{self.hull.leg_side_scratch.reserve(legs.pairs.len()*2);}}
    /// The native host lends its generated command/correction bank once per
    /// frame. Publish this private row's prerequisites into that same bank;
    /// no class geometry or authoritative store is needed at the boundary.
    pub(super) fn native_frame(&self,words:&mut Vec<f64>,schema:u32,level:f64)->Result<u32,&'static str>{
        self.publish(words,ClientHull{slot:self.hull.id.slot,generation:self.hull.id.gen,seats_count:self.hull.seats.len()as u32,wheels_count:self.row.wheels.len()as u32,correction_schema:schema,level,assists:true,..Default::default()})
    }
    // updateFlightTarget used to write through the clone's shared weapon list.
    // Return its private lock as a value for the same session's weapon replay.
    pub(super) fn flight_lock(&self) -> Option<crate::flight::state::Lock> { self.hull.flight.lock }
    pub fn reset(&mut self) { self.life = None; }

    /// Native hosts can save the same exact checkpoint the page receives.
    /// Borrow its retained bytes; only an explicit caller copy owns new memory.
    pub fn checkpoint(&mut self) -> Result<&[u8], &'static str> {
        self.store(); self.capture_checkpoint()?; Ok(&self.checkpoint)
    }

    /// PredictionWindow.select and predictedExactly: compare normalized ground
    /// bytes, never approximate poses. Rounded flyer corrections always rewind.
    pub(super) fn predict(&mut self, f: &ClientFrame, words: &[f64], commands:&[f64], state: &mut State,
        result: &mut ClientResult, output: &mut Vec<f64>) -> Result<(), &'static str> {
        let h = ClientHull::take(span(words, f.hull_at, 1, ClientHull::WORDS)?, &[]);
        if h.slot != self.hull.id.slot || h.generation != self.hull.id.gen { return Err(INVALID); }
        let restored = ClientHullState::take(span(words, h.state_at, 1, ClientHullState::WORDS)?, &[]);
        let seats = span(words, h.seats_at, h.seats_count, ClientHullSeat::WORDS)?;
        let wheels = span(words, h.wheels_at, h.wheels_count, VehicleWheel::WORDS)?;
        let legs = span(words, h.legs_at, h.legs_count, 1)?;
        if h.seats_count as usize != self.hull.seats.len() || h.wheels_count as usize != self.hull.wheels.len() { return Err(INVALID); }
        let flags = span(words, f.flags_at, f.flags_count, 1)?;
        // Body registration is a frame fact, not a query callback. The private
        // view still owns its flags and never moves a registered shared body.
        #[cfg(target_arch = "wasm32")]
        if let Some(source) = crate::vehicles::module::collision_world() { self.collision.prediction_names(source); }
        if state.worlds.get(self.collision.world).map(|w| w.order.len()) != Some(flags.len())
            || !self.collision.prediction_flags(flags) { return Err(INVALID); }
        self.collision.prepare(state);
        // Flight targeting reads a snapshot of remote facts, just as the old
        // page did. No lazy unit aim callback is allowed during pending ticks.
        // Registry order breaks equal target scores. Copy into retained slots
        // without sorting, deduplicating, or retaining a JS registry object.
        self.members.clear();
        for row in span(words, h.members_at, h.members_count, ClientHullMember::WORDS)?.chunks_exact(ClientHullMember::WORDS) {
            let m = ClientHullMember::take(row, &[]);
            self.members.push(Member { id: m.id, generation: m.generation, alive: m.alive, team: m.team,
                own: m.own, unit: m.unit, body: (m.body >= 0.0).then_some(m.body as u32), point: Vector3::new(m.point[0], m.point[1], m.point[2]) });
        }
        // Nullable teams are legitimate keys. The supplied square relation
        // table preserves battle rules without a per-target eligibility call.
        self.teams.clear();
        for &team in span(words, h.teams_at, h.teams_count, 1)? { self.teams.push((!team.is_nan()).then_some(team)); }
        let relations = span(words, h.relations_at, h.teams_count.checked_mul(h.teams_count).ok_or(INVALID)?, 1)?;
        self.hull.target_scratch.reserve(self.members.len());
        self.map.level = h.level; self.environment.assists = h.assists;
        // The source generation distinguishes a respawned hull at a reused
        // slab slot. A unit life and reconnect epoch must also start afresh.
        let life = [f.epoch, f.identity, f.generation, f.source];
        let same = self.life == Some(life);
        let mut continued = same && (f.authority_tick == self.authority || f.authority_tick < self.tick - super::LIMIT);
        // Loading the incoming row is independent of the working hull. Exact
        // acceptance can inspect authority without destroying a continuation.
        restored.write(&mut self.row);
        // The wire bytes own solver state. The generated row supplies class
        // prerequisites and inactive presentation state, never a JS replay.
        let incoming_seat = self.restore_correction(f, &h, words)?;
        if !continued && same && !self.hull.flying && f.authority_tick > self.authority && f.authority_tick <= self.tick {
            let mut bytes = [0; GROUND_BYTES];
            ground_bytes(&self.row, seats, true, &mut bytes)?;
            continued = self.history.iter().any(|c| c.tick == f.authority_tick && c.bytes == bytes);
        }
        // Ground equality admits the previous continuation, preserving wheel
        // and gait state that the compact authority body deliberately omits.
        // Flyers retain the old conservative rewind on a new authority tick.
        let after = if continued { self.sequence } else { 0.0 };
        if !continued {
            self.hull.load(&self.row);
            if self.hull.flying {
                // Flight::from_store is a fixed stack value; class and gear
                // handles are retained, and all message texts were interned.
                self.hull.flight = Flight::from_store(&self.row, &self.strings,
                    self.hull.flight.config, self.hull.flight.gear_clip.clone()).map_err(|_| INVALID)?;
            }
            // Motion wire formats omit local visual cursors. These initial
            // wheel/gait facts come from the corrected facade on a rewind.
            for (out, row) in self.hull.wheels.iter_mut().zip(wheels.chunks_exact(VehicleWheel::WORDS)) {
                let w = VehicleWheel::take(row, &[]); out.phase = w.phase; out.previous = w.previous; out.tick_from = w.tick_from;
            }
            // None means first contact has not happened. Preserve that state
            // while keeping a spare backing buffer for the next initialization.
            if h.has_legs {
                if self.hull.leg_side.is_none() { self.hull.leg_side = Some(std::mem::take(&mut self.hull.leg_side_scratch)); }
                let sides = self.hull.leg_side.as_mut().unwrap(); sides.clear(); sides.extend_from_slice(legs);
            }
            else if let Some(mut sides) = self.hull.leg_side.take() {
                // predictVehicle restores absence before first contact. Keep
                // capacity, but discard old values so resize initializes zeros.
                sides.clear(); self.hull.leg_side_scratch = sides;
            }
            let pose = self.hull.pose.as_mut().ok_or(INVALID)?;
            pose.walk_phase = h.walk_phase; pose.walk_blend = h.walk_blend;
            self.from_walk = [h.walk_from_phase, h.walk_from_blend];
            self.history.clear(); self.tick = f.authority_tick;
            self.sequence = 0.0; self.held = f.held;
        }
        self.life = Some(life); self.authority = f.authority_tick;
        // The clone always had a local driver and no leases. Crew aim arrives
        // from authority; replay must never step another occupant's controls.
        self.hull.driver = true; self.hull.seat_index = 0;
        // Unsent commands still carry actions and advance the sequence.
        // Their held keys and mouse samples remain excluded by the replay.
        let mut last = self.tick; let mut mine = false;
        for row in commands.chunks_exact(ClientCommand::WORDS) {
            let c = ClientCommand::take(row, &[]);
            if !c.tick.is_finite() || c.tick.fract() != 0.0 { return Err(INVALID); }
            span(words, c.look_at, c.look_count, ClientLook::WORDS)?;
            if c.sequence > after && c.generation == f.generation { mine = true; last = last.max(c.tick); }
        }
        // predictionSeatLook starts from corrected seat aim. Pose yaw history
        // belongs to authority and is normalized only for equality checking.
        if mine || !continued {
            for (i, (seat, row)) in self.hull.seats.iter_mut().zip(seats.chunks_exact(ClientHullSeat::WORDS)).enumerate() {
                let s = if i == 0 { incoming_seat.clone() } else { ClientHullSeat::take(row, &[]) };
                if !continued || i != 0 { seat.aim_yaw = s.aim_yaw; seat.pitch = s.pitch; seat.look_yaw = s.look_yaw; seat.look_pitch = s.look_pitch; }
                seat.lease = None;
                let p = &mut self.hull.pose.as_mut().unwrap().seats[i]; p.yaw_rate = s.yaw_rate; p.last_yaw = s.last_yaw;
            }
            if mine && last == self.tick { last += 1.0; }
        }
        if mine {
            // The old clone trimmed prior history at its starting tick, then
            // retained the entire new replay. Reserve before entering ticks;
            // ordinary bounded streams fit the construction-time capacity.
            let from = self.tick;
            self.history.retain(|c| c.tick > from - HISTORY as f64);
            self.history.reserve((last - from) as usize);
        }
        // Each replay starts with default view settings, including on a
        // continuation. This is the old replayTicks initial-view quirk.
        let mut cursor = 0; let mut tick = self.tick + 1.0;
        let mut mouse = 1.0; let mut invert = false;
        let mut flight_held = if continued { self.checkpoint_flight_held } else { f.flight_held }; let mut pad_axes = [0.0; 4]; let mut pad_buttons = 0; let mut flight_scale = 1.0; let mut flight_invert = false; let mut curve = f64::NAN;
        // replayTicks combines all late commands into the first eligible tick.
        // Its final view scales every look sample in that combined tick.
        while mine && tick <= last {
            let start = cursor; let mut actions = 0;
            while cursor < commands.len() / ClientCommand::WORDS {
                let c = ClientCommand::take(&commands[cursor * ClientCommand::WORDS..], &[]);
                if c.sequence <= after { cursor += 1; continue; }
                if c.tick > tick { break; } cursor += 1;
                if c.generation != f.generation { continue; }
                if self.hull.flying {
                    let flight = ClientFlightCommand::take(span(words, c.flight_at, 1, ClientFlightCommand::WORDS)?, &[]);
                    actions |= if f.merge_commands {c.flight_actions}else{flight.actions};
                    if c.sent { flight_held = flight.held; flight_scale = flight.scale; flight_invert = flight.invert;
                        curve = flight.curve; pad_axes = flight.pad_axes; pad_buttons = flight.pad_buttons; }
                }
                if c.sent {
                    self.held = c.held; mouse = if c.has_mouse_scale {c.mouse_scale}else{mouse}; invert = c.invert.unwrap_or(invert);
                }
                self.sequence = self.sequence.max(c.sequence);
            }
            let scales = crate::seats::network::look_scales(mouse, invert, flight_scale, flight_invert);
            for row in commands[start * ClientCommand::WORDS..cursor * ClientCommand::WORDS].chunks_exact(ClientCommand::WORDS) {
                let c = ClientCommand::take(row, &[]);
                if c.sequence <= after || c.generation != f.generation || !c.sent { continue; }
                for row in span(words, c.look_at, c.look_count, ClientLook::WORDS)?.chunks_exact(ClientLook::WORDS) {
                    let look = ClientLook::take(row, &[]);
                    seat_look_with(&mut Look(&mut self.hull), look.x, look.y, &scales).unwrap();
                }
            }
            if !self.hull.flying {
                let s = &mut self.hull.seats[0]; s.aim_yaw = s.look_yaw.unwrap_or(s.aim_yaw); s.pitch = s.look_pitch.unwrap_or(s.pitch);
                self.hull.clamp_aim(0);
            }
            let mut world = World { collision: &mut self.collision, state, map: &self.map, environment: &self.environment,
                tunings: self.tunings, members: &self.members, teams: &self.teams, relations, body: self.hull.body };
            self.hull.capture_tick(); self.hull.capture_flight_tick();
            if self.hull.flying {
                let has = |key: Held| flight_held & (1 << key as u8) != 0;
                let take = |key: Action| f64::from(actions & (1 << key as u8) != 0);
                let dead = |x: f64| if v8math::abs(x) > 0.12 { x } else { 0.0 };
                let forward = f64::from(has(Held::Throttle)) - f64::from(has(Held::Brake));
                let right = f64::from(has(Held::RollRight)) - f64::from(has(Held::RollLeft));
                let stick = dead(pad_axes[0]) != 0.0 || dead(pad_axes[1]) != 0.0;
                let input = FlightInput { forward: if forward != 0.0 { forward } else { -dead(pad_axes[3]) },
                    right: if right != 0.0 { right } else { dead(pad_axes[2]) },
                    stick_x: stick.then_some(dead(pad_axes[0])), stick_y: stick.then_some(-dead(pad_axes[1])), boost: f64::from(has(Held::Boost) || pad_buttons & 2 != 0),
                    roll_lock: f64::from(has(Held::RollLock)), takeoff: take(Action::Takeoff), trick: f64::from(take(Action::Trick) != 0.0 || pad_buttons & 1 != 0),
                    immelmann: take(Action::Immelmann), target_lock: take(Action::TargetLock), response_curve: curve, no_assist: true,
                    ..FlightInput::default() };
                self.hull.flyer_tick(&mut world, STEP, &input, None);
            } else {
                let input = Input { forward: axis(self.held, FORWARD, BACKWARD), right: axis(self.held, RIGHT, LEFT),
                    turn: axis(self.held, TURN_RIGHT, TURN_LEFT), brake: self.held & (1 << crate::gen::ClientHeld::Space as u8) != 0 };
                self.hull.tick(&mut world, SUBSTEP, &input); self.hull.tick(&mut world, SUBSTEP, &input);
            }
            let pose = self.hull.pose.as_ref().unwrap(); self.from_walk = [pose.walk_phase, pose.walk_blend];
            self.hull.walk(STEP); self.store(); self.tick = tick;
            let mut bytes = [0; GROUND_BYTES];
            if !self.hull.flying { ground_hull_bytes(&self.row, &self.hull, true, &mut bytes)?; }
            self.history.push(Checkpoint { tick, position: self.row.position, bytes });
            result.ticks += 1; tick += 1.0;
        }
        // An empty stream still returns its restored pose, but creates no
        // artificial held-key ticks. Rolled-back commands above run once.
        // PredictionWindow.save kept the last command's down keys even when
        // that command was unsent. Keep this checkpoint quirk separate from
        // the sent-only held state used while the current replay advances.
        for row in commands.chunks_exact(ClientCommand::WORDS) {
            let c = ClientCommand::take(row, &[]);
            if c.sequence > after { self.held = c.held; self.sequence = c.sequence; if self.hull.flying { self.checkpoint_flight_held = ClientFlightCommand::take(span(words, c.flight_at, 1, ClientFlightCommand::WORDS)?, &[]).default_held; } }
        }
        // Save native state before serializing. The returned pose, checkpoint
        // and smoothing comparison all describe the same completed replay.
        self.store(); result.tick = self.tick; result.continued = continued;
        result.sequence = self.sequence; result.held = self.held;
        // A rewind rebuilt history on the corrected baseline. Its old-tick
        // point is what CorrectionSmoother needs to avoid counting new motion.
        if !continued {
            if let Some(c) = self.history.iter().find(|c| c.tick == h.check_tick) {
                result.check_found = true; result.check_position = c.position;
            }
        }
        result.hull_at = self.publish(output, h)?;
        result.hull_words = output.len() as u32 - result.hull_at;
        // The exact checkpoint is also exposed for cross-build correction
        // gates. Normal presentation reads the retained output without copying.
        self.capture_checkpoint()?;
        result.correction_at = output.len() as u32; result.correction_bytes = self.checkpoint.len() as u32;
        for chunk in self.checkpoint.chunks(4) {
            let mut bytes = [0; 4]; bytes[..chunk.len()].copy_from_slice(chunk);
            output.push(u32::from_le_bytes(bytes) as f64);
        }
        Ok(())
    }
    // restoreGroundCorrection/restoreFlightCorrection previously decoded on
    // the page before replay. Decode into retained native storage instead.
    // Only validated wire state reaches the working hull; no decoder vectors
    // are created for the pending ticks or for a warm authority update.
    fn restore_correction(&mut self, frame: &ClientFrame, h: &ClientHull, words: &[f64]) -> Result<ClientHullSeat, &'static str> {
        self.incoming.clear();
        // Frame blocks pack four existing little-endian wire bytes per word.
        // Reject nonintegral values before restoring any working hull state.
        for &word in span(words, frame.correction_at, frame.correction_bytes.div_ceil(4), 1)? {
            if !word.is_finite() || word < 0.0 || word > u32::MAX as f64 || word.fract() != 0.0 { return Err(INVALID); }
            for byte in (word as u32).to_le_bytes() { if self.incoming.len() < frame.correction_bytes as usize { self.incoming.push(byte); } }
        }
        if !self.hull.flying && h.correction_schema == GROUND_SCHEMA {
            let c = corrections::decode_ground(Some(&self.incoming), true, [true; 2])?;
            super::hull_correction::ground(&mut self.row, &c);
            return Ok(super::hull_correction::seat(&c.seat));
        }
        if !self.hull.flying { return Err(INVALID); }
        match h.correction_schema {
            FULL_FLIGHT_SCHEMA => corrections::decode_flight_into(Some(&self.incoming), FULL_FLIGHT_SCHEMA as f64,
                true, [true; 13], true, &mut self.flight_correction)?,
            COMPACT_FLIGHT_SCHEMA => corrections::decode_compact_flight_into(Some(&self.incoming), true,
                [true; 9], [true; 4], &mut self.flight_correction)?,
            _ => return Err(INVALID),
        }
        super::hull_correction::flight(&mut self.row, &self.flight_correction, &self.texts);
        Ok(super::hull_correction::seat(&self.flight_correction.seat))
    }
    // captureGroundCorrection and captureFlightCorrection retain their existing
    // wire order and nullable seat tags. Never quantize a continuation checkpoint.
    fn capture_checkpoint(&mut self) -> Result<(), &'static str> {
        self.checkpoint.clear();
        if !self.hull.flying {
            let mut bytes = [0; GROUND_BYTES];
            ground_hull_bytes(&self.row, &self.hull, false, &mut bytes)?;
            self.checkpoint.extend_from_slice(&bytes); return Ok(());
        }
        let v = &self.row; let f = &self.hull.flight; let out = &mut self.checkpoint;
        // Flight's full checkpoint includes interpolation endpoints, even
        // though the compact authority format derives some of them on restore.
        for n in [v.health, v.team.unwrap_or(f64::NAN), v.yaw, v.previous_yaw, v.tilt, v.previous_tilt,
            v.roll, v.previous_roll, v.yaw_rate, v.forward_input, v.right_input, v.altitude, v.throttle,
            v.airspeed, v.tilt_rate, v.state_time] { super::number(out, n)?; }
        if !v.floor_y.is_finite() && !v.floor_y.is_nan() { return Err(INVALID); }
        out.extend(v.floor_y.to_le_bytes());
        for n in [v.bank, f.power, f.stick_x, f.stick_y, f.roll_rate, f.trick_time, f.trick_cooldown,
            f.trick_pending, f.trick_direction, f.gear, f.landing_message_time, f.clock, f.assist_id,
            f.lock_id, f.lock_generation, f.lock_timer, f.lost_los, v.tick_from_yaw.ok_or(INVALID)?,
            v.tick_from_tilt.ok_or(INVALID)?, v.tick_from_roll.ok_or(INVALID)?] { super::number(out, n)?; }
        for vector in [v.position, v.previous, v.velocity, v.tick_from.ok_or(INVALID)?,
            [f.start.x, f.start.y, f.start.z], [f.end.x, f.end.y, f.end.z], [f.trick_right.x, f.trick_right.y, f.trick_right.z]] {
            for n in vector { super::number(out, n)?; }
        }
        for q in [self.hull.quaternion, f.previous_q, f.tick_q, f.start_q, f.trick_loop_q, f.trick_end_q] {
            for n in [q.x, q.y, q.z, q.w] { super::number(out, n)?; }
        }
        // Before a craft's first tick, the three action latches are undefined.
        // Their tag 2 is distinct from a released false key in the checkpoint.
        for value in [Some(v.alive), v.sleeping, Some(v.grounded), Some(f.roll_lock), Some(f.boosting),
            Some(f.boost_exhausted), Some(f.terrain_warning), f.action_held, f.trick_held, f.immelmann_held] {
            out.push(value.map_or(2, u8::from));
        }
        out.extend([f.state.ok_or(INVALID)? as u8, f.trick as u8, f.lock_state as u8,
            f.warning as u8, f.landing_message as u8, f.landing_reason as u8]);
        let s = &self.hull.seats[0]; let p = &self.hull.pose.as_ref().unwrap().seats[0];
        for value in [Some(s.aim_yaw), Some(s.pitch), s.look_yaw, s.look_pitch, Some(p.yaw_rate), p.last_yaw] {
            out.push(u8::from(value.is_none())); super::number(out, value.unwrap_or(0.0))?;
        }
        Ok(())
    }
    // Hull::to_store is a private row projection here. It never publishes to
    // the fleet slab, whose simultaneous world step owns a separate lifetime.
    fn store(&mut self) {
        self.hull.to_store(&mut self.row, &mut self.strings);
        self.hull.flight.to_store_cached(&mut self.row, &self.texts);
    }
    // The generated flat row is a presentation/correction mirror. Static
    // class geometry never makes another trip across the frame boundary.
    fn publish(&self, out: &mut Vec<f64>, mut h: ClientHull) -> Result<u32, &'static str> {
        // Fixed generated records carry solver values; variable ranges carry
        // only the configured seat/wheel/support counts, never class geometry.
        let at = out.len(); out.resize(at + ClientHull::WORDS, 0.0);
        h.state_at = append_record(out, ClientHullState::WORDS, &ClientHullState::read(&self.row), ClientHullState::put);
        h.seats_at = out.len() as u32;
        for (i, s) in self.hull.seats.iter().enumerate() {
            let p = &self.hull.pose.as_ref().unwrap().seats[i];
            append_record(out, ClientHullSeat::WORDS, &ClientHullSeat { aim_yaw: s.aim_yaw, pitch: s.pitch,
                look_yaw: s.look_yaw, look_pitch: s.look_pitch, yaw_rate: p.yaw_rate, last_yaw: p.last_yaw }, ClientHullSeat::put);
        }
        // Keep the clone's interpolation cursors. Resetting them on every
        // authority packet would make wheels and walking feet visibly jump.
        h.wheels_at = out.len() as u32;
        for w in &self.row.wheels { append_record(out, VehicleWheel::WORDS, w, VehicleWheel::put); }
        h.legs_at = out.len() as u32;
        h.has_legs = self.hull.leg_side.is_some(); h.legs_count = self.hull.leg_side.as_ref().map_or(0, |s| s.len()) as u32;
        if let Some(sides) = &self.hull.leg_side { out.extend_from_slice(sides); }
        let p = self.hull.pose.as_ref().unwrap();
        h.walk_phase = p.walk_phase; h.walk_blend = p.walk_blend;
        h.walk_from_phase = self.from_walk[0]; h.walk_from_blend = self.from_walk[1];
        h.put(&mut out[at..], &mut Vec::new()); Ok(at as u32)
    }
}

// predictionSeatLook uses the existing native seat-look algorithm. This
// access shim changes only the private working hull, never a store command.
struct Look<'a>(&'a mut Hull);
impl LookAccess for Look<'_> {
    type Error = std::convert::Infallible;
    fn control(&mut self) -> Result<Option<SeatControl>, Self::Error> { Ok(Some(if self.0.flying { SeatControl::Flight } else { SeatControl::Drive })) }
    fn look_read(&mut self, f: LookField) -> Result<f64, Self::Error> {
        Ok(match f { LookField::StickX => self.0.flight.stick_x, LookField::StickY => self.0.flight.stick_y,
            LookField::Yaw => self.0.seats[0].look_yaw.unwrap_or(self.0.seats[0].aim_yaw), LookField::Pitch => self.0.seats[0].look_pitch.unwrap_or(self.0.seats[0].pitch) })
    }
    fn look_write(&mut self, f: LookField, v: f64) -> Result<(), Self::Error> {
        match f { LookField::StickX => self.0.flight.stick_x = v, LookField::StickY => self.0.flight.stick_y = v,
            LookField::Yaw => self.0.seats[0].look_yaw = Some(v), LookField::Pitch => self.0.seats[0].look_pitch = Some(v) }; Ok(())
    }
    fn clamp(&mut self) -> Result<(), Self::Error> { self.0.clamp_aim(0); Ok(()) }
}

// hullState ignores pilot yaw-rate history: world aimSeats advances it, but
// replay does not. Preserve its exact nullable tags and little-endian doubles.
fn ground_bytes(v: &Vehicle, seats: &[f64], normalized: bool, out: &mut [u8; GROUND_BYTES]) -> Result<(), &'static str> {
    let s = ClientHullSeat::take(seats, &[]);
    encode_ground(v, &s, normalized, out)
}
fn ground_hull_bytes(v: &Vehicle, hull: &Hull, normalized: bool, out: &mut [u8; GROUND_BYTES]) -> Result<(), &'static str> {
    let s = &hull.seats[0]; let p = &hull.pose.as_ref().unwrap().seats[0];
    encode_ground(v, &ClientHullSeat { aim_yaw: s.aim_yaw, pitch: s.pitch, look_yaw: s.look_yaw,
        look_pitch: s.look_pitch, yaw_rate: p.yaw_rate, last_yaw: p.last_yaw }, normalized, out)
}
fn encode_ground(v: &Vehicle, s: &ClientHullSeat, normalized: bool, out: &mut [u8; GROUND_BYTES]) -> Result<(), &'static str> {
    let mut at = 0;
    for n in [v.health, v.team.unwrap_or(f64::NAN), v.yaw, v.tilt, v.roll, v.yaw_rate, v.forward_input,
        v.right_input, v.tilt_rate, v.roll_rate, v.support].into_iter().chain(v.position).chain(v.velocity).chain(v.quaternion) {
        if !n.is_finite() { return Err(INVALID); } out[at..at + 8].copy_from_slice(&n.to_le_bytes()); at += 8;
    }
    out[at] = u8::from(v.alive); out[at + 1] = u8::from(v.sleeping.ok_or(INVALID)?); at += 2;
    for n in [Some(s.aim_yaw), Some(s.pitch), s.look_yaw, s.look_pitch,
        Some(if normalized { 0.0 } else { s.yaw_rate }), if normalized { None } else { s.last_yaw }] {
        out[at] = u8::from(n.is_none()); at += 1;
        out[at..at + 8].copy_from_slice(&n.unwrap_or(0.0).to_le_bytes()); at += 8;
    }
    Ok(())
}

// predictionCollision and VehiclesWorld's private damage override. The only
// mutation of shared state is the kernel's reusable query scratch/cache; body
// transforms, enabled flags, damage, effects and lease lifecycle stay private.
struct World<'a> {
    collision: &'a mut Collision, state: &'a mut State, map: &'a MapRules,
    environment: &'a Environment, tunings: Tunings, members: &'a [Member], teams: &'a [Team], relations: &'a [f64], body: BodyId,
}
impl VehicleWorld for World<'_> {
    fn damage_allowed(&self) -> bool { false }
    fn cast(&mut self, o: &Vector3, d: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> { self.collision.cast(self.state, o, d, far, ignore)? }
    fn push_walls(&mut self, _: usize, p: &[Vector3], r: f64, q: &Quaternion, own: Option<&Quaternion>, previous: &Vector3, position: &mut Vector3, velocity: &mut Vector3, terrain: bool) -> bool {
        let Some(undo) = self.collision.push_walls(self.state, self.body, p, r, q, own, previous, position, velocity, terrain) else { return false };
        undo
    }
    fn ground(&mut self, x: f64, z: f64, top: f64) -> Option<Ground> { self.collision.ground(self.state, x, z, top) }
    // hoverContacts borrows the collision gather until its next query, just
    // as the authority does. Prediction keeps that scratch in its own view.
    fn spring_polygons<'a>(&'a mut self, _: usize, index: usize, center: &Vector3, radius: f64, _out: &'a mut Vec<Polygon>) -> SpringPolygons<'a> {
        let Some(polygons) = self.collision.spring_polygons(self.state, self.body, index, center, radius) else { return SpringPolygons::Native(&[]) };
        SpringPolygons::Native(polygons)
    }
    // syncBody updates the private hull matrix for its own equations. Its
    // collider must stay at authority for other actors and later page frames.
    fn move_body(&mut self, _: BodyId, _: &Matrix4) {}
    fn enable_body(&mut self, body: BodyId, enabled: bool) { let Some(()) = self.collision.enable_body(self.state, body, enabled) else { return }; }
    fn body_enabled(&mut self, body: BodyId) -> bool { let Some(enabled) = self.collision.body_enabled(self.state, body) else { return false }; enabled }
    fn water_over(&mut self, x: f64, z: f64) -> bool { self.map.water.as_ref().is_some_and(|w| w.over(x, z)) }
    fn water_level(&mut self) -> f64 { self.map.level }
    fn has_water(&mut self) -> bool { self.map.water.is_some() }
    fn in_death_region(&mut self, p: &Vector3) -> bool { self.map.in_death_region(p) }
    // Private prediction never owns damage, destruction, spawn or effects.
    // Those arrive as authoritative snapshots and ordered presentation events.
    fn damaged(&mut self, _: usize, _: f64, _: &OrdnanceOwner) {}
    fn destroyed_crew(&mut self, _: usize) {}
    fn destroyed(&mut self, _: usize) {}
    fn explosion(&mut self, _: usize, _: bool, _: &Vector3) {}
    fn effect(&mut self, _: &str, _: &Vector3) {}
    // RaycastUnits is a pose reader, not used by private movement or targeting.
    fn raycast_units(&mut self, _: &Vector3, _: &Vector3, _: f64) -> Option<f64> { unreachable!("prediction does not pose crew") }
    fn clock(&mut self) -> f64 { 0.0 }
    fn tow_released(&mut self, _: usize, _: &str) { unreachable!("prediction does not execute cables") }
}
impl FlightWorld for World<'_> {
    fn environment(&mut self) -> &Environment { self.environment }
    fn tuning(&mut self, c: FlightClass) -> Tuning { self.tunings.of(c) }
    fn member_count(&mut self) -> usize { self.members.len() }
    fn member(&mut self, i: usize) -> Member { self.members[i] }
    // Preserve numeric/null team equivalence, including the registry's NaN
    // key behavior. No map/string lookup occurs while replay selects targets.
    fn relation(&mut self, a: Team, b: Team) -> f64 {
        let same = |x: &Team, y: Team| *x == y || x.is_some_and(|x| x.is_nan()) && y.is_some_and(|y| y.is_nan());
        let i = self.teams.iter().position(|t| same(t, a)).expect("prediction team");
        let j = self.teams.iter().position(|t| same(t, b)).expect("prediction team"); self.relations[i * self.teams.len() + j]
    }
    fn aim_point(&mut self, i: usize) -> Vector3 { self.members[i].point }
    fn body_is_hull(&mut self, body: BodyId) -> bool { self.members.iter().any(|m| m.body == Some(body)) }
    fn system_death_region(&mut self, p: &Vector3) -> bool { self.in_death_region(p) }
}

#[cfg(test)]
mod robustness_tests {
    use super::*;

    #[test]
    fn prediction_queries_skip_refused_bodies() {
        let mut collision = Collision::new(0, None);
        let mut state = State::default();
        let map = MapRules::from_json(&bfe_content::json::parse("{}").unwrap(), -100.0);
        let environment = Environment::default();
        let mut world = World { collision: &mut collision, state: &mut state, map: &map,
            environment: &environment, tunings: Tunings::default(), members: &[], teams: &[], relations: &[], body: u32::MAX };
        let mut position = Vector3::new(1.0, 2.0, 3.0);
        let mut velocity = Vector3::new(4.0, 5.0, 6.0);
        assert!(world.cast(&position, &velocity, 1.0, Some(u32::MAX)).is_none());
        assert!(!world.push_walls(0, &[], 1.0, &Quaternion::default(), None,
            &Vector3::default(), &mut position, &mut velocity, false));
        assert_eq!(position, Vector3::new(1.0, 2.0, 3.0));
        assert_eq!(velocity, Vector3::new(4.0, 5.0, 6.0));
        assert!(matches!(world.spring_polygons(0, 0, &position, 1.0, &mut Vec::new()), SpringPolygons::Native(p) if p.is_empty()));
        world.enable_body(u32::MAX, true);
        assert!(!world.body_enabled(u32::MAX));
    }
}
