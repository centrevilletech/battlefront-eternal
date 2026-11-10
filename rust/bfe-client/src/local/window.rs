//! The local desktop window supplies clocks, focus and raw input only.
//! Context, presentation, GL resources and the audio mixer retain their
//! existing owners. Cold admission finishes while the window is hidden.
use super::{battle::Battle, runtime::Engine};
use bfe_client::{audio::AudioDevice, host::Host, scene::Scene};
use glutin::{config::ConfigTemplateBuilder, context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version},
    display::{GetGlDisplay, GlDisplay}, prelude::*, surface::{Surface, SwapInterval, WindowSurface}};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::{num::NonZeroU32, time::{Duration, Instant}};
use winit::{application::ApplicationHandler, dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop}, keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId}};

// Diagnostic observers bracket only the actual frame. They are Rust hosts,
// never callbacks from an entity loop; production's unit observer is inlined
// away. Installed-content tests use this boundary for allocation accounting.
pub trait Observer {
    fn begin(&mut self) {}
    fn input(&mut self, _input: &mut bfe_client::input::Input, _tick: f64) {}
    fn state(&mut self, _own: &bfe_client::online::receiver::Own, _scene: &Scene) {}
    fn end(&mut self, _tick: f64) -> Result<(), &'static str> { Ok(()) }
    fn complete(&self) -> bool { false }
}
impl Observer for () {}

pub struct Admitted<E: Engine> { pub scene: Box<Scene>, pub battle: Battle<E> }

// Audio drops before its producer and GL resources drop before the current
// context. This declaration order is intentional: the callback must be stopped
// before decoder teardown, and every GL destructor needs its creating context.
struct Running<E: Engine> {
    audio: Option<AudioDevice>,
    scene: Box<Scene>,
    host: Box<Host<Battle<E>>>,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    window: Window,
    last: Instant,
    start: Instant,
    next: Instant,
    refresh: Duration,
    focused: bool,
    captured: bool,
}

impl<E: Engine> Running<E> {
    fn new(el: &ActiveEventLoop, audio: bool,
        factory: impl FnOnce([i32; 2], &mut dyn FnMut() -> glow::Context) -> Result<Admitted<E>, String>) -> Result<Box<Self>, String> {
        let size = [1280u32, 720u32];
        // Match session_window's initial lens and render target. Resizing is
        // disabled because the existing native scene admits size-dependent
        // post-chain targets once, rather than reallocating them on a frame.
        let attrs = Window::default_attributes().with_title("Battlefront Eternal")
            .with_visible(false).with_resizable(false).with_inner_size(PhysicalSize::new(size[0], size[1]));
        let (window, config) = DisplayBuilder::new().with_window_attributes(Some(attrs)).build(el,
            ConfigTemplateBuilder::new().with_alpha_size(8).with_depth_size(24).with_stencil_size(8),
            |configs| configs.min_by_key(|c| c.num_samples()).expect("no GL configuration"))
            .map_err(|e| e.to_string())?;
        // The hidden window prevents an empty scene from being presented while
        // content and the Context's cold owners are still being admitted.
        let window = window.ok_or("window creation failed")?;
        let display = config.display();
        let raw = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let attrs = ContextAttributesBuilder::new().with_context_api(ContextApi::Gles(Some(Version::new(3, 0)))).build(Some(raw));
        // Native rendering uses the same GLES shader owner as direct joins.
        // A context creation failure ends this launch before world stepping.
        // SAFETY: the window handle belongs to this event-loop thread and
        // outlives both the surface and context stored below.
        let context = unsafe { display.create_context(&config, &attrs) }.map_err(|e| e.to_string())?;
        let attrs = window.build_surface_attributes(Default::default()).map_err(|e| e.to_string())?;
        // SAFETY: these surface attributes name the same live window/config.
        let surface = unsafe { display.create_window_surface(&config, &attrs) }.map_err(|e| e.to_string())?;
        let context = context.make_current(&surface).map_err(|e| e.to_string())?;
        surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap())).map_err(|e| e.to_string())?;
        // SAFETY: admission and every frame run while this context is current;
        // loaders resolve function pointers from the matching native display.
        let mut load = || unsafe { glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name) as *const _) };
        let Admitted { mut scene, battle } = factory([size[0] as i32, size[1] as i32], &mut load)?;
        // Admission transfers the existing sound owner's device output once.
        // The audio callback consumes its bounded ring; it never steps a world
        // or borrows the thread-affine Context from another device thread.
        let audio = if audio {
            let output = scene.online.as_mut().and_then(|r| r.sound.output.take()).ok_or("missing local battle audio output")?;
            Some(AudioDevice::open_live(output)?)
        } else { None };
        let hz = window.current_monitor().and_then(|m| m.refresh_rate_millihertz()).unwrap_or(60000).max(1000);
        // Display pacing follows this monitor, while Host retains its fixed
        // sixty-Hz authority clock. Monitor rate cannot change physics dt.
        let now = Instant::now();
        window.set_visible(true); window.focus_window();
        Ok(Box::new(Self { audio, scene, host: Box::new(Host::new(battle)), surface, context, window,
            last: now, start: now, next: now, refresh: Duration::from_secs_f64(1000. / hz as f64), focused: true, captured: false }))
    }

    fn capture(&mut self, on: bool) -> Result<(), String> {
        // Locked and confined are platform cursor mechanisms, both feeding
        // the same raw-motion input path. Releasing capture leaves keyboard
        // input active so Escape does not silently pause local authority.
        if on {
            self.window.set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined)).map_err(|e| e.to_string())?;
        } else { self.window.set_cursor_grab(CursorGrabMode::None).map_err(|e| e.to_string())?; }
        self.window.set_cursor_visible(!on); self.captured = on; Ok(())
    }

    fn frame(&mut self, observer: &mut impl Observer) -> Result<(), String> {
        if let Some(audio) = &self.audio { audio.check()?; }
        let now = Instant::now();
        let elapsed = if self.focused { now.duration_since(self.last).as_secs_f64() } else { 0. };
        self.last = now;
        observer.begin();
        // Bracket stepping and all presentation work together. Swap and event
        // dispatch belong to the OS driver, outside the steady-frame claim;
        // even the first measured gameplay frame must fit admitted capacities.
        let result: Result<(), String> = (|| {
            observer.input(&mut self.host.input, self.host.world.runtime.tick());
            self.host.frame(elapsed)?;
            self.host.world.draw(&mut self.scene)?;
            observer.state(&self.host.world.runtime.receiver.own, &self.scene);
            Ok(())
        })();
        let observed = observer.end(self.host.world.runtime.tick());
        // Always close the measurement window, including a failing frame.
        // Preserve its gameplay fault in preference to a diagnostic fault.
        result?; observed.map_err(str::to_string)?;
        self.window.pre_present_notify();
        self.surface.swap_buffers(&self.context).map_err(|e| e.to_string())?;
        self.next = (self.next + self.refresh).max(Instant::now());
        Ok(())
    }

    // Timed diagnostics run after the last frame and outside allocation
    // accounting. Pixel readback and JSON never participate in steady play.
    fn summary(&self) -> Result<serde_json::Value, String> {
        let local = &self.host.world.runtime;
        let resources = self.scene.online.as_ref().ok_or("missing local presentation")?;
        let presented: serde_json::Map<String, serde_json::Value> = bfe_client::online::replicas::presented::SHOWN
            .iter().zip(resources.presented.shown).map(|(name, count)| (name.to_string(), count.into())).collect();
        Ok(serde_json::json!({"ticks":local.ticks,"frames":self.host.world.frames,
            "generation":local.receiver.own.generation,"alive":local.receiver.own.alive,
            "position":local.receiver.own.position,"hulls":resources.hulls.visible,"items":resources.items.visible,
            "hudQuads":resources.hud.quads,"projectileQuads":resources.combat.total_quads,
            "effects":resources.combat.total_effects,"audioFrames":resources.sound.device_frames(),
            "audioPeak":resources.sound.peak(),"audioEvents":resources.sound.event_counts.to_vec(),
            "combatRecords":resources.combat.received.to_vec(),"presented":presented,
            "refused":local.refusals()+local.receiver.refused,
            "pixelHash":self.scene.hash()?}))
    }
}

struct App<E: Engine, F, O> {
    factory: Option<F>,
    running: Option<Box<Running<E>>>,
    observer: O,
    audio: bool,
    duration: Option<Duration>,
    error: Option<String>,
}
impl<E: Engine, F, O> App<E, F, O> {
    fn stop(&mut self, el: &ActiveEventLoop, error: Option<String>) {
        // Taking the running owner unifies explicit close, timed completion
        // and failures. Later event-loop callbacks cannot step a retired world.
        self.error = error; self.running.take(); el.exit();
    }
}
impl<E, F, O> ApplicationHandler for App<E, F, O>
where E: Engine, O: Observer,
    F: FnOnce([i32; 2], &mut dyn FnMut() -> glow::Context) -> Result<Admitted<E>, String> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.running.is_some() { return; }
        let Some(factory) = self.factory.take() else { self.stop(el, Some("local battle already closed".into())); return; };
        match Running::new(el, self.audio, factory) {
            Ok(running) => { running.window.request_redraw(); self.running = Some(running); }
            Err(error) => self.stop(el, Some(error)),
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.running else { return; };
        let result = match event {
            WindowEvent::CloseRequested => { self.stop(el, None); return; }
            WindowEvent::RedrawRequested => {
                if let Err(error) = r.frame(&mut self.observer) { self.stop(el, Some(error)); return; }
                if self.observer.complete() || self.duration.is_some_and(|duration| r.start.elapsed() >= duration) {
                    let summary = r.summary();
                    match summary { Ok(summary) => { println!("{summary}"); self.stop(el, None); }, Err(error) => self.stop(el, Some(error)) }
                }
                return;
            }
            WindowEvent::Focused(focused) => {
                // Offline time actually pauses. Clear both device state and
                // debt, so focus return cannot fast-forward an unseen battle.
                r.focused = focused; r.host.pause(); r.last = Instant::now();
                if focused { r.audio.as_ref().map_or(Ok(()), AudioDevice::play) }
                else { r.capture(false).and_then(|_| r.audio.as_ref().map_or(Ok(()), AudioDevice::pause)) }
            }
            WindowEvent::KeyboardInput { event, .. } if r.focused => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if key == KeyCode::Escape && event.state == ElementState::Pressed { r.capture(false) }
                    else { r.host.input.key(key, event.state == ElementState::Pressed); Ok(()) }
                } else { Ok(()) }
            }
            WindowEvent::MouseInput { state, button, .. } if r.focused => {
                // Submit the click that acquires capture too, matching the
                // native online window's press/release semantics.
                r.host.input.mouse(button, state == ElementState::Pressed);
                if state == ElementState::Pressed && !r.captured { r.capture(true) } else { Ok(()) }
            }
            WindowEvent::MouseWheel { delta, .. } if r.focused => {
                r.host.input.wheel(match delta { MouseScrollDelta::LineDelta(_, y) => y as f64, MouseScrollDelta::PixelDelta(p) => p.y }); Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = result { self.stop(el, Some(error)); }
    }
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let Some(r) = &mut self.running {
            if r.focused && r.captured { if let DeviceEvent::MouseMotion { delta } = event { r.host.input.motion(delta.0, delta.1); } }
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        // Sleep only until the next display deadline. Winit still wakes for
        // raw input and focus events; no busy timer or separate step thread
        // can race admission, rendering or Context disposal.
        if let Some(r) = &self.running {
            if Instant::now() >= r.next { r.window.request_redraw(); }
            else { el.set_control_flow(ControlFlow::WaitUntil(r.next)); }
        }
    }
}

pub fn run<E, F, O>(factory: F, audio: bool, duration: Option<Duration>, observer: O) -> Result<(), String>
where E: Engine, O: Observer,
    F: FnOnce([i32; 2], &mut dyn FnMut() -> glow::Context) -> Result<Admitted<E>, String> {
    run_on(EventLoop::new().map_err(|e| e.to_string())?, factory, audio, duration, observer)
}

// Installed-content hosts can create their event loop on the test thread.
// Ownership and frame execution remain identical to an ordinary local window.
pub fn run_on<E, F, O>(events: EventLoop<()>, factory: F, audio: bool, duration: Option<Duration>, observer: O) -> Result<(), String>
where E: Engine, O: Observer,
    F: FnOnce([i32; 2], &mut dyn FnMut() -> glow::Context) -> Result<Admitted<E>, String> {
    let mut app = App { factory: Some(factory), running: None, observer, audio, duration, error: None };
    events.run_app(&mut app).map_err(|e| e.to_string())?;
    app.error.map_or(Ok(()), Err)
}
