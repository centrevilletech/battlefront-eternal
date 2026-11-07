//! Window host for a full recorded world stream. Native presentation and GL
//! stay on this thread; input uses the same generated records as a live World.
use crate::{
    audio::AudioDevice,
    host::{Host, STEP},
    session::{RecordedWorld, Session},
};
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder, PossiblyCurrentContext, Version},
    display::{GetGlDisplay, GlDisplay},
    prelude::*,
    surface::{Surface, SwapInterval, WindowSurface},
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::{
    num::NonZeroU32,
    path::PathBuf,
    time::{Duration, Instant},
};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, ElementState, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};
struct Options {
    dir: PathBuf,
    map: Option<String>,
    join: Option<String>,
    identity: PathBuf,
    name: String,
    seconds: Option<Duration>,
    root: PathBuf,
    audio: bool,
    pcm: Vec<f32>,
    sound: Option<PathBuf>,
    desktop_gl: bool,
}
impl Options {
    fn read(audio: bool) -> Result<Self, String> {
        let mut args = std::env::args().skip(1);
        let first = args
            .next()
            .ok_or("usage: bfe-client --join HOST:PORT [CONTENT_DIR] or --map MAP [CONTENT_DIR] or SESSION_DIR [CONTENT_DIR]")?;
        let join=if first=="--join"{Some(args.next().ok_or("missing server address")?)}else{None};
        let map = if first == "--map" {
            Some(args.next().ok_or("missing map id")?)
        } else {
            None
        };
        let dir = if map.is_some() || join.is_some() {
            PathBuf::new()
        } else {
            PathBuf::from(first)
        };
        // Select inputs before creating the event loop. calls.json belongs to
        // the separate headless replay tool, never to a second window host.
        if map.is_none() && join.is_none() && !dir.join("session.json").is_file() {
            return Err("expected a version-2 session.json archive or --map; near captures use bfe-native".into());
        }
        let mut identity=None;let mut name=String::from("Player");let mut audio=audio;
        let mut seconds = None;
        let mut desktop_gl = false;
        let mut root = std::env::var_os("BFE_CONTENT_DIR").map(PathBuf::from);
        let mut pcm = Vec::new();
        let mut sound = None;
        while let Some(arg) = args.next() {
            if arg=="--identity"{identity=Some(PathBuf::from(args.next().ok_or("missing identity path")?));}
            else if arg=="--name"{name=args.next().ok_or("missing player name")?;}
            else if arg=="--no-audio"{audio=false;}
            else if arg == "--seconds" {
                let value: f64 = args
                    .next()
                    .ok_or("missing duration")?
                    .parse()
                    .map_err(|_| "invalid duration")?;
                if !value.is_finite() || !(0.1..=86400.0).contains(&value) {
                    return Err("invalid duration".into());
                }
                seconds = Some(Duration::from_secs_f64(value));
            } else if arg == "--desktop-gl" {
                desktop_gl = true;
            } else if arg == "--sound" {
                sound = Some(PathBuf::from(args.next().ok_or("missing sound archive")?));
            } else if arg == "--pcm" {
                let bytes = std::fs::read(args.next().ok_or("missing PCM path")?)
                    .map_err(|e| e.to_string())?;
                if bytes.len() % 8 != 0 {
                    return Err("PCM requires complete stereo f32 frames".into());
                }
                pcm = bytes
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
                    .collect();
            } else if arg.starts_with('-') {
                return Err(format!("unknown option {arg}"));
            } else {
                root = Some(arg.into());
            }
        }
        Ok(Self {
            dir,
            map,
            join,
            identity:match identity{Some(p)=>p,None=>crate::online::info::default_identity()?},
            name,
            seconds,
            root: root.ok_or("content directory required")?,
            audio,
            pcm,
            sound,
            desktop_gl,
        })
    }
}
// One window/context hosts either immutable replay or an admitted map. The
// World frame type stays on its own side of this enum; device input is shared.
enum SceneDriver {
    Recorded {
        session: Session,
        host: Host<RecordedWorld>,
        frame: usize,
    },
    Online {
        scene:crate::scene::Scene,
        host:Host<crate::online::client::Client>,
        last:Instant,start:Option<Instant>,seconds:Option<Duration>,initial:Option<String>,
    },
    Native {
        scene: crate::scene::Scene,
        host: Host<crate::scene::MapCamera>,
        last: Instant,
        start: Option<Instant>,
        seconds: Option<Duration>,
        initial: Option<String>,
        origin: [f64; 3],
        frames: u64,
    },
}
impl SceneDriver {
    fn input(&mut self) -> &mut crate::input::Input {
        match self {
            Self::Recorded { host, .. } => &mut host.input,
            Self::Native { host, .. } => &mut host.input,
            Self::Online { host, .. } => &mut host.input,
        }
    }
    fn pause(&mut self) {
        match self {
            Self::Recorded { host, .. } => host.pause(),
            Self::Native { host, last, .. } => {
                host.pause();
                *last = Instant::now();
            }
            Self::Online{host,last,..}=>{host.pause();*last=Instant::now();}
        }
    }
    fn redraw(&mut self) -> Result<bool, String> {
        match self {
            Self::Recorded {
                session,
                host,
                frame,
            } => {
                let commands = host.frame(STEP)?.clone();
                session.play_range(commands)?;
                *frame += 1;
                let done = *frame == host.world.len();
                if done {
                    println!("{}", session.compare()?);
                }
                Ok(done)
            }
            Self::Online{scene,host,last,start,seconds,initial}=>{
                let now=Instant::now();let elapsed=now.duration_since(*last).as_secs_f64();*last=now;
                host.world.receive(elapsed)?;
                // Copy only the fixed frame bank. Replica phases borrow the
                // receiver beside it, without borrowing a World or JS facade.
                let view=crate::scene::View{values:host.frame(elapsed)?.values};
                host.world.send()?;
                if let Some(resources)=&mut scene.online{resources.frame(&host.world.receiver,host.world.receiver.clock.shown,elapsed.min(8./60.),host.world.camera.position);}
                scene.draw(&view)?;
                if start.is_none(){*start=Some(now);if seconds.is_some(){*initial=Some(scene.hash()?);}eprintln!("native online scene ready");}
                if seconds.is_some_and(|duration|now.duration_since(start.unwrap_or(now))>=duration){
                    let mut summary=host.world.summary();summary["draws"]=scene.draws.into();summary["initial"]=initial.clone().into();summary["final"]=scene.hash()?.into();summary["visible"]=scene.online.as_ref().map_or(0,|r|r.visible).into();println!("{summary}");return Ok(true);
                }Ok(false)
            }
            Self::Native {
                scene,
                host,
                last,
                start,
                seconds,
                initial,
                origin,
                frames,
            } => {
                let now = Instant::now();
                let elapsed = now.duration_since(*last).as_secs_f64();
                *last = now;
                let view = host.frame(elapsed)?;
                scene.draw(view)?;
                *frames += 1;
                if start.is_none() {
                    *start = Some(now);
                    if seconds.is_some() {
                        *initial = Some(scene.hash()?);
                        eprintln!("native scene ready");
                    }
                }
                if seconds
                    .is_some_and(|duration| now.duration_since(start.unwrap_or(now)) >= duration)
                {
                    let final_hash = scene.hash()?;
                    println!(
                        "{}",
                        serde_json::json!({"frames":frames,"admitted":scene.admitted,"deferred":scene.deferred,"draws":scene.draws,"initial":initial,"initialPosition":origin,"final":final_hash,"position":host.world.position})
                    );
                    return Ok(true);
                }
                Ok(false)
            }
        }
    }
    fn show(&self) {
        match self {
            Self::Recorded { session, .. } => session.canvas.show(),
            Self::Native { scene, .. } => scene.canvas.show(),
            Self::Online { scene, .. } => scene.canvas.show(),
        }
    }
}

// Renderer/resource owners drop before the current context. Repeated resume
// and close notifications converge on the same device/GL cleanup path.
struct Running {
    scene: SceneDriver,
    audio: Option<AudioDevice>,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    window: Window,
    next: Instant,
    refresh: Duration,
    captured: bool,
    focused: bool,
}
impl Running {
    fn new(el: &ActiveEventLoop, options: &mut Options) -> Result<Self, String> {
        let meta: serde_json::Value = if options.map.is_some() || options.join.is_some() {
            serde_json::json!({"size":[1280,720]})
        } else {
            serde_json::from_slice(
                &std::fs::read(options.dir.join("session.json")).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?
        };
        let size = |i: usize| {
            meta["size"][i]
                .as_u64()
                .filter(|n| (1..=8192).contains(n))
                .map(|n| n as u32)
                .ok_or("invalid session viewport")
        };
        let attrs = Window::default_attributes()
            .with_title("Battlefront Eternal")
            .with_active(options.audio)
            .with_resizable(false)
            .with_inner_size(PhysicalSize::new(size(0)?, size(1)?));
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(attrs))
            .build(
                el,
                ConfigTemplateBuilder::new()
                    .with_alpha_size(8)
                    .with_depth_size(24)
                    .with_stencil_size(8),
                |c| {
                    c.min_by_key(|c| c.num_samples())
                        .expect("no GL configuration")
                },
            )
            .map_err(|e| e.to_string())?;
        let window = window.ok_or("window creation failed")?;
        let display = config.display();
        let raw = window.window_handle().map_err(|e| e.to_string())?.as_raw();
        let api = if options.desktop_gl {
            ContextApi::OpenGl(Some(Version::new(4, 5)))
        } else {
            ContextApi::Gles(Some(Version::new(3, 0)))
        };
        let attrs = ContextAttributesBuilder::new()
            .with_context_api(api)
            .build(Some(raw));
        let context =
            unsafe { display.create_context(&config, &attrs) }.map_err(|e| e.to_string())?;
        let surface = unsafe {
            display.create_window_surface(
                &config,
                &window
                    .build_surface_attributes(Default::default())
                    .map_err(|e| e.to_string())?,
            )
        }
        .map_err(|e| e.to_string())?;
        let context = context.make_current(&surface).map_err(|e| e.to_string())?;
        surface
            .set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::new(1).unwrap()))
            .map_err(|e| e.to_string())?;
        let load = || unsafe {
            glow::Context::from_loader_function_cstr(|name| {
                display.get_proc_address(name) as *const _
            })
        };
        let scene = if let Some(address)=&options.join{
            let server=crate::online::info::Server::read(address)?;
            let (scene,camera)=crate::scene::Scene::load_online(&options.root,&server.query,[size(0)?as i32,size(1)?as i32],load)?;
            let resources=scene.online.as_ref().ok_or("missing online resources")?;
            let client=crate::online::client::Client::connect(address,&server,&options.root,&options.identity,&options.name,resources,camera)?;
            SceneDriver::Online{scene,host:Host::new(client),last:Instant::now(),start:None,seconds:options.seconds,initial:None}
        }else if let Some(map) = &options.map {
            let (scene, camera) = crate::scene::Scene::load(
                &options.root,
                map,
                [size(0)? as i32, size(1)? as i32],
                load,
            )?;
            SceneDriver::Native {
                scene,
                origin: camera.position,
                host: Host::new(camera),
                last: Instant::now(),
                start: None,
                seconds: options.seconds,
                initial: None,
                frames: 0,
            }
        } else {
            let session = Session::load(&options.dir, &options.root, load)?;
            let host = Host::new(session.world());
            SceneDriver::Recorded {
                session,
                host,
                frame: 0,
            }
        };
        let audio = if options.audio {
            Some(if let Some(dir) = &options.sound {
                if !options.pcm.is_empty() {
                    return Err("choose a sound archive or PCM source".into());
                }
                AudioDevice::open_sound(crate::sound::SoundOutput::load(dir)?)?
            } else {
                AudioDevice::open(std::mem::take(&mut options.pcm), 48000)?
            })
        } else {
            None
        };
        let hz = window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz())
            .unwrap_or(60000)
            .max(1000);
        let refresh = Duration::from_secs_f64(1000.0 / hz as f64);
        if options.audio {
            window.focus_window();
        }
        Ok(Self {
            scene,
            audio,
            surface,
            context,
            window,
            next: Instant::now(),
            refresh,
            captured: false,
            focused: true,
        })
    }
    fn grab(&mut self, on: bool) -> Result<(), String> {
        if on {
            self.window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| self.window.set_cursor_grab(CursorGrabMode::Confined))
                .map_err(|e| e.to_string())?;
        } else {
            self.window
                .set_cursor_grab(CursorGrabMode::None)
                .map_err(|e| e.to_string())?;
        }
        self.window.set_cursor_visible(!on);
        self.captured = on;
        Ok(())
    }
    fn redraw(&mut self) -> Result<bool, String> {
        if let Some(audio) = &self.audio {
            audio.check()?;
        }
        let done = self.scene.redraw()?;
        self.scene.show();
        self.window.pre_present_notify();
        self.surface
            .swap_buffers(&self.context)
            .map_err(|e| e.to_string())?;
        self.next = (self.next + self.refresh).max(Instant::now());
        Ok(done)
    }
}
struct App {
    options: Options,
    running: Option<Running>,
    error: Option<String>,
}
impl App {
    fn stop(&mut self, el: &ActiveEventLoop, error: Option<String>) {
        self.error = error;
        self.running.take();
        el.exit();
    }
}
impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match Running::new(el, &mut self.options) {
            Ok(r) => {
                r.window.request_redraw();
                self.running = Some(r);
            }
            Err(e) => self.stop(el, Some(e)),
        }
    }
    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let Some(r) = &mut self.running else {
            return;
        };
        let result = match event {
            WindowEvent::CloseRequested => {
                self.stop(el, None);
                return;
            }
            WindowEvent::RedrawRequested => match r.redraw() {
                Ok(true) => {
                    self.stop(el, None);
                    return;
                }
                Ok(false) => Ok(()),
                Err(e) => Err(e),
            },
            WindowEvent::Focused(focused) => {
                r.focused = focused;
                r.scene.pause();
                if focused {
                    r.audio.as_ref().map_or(Ok(()), AudioDevice::play)
                } else {
                    r.grab(false)
                        .and_then(|_| r.audio.as_ref().map_or(Ok(()), AudioDevice::pause))
                }
            }
            WindowEvent::KeyboardInput { event, .. } if r.focused => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    if key == KeyCode::Escape && event.state == ElementState::Pressed {
                        r.grab(false)
                    } else {
                        r.scene
                            .input()
                            .key(key, event.state == ElementState::Pressed);
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }
            WindowEvent::MouseInput { state, button, .. } if r.focused => {
                r.scene
                    .input()
                    .mouse(button, state == ElementState::Pressed);
                if state == ElementState::Pressed && !r.captured {
                    r.grab(true)
                } else {
                    Ok(())
                }
            }
            WindowEvent::MouseWheel { delta, .. } if r.focused => {
                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y,
                };
                r.scene.input().wheel(y);
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(e) = result {
            self.stop(el, Some(e));
        }
    }
    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, event: DeviceEvent) {
        if let Some(r) = &mut self.running {
            if r.focused && r.captured {
                if let DeviceEvent::MouseMotion { delta } = event {
                    r.scene.input().motion(delta.0, delta.1);
                }
            }
        }
    }
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(r) = &self.running {
            if Instant::now() >= r.next {
                r.window.request_redraw();
            } else {
                el.set_control_flow(ControlFlow::WaitUntil(r.next));
            }
        }
    }
}
/// The diagnostic executable has no audio device dependency. The client uses
/// the same window host with its Rust master chain attached to cpal.
pub fn run() {
    run_with_audio(false)
}
pub fn run_with_audio(audio: bool) {
    let result = (|| {
        let options = Options::read(audio)?;
        let mut app = App {
            options,
            running: None,
            error: None,
        };
        let el = EventLoop::new().map_err(|e| e.to_string())?;
        el.run_app(&mut app).map_err(|e| e.to_string())?;
        app.error.map_or(Ok(()), Err)
    })();
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
