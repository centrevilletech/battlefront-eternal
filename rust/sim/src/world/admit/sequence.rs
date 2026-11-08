//! The owner construction sequence: game.js createGame headless, then
//! createLocalServer, as the ordered stages below. Each stage drives the
//! existing `_in` bodies with the rows made for it, exactly as the page's
//! `bridge.callArgs(...)` then export call does; nothing of an owner is
//! decided here, only the order and the handles kept for the host.
use super::rows::{self, Call, Cx};
use super::{Admitted, AdmitError, Options, Rows};
use crate::world::owner::World;
use bfe_content::content::Content;
use bfe_content::files::Files;

/// The stages in order. A failure names the stage it stopped at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Loading the battle's files and making the rows.
    Content,
    /// validateStartupClass (f4_startup_class).
    Startup,
    /// The collision kernel from content triangles (ray-kernel.js, collision.js).
    Kernels,
    /// The aim cameras (camera_init, camera_add_definition).
    Cameras,
    /// missionTeams names (ai_mission_teams).
    Teams,
    /// The store's reservation, ordnance definitions and items (ord_def, items_def, items_pools).
    Store,
    /// attachF3Combat (cmb_new, cmb_class, cmb_weapon_set, cmb_explosion).
    Combat,
    /// attachF2 and the hull content (veh_pack, f2_init, veh_class, f2_register, seats, firing, colliders).
    Fleet,
    /// attachF3 (f3_init, registries, props, hulls, items, recon).
    Units,
    /// bindWeaponPolicyContent (ai_weapon_policies).
    Policies,
    /// createNativeAI (ai_world_content, begin, roster, activate) and the mission teams' calls.
    Ai,
    /// createWorldSim and the world clock/events/local owners.
    World,
    /// F4Live, then F4Battle inside F4Tail (game.js 3136-3143, after the AI
    /// roster): f4_live_init, f4_battle_init/capacity, f4_tail_init,
    /// connections, roster.
    LiveTail,
    /// createMultiplayerWorld and createSnapshotHost: players, mailbox, wire session, host and publication.
    Session,
    /// addPlayer, the body placed, the welcome packets.
    Player,
    /// Not a stage of the sequence: an export a host drives itself through
    /// `Context::invoke_export`.
    Host,
}
/// The order the host constructs them in.
pub const ORDER: [Stage; 15] = [Stage::Content, Stage::Startup, Stage::Kernels, Stage::Cameras, Stage::Teams, Stage::Store,
    Stage::Combat, Stage::Fleet, Stage::Units, Stage::Policies, Stage::Ai, Stage::World, Stage::LiveTail, Stage::Session, Stage::Player];

/// One call against the world as the page makes it: the strings interned
/// (the builder interned them through the context's copy of the table in
/// the same order, so the world gives the same ids), the scratch bytes
/// written, then the argument row and heap in the exchange the `_in` body
/// reads with `sim.calls.args()`.
pub(crate) fn args(world: &mut World, call: &Call) -> Result<(), AdmitError> {
    for units in &call.strings {
        let id = world.sim.strings.intern(units);
        // A builder that interned through another table would misplace
        // every id after this one: stop rather than admit a wrong world.
        if world.sim.strings.list.get(id.0 as usize).map(|s| s.as_slice()) != Some(units.as_slice()) {
            return Err(AdmitError::Content { stage: Stage::Content, message: "string table out of step with the builders".into() });
        }
    }
    if !call.scratch.is_empty() { world.sim.scratch_room(call.scratch.len()).copy_from_slice(&call.scratch); }
    if !call.row.is_empty() {
        world.sim.calls.result(call.row.len(), |row, heap| {
            row.copy_from_slice(&call.row);
            heap.extend_from_slice(&call.heap);
        });
    }
    Ok(())
}

/// One stage's calls against their bodies (dispatch.rs). Every answer is fed
/// back into the context for the owners after this stage.
fn replay(world: &mut World, stage: Stage, calls: &[(Stage, &'static str, Call)], cx: &mut Cx) -> Result<(), AdmitError> {
    for (_, name, call) in calls {
        args(world, call)?;
        let answer = super::dispatch::call(world, stage, name, call)?;
        super::dispatch::record(cx, stage, name, answer)?;
    }
    Ok(())
}

/// Runs every stage on the owning world, in order: asks the stage's owners
/// for their calls through the shared context, dispatches them, feeds back
/// what they returned. Stops at the first stage without a body
/// (Unavailable, naming it). A stage that ran leaves its owner admitted.
pub fn run(world: &mut World, content: &Content, files: &dyn Files, options: &Options) -> Result<(Admitted, Rows), AdmitError> {
    let battle = &content.battle;
    let defs = bfe_content::defs::Defs::build(content).map_err(|message| AdmitError::Content { stage: Stage::Content, message })?;
    let mut cx = Cx::new(content, &defs, files, options, world.sim.strings.clone());
    let mut rows = Rows { setup: rows::setup(battle), ..Default::default() };
    let mut admitted = Admitted { setup: rows.setup.clone(), ..Default::default() };
    for stage in ORDER {
        match stage {
            Stage::Content => {}
            Stage::Kernels => { cx.ids.kernel = Some(super::kernels::install(world, content)?); }
            // createWorldSim only wraps the clock, event and local exports the
            // world already owns: nothing to construct natively.
            Stage::World => {}
            // The local server session (players, mailbox, wire session, host,
            // publication, the local player's admission and welcome) is the
            // session module's `session::run(world, &mut cx)`, called here
            // once it lands.
            Stage::Session | Stage::Player => return Err(AdmitError::Unavailable(stage)),
            _ => {
                let calls = rows::build(stage, &mut cx)?;
                replay(world, stage, &calls, &mut cx)?;
                rows.calls.push((stage, calls));
            }
        }
    }
    admitted.units = 0;
    Ok((admitted, rows))
}
