//! The desktop Host drives this local battle at 60 Hz. Its authoritative
//! Context publishes into the same session leases as an online client; the
//! existing private replay owner restores the latest own checkpoint with an
//! empty pending-command tail, rather than simulating another local world.
use super::runtime::{Engine, Runtime};
use bfe_client::{generated_input::{ClientCommand, ClientLook, ClientFlightCommand}, host::World, input::ViewActions,
    online::{input::Sender, replay::Replay}, scene::{MapCamera, Scene, View}};
use bfe_presentation::{aim::View as AimView, cameras::math::V8};
use three::Vector3;

pub struct Battle<E: Engine> {
    pub runtime: Runtime<E>,
    pub camera: MapCamera,
    replay: Replay,
    sender: Sender,
    shown: f64,
    dt: f64,
    pub frames: u64,
}

impl<E: Engine> Battle<E> {
    // Scene::load_online is resource admission, independent of transport.
    // Take its one replay owner once; every later frame borrows the same
    // generated banks. The sender supplies only its empty native history.
    pub fn new(runtime: Runtime<E>, scene: &mut Scene, camera: MapCamera) -> Result<Self, String> {
        let resources = scene.online.as_mut().ok_or("missing native battle presentation resources")?;
        let replay = resources.replay.take().ok_or("native replay resources already admitted")?;
        let sender = Sender::new(&resources.turns);
        let shown = runtime.tick();
        Ok(Self { runtime, camera, replay, sender, shown, dt: 0., frames: 0 })
    }

    // Scene.present_network consumes the retained Session, not a socket.
    // Keep this whole-frame boundary identical to direct joins: body/hull
    // publication, seat camera, HUD and audio observe the same completed tick.
    pub fn draw(&mut self, scene: &mut Scene) -> Result<(), String> {
        if let Some(error) = self.runtime.receiver.error() { return Err(error); }
        scene.present_network(&self.runtime.receiver, &self.replay, &self.sender.history, &mut self.camera, self.dt);
        scene.draw(self.camera.snapshot())?;
        // A catch-up display can contain several authoritative publications.
        // Keep all their event records until the one whole-scene presentation
        // has consumed them, then retire the frame journal together.
        self.runtime.receiver.publications.clear();
        Ok(())
    }
}

impl<E: Engine> World for Battle<E> {
    type Frame = View;
    fn generation(&self) -> u32 { self.runtime.receiver.own.generation as u32 }
    fn step(&mut self, command: &ClientCommand, look: &ClientLook, flight: &ClientFlightCommand, view: &ViewActions, _: f64) -> Result<(), String> {
        let mut command = command.clone();
        // Scene's existing native lens is first person. Supply its actual
        // aspect to the authority rather than a desktop input default, so
        // aim/camera facts agree with the view this frame will draw.
        command.aspect = self.camera.aspect(); command.has_aspect = true; command.first_person = Some(true);
        self.runtime.step(&command, look, flight, view)
    }

    // Client::present uses AimView's canonical eye before the shared camera
    // selector applies the foot or mounted lens. Preserve that ownership here:
    // this host chooses no posture offset, turret pose or camera algorithm.
    fn present(&mut self, alpha: f64) -> &View {
        let tick = self.runtime.tick() + alpha;
        self.dt = ((tick - self.shown) / 60.).clamp(0., 8. / 60.);
        self.shown = tick;
        let receiver = &mut self.runtime.receiver;
        if let Err(error) = receiver.clock.advance(self.dt) { receiver.fault = Some(error); }
        if let Err(error) = self.replay.frame(receiver, &self.sender) { receiver.fault = Some(error); }
        let mut own = receiver.own;
        if let Some(hull) = &self.replay.fleet.state { own.position = hull.position; }
        if self.replay.fleet.identity.is_some() { own.yaw = self.replay.fleet.aim[0]; own.pitch = self.replay.fleet.aim[1]; }
        if let Some(body) = &self.replay.body {
            own.position = body.position; own.yaw = body.aim_yaw; own.pitch = body.aim_pitch;
            own.posture = body.posture as usize; own.air = body.air.map_or(0, |air| air as u8 + 1);
        }
        if receiver.corrections != 0 {
            let position = Vector3::new(own.position[0], own.position[1], own.position[2]);
            let aim = AimView { position, feet: position, yaw: own.yaw, pitch: own.pitch,
                posture: own.posture, air: own.air != 0, zoom: own.zoom, ..Default::default() }.sample::<V8>();
            self.camera.follow([aim.eye.x, aim.eye.y, aim.eye.z], own.yaw, own.pitch, self.dt);
        }
        self.frames += 1;
        self.camera.snapshot()
    }
}
