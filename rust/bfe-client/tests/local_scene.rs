//! Installed-content local play uses the production admission and window
//! factory. The measured region includes desktop input, the enclosing world
//! tick, canonical publication, native presentation and drawing, from the
//! first frame after cold admission. Audio keeps its existing device owner.
use bfe_client::local;
use bfe_client::input::Input;
use glow::HasContext;
use local::window::Observer;
use std::{alloc::{GlobalAlloc, Layout, System}, cell::{Cell, RefCell}, rc::Rc, time::Duration};
use winit::{event::MouseButton, event_loop::EventLoop, keyboard::KeyCode};
// The event loop is built off the main thread; each desktop platform names that permission differently.
#[cfg(windows)]
use winit::platform::windows::EventLoopBuilderExtWindows;
#[cfg(all(unix, not(target_os = "macos")))]
use winit::platform::x11::EventLoopBuilderExtX11;

thread_local! { static COUNT: Cell<Option<usize>> = const { Cell::new(None) }; }
struct Allocator;
fn counted() {
    let _ = COUNT.try_with(|count| { if let Some(n) = count.get() { count.set(Some(n + 1)); } });
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 { counted(); System.alloc(layout) }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 { counted(); System.alloc_zeroed(layout) }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 { counted(); System.realloc(ptr, layout, size) }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { System.dealloc(ptr, layout) }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[derive(Default)]
struct Counts {
    frames: u64, tick: f64, allocations: usize, worst: usize,
    origin: Option<[f64; 3]>, generation: Option<f64>, yaw: f64, yaw_changed: bool,
    distance_squared: f64, alive: bool,
    hud: usize, projectiles: usize, audio_peak: f64,
}
struct Audit { counts: Rc<RefCell<Counts>>, target: f64 }
impl Observer for Audit {
    fn begin(&mut self) { COUNT.with(|count| count.set(Some(0))); }
    fn input(&mut self, input: &mut Input, tick: f64) {
        // Exercise the actual generated desktop path. This stress sequence is
        // deliberately separate from recorded-input hash proofs: its purpose
        // is continued spawn/movement/fire/action work during a long battle.
        let phase = (tick as u64 / 240) % 6;
        input.key(KeyCode::KeyW, phase < 3);
        input.key(KeyCode::KeyA, phase == 3);
        input.key(KeyCode::KeyD, phase == 4);
        input.key(KeyCode::Space, tick as u64 % 600 < 2);
        input.key(KeyCode::KeyR, tick as u64 % 900 < 2);
        input.mouse(MouseButton::Left, phase % 2 == 0);
        input.motion(0.2, 0.);
    }
    fn end(&mut self, tick: f64) -> Result<(), &'static str> {
        // Turn accounting off before diagnostics can format an error. The
        // window calls this on failed frames too, so an error cannot leave
        // the test harness's allocator measurement enabled during teardown.
        let allocations = COUNT.with(|count| count.replace(None).unwrap());
        let mut totals = self.counts.borrow_mut();
        totals.frames += 1; totals.tick = tick;
        totals.allocations += allocations; totals.worst = totals.worst.max(allocations);
        if allocations == 0 { Ok(()) }
        else { eprintln!("local frame tick={tick} allocations={allocations}"); Err("local frame allocated after admission") }
    }
    fn state(&mut self, own: &bfe_client::online::receiver::Own, scene: &bfe_client::scene::Scene) {
        let mut counts = self.counts.borrow_mut();
        if own.alive {
            // Respawning elsewhere is not proof that desktop movement works.
            // Keep displacement within one life, and require accepted look too.
            if counts.generation != Some(own.generation) {
                counts.generation = Some(own.generation); counts.origin = Some(own.position); counts.yaw = own.yaw;
            }
            let origin = *counts.origin.get_or_insert(own.position);
            let moved = own.position.iter().zip(origin).map(|(a, b)| (a - b) * (a - b)).sum::<f64>();
            counts.distance_squared = counts.distance_squared.max(moved); counts.alive = true;
            let turned = (own.yaw - counts.yaw).abs() > 0.01;
            counts.yaw_changed |= turned;
        }
        if let Some(resources) = &scene.online {
            counts.hud = counts.hud.max(resources.hud.quads as usize);
            counts.projectiles = counts.projectiles.max(resources.combat.total_quads as usize);
            counts.audio_peak = counts.audio_peak.max(resources.sound.peak() as f64);
        }
    }
    fn complete(&self) -> bool { self.counts.borrow().tick >= self.target }
}

#[test]
#[ignore = "requires installed content, complete native admission, an NVIDIA display and audio output"]
fn local_battle_steps_and_draws_without_frame_allocations() {
    // Keep the full release harness linked while construction is unavailable;
    // a constant false assertion would otherwise remove every device path.
    assert!(std::hint::black_box(bfe_sim::world::admit::provided()), "native owner construction is unavailable");
    let map = std::env::var("BFE_LOCAL_MAP").unwrap_or_else(|_| "nab2".into());
    let side = std::env::var("BFE_LOCAL_SIDE").unwrap_or_else(|_| if map == "hot1" { "imp".into() } else { "rep".into() });
    let target: u32 = std::env::var("BFE_LOCAL_TICKS").map_or(Ok(36000), |v| v.parse()).unwrap();
    assert!((600..=216000).contains(&target));
    let args = vec!["--local".into(), map.clone(), "--side".into(), side, "--seed".into(), "1".into()];
    let mut options = local::options::Options::parse(&args, std::env::var_os("BFE_CONTENT_DIR")).unwrap().unwrap();
    // The authority must actually complete ten minutes, including after a
    // brief focus interruption. Wall time is only a watchdog; reaching it
    // before the tick target fails instead of certifying a paused battle.
    options.duration = Some(Duration::from_secs(u64::from(target) / 60 + 300));
    let audio = options.audio;
    let prepared = std::hint::black_box(local::launch::Prepared::new(&options)).unwrap();
    let mut builder = EventLoop::builder(); builder.with_any_thread(true);
    let events = builder.build().unwrap();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let audit = Audit { counts: counts.clone(), target: f64::from(target) };
    let result = local::window::run_on(events, move |size, load| {
        let mut checked = false;
        let mut inspect = || {
            let gl = load();
            // SAFETY: the production factory calls this loader while its
            // creating context is current, on this event-loop thread.
            let vendor = unsafe { gl.get_parameter_string(glow::VENDOR) };
            assert!(vendor.contains("NVIDIA"), "native GL vendor {vendor}");
            checked = true; gl
        };
        let admitted = prepared.open(size, &mut inspect)?;
        assert!(checked, "native scene did not create its renderer");
        Ok(admitted)
    }, audio, options.duration, audit);
    let counts = counts.borrow();
    println!("{}", serde_json::json!({"map":map,"targetTicks":target,"ticks":counts.tick,
        "frames":counts.frames,"allocations":counts.allocations,"worstFrame":counts.worst,
        "alive":counts.alive,"distanceSquared":counts.distance_squared,"hudQuads":counts.hud,
        "lookChanged":counts.yaw_changed,"projectileQuads":counts.projectiles,"audioPeak":counts.audio_peak}));
    result.unwrap();
    assert!(counts.frames > 0); assert!(counts.tick >= f64::from(target), "battle stopped before its tick target");
    assert_eq!(counts.allocations, 0); assert_eq!(counts.worst, 0);
    assert!(counts.alive); assert!(counts.distance_squared > 1.); assert!(counts.yaw_changed);
    assert!(counts.hud > 0); assert!(counts.projectiles > 0); assert!(counts.audio_peak > 0.);
}
