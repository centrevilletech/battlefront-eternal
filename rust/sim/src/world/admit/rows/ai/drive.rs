//! The two string-birth seams inside createNativeAI. The enclosing driver
//! calls initial after fleet/F3 and activation after inactive stock loads.
use super::super::{Calls, Cx};
use super::*;
use crate::world::{
    admit::{dispatch, sequence},
    owner::World,
};

fn dispatch_suffix(world: &mut World, cx: &mut Cx, calls: &Calls) -> Result<(), AdmitError> {
    for (stage, name, call) in calls {
        sequence::args(world, call)?;
        let answer = dispatch::call(world, *stage, name, call)?;
        if matches!(
            *name,
            "ai_world_begin" | "ai_world_content" | "ai_world_roster" | "ai_world_activate"
        ) && matches!(answer, dispatch::Returned::Scalar(0.0))
        {
            return Err(AdmitError::Refused {
                stage: *stage,
                call: (*name).into(),
            });
        }
        dispatch::record(cx, *stage, name, answer)?;
        // ai_world_begin owns voice strings; ai_world_roster owns names.
        // Later generated rows must intern against their resulting table.
        cx.refresh_strings(&world.sim.strings);
    }
    Ok(())
}

pub(crate) fn run_initial(world: &mut World, cx: &mut Cx) -> Result<Calls, AdmitError> {
    let mut calls = Vec::new();
    loop {
        let part = cx
            .ids
            .ai
            .as_ref()
            .ok_or_else(|| error("AI construction observations are unavailable"))?
            .part;
        if matches!(part, Part::Activate | Part::Complete) {
            break;
        }
        let suffix = build(cx)?;
        dispatch_suffix(world, cx, &suffix)?;
        calls.extend(suffix);
    }
    // Roster identities are native results. Preserve the actual factions
    // and slab references rather than predicting names, counts or slots.
    cx.ids.factions = world
        .sim
        .store
        .spawning
        .teams
        .iter()
        .map(|t| t.faction)
        .collect();
    cx.ids.mobile_posts = world
        .sim
        .store
        .mobile_post
        .iter()
        .map(|(reference, _)| reference.slot as f64)
        .collect();
    if let Some(facts) = &mut cx.ids.battle {
        facts.characters = world.sim.store.battle.characters.clone();
        facts.character = facts
            .characters
            .iter()
            .copied()
            .find(|r| world.sim.store.character.get(*r).is_some_and(|c| c.human));
        facts.next_id = jsrt::to_uint32(world.sim.store.units.next_id);
    }
    Ok(calls)
}

pub(crate) fn run_activation(world: &mut World, cx: &mut Cx) -> Result<Calls, AdmitError> {
    if cx.ids.ai.as_ref().is_none_or(|r| r.part != Part::Activate) {
        return Err(error("AI activation requires a completed initial roster"));
    }
    let calls = build(cx)?;
    dispatch_suffix(world, cx, &calls)?;
    Ok(calls)
}
