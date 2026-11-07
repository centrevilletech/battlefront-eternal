//! A recorded receive stream drives the production native admission and draw.
//! GL resources and packet storage are cold; the measured frames include the
//! first snapshot publication and the first partial audio device quantum.
use bfe_client::{
    audio::Status,
    online::{input::Sender, receiver::Receiver},
    scene::Scene,
};
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder, Version},
    display::{GetGlDisplay, GlDisplay},
    prelude::*,
};
use glutin_winit::{DisplayBuilder, GlWindow};
use raw_window_handle::HasWindowHandle;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    num::NonZeroU32,
    path::Path,
};
use winit::{dpi::PhysicalSize, event_loop::EventLoop, platform::x11::EventLoopBuilderExtX11, window::Window};
thread_local! {static ALLOCS:Cell<Option<usize>>=const{Cell::new(None)};}
struct Allocator;
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        ALLOCS.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        System.alloc(l)
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        ALLOCS.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        System.alloc_zeroed(l)
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        ALLOCS.with(|c| {
            if let Some(n) = c.get() {
                c.set(Some(n + 1));
            }
        });
        System.realloc(p, l, n)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        System.dealloc(p, l)
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;
#[test]
#[ignore = "requires a display, installed content and a recorded network stream"]
fn recorded_live_frames_and_device_quanta_allocate_nothing_after_admission() {
    let fixture: serde_json::Value =
        serde_json::from_slice(&std::fs::read(std::env::var_os("BFE_ONLINE_FIXTURE").expect("network fixture")).unwrap()).unwrap();
    let packets: Vec<(f64, Vec<u8>)> = fixture["packets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p["now"].as_f64().unwrap(), p["bytes"].as_array().unwrap().iter().map(|n| n.as_u64().unwrap() as u8).collect()))
        .collect();
    assert!(packets.len() > 100);
    let mut builder = EventLoop::builder();
    builder.with_any_thread(true);
    let events = builder.build().unwrap();
    let attrs = Window::default_attributes().with_visible(false).with_inner_size(PhysicalSize::new(1280, 720));
    let (window, config) = DisplayBuilder::new()
        .with_window_attributes(Some(attrs))
        .build(&events, ConfigTemplateBuilder::new().with_alpha_size(8).with_depth_size(24).with_stencil_size(8), |c| {
            c.min_by_key(|c| c.num_samples()).unwrap()
        })
        .unwrap();
    let window = window.unwrap();
    let display = config.display();
    let raw = window.window_handle().unwrap().as_raw();
    let attrs = ContextAttributesBuilder::new().with_context_api(ContextApi::Gles(Some(Version::new(3, 0)))).build(Some(raw));
    let context = unsafe { display.create_context(&config, &attrs) }.unwrap();
    let surface = unsafe { display.create_window_surface(&config, &window.build_surface_attributes(Default::default()).unwrap()) }.unwrap();
    let context = context.make_current(&surface).unwrap();
    surface.set_swap_interval(&context, glutin::surface::SwapInterval::Wait(NonZeroU32::new(1).unwrap())).unwrap();
    let root = std::env::var_os("BFE_CONTENT_DIR").expect("battle content directory");
    let (mut scene, mut camera) = Scene::load_online(Path::new(&root), "map=nab2&side=rep&ai=off&seed=1", [1280, 720], || unsafe {
        glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name) as *const _)
    })
    .unwrap();
    let resources = scene.online.as_mut().unwrap();
    let mut replay = resources.replay.take().unwrap();
    let mut output = resources.sound.output.take().unwrap();
    let mut receiver = Receiver::new(resources.descriptor.clone());
    receiver.registry = true;
    receiver.welcomed = true;
    receiver.own.id = fixture["playerId"].as_f64().unwrap();
    let sender = Sender::default();
    let status = Status::default();
    let mut samples = [0f32; 257];
    let mut frames = 0;
    let mut hulls = 0;
    ALLOCS.with(|c| c.set(Some(0)));
    for (now, bytes) in &packets {
        receiver.message(false, bytes, *now);
        assert!(receiver.fault.is_none(), "{:?}", receiver.fault);
        receiver.clock.advance(1. / 30.).unwrap();
        replay.frame(&receiver, &sender).unwrap();
        output.fill(&mut samples, &status);
        scene.present_network(&receiver, &replay, &sender.history, &mut camera, 1. / 30.);
        scene.draw(camera.snapshot()).unwrap();
        receiver.publications.clear();
        frames += 1;
        hulls = hulls.max(scene.online.as_ref().unwrap().hulls.visible);
    }
    let allocations = ALLOCS.with(|c| c.replace(None).unwrap());
    assert_eq!(allocations, 0, "recorded native frame allocation count");
    assert!(receiver.complete);
    assert!(frames > 100);
    assert!(hulls > 0);
    assert!(scene.online.as_ref().unwrap().hud.quads > 0);
}
