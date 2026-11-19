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
pub struct Client {
    pub receiver: Box<Receiver>,
    pub sender: Box<Sender>,
    pub replay: Box<super::replay::Replay>,
    pub endpoint: Box<Endpoint>,
    pub camera: Box<MapCamera>,
    rotation_probe: Option<super::rotation::Probe>,
    pub rotation_ready: bool,
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
}
impl Client {
    // Discovery, identity and DNS finish before the window event loop starts.
    // Handshake retries belong to receive(), so close and paint remain live.
    pub fn connect(endpoint: Box<Endpoint>, resources: &mut Replicas, camera: MapCamera, target: String, query: String) -> Result<Self, String> {
        let mut receiver = Box::new(Receiver::new(resources.descriptor.clone()));
        receiver.clock = super::clock::Clock::online();
        let replay = Box::new(resources.replay.take().ok_or("native prediction resources already admitted")?);
        let connecting_at = endpoint.now();
        Ok(Self {
            receiver,
            replay,
            sender: Box::new({let mut sender=Sender::new(&resources.turns);sender.aim_scene=Some(resources.shot_aim.clone());sender}),
            endpoint,
            camera: Box::new(camera),
            rotation_probe: None,
            rotation_ready: false,
            target,
            query,
            rejoin: None,
            dt: 0.,
            started: false,
            authenticated: false,
            connecting_at,
            frames: 0,
        })
    }
    // Packet receipts use the endpoint monotonic clock, not display time.
    // Poll even when focus pauses generated input, so the lease stays alive.
    pub fn receive(&mut self, dt: f64) -> Result<(), String> {
        self.dt = dt.clamp(0., 8. / 60.);
        let receiver = &mut *self.receiver;
        let receipt = self.endpoint
            .pump(&mut |lane, text, bytes, now| {
                if text || lane == Lane::State {
                    receiver.message(text, bytes, now);
                }
            })
            .map_err(|e| self.endpoint.failure(e));
        // The handshake starts only once the battle has loaded. A server that changed battle
        // in the meantime refuses the old battle's content: follow it instead of failing.
        let refused = self.endpoint.connection.as_ref().and_then(|c| c.closed).is_some_and(|c| c.code == close_code::VERSION as u64);
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
        self.camera.set_flight_mode(self.receiver.in_aircraft());
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
        self.replay.frame(&self.receiver, &self.sender);
        self.replay.present(alpha, self.dt, &self.sender.history.loose, self.receiver.own.zoom);
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
