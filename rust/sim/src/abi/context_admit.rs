//! Native cold admission on the context (world/admit): the native client's
//! local battle hands over its content folder and options; the replay
//! driver hands over a recording's assets and options. Both end in the same
//! owner construction, inside the owning borrow, so a panic poisons the
//! context like any other owning entry.
use super::super::context::{Context, Error, ErrorKind};
use crate::world::admit::{self, AdmitError, Admitted, Call, Options, Returned, Stage};
use bfe_content::files::Files;

/// Admission's refusal as a context error: a missing body is Unavailable
/// naming its stage; content the stage cannot play is a host error with the
/// loader's message; a refused row says which call refused.
pub(crate) fn admit_error(error: AdmitError) -> Error {
    match error {
        AdmitError::MissionSetup(message) => Error::new(ErrorKind::MissionSetup, message),
        AdmitError::Unavailable(stage) => Error::new(ErrorKind::Unavailable, format!("world_admit {stage:?}")),
        other => Error::host(other.to_string()),
    }
}

impl Context {
    /// One admission or spawn-time export as the module's host calls it,
    /// against the same body the WebAssembly export wraps (world/admit/dispatch):
    /// a host that marshals its own rows (the dedicated server's JavaScript
    /// through the addon) admits a native world call by call. The call's
    /// scalar arguments are the export's; pool keys stand where the module
    /// passes addresses; its argument block, scratch bytes and interned
    /// strings travel with it. An owning entry: a panic poisons the context.
    pub fn invoke_export(&self, export: &str, call: &Call) -> Result<Returned, Error> {
        let outcome = self.owned(|w| -> Result<Returned, AdmitError> {
            admit::sequence::args(w, call)?;
            admit::dispatch::call(w, Stage::Host, export, call)
        })?;
        match outcome {
            Ok(Returned::Refused) => Err(Error::host(format!("{export} refused its arguments"))),
            Ok(answer) => Ok(answer),
            Err(AdmitError::NoBody { call, .. }) => Err(Error::new(ErrorKind::Unavailable, format!("invoke_export {call}"))),
            Err(other) => Err(Error::host(other.to_string())),
        }
    }

    /// preparePhase's foreign-row count for the next tick
    /// (`WorldTickInput.units`): the built bodies still in play that neither
    /// a network player nor a native intent drives. A host reads it before
    /// each tick, as the admission's `Admitted.units` gives the first.
    pub fn world_foreign_units(&self) -> Result<u32, Error> {
        self.owned(|w| w.foreign_unit_rows())
    }

    /// Native preparePhase writes the generated foreign rows and returns their
    /// count for the immediately following tick. The row bank is preallocated.
    pub fn world_prepare_units(&self) -> Result<u32, Error> {
        self.owned(|w| w.prepare_foreign_unit_rows())?
            .ok_or_else(|| Error::host("foreign unit row capacity exceeded"))
    }

    /// Admit a menu-selected battle with the native spawn screen policy.
    pub fn world_admit_spawn_screen(&self, files: &dyn Files, options: &Options) -> Result<Admitted, Error> {
        let admitted = self.owned(|w| admit::admit_spawn_screen(w, files, options))?.map_err(admit_error)?;
        self.world_host_admitted_random(options.seed, admitted.handles.player)?;
        Ok(admitted)
    }

    /// Developer-only local authority admission with a resolved mission.
    pub fn world_admit_mission(&self, files: &dyn Files, options: &Options, selection: &admit::mission::MissionSelection,
        preferences: admit::mission::MissionPreferences) -> Result<Admitted, Error> {
        self.world_mission_content(files, options, selection, preferences).map(|(admitted,_)|admitted)
    }

    /// True once startup has completed. A setup error carries the local player's message.
    pub fn world_mission_started(&self) -> Result<bool, Error> {
        self.owned(|world| {
            let (rules, _) = world.mission_state().ok_or_else(|| Error::host("missing mission authority"))?;
            if let Some(message) = rules.setup_failure() {
                return Err(Error::new(ErrorKind::MissionSetup, message));
            }
            Ok(!matches!(rules.host().lifecycle, crate::battle::script::LifecycleState::Cold | crate::battle::script::LifecycleState::Loaded))
        })?
    }

    pub fn world_mission_content(&self, files:&dyn Files,options:&Options,selection:&admit::mission::MissionSelection,
        preferences:admit::mission::MissionPreferences)->Result<(Admitted,admit::mission::MissionJournal),Error> {
        let result=self.owned(|w|admit::mission::admit_developer_mission(w,files,options,selection,preferences)
            .map(|(admitted,journal,_)|(admitted,journal)))?.map_err(admit_error)?;
        if preferences.direct_spawn {
            self.host(|host| host.bind_deployed_dispensers(self))?.map_err(Error::host)?;
        }
        self.world_host_admitted_random(options.seed,result.0.handles.player)?;
        Ok(result)
    }

    /// Builds the battle's owners from `files` (the content folder, or the
    /// recording's bytes) for `options`, then admits the local human and the
    /// publication host. One call a battle, before `world_step_init`.
    pub fn world_admit(&self, files: &dyn Files, options: &Options) -> Result<Admitted, Error> {
        let admitted = self.owned(|w| admit::admit(w, files, options))?.map_err(admit_error)?;
        self.world_host_admitted_random(options.seed, admitted.handles.player)?;
        Ok(admitted)
    }

    /// Family-explicit battle admission (developer/test seam for 2005 maps).
    pub fn world_admit_for_family(
        &self,
        files: &dyn Files,
        family: bfe_content::GameFamily,
        options: &Options,
        mission: Option<&str>,
        spawn_screen: bool,
    ) -> Result<Admitted, Error> {
        let admitted = self
            .owned(|w| admit::admit_for_family(w, files, family, options, mission, spawn_screen))?
            .map_err(admit_error)?;
        self.world_host_admitted_random(options.seed, admitted.handles.player)?;
        Ok(admitted)
    }
}


#[cfg(test)]
mod mission_tests {
    use super::*;
    use crate::world::admit::mission::{MissionPreferences, MissionSelection};

    #[test]
    #[ignore = "requires a prepared mission and a player class carrying a dispenser"]
    fn direct_deployment_binds_dispensers_to_the_playing_life() {
        let root = std::env::var_os("BFE_SWBF2_PREPARED").expect("prepared mission");
        let unit = std::env::var("BFE_MATCH_UNIT").expect("class with an item dispenser");
        let selection = MissionSelection {
            family: bfe_content::GameFamily::Swbf2,
            map: "tat2".into(), era: "gcw".into(), mode: "eli".into(), seed: 7,
        };
        let options = Options {
            family: selection.family, map: selection.map.clone(), mode: selection.mode.clone(),
            seed: selection.seed, unit: Some(unit), side: "all".into(), ..Default::default()
        };
        let files = bfe_content::files::DirFiles::from_resolved_root(std::path::Path::new(&root), bfe_family::GameFamily::Swbf2);
        let context = Context::new();
        context.world_mission_content(&files, &options, &selection, MissionPreferences {
            direct_spawn: true, ..Default::default()
        }).unwrap();
        let checked = context.owned(|world| {
            let items = crate::units::f3::items_read(&world.units);
            let weapons = &crate::combat::module::defs_ref(&world.combat).weapons;
            let mut checked = 0;
            for (r, body) in world.sim.store.unit.iter().filter(|(_, body)| body.human) {
                for gun in &body.combat.weapons {
                    for (level, data) in weapons.class(gun.def).levels.iter().enumerate() {
                        if data.obj.get("item").truthy() {
                            assert!(items.owned_key(r.slot, body.generation, gun.def, level as u32).is_some());
                            checked += 1;
                        }
                    }
                }
            }
            checked
        }).unwrap();
        assert!(checked > 0);
    }
}
