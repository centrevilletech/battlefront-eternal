//! Select local authority before any device or archive host is opened.
//! createLocalServer's native counterpart admits once, then the existing
//! device Host and whole-scene presentation consume its completed ticks.
use super::{battle::Battle, context::Native, options::Options, runtime::Runtime, setup::{Admitted, NativeSource, Source}, window};
use bfe_client::scene::Scene;
use bfe_content::files::DirFiles;
use bfe_content_native::setup::admission::{CLOCK_START_MS, CLOCK_UNIX_MS};
use bfe_sim::abi::context::Context;

// Other native modes keep their existing parser and owner. Consuming a local
// launch also consumes a reported error; it never falls through to --map or
// a recorded battle because a Context could not construct a local world.
pub fn dispatch() -> bool {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("--local") { return false; }
    let result = Options::parse(&args, std::env::var_os("BFE_CONTENT_DIR"))
        .and_then(|options| options.ok_or_else(|| "missing local battle mode".into()))
        .and_then(run);
    if let Err(error) = result { eprintln!("bfe-client: {error}"); std::process::exit(1); }
    true
}

pub struct Prepared {
    pub engine: Box<Native>,
    pub admission: Box<Admitted>,
    pub root: std::path::PathBuf,
}

impl Prepared {
    pub fn new(options: &Options) -> Result<Self, String> {
        let world_options = options.world()?;
        if !bfe_sim::world::admit::provided() {
            return Err(super::setup::describe(bfe_sim::abi::context::Error::new(
                bfe_sim::abi::context::ErrorKind::Unavailable,
                "world_admit",
            )));
        }
        let files = DirFiles::new(&options.root);
        let source = NativeSource { files: &files, options: world_options };
        let context = Context::new();
        // Admission refuses before creating GL/audio devices when construction is
        // unavailable. Dispose explicitly so even a retained diagnostic alias
        // cannot continue using owners from a failed launch.
        let admission = match source.admit(&context) {
            Ok(admission) => Box::new(admission),
            Err(error) => { let _ = context.dispose(); return Err(error); }
        };
        Ok(Self { engine: Box::new(Native::new(context, &admission)?), admission, root: options.root.clone() })
    }

    // Both a desktop launch and the installed-content frame gate use this exact
    // factory. Admission is complete before display resources, and any resource
    // failure drops the Context on the same owning event-loop thread.
    pub fn open(self, size: [i32; 2], load: &mut dyn FnMut() -> glow::Context) -> Result<window::Admitted<Native>, String> {
        let (mut scene, camera) = Scene::load_online(&self.root, &self.admission.query, size, load)?;
        let runtime = Runtime::new(*self.engine, *self.admission, CLOCK_START_MS, CLOCK_UNIX_MS)?;
        let battle = Battle::new(runtime, &mut scene, camera)?;
        Ok(window::Admitted { scene: Box::new(scene), battle })
    }
}

pub fn run(options: Options) -> Result<(), String> {
    let prepared = Prepared::new(&options)?;
    window::run(move |size, load| prepared.open(size, load), options.audio, options.duration, ())
}
