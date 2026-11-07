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
use bfe_transport::{connection::Lane, endpoint::Endpoint};
use std::time::Duration;
pub struct Client {
    pub receiver: Receiver,
    pub sender: Sender,
    pub replay: super::replay::Replay,
    pub endpoint: Endpoint,
    pub camera: MapCamera,
    dt: f64,
    started: bool,
    authenticated: bool,
    connecting_at: f64,
    pub frames: u64,
}
impl Client {
    // Discovery, identity and DNS finish before the window event loop starts.
    // Handshake retries belong to receive(), so close and paint remain live.
    pub fn connect(endpoint: Endpoint, resources: &mut Replicas, camera: MapCamera) -> Result<Self, String> {
        let receiver = Receiver::new(resources.descriptor.clone());
        let replay = resources.replay.take().ok_or("native prediction resources already admitted")?;
        let connecting_at = endpoint.now();
        Ok(Self {
            receiver,
            replay,
            sender: Sender::new(&resources.turns),
            endpoint,
            camera,
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
        let receiver = &mut self.receiver;
        self.endpoint
            .pump(&mut |lane, text, bytes, now| {
                if text || lane == Lane::State {
                    receiver.message(text, bytes, now);
                }
            })
            .map_err(|e| self.endpoint.failure(e))?;
        if !self.authenticated && self.endpoint.ready() {
            info::authenticated(&self.endpoint.server_hello)?;
            self.authenticated = true;
        }
        if !self.authenticated && self.endpoint.now() - self.connecting_at >= Duration::from_secs(30).as_millis() as f64 {
            return Err("battle connection timed out".into());
        }
        if let Some(error) = receiver.error() {
            return Err(error);
        }
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
    // A queued input and its durable edges share one transport flush.
    // The sender advances delivery accounting only after queue admission.
    pub fn send(&mut self) -> Result<(), String> {
        self.sender.spawn(&self.receiver);
        self.sender.send(&mut self.endpoint, &mut self.receiver)?;
        self.endpoint.flush().map_err(|e| e.to_string())
    }
    // Timed diagnostic output is cold: formatting never enters a frame.
    pub fn summary(&self) -> serde_json::Value {
        let own = self.receiver.own;
        serde_json::json!({"frames":self.frames,"identity":self.endpoint.identity.iter().map(|b|format!("{b:02x}")).collect::<String>(),"welcomed":self.receiver.welcomed,"playerId":own.id,"generation":own.generation,"alive":own.alive,"position":own.position,"packets":self.receiver.packets,"corrections":self.receiver.corrections,"sent":self.sender.sent,"fireEdges":self.sender.fire_edges,"refused":self.receiver.refused+self.sender.refused})
    }
}
impl World for Client {
    type Frame = View;
    fn generation(&self) -> u32 {
        self.receiver.own.generation as u32
    }
    fn step(&mut self, c: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand, _: &ViewActions, _: f64) -> Result<(), String> {
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
        if let Err(error) = self.replay.frame(&self.receiver, &self.sender) {
            self.receiver.fault = Some(error);
        }
        let mut own = self.receiver.own;
        if let Some(hull) = &self.replay.fleet.state {
            own.position = hull.position;
        }
        if self.replay.fleet.identity.is_some() {
            self.sender.yaw = self.replay.fleet.aim[0];
            self.sender.pitch = self.replay.fleet.aim[1];
        }
        if let Some(body) = &self.replay.body {
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
