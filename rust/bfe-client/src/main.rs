//! Window/device shell. No simulation state or asset policy belongs to winit.
use bfe_client::{audio::AudioDevice, host::{CaptureWorld, Host}};
use bfe_native::{draw, load_scene, target::Target, Ranges};
use bfe_render::renderer::Renderer;
use glow::HasContext;
use glutin::{config::ConfigTemplateBuilder, context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version},
    display::{GetGlDisplay, GlDisplay}, prelude::*, surface::{Surface, SwapInterval, WindowSurface}};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::{num::NonZeroU32, path::PathBuf, time::{Duration, Instant}};
use winit::{application::ApplicationHandler, dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, ControlFlow}, keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId}};

type NativeRenderer = Renderer<glow::Context, Ranges>;
// Capture dimensions describe the render target, not logical desktop units.
// Keeping those dimensions fixed makes a resized window a pure output scale.
struct Options {
    capture: PathBuf,
    seconds: Option<Duration>,
    hash_frame: usize,
    width: u32,
    height: u32,
    samples: i32,
    pcm: Vec<f32>,
}
impl Options {
    // Options are cold admission work. Neither filenames nor JSON values
    // survive into the redraw or device callback paths.
    fn read() -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let capture = PathBuf::from(args.next().ok_or("usage: bfe-client CAPTURE_DIR [--seconds N] [--hash-frame N] [--pcm STEREO_F32LE]")?);
        // Older near captures omit metadata; retain the headless tool defaults
        // for those files, while current captures carry their actual layout.
        let meta: serde_json::Value = serde_json::from_slice(&std::fs::read(capture.join("calls.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut options = Self { capture, seconds: None, hash_frame: 0,
            width: meta["size"][0].as_u64().unwrap_or(1600).min(8192) as u32,
            height: meta["size"][1].as_u64().unwrap_or(900).min(8192) as u32,
            samples: meta["samples"].as_i64().unwrap_or(2).clamp(0, 16) as i32, pcm: Vec::new() };
        while let Some(arg) = args.next() {
            let value = args.next().ok_or("missing option value")?;
            match arg.as_str() {
                "--seconds" => {
                    let seconds: f64 = value.parse().map_err(|_| "invalid duration")?;
                    if !seconds.is_finite() || !(0.1..=86400.0).contains(&seconds) { return Err("invalid duration".into()); }
                    options.seconds = Some(Duration::from_secs_f64(seconds));
                }
                "--hash-frame" => options.hash_frame = value.parse().map_err(|_| "invalid frame index")?,
                // Decode only the transport representation here. The master
                // admits finite samples before starting the device thread.
                "--pcm" => {
                    let bytes = std::fs::read(value).map_err(|e| e.to_string())?;
                    if bytes.len() % 8 != 0 { return Err("PCM must contain complete stereo f32 frames at 48000 Hz".into()); }
                    options.pcm = bytes.chunks_exact(4).map(|b| f32::from_le_bytes(b.try_into().unwrap())).collect();
                }
                _ => return Err(format!("unknown option {arg}")),
            }
        }
        Ok(options)
    }
}

// Field order keeps renderer and attachments alive only while the current
// context and its window still exist. Shutdown releases GL before exiting.
struct Running {
    renderer: NativeRenderer,
    target: Target,
    host: Host<CaptureWorld>,
    audio: AudioDevice,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    window: Window,
    start: Instant,
    last: Instant,
    frames: u64,
    focused: bool,
    minimized: bool,
    captured: bool,
    next_frame: Instant,
    refresh: Duration,
}
impl Running {
    // Create the context on the event-loop thread. All renderer calls stay on
    // that thread; cpal owns a separate processor with no GL references.
    // GLES matches bfe-native and the source-owned renderer shader vocabulary.
    fn new(event_loop: &ActiveEventLoop, options: &mut Options) -> Result<Self, String> {
        let attrs = Window::default_attributes().with_title("Battlefront Eternal")
            .with_inner_size(PhysicalSize::new(options.width, options.height));
        let (window, config) = DisplayBuilder::new().with_window_attributes(Some(attrs))
            .build(event_loop, ConfigTemplateBuilder::new().with_alpha_size(8),
                |configs| configs.max_by_key(|c| c.num_samples()).expect("no GL window configuration"))
            .map_err(|e| e.to_string())?;
        let window = window.ok_or("window creation failed")?;
        // The raw window handle is borrowed only to construct the context and
        // surface. The retained Window outlives both native GL objects.
        let raw = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let display = config.display();
        let attributes = ContextAttributesBuilder::new().with_context_api(ContextApi::Gles(Some(Version::new(3, 0)))).build(Some(raw));
        let context = unsafe { display.create_context(&config, &attributes) }.map_err(|e| e.to_string())?;
        let attrs = window.build_surface_attributes(Default::default()).map_err(|e| e.to_string())?;
        let surface = unsafe { display.create_window_surface(&config, &attrs) }.map_err(|e| e.to_string())?;
        let context = context.make_current(&surface).map_err(|e| e.to_string())?;
        // Require a synchronized swap. A backend that cannot provide it fails
        // at startup instead of spinning an unbounded render loop.
        surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap())).map_err(|e| e.to_string())?;
        let load = |name: &std::ffi::CStr| display.get_proc_address(name) as *const _;
        let gl = unsafe { glow::Context::from_loader_function_cstr(load) };
        let multi = Ranges(unsafe { glow::Context::from_loader_function_cstr(load) });
        unsafe { eprintln!("GL: {} / {}; vsync=1", gl.get_parameter_string(glow::VERSION), gl.get_parameter_string(glow::RENDERER)); }
        // The window default framebuffer may differ in samples or colour
        // layout. Draw into the same explicit attachments as headless replay.
        let target = Target::new(&gl, options.width as i32, options.height as i32, options.samples)?;
        target.bind(&gl);
        let (renderer, frames) = load_scene(&options.capture, gl, multi)?;
        if options.hash_frame >= frames.len() { return Err("hash frame exceeds capture".into()); }
        let host = Host::new(CaptureWorld::new(frames)?);
        // Device construction follows scene admission. Failed GPU registration
        // must not leave a running audio stream behind a failed window.
        let audio = AudioDevice::open(std::mem::take(&mut options.pcm), 48000)?;
        let now = Instant::now();
        // Virtual displays may accept swap interval 1 without a vblank source.
        // A monitor deadline also bounds their redraw rate without busy-waiting.
        let hz = window.current_monitor().and_then(|m| m.refresh_rate_millihertz()).unwrap_or(60000).max(1000);
        let refresh = Duration::from_secs_f64(1000.0 / hz as f64);
        window.focus_window();
        Ok(Self { renderer, target, host, audio, surface, context, window,
            start: now, last: now, frames: 0, focused: true, minimized: false, captured: false, next_frame: now, refresh })
    }
    // X11 commonly supports confinement instead of locked cursor mode. Both
    // deliver raw DeviceEvent motion; failure is reported to the window host.
    fn grab(&mut self, grab: bool) -> Result<(), String> {
        if grab {
            self.window.set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined)).map_err(|e| e.to_string())?;
        } else { self.window.set_cursor_grab(CursorGrabMode::None).map_err(|e| e.to_string())?; }
        self.window.set_cursor_visible(!grab);
        self.captured = grab;
        Ok(())
    }
    // World step and presentation happen before renderer submission. The
    // borrowed frame cannot escape the call, so a future live owner can reuse
    // the same output bank on the next displayed frame.
    fn redraw(&mut self) -> Result<(), String> {
        self.audio.check()?;
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        // Zero-sized surfaces have no drawable framebuffer. Keep their World
        // paused until a nonzero resize requests another redraw.
        if self.minimized { self.host.pause(); return Ok(()); }
        self.target.bind(&self.renderer.core.gl);
        let frame = self.host.frame(if self.focused { elapsed } else { 0.0 })?;
        draw(&mut self.renderer, frame)?;
        let size = self.window.inner_size();
        self.target.show(&self.renderer.core.gl, size.width as i32, size.height as i32);
        // Resolve before scaling to satisfy GLES multisample blit rules. Swap
        // pacing then gives the event loop its next opportunity to collect input.
        self.window.pre_present_notify();
        self.surface.swap_buffers(&self.context).map_err(|e| e.to_string())?;
        self.frames += 1;
        // Real synchronized swaps usually consume the interval themselves. If
        // they do not, winit sleeps until this deadline before another redraw.
        self.next_frame = (self.next_frame + self.refresh).max(Instant::now());
        Ok(())
    }
    // Timed runs can stop on different animation frames at different refresh
    // rates. Re-render the requested source frame for a reproducible verdict.
    // Readback allocation occurs only here, after the measured window interval.
    fn finish(&mut self, index: usize) -> Result<(), String> {
        self.target.bind(&self.renderer.core.gl);
        draw(&mut self.renderer, self.host.world.frame(index).ok_or("missing hash frame")?)?;
        let hash = self.target.hash(&self.renderer.core.gl)?;
        println!("frame_hash {index} {hash}");
        let stats = self.renderer.core.stats;
        println!("window draw world {} instances {} GL calls {}", stats.world_draws, stats.instance_draws, stats.gl_calls);
        println!("window frames {} seconds {:.3} world_ticks {} audio_blocks {}", self.frames,
            self.start.elapsed().as_secs_f64(), self.host.tick,
            self.audio.status.blocks.load(std::sync::atomic::Ordering::Relaxed));
        self.audio.check()
    }
}
impl Drop for Running {
    fn drop(&mut self) { self.target.delete(&self.renderer.core.gl); }
}
struct App { options: Options, running: Option<Running>, error: Option<String> }
impl App {
    // Converge window close and device/GPU errors on one cleanup path while
    // the context is current. The event loop retains only the error message.
    fn stop(&mut self, event_loop: &ActiveEventLoop, result: Result<(), String>) {
        if let Err(error) = result { self.error = Some(error); }
        self.running.take();
        event_loop.exit();
    }
}
impl ApplicationHandler for App {
    // winit permits repeated resume notifications. Avoid a second device or
    // renderer when this desktop window already owns its current context.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() { return; }
        match Running::new(event_loop, &mut self.options) {
            Ok(running) => { running.window.request_redraw(); self.running = Some(running); }
            Err(error) => self.stop(event_loop, Err(error)),
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.running else { return; };
        let result = match event {
            WindowEvent::CloseRequested => {
                let result = r.finish(self.options.hash_frame); self.stop(event_loop, result); return;
            }
            WindowEvent::RedrawRequested => {
                let result = r.redraw();
                if result.is_ok() && self.options.seconds.is_some_and(|s| r.start.elapsed() >= s) {
                    let result = r.finish(self.options.hash_frame); self.stop(event_loop, result); return;
                }
                result
            }
            // Resizing changes presentation size only. Clear accumulated input
            // debt so an interactive resize never causes a later burst of ticks.
            WindowEvent::Resized(size) => {
                r.minimized = size.width == 0 || size.height == 0;
                if !r.minimized {
                    r.surface.resize(&r.context, NonZeroU32::new(size.width).unwrap(), NonZeroU32::new(size.height).unwrap());
                    r.window.request_redraw();
                }
                r.host.pause(); r.last = Instant::now(); Ok(())
            }
            // Controls.clear on blur prevents stuck movement and fire. Reset
            // the wall-time anchor too, so returning focus cannot replay a pause.
            WindowEvent::Focused(focused) => {
                r.focused = focused; r.host.pause(); r.last = Instant::now();
                if focused { r.audio.play() } else { r.grab(false).and_then(|_| r.audio.pause()) }
            }
            WindowEvent::KeyboardInput { event, .. } if r.focused => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if key == KeyCode::Escape && event.state == ElementState::Pressed { r.host.pause(); r.grab(false) }
                    else { r.host.input.key(key, event.state == ElementState::Pressed); Ok(()) }
                } else { Ok(()) }
            }
            // The click that captures the cursor is an activation gesture.
            // Subsequent press/release events enter the gameplay command bank.
            WindowEvent::MouseInput { button, state, .. } if r.focused => {
                if state == ElementState::Pressed && !r.captured { r.grab(true) }
                else { r.host.input.mouse(button, state == ElementState::Pressed); Ok(()) }
            }
            WindowEvent::MouseWheel { delta, .. } if r.focused => {
                let y = match delta { MouseScrollDelta::LineDelta(_, y) => y as f64, MouseScrollDelta::PixelDelta(p) => p.y };
                r.host.input.wheel(y); Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = result { self.stop(event_loop, Err(error)); }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(r) = &self.running else { return; };
        if r.minimized { event_loop.set_control_flow(ControlFlow::Wait); return; }
        if Instant::now() >= r.next_frame { r.window.request_redraw(); }
        event_loop.set_control_flow(ControlFlow::WaitUntil(r.next_frame));
    }
    // Cursor position is unsuitable for FPS look: it stops at screen edges.
    // Raw motion is admitted only while this window has focus and capture.
    fn device_event(&mut self, _event_loop: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let Some(r) = &mut self.running {
            if r.focused && r.captured {
                if let DeviceEvent::MouseMotion { delta: (x, y) } = event { r.host.input.motion(x, y); }
            }
        }
    }
    // This desktop slice does not restore a lost surface from stale handles.
    // Report suspension so the eventual retained-resource owner can rebuild it.
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.stop(event_loop, Err("window context suspended".into()));
    }
}
fn run() -> Result<(), String> {
    let options = Options::read()?;
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let mut app = App { options, running: None, error: None };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    match app.error { Some(error) => Err(error), None => Ok(()) }
}
fn main() {
    if let Err(error) = run() { eprintln!("bfe-client: {error}"); std::process::exit(1); }
}
