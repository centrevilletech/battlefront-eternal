//! One native frame receives packets, consumes the input batch and publishes
//! an own camera. No authoritative World or Node host is created by a join.
use super::{info, input::Sender, receiver::Receiver, replicas::Replicas};
use crate::{
    generated_input::{ClientCommand, ClientFlightCommand, ClientLook},
    host::World,
    input::ViewActions,
    scene::{MapCamera, View},
};
use bfe_presentation::{aim::View as AimView, cameras::math::V8};
use bfe_transport::{connection::Lane, endpoint::Endpoint, layout::close_code};
use std::time::Duration;
/// How long after the next battle was due the client waits for it before giving up.
const NEXT_BATTLE_PATIENCE_MS: f64 = 60_000.;
/// Whether the announced next battle (due at `ready_at`) is so late that the server is gone.
fn next_battle_overdue(now: f64, ready_at: f64) -> bool {
    now > ready_at + NEXT_BATTLE_PATIENCE_MS
}
#[cfg(test)]
#[test]
fn an_end_table_waits_a_minute_for_the_next_battle_then_gives_up() {
    assert!(!next_battle_overdue(10_000., 10_000.));
    assert!(!next_battle_overdue(70_000., 10_000.));
    assert!(next_battle_overdue(70_001., 10_000.));
}
mod bfe_server_reason {
    /// The refusal a server sends for a 2005 match that ended while joining.
    pub const MATCH_CHANGED: &str = "match-changed";
}
/// Whether the server closed `endpoint` refusing the join for its version,
/// or because the 2005 match the join named ended: a battle join and a
/// sit-out join then both look for the server's current battle before they
/// give up.
pub(super) fn refused_for_another_battle(endpoint: &Endpoint) -> bool {
    endpoint.connection.as_ref().and_then(|c| c.closed).is_some_and(|c| c.code == close_code::VERSION as u64
        || c.code == close_code::REJECTED as u64 && c.reason() == bfe_server_reason::MATCH_CHANGED.as_bytes())
}
pub struct Client {
    pub receiver: Box<Receiver>,
    pub sender: Box<Sender>,
    pub replay: Box<super::replay::Replay>,
    pub endpoint: Box<Endpoint>,
    pub camera: Box<MapCamera>,
    rotation_probe: Option<super::rotation::Probe>,
    pub rotation_ready: bool,
    // The announced battle's server runs another version: the end of this join.
    rotation_mismatch: Option<String>,
    // The `HOST:PORT` that answered /info, and the battle query loaded from it.
    target: String,
    query: String,
    // Refused for content before the welcome: did the server change battle meanwhile?
    rejoin: Option<(super::rotation::Probe, f64, String)>,
    dt: f64,
    started: bool,
    authenticated: bool,
    connecting_at: f64,
    pub frames: u64,
    /// A 2005 join: the match asked for, the match joined once its signed
    /// answer checked out, and the messages that arrived before that check.
    swbf2: Option<Swbf2Join>,
    /// Whether prediction took the 2005 match's flight ceiling from its welcome.
    ceiling_applied: bool,
    /// Whether the server's signed answer gave this 2005 join the sit-out
    /// role: nothing more is read here, and the map-free session takes the
    /// connection (`hand_over`).
    sat_out: bool,
}
struct Swbf2Join {
    expected: super::match2005::Expected,
    joined: Option<super::match2005::Joined>,
    held: super::match2005::Held,
}
impl Client {
    // Discovery, identity and DNS finish before the window event loop starts.
    // Handshake retries belong to receive(), so close and paint remain live.
    pub fn connect(endpoint: Box<Endpoint>, resources: &mut Replicas, camera: MapCamera, target: String, query: String) -> Result<Self, String> {
        let mut receiver = Box::new(Receiver::with_bounds(resources.descriptor.clone(), resources.wire_bounds));
        receiver.clock = super::clock::Clock::online();
        let replay = Box::new(resources.replay.take().ok_or("native prediction resources already admitted")?);
        if let Some(space) = &replay.space {
            let meta = space.meta.clone();
            receiver.space = Some(crate::local::space::History::new(meta.counts));
            receiver.space_online = Some(Box::new(super::space_receiver::OnlineSpace::new(
                meta.clone(),
                resources.wire_bounds,
            )));
            if let Some(fleet) = &mut resources.space_fleet {
                fleet.reserve(meta.counts.fighters);
            }
            if let Some(effects) = &mut resources.space_effects {
                effects.reserve_fleet(meta.counts.fighters);
            }
        }
        let connecting_at = endpoint.now();
        Ok(Self {
            receiver,
            replay,
            sender: Box::new({let mut sender=Sender::new(&resources.turns);sender.set_swbf2_models(resources.swbf2_turn_models.clone());sender.aim_scene=Some(resources.shot_aim.clone());sender}),
            endpoint,
            camera: Box::new(camera),
            rotation_probe: None,
            rotation_mismatch: None,
            rotation_ready: false,
            target,
            query,
            rejoin: None,
            dt: 0.,
            started: false,
            authenticated: false,
            connecting_at,
            frames: 0,
            swbf2: None,
            ceiling_applied: false,
            sat_out: false,
        })
    }
    /// Makes this join a 2005 match's: nothing the server sends is read
    /// until its signed answer names that match and a record table this
    /// build can read.
    pub fn expect_match(&mut self, expected: super::match2005::Expected) {
        self.swbf2 = Some(Swbf2Join { expected, joined: None, held: Default::default() });
    }
    /// The 2005 match this client joined, once checked.
    pub fn joined_match(&self) -> Option<&super::match2005::Joined> { self.swbf2.as_ref().and_then(|j| j.joined.as_ref()) }
    /// Whether the server's signed answer gave this join the sit-out role
    /// (its verified content is not the content this client loaded).
    pub fn sits_out(&self) -> bool { self.sat_out }
    /// The map-free session of a join the server gave the sit-out role: this
    /// connection, the messages held while its signed answer was checked (read
    /// there under the same check), the notice that the battle's game data is
    /// not this client's, and the next battle to follow. This client keeps no
    /// connection afterwards.
    pub fn hand_over(&mut self) -> Option<super::sitout::SitOut> {
        if !self.sat_out { return None; }
        let join = self.swbf2.take()?;
        let endpoint = std::mem::replace(&mut self.endpoint, Box::new(Endpoint::detached()));
        let client = super::lobby_client::LobbyClient::resume(endpoint, join.expected, join.held);
        Some(super::sitout::SitOut::new(client, self.target.clone(), self.query.clone(), super::sitout::Why::OtherData))
    }
    // Packet receipts use the endpoint monotonic clock, not display time.
    // Poll even when focus pauses generated input, so the lease stays alive.
    pub fn receive(&mut self, dt: f64) -> Result<(), String> {
        if self.sat_out { return Ok(()); }
        self.dt = dt.clamp(0., 8. / 60.);
        let receiver = &mut *self.receiver;
        let mut overflow = false;
        let receipt = match self.swbf2.as_mut().filter(|join| join.joined.is_none()) {
            // A 2005 join holds what arrives until the signed answer is checked.
            Some(join) => self.endpoint.pump(&mut |lane, text, bytes, now| {
                if (text || lane == Lane::State) && !join.held.hold(text, bytes, now) { overflow = true; }
            }),
            None => self.endpoint.pump(&mut |lane, text, bytes, now| {
                if text || lane == Lane::State {
                    receiver.message(text, bytes, now);
                }
            }),
        }
        .map_err(|e| self.endpoint.failure(e));
        if overflow { return Err("the server sent a Battlefront II match's messages before its signed answer".into()); }
        // While a rejoin is pending its probe decides; the signed answer that
        // started it is not read again.
        if let Some(join) = self.swbf2.as_mut().filter(|join| join.joined.is_none() && self.endpoint.ready() && self.rejoin.is_none()) {
            let joined = match super::match2005::accept(&self.endpoint.server_hello, &join.expected) {
                Ok(joined) => joined,
                // The match ended while joining: look for the server's current battle.
                Err(super::match2005::Refusal::Changed) => {
                    let probe = super::rotation::Probe::changed(self.target.clone(), self.query.clone());
                    self.rejoin = Some((probe, self.endpoint.now(), super::match2005::CHANGED.into()));
                    return Ok(());
                }
                Err(refusal) => return Err(refusal.text()),
            };
            // A client with the map plays only the match whose verified
            // content it loaded. Otherwise the server signed the sit-out role:
            // the battle is sat out in the map-free session, which takes this
            // connection and what it held, unread (`hand_over`).
            if joined.role != super::match2005::Role::Battle {
                self.sat_out = true;
                return Ok(());
            }
            // The predictor needs every soldier class's movement numbers, read
            // from this client's copy of the content the server admitted by the
            // same rule; content that cannot give one cannot take part.
            if let Some((class, why)) = &self.replay.swbf2_refused {
                return Err(super::match2005::class_refusal(class, why));
            }
            // The sender writes the input revision the plan selects; this build
            // writes revision 1, the 2005 tables.
            let inputs = match joined.plan.of(bfe_sim::wire::revisions::INPUT) {
                bfe_sim::wire::revisions::Use::Revision(1) => crate::input_family::WireInputs::new(bfe_family::GameFamily::Swbf2),
                _ => return Err(super::match2005::Refusal::Malformed.text()),
            };
            // Every 2005 record is checked against the match's plan and the
            // classes' movement numbers before it is stored or acknowledged.
            self.receiver.network.borrow_mut().check_records(Box::new(bfe_sim::wire::revisions::Checks::new(
                joined.plan.clone(), self.replay.swbf2_classes.clone())));
            self.sender.set_inputs(inputs);
            self.receiver.swbf2_plan = Some(joined.plan.clone());
            join.joined = Some(joined);
            self.authenticated = true;
            for (text, bytes, now) in join.held.drain() { self.receiver.message(text, &bytes, now); }
        }
        // The handshake starts only once the battle has loaded. A server that changed battle
        // in the meantime refuses the old battle's content: follow it instead of failing.
        // A 2005 match that ended while joining is refused as changed; both
        // look for the server's current battle the same way.
        let refused = refused_for_another_battle(&self.endpoint);
        if refused && !self.receiver.welcomed && self.rejoin.is_none() {
            if let Err(error) = &receipt {
                // The refusal speaks of different game data; that is only so if /info still names
                // the battle this client loaded (below), so it is not logged here.
                eprintln!("online join: refused for its battle; checking whether the server changed battle");
                let probe = super::rotation::Probe::changed(self.target.clone(), self.query.clone());
                self.rejoin = Some((probe, self.endpoint.now(), error.clone()));
            }
        }
        if let Some((probe, since, error)) = &self.rejoin {
            // Another version on the server is final, whatever the refusal said.
            if let Some(text) = probe.mismatch() { return Err(text); }
            match probe.answer() {
                Some(true) => {
                    eprintln!("online join: the server changed battle while this one loaded; joining the new one");
                    self.rotation_ready = true;
                }
                Some(false) => return Err(error.clone()),
                None if self.endpoint.now() - since > 60_000. => return Err(error.clone()),
                None => return Ok(()),
            }
        }

        if self.rotation_ready { return Ok(()); }
        if let Some(text) = &self.rotation_mismatch { return Err(text.clone()); }
        // After a battle the old connection may close: the next battle is awaited instead. Not
        // for ever: when it has not answered a minute after it was due, the server is gone.
        if let Some(next) = &self.receiver.next_match {
            if next_battle_overdue(self.endpoint.now(), next.ready_at) {
                return Err("battle connection timed out: the server did not start the next battle".into());
            }
        }
        if self.receiver.next_match.is_none() { receipt?; }
        let receiver = &mut *self.receiver;
        if !self.authenticated && self.endpoint.ready() {
            info::authenticated(&self.endpoint.server_hello)?;
            self.authenticated = true;
        }
        if !self.authenticated && self.endpoint.now() - self.connecting_at >= Duration::from_secs(30).as_millis() as f64 {
            return Err(self.endpoint.waiting_reason().unwrap_or_else(|| "battle connection timed out".into()));
        }
        if let Some(error) = receiver.error() {
            return Err(error);
        }
        // A server closes the old transport while loading its next world.
        // Preserve the end table and wait for the readiness probe in that gap.
        if receiver.next_match.is_some() { return Ok(()); }
        while let Some(bytes) = receiver.pong() {
            if !self.endpoint.send(Lane::Events, true, bytes).map_err(|e| self.endpoint.failure(e))? {
                break;
            }
            receiver.pong_sent();
        }
        // A 2005 match's welcome names the epoch's flight ceiling: prediction
        // takes it before any predicted tick.
        if let Some(ceiling) = receiver.flight_ceiling.filter(|_| !self.ceiling_applied) {
            self.replay.set_flight_ceiling(ceiling);
            self.ceiling_applied = true;
        }
        // NetworkSession calls start after welcome; authentication alone does
        // not subscribe a client to the server's snapshot publication owner.
        if receiver.welcomed && !self.started {
            self.started = self
                .endpoint
                .send(Lane::Events, true, br#"{"type":"call","id":1,"method":"start","args":{}}"#)
                .map_err(|e| e.to_string())?;
        }
        self.sender.feedback(receiver);
        receiver.clock.advance(self.dt).map_err(str::to_string)?;
        Ok(())
    }
    // Reliable chat is echoed only by the authority, as NetworkSession.chat.
    pub fn chat(&mut self, text: &str, team: bool) -> Result<(), String> {
        let bytes = super::chat::encode(text, team);
        if self
            .endpoint
            .send(Lane::Events, true, &bytes)
            .map_err(|e| e.to_string())?
        {
            Ok(())
        } else {
            Err("Chat could not be sent: server is not connected".into())
        }
    }
    // A queued input and its durable edges share one transport flush.
    // The sender advances delivery accounting only after queue admission.
    pub fn send(&mut self) -> Result<(), String> {
        if self.receiver.next_match.is_some() { return Ok(()); }
        self.sender.spawn(&self.receiver);
        self.sender.send(&mut self.endpoint, &mut self.receiver)?;
        self.endpoint.flush().map_err(|e| e.to_string())
    }
    // NetworkSession.scheduleRotation waits out the advertised delay before
    // probing. Starting one worker is a cold match-boundary operation.
    pub fn poll_rotation(&mut self) {
        if self.rotation_ready { return; }
        if let Some(next) = &self.receiver.next_match {
            if self.rotation_probe.is_none() && self.endpoint.now() >= next.ready_at {
                self.rotation_probe = Some(super::rotation::Probe::start(self.target.clone(), next.query.clone()));
            }
        }
        if self.rotation_probe.as_ref().is_some_and(|p| p.ready()) { self.rotation_ready = true; }
        if let Some(text) = self.rotation_probe.as_ref().and_then(|p| p.mismatch()) { self.rotation_mismatch = Some(text); }
    }
    // Timed diagnostic output is cold: formatting never enters a frame.
    pub fn summary(&self) -> serde_json::Value {
        let own = self.receiver.own;
        serde_json::json!({"frames":self.frames,"identity":self.endpoint.identity.iter().map(|b|format!("{b:02x}")).collect::<String>(),"welcomed":self.receiver.welcomed,"playerId":own.id,"generation":own.generation,"alive":own.alive,"position":own.position,"team":own.team,"yaw":own.yaw,"predictedYaw":self.sender.yaw,"packets":self.receiver.packets,"corrections":self.receiver.corrections,"sent":self.sender.sent,"fireEdges":self.sender.fire_edges,"refused":self.receiver.refused+self.sender.refused})
    }
}
impl World for Client {
    type Frame = View;
    fn generation(&self) -> u32 {
        self.receiver.own.generation as u32
    }
    fn step(&mut self, c: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand, actions: &ViewActions, _: f64) -> Result<(), String> {
        self.camera.set_flight_mode(self.receiver.uses_flight_controls());
        if actions.perspective { self.camera.first_person = !self.camera.first_person; }
        let mut command = c.clone();
        command.first_person = Some(self.camera.first_person);
        let c = &command;
        if !self.receiver.own.alive && c.actions & 1 != 0 {
            self.sender.spawned = false;
        }
        let rtt = self.endpoint.connection.as_ref().map_or(100., |c| c.rtt);
        self.sender.tick(c, look, flight, &self.receiver, rtt, self.endpoint.now());
        Ok(())
    }
    fn present(&mut self, alpha: f64) -> &View {
        self.frames += 1;
        self.sender.preview(&self.receiver);
        self.sender.ease_walker_turn(&self.receiver, self.dt);
        self.replay.frame(&self.receiver, &self.sender);
        self.replay.present(alpha, self.dt, &self.sender.history.loose, &self.receiver.own);
        crate::app::trace_own_aim(&self.receiver, &self.sender, &self.replay);
        let mut own = self.receiver.own;
        if let Some(hull) = &self.replay.fleet.state {
            let offset = self.replay.fleet.offset();
            own.position = std::array::from_fn(|i| hull.position[i] + offset[i]);
        }
        if self.replay.fleet.identity.is_some() {
            self.sender.yaw = self.replay.fleet.aim[0];
            self.sender.pitch = self.replay.fleet.aim[1];
        }
        if let Some(body) = &self.replay.displayed {
            own.position = body.position;
            own.posture = body.posture as usize;
            own.air = body.air.map_or(0, |a| a as u8 + 1);
            self.sender.yaw = body.aim_yaw;
            self.sender.pitch = body.aim_pitch;
        }
        if let Some(walker) = &self.replay.walker {
            own.position = walker.position;
        }
        self.replay.view = [self.sender.yaw, self.sender.pitch];
        // AimView supplies the canonical first-person eye for posture/air.
        // Preview replays only uncorrected pointer deltas on the checkpoint.
        if self.receiver.corrections != 0 {
            let position = three::Vector3::new(own.position[0], own.position[1], own.position[2]);
            let aim = AimView {
                position,
                feet: position,
                yaw: self.sender.yaw,
                pitch: self.sender.pitch,
                posture: own.posture,
                air: own.air != 0,
                zoom: own.zoom,
                ..Default::default()
            }
            .sample::<V8>();
            self.camera.follow([aim.eye.x, aim.eye.y, aim.eye.z], self.sender.yaw, self.sender.pitch, self.dt);
        }
        self.camera.present(alpha)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.endpoint.close(0, b"left battle");
        let _ = self.endpoint.flush();
    }
}
