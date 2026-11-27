//! Submit cold particle materials on an independent driver queue. The window
//! can keep presenting the loading movie while a driver specializes a shader.
use glutin::{
    config::GetGlConfig,
    context::{AsRawContext, ContextApi, ContextAttributesBuilder, GlContext, RawContext, PossiblyCurrentContext, Version},
    display::{GetGlDisplay, GlDisplay},
};
#[cfg(not(target_os = "macos"))]
use glutin::context::NotCurrentGlContext;
#[cfg(not(target_os = "macos"))]
use glutin::surface::{PbufferSurface, SurfaceAttributesBuilder};
#[cfg(not(target_os = "macos"))]
use std::num::NonZeroU32;
use std::{cell::RefCell, rc::{Rc, Weak}, thread};

#[cfg(not(target_os = "macos"))]
type WarmSurface = Option<glutin::surface::Surface<PbufferSurface>>;
#[cfg(target_os = "macos")]
type WarmSurface = Option<()>;

thread_local! {
    static DISPLAY_CONTEXT: RefCell<Weak<PossiblyCurrentContext>> = RefCell::default();
}
/// Register the window context used for loading and for drawing its admitted scenes.
pub fn install(context: &Rc<PossiblyCurrentContext>) {
    DISPLAY_CONTEXT.with(|slot| slot.replace(Rc::downgrade(context)));
}

// The driver accepts an opaque sharing handle on another thread. This wrapper
// exposes only that name. The window context stays current on its own thread
// and alive until the worker has joined, including cancellation.
struct Share(RawContext);
unsafe impl Send for Share {}
impl AsRawContext for Share {
    fn raw_context(&self) -> RawContext { self.0 }
}

// Cancelling scene admission must finish all reads of that scene's buffers,
// textures and programs before dropping the scene on the window thread.
struct Worker(Option<thread::JoinHandle<Result<(), String>>>);
impl Drop for Worker {
    fn drop(&mut self) {
        if let Some(worker) = self.0.take() {
            let _ = worker.join();
        }
    }
}

pub(crate) async fn precache(
    bridge: &crate::render_bridge::Bridge,
    batches: impl Iterator<Item = u32>,
) -> Result<(), String> {
    let Some(window) = DISPLAY_CONTEXT.with(|c| c.borrow().upgrade()) else {
        return Ok(());
    };
    let config = window.config();
    let display = config.display();
    let api = match window.context_api() {
        ContextApi::Gles(_) => ContextApi::Gles(Some(Version::new(3, 0))),
        _ => ContextApi::OpenGl(Some(Version::new(3, 3))),
    };
    let source = Share(window.raw_context());
    // Once this fence signals, every shared resource has completed admission.
    // Only the window context owns and subsequently changes those resources.
    bfe_render::effects::finish_warm_commands(&bridge.r.core.gl, crate::loading_work::idle).await?;
    let batches = bridge.r.effects.shared_batches(&bridge.r.core, batches);
    let handle = thread::Builder::new().name("particle-loading".into()).spawn(move || {
        // Some drivers spend a display frame just creating a sharing context.
        let attrs = ContextAttributesBuilder::new().with_context_api(api).with_sharing(&source).build(None);
        let context = unsafe { display.create_context(&config, &attrs) }.map_err(|e| e.to_string())?;
        let (_context, _surface): (_, WarmSurface) = match context {
            #[cfg(target_os = "linux")]
            glutin::context::NotCurrentContext::Egl(c) => (
                PossiblyCurrentContext::Egl(c.make_current_surfaceless().map_err(|e| e.to_string())?),
                None,
            ),
            // CGL has no pbuffers; the draw renders into its own framebuffer,
            // so a surfaceless current context is all it needs.
            #[cfg(target_os = "macos")]
            glutin::context::NotCurrentContext::Cgl(c) => (
                PossiblyCurrentContext::Cgl(c.make_current_surfaceless().map_err(|e| e.to_string())?),
                None,
            ),
            #[cfg(not(target_os = "macos"))]
            other => {
                let attrs = SurfaceAttributesBuilder::<PbufferSurface>::new().build(NonZeroU32::MIN, NonZeroU32::MIN);
                let surface = unsafe { display.create_pbuffer_surface(&config, &attrs) }.map_err(|e| e.to_string())?;
                let current = other.make_current(&surface).map_err(|e| e.to_string())?;
                (current, Some(surface))
            }
        };
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| display.get_proc_address(name)) };
        batches.draw(&gl)
    }).map_err(|e| e.to_string())?;
    let mut worker = Worker(Some(handle));
    while !worker.0.as_ref().unwrap().is_finished() {
        crate::loading_work::idle().await;
    }
    worker.0.take().unwrap().join().map_err(|_| String::from("particle loading thread stopped"))?
}
