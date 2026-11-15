//! Select local authority before any device or archive host is opened.
//! createLocalServer's native counterpart admits once, then the existing
//! device Host and whole-scene presentation consume its completed ticks.
use super::{context::Native, options::Options, setup::{Admitted, NativeSource, Source}};
use bfe_content::files::DirFiles;
use bfe_sim::abi::context::Context;

pub struct Prepared {
    pub engine: Box<Native>,
    pub admission: Box<Admitted>,
    pub root: std::path::PathBuf,
}

impl Prepared {
    pub fn new(options: &Options) -> Result<Self, String> {
        Self::with_spawn_screen(options, false)
    }

    pub fn with_spawn_screen(options: &Options, spawn_screen: bool) -> Result<Self, String> {
        let world_options = options.world()?;
        if !bfe_sim::world::admit::provided() {
            return Err(super::setup::describe(bfe_sim::abi::context::Error::new(
                bfe_sim::abi::context::ErrorKind::Unavailable,
                "world_admit",
            )));
        }
        let files = DirFiles::new(&options.root);
        let source = NativeSource { spawn_screen, files: &files, options: world_options };
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
}
