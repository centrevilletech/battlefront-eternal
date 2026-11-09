//! Shared-context entry points for the within-stage boundaries of AiWorld.
//! The sequence dispatches begin, content, roster and activation separately
//! because native string births occur between those pieces.
use super::*;
use crate::gen::*;
use crate::world::admit::{
    rows::{Call, Calls, Cx},
    AdmitError, Stage,
};

struct Interner<'a, 'b> {
    cx: &'a mut Cx<'b>,
    call: &'a mut Call,
}

impl AiInterner for Interner<'_, '_> {
    fn intern_str(&mut self, text: &str) -> StrId {
        self.cx.intern(self.call, text)
    }
}

/// createNativeAI first reserves every named side item in bridge.defs.item,
/// then sends the complete list. Embedded drops/weapon items retain their
/// earlier slots, including separate objects with an equal class name.
pub fn items_cx(cx: &mut Cx) -> Result<Vec<(DefId, bfe_content::value::Value)>, AdmitError> {
    let battle = cx.battle;
    let (items, _) = super::super::combat::with_bindings(cx, Stage::Ai, |bindings, _| {
        for side in &battle.sides {
            if let Some(items) = side.items.as_object() {
                for item in items.values() {
                    bindings.items.definitions.id(item, Stage::Ai)?;
                }
            }
        }
        let items = bindings
            .items
            .definitions
            .values
            .iter()
            .enumerate()
            .map(|(id, value)| (DefId(id as u32), value.clone()))
            .collect();
        Ok((items, Vec::new()))
    })?;
    Ok(items)
}

/// setupRows obtains raw navigation from Files. The stream ids, current
/// states and capacities are receipts of F3/fleet construction, not seeds
/// guessed from the number of class definitions that happened to load.
pub fn begin_cx(cx: &mut Cx, facts: &SetupFacts) -> Result<Calls, AdmitError> {
    let assets = AiAssets::read(cx.battle, cx.files)?;
    let (_, calls) = mission_calls(cx.battle);
    let options = cx.options;
    let team = cx.battle.policy.team;
    let mut call = Call::default();
    let setup = setup_rows(
        &mut Interner {
            cx,
            call: &mut call,
        },
        options,
        team,
        &calls,
        facts,
        &assets,
    )?;
    call.row.resize(AiWorldSetup::WORDS, 0.0);
    setup.put(&mut call.row, &mut call.heap);
    let mut out = vec![
        (Stage::Ai, "ai_world_begin", call),
        (Stage::Ai, "ai_world_boundary", Call::default()),
    ];
    for index in 0..facts.buildings {
        out.push((
            Stage::Ai,
            "ai_world_prop_owner",
            Call {
                args: vec![index as f64],
                ..Default::default()
            },
        ));
    }
    Ok(out)
}

/// The canonical class/item/owner references come from the earlier native
/// constructors. Only their immutable authored policy is transported here.
pub fn content_cx(cx: &mut Cx, facts: &ContentFacts<'_>) -> Result<Calls, AdmitError> {
    encoded_in_context(cx, facts, true)
}

fn encoded_in_context(
    cx: &mut Cx,
    facts: &ContentFacts<'_>,
    initial: bool,
) -> Result<Calls, AdmitError> {
    let mut interns = Call::default();
    let mut content = content_rows(
        &mut Interner {
            cx,
            call: &mut interns,
        },
        facts,
    )?;
    if initial {
        // F3's canonical weapon list is the same one the earlier policy
        // source uses. Do not rebuild it by weapon class name here.
        let weapons = super::super::weapon_policies::weapon_ids(cx)?;
        let source = super::super::weapon_policies::source(cx.battle, cx.files, &weapons);
        content.weapons = source.weapons;
        content.weapon_data = source.data;
    }
    let mut call = encoded_content(&content, facts)?;
    call.strings = interns.strings;
    Ok(vec![
        (Stage::Ai, "ai_world_content", call),
        (Stage::Ai, "ai_world_weapon_policies", Call::default()),
    ])
}

/// Run after the content calls. The native roster can intern names of its
/// own, so its completion must refresh Cx.strings before late content. The
/// pads' hull names and labels are interned where the generated writer
/// reaches the pad field, after the prepared names.
pub fn roster_cx(
    cx: &mut Cx,
    prepared: &[PreparedClass<'_>],
    unit_scale: f64,
    pads: &[super::PadFacts],
) -> Result<Calls, AdmitError> {
    let (mission, calls) = mission_calls(cx.battle);
    let options = cx.options;
    let team = cx.battle.policy.team;
    let mut call = Call::default();
    let roster = roster_rows_with_pads(
        &mut Interner {
            cx,
            call: &mut call,
        },
        options,
        team,
        unit_scale,
        &calls,
        mission.as_ref(),
        prepared,
        |strings| Ok(pads.iter().map(|pad| pad.setup(&mut |text| strings.intern_str(text))).collect()),
    )?;
    call.row.resize(AiRosterSetup::WORDS, 0.0);
    roster.put(&mut call.row, &mut call.heap);
    Ok(vec![(Stage::Ai, "ai_world_roster", call)])
}

/// ai_world_roster's result is count+1, not a predicted capacity. This also
/// handles empty accepted rosters while refusing the native failure value.
pub fn roster_characters_cx(cx: &Cx) -> Result<Calls, AdmitError> {
    let count = cx
        .returned("ai_world_roster")
        .ok_or_else(|| error("AI roster result is unavailable"))?;
    if !count.is_finite() || count < 1.0 || count > u32::MAX as f64 || count.fract() != 0.0 {
        return Err(error("AI roster returned an invalid Character count"));
    }
    Ok((0..count as u32 - 1)
        .map(|index| {
            (
                Stage::Ai,
                "ai_world_roster_character",
                Call {
                    args: vec![index as f64],
                    ..Default::default()
                },
            )
        })
        .collect())
}

/// createNativeAI.activate appends only newly prepared unit classes and
/// items. Existing hulls, posts and weapon policy bytes are not resent.
pub fn activate_cx(cx: &mut Cx, late: &ContentFacts<'_>) -> Result<Calls, AdmitError> {
    if !late.weapons.is_empty()
        || !late.weapon_data.is_empty()
        || !late.posts.is_empty()
        || !late.hulls.is_empty()
        || !late.props.is_empty()
    {
        return Err(error(
            "AI activation content may add only classes and items",
        ));
    }
    let mut calls = if late.classes.is_empty() && late.items.is_empty() {
        Vec::new()
    } else {
        encoded_in_context(cx, late, false)?
    };
    calls.push((Stage::Ai, "ai_world_activate", Call::default()));
    Ok(calls)
}
