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
    /// Admit an explicitly resolved playlist slot on the native developer authority.
    pub fn developer_playlist_entry(
        root: &std::path::Path,
        options: &bfe_sim::world::admit::Options,
        entry: &bfe_shell::launch::PlaylistEntry,
        preferences: bfe_sim::world::admit::mission::MissionPreferences,
    ) -> Result<(Self, bfe_sim::world::admit::mission::MissionJournal), String> {
        let selection = bfe_sim::world::admit::mission::MissionSelection {
            family: match entry.family {
                bfe_shell::launch::GameFamily::Swbf2 => bfe_content::GameFamily::Swbf2,
                bfe_shell::launch::GameFamily::Swbf1 => return Err("scripted mission requires the 2005 family".into()),
            },
            map: entry.map.clone(),
            era: entry.era.clone().ok_or("scripted mission needs a resolved era")?,
            mode: entry.mode.clone(),
            seed: options.seed,
        };
        Self::developer_mission(root, options, &selection, preferences)
    }

    /// Cold native launch for a resolved developer selection. The release menu gate owns player requests.
    pub fn developer_mission(
        root: &std::path::Path, options: &bfe_sim::world::admit::Options,
        selection: &bfe_sim::world::admit::mission::MissionSelection,
        preferences: bfe_sim::world::admit::mission::MissionPreferences,
    ) -> Result<(Self, bfe_sim::world::admit::mission::MissionJournal), String> {
        Self::developer_mission_in(Context::new(), root, options, selection, preferences)
    }

    /// Admit on a supplied authority when a local observer needs completed fixed ticks.
    pub fn developer_mission_in(
        context: Context, root: &std::path::Path, options: &bfe_sim::world::admit::Options,
        selection: &bfe_sim::world::admit::mission::MissionSelection,
        preferences: bfe_sim::world::admit::mission::MissionPreferences,
    ) -> Result<(Self, bfe_sim::world::admit::mission::MissionJournal), String> {
        let files = DirFiles::from_resolved_root(root, selection.family);
        super::setup::select_developer_spawn_path(&context, selection.family)?;
        let (world, journal) = context.world_mission_content(&files, options, selection, preferences)
            .map_err(super::setup::describe)?;
        if options.family == bfe_family::GameFamily::Swbf2 && options.mode == "ass" {
            let seated = std::env::var_os("BFE_DEV_SWBF2_SEATED").as_deref() == Some(std::ffi::OsStr::new("1"));
            let gunner = std::env::var_os("BFE_DEV_SWBF2_GUNNER").as_deref() == Some(std::ffi::OsStr::new("1"));
            let scripted = std::env::var("BFE_DEV_SWBF2_SCRIPTED_PILOTS").as_deref() == Ok("1");
            // The player reaches his craft seats and chairs through the seat driver every player
            // shares; the window draws every round and shows the mission's kills.
            let (player, mailbox) = (world.handles.player, world.handles.mailbox);
            context.owned(|w| {
                if w.connect_space_pilot(player, mailbox) {
                    w.observe_local_space();
                    w.start_local_space_developer(player, seated, gunner, scripted);
                }
            }).map_err(super::setup::describe)?;
        }
        let mut admission = Admitted::from_world(world, options)?;
        admission.query = journal.resources.query.clone();
        (admission.explosions, admission.host_content) = bfe_sim::host::mission_content_tables(
            &context, admission.handles.store_source, &journal.resources.registry,
            &journal.resources.definitions.registry.combat, &journal.resources.building_sources,
        )?;
        // A script whose setup fails ends as a draw: the engine reports it on its opening tick.
        let engine = Box::new(Native::scripted(context, &admission, Some(&journal.resources.definitions))?);
        Ok((Self { engine, admission: Box::new(admission), root: root.into() }, journal))
    }

    pub fn new(options: &Options) -> Result<Self, String> {
        Self::with_spawn_screen(options, false)
    }

    pub fn with_spawn_screen(options: &Options, spawn_screen: bool) -> Result<Self, String> {
        let world_options = options.world()?;
        world_options
            .family
            .require_available()
            .map_err(|e| e.to_string())?;
        Self::selected(&options.root, world_options, spawn_screen)
    }

    pub(super) fn selected(root: &std::path::Path, world_options: bfe_sim::world::admit::Options, spawn_screen: bool) -> Result<Self, String> {
        if !bfe_sim::world::admit::provided() {
            return Err(super::setup::describe(bfe_sim::abi::context::Error::new(
                bfe_sim::abi::context::ErrorKind::Unavailable,
                "world_admit",
            )));
        }
        let files = DirFiles::from_resolved_root(root, world_options.family);
        let source = NativeSource { spawn_screen, files: &files, options: world_options };
        let context = Context::new();
        // Admission refuses before creating GL/audio devices when construction is
        // unavailable. Dispose explicitly so even a retained diagnostic alias
        // cannot continue using owners from a failed launch.
        let admission = match source.admit(&context) {
            Ok(admission) => Box::new(admission),
            Err(error) => { let _ = context.dispose(); return Err(error); }
        };
        Ok(Self { engine: Box::new(Native::new(context, &admission)?), admission, root: root.to_path_buf() })
    }
}
