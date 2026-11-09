//! Observations retained at createNativeAI's construction seams. These are
//! qualified earlier-owner results, rather than a second identity allocator.
use super::super::{combat, Calls, Cx};
use super::*;
use crate::gen::*;
use bfe_content::Value;

#[derive(Clone, Debug)]
pub struct PostReceipt {
    pub reference: SpawnPost,
    pub building: Option<Ref<Building>>,
    pub name: Option<Value>,
    pub desc: Value,
    pub ai: Value,
}

#[derive(Clone, Debug)]
pub struct HullReceipt {
    pub hull: Ref<Vehicle>,
    pub name: String,
    pub bounds: [f64; 6],
    pub max_health: f64,
    pub speed: f64,
    pub turn_rate: f64,
    pub prepared: bool,
    pub pad: usize,
}

/// preparedPads' padHull (f4-tail.js) before its strings are interned: a
/// hull on a command vehicle's pad as the roster setup publishes it.
#[derive(Clone, Debug)]
pub struct PadHullFacts {
    pub vehicle: Ref<Vehicle>,
    pub alive: bool,
    pub command: bool,
    pub generation: f64,
    pub team: f64,
    pub controlling_team: f64,
    pub seated: bool,
    pub yaw: f64,
    pub name: String,
    pub label: String,
    pub radius: f64,
    pub speed: f64,
    pub value_bleed: f64,
    pub record: Option<PsClass>,
}

/// One mobile post's pad: the hulls constructed for it, in construction
/// order, and the live one.
#[derive(Clone, Debug)]
pub struct PadFacts {
    pub index: u32,
    pub variants: Vec<PadHullFacts>,
    pub vehicle: Option<PadHullFacts>,
}

impl PadFacts {
    /// The generated pad row, its hulls' names and labels interned in the
    /// writer's order (each hull's name, then its label; variants first).
    pub fn setup(&self, intern: &mut dyn FnMut(&str) -> StrId) -> F4PadSetup {
        let mut hull = |h: &PadHullFacts| PsHull {
            vehicle: h.vehicle, alive: h.alive, command: h.command, generation: h.generation, team: h.team,
            controlling_team: h.controlling_team, seated: h.seated, yaw: h.yaw,
            name: intern(&h.name), label: intern(&h.label),
            radius: h.radius, speed: h.speed, value_bleed: h.value_bleed, record: h.record.clone(),
        };
        let variants = self.variants.iter().map(&mut hull).collect();
        let vehicle = self.vehicle.as_ref().map(&mut hull);
        F4PadSetup { index: self.index, pad: PsPad { variants, vehicle } }
    }
    /// The most spawn points any of its hulls' class records carries.
    pub fn points(&self) -> usize {
        self.variants.iter().chain(self.vehicle.iter()).filter_map(|h| h.record.as_ref()).map(|r| r.points.len()).max().unwrap_or(0)
    }
}

/// Retained by the admission driver across native voice/Character births.
/// Only live motion, preparation and post metadata need separate receipts;
/// definitions, members and building bindings stay in the shared context.
#[derive(Clone, Debug, Default)]
pub struct Receipts {
    pub setup: SetupFacts,
    pub part: Part,
    pub prepared: Vec<String>,
    pub hulls: Vec<HullReceipt>,
    pub posts: Vec<PostReceipt>,
    pub pads: Vec<PadFacts>,
    pub initial_classes: Vec<Value>,
    pub initial_items: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Part {
    #[default]
    Begin,
    Content,
    Roster,
    Characters,
    Activate,
    Complete,
}

/// Cx owns this admission's cursor. Builders never put runtime observations
/// in the export-result list under invented names.
pub fn build_next(cx: &mut Cx) -> Result<Calls, AdmitError> {
    let mut receipts = cx
        .ids
        .ai
        .take()
        .ok_or_else(|| error("AI construction observations are unavailable"))?;
    let part = receipts.part;
    let result = build_with_receipts(cx, &mut receipts, part);
    if result.is_ok() {
        receipts.part = match part {
            Part::Begin => Part::Content,
            Part::Content => Part::Roster,
            Part::Roster => Part::Characters,
            Part::Characters => Part::Activate,
            Part::Activate | Part::Complete => Part::Complete,
        };
    }
    cx.ids.ai = Some(receipts);
    result
}

fn unit_classes<'a>(
    cx: &Cx<'a>,
    names: &[String],
) -> Result<Vec<(&'a str, &'a Value)>, AdmitError> {
    let mut classes = Vec::new();
    // UnitFactory walks each side's authored table, not the order in which
    // individual bodies first happened to prepare that model.
    for side in &cx.battle.sides {
        if let Some(table) = side.units.as_object() {
            for (name, definition) in table.entries() {
                if names.iter().any(|prepared| prepared == name) {
                    classes.push((name, definition));
                }
            }
        }
    }
    if names
        .iter()
        .any(|name| !classes.iter().any(|(found, _)| found == name))
    {
        return Err(error(
            "prepared AI class is absent from the shared side tables",
        ));
    }
    Ok(classes)
}

fn definition_ids(cx: &mut Cx, values: &[&Value]) -> Result<Vec<DefId>, AdmitError> {
    let (ids, _) = combat::with_bindings(cx, Stage::Ai, |bindings, _| {
        let ids = values
            .iter()
            .map(|v| bindings.combat.classes.id(v, Stage::Ai))
            .collect::<Result<_, _>>()?;
        Ok((ids, Vec::new()))
    })?;
    Ok(ids)
}

/// One suffix at a time: the driver dispatches it, records native answers,
/// refreshes Cx.strings, and only then asks for the following suffix.
pub fn build_with_receipts(
    cx: &mut Cx,
    receipts: &mut Receipts,
    part: Part,
) -> Result<Calls, AdmitError> {
    match part {
        Part::Begin => {
            let mut setup = receipts.setup.clone();
            setup.buildings = cx.ids.props.len() as u32;
            setup.hull_seats = receipts
                .hulls
                .iter()
                .map(|h| {
                    cx.defs
                        .vehicles
                        .iter()
                        .find(|d| cx.defs.names.text(d.name) == h.name)
                        .map(|d| d.seats.len() as u32)
                        .ok_or_else(|| error("AI hull definition is unavailable"))
                })
                .collect::<Result<_, _>>()?;
            setup.classes = unit_classes(cx, &receipts.prepared)?.len() as u32;
            setup.posts = receipts.posts.len() as u32;
            begin_cx(cx, &setup)
        }
        Part::Content | Part::Activate => {
            content_from_receipts(cx, receipts, part == Part::Activate)
        }
        Part::Roster => {
            let classes = unit_classes(cx, &receipts.prepared)?;
            let ids = definition_ids(cx, &classes.iter().map(|(_, d)| *d).collect::<Vec<_>>())?;
            let prepared = classes
                .into_iter()
                .zip(ids)
                .map(|((name, definition), id)| PreparedClass {
                    name,
                    definition,
                    id,
                })
                .collect::<Vec<_>>();
            roster_cx(
                cx,
                &prepared,
                receipts.setup.unit_scale,
                &receipts.pads,
            )
        }
        Part::Characters => roster_characters_cx(cx),
        Part::Complete => Ok(Vec::new()),
    }
}

fn content_from_receipts(
    cx: &mut Cx,
    receipts: &mut Receipts,
    late: bool,
) -> Result<Calls, AdmitError> {
    let battle = cx.battle;
    let defs = cx.defs;
    let mut classes = unit_classes(cx, &receipts.prepared)?;
    if late {
        classes.retain(|(_, d)| !receipts.initial_classes.iter().any(|old| old.same(d)));
    }
    let ids = definition_ids(cx, &classes.iter().map(|(_, d)| *d).collect::<Vec<_>>())?;
    let classes = classes
        .into_iter()
        .zip(ids)
        .map(|((name, definition), id)| PreparedClass {
            name,
            definition,
            id,
        })
        .collect::<Vec<_>>();
    let items = items_cx(cx)?;
    let first = if late { receipts.initial_items } else { 0 };
    if first > items.len() {
        return Err(error(
            "AI canonical item table shrank after initial content",
        ));
    }
    let item_facts = items[first..]
        .iter()
        .map(|(id, definition)| ItemFacts {
            id: *id,
            definition,
        })
        .collect();
    let mut facts = ContentFacts {
        classes,
        items: item_facts,
        ..Default::default()
    };
    if !late {
        facts.posts = receipts
            .posts
            .iter()
            .map(|p| PostFacts {
                reference: p.reference,
                name: p.name.as_ref(),
                desc: &p.desc,
                ai: &p.ai,
            })
            .collect();
        // classFacts reserves hull/prop definition identities in the same
        // order as putAiWorldContent, including distinct equal objects.
        let hull_defs = receipts
            .hulls
            .iter()
            .map(|h| {
                defs.vehicles
                    .iter()
                    .find(|d| defs.names.text(d.name) == h.name)
                    .ok_or_else(|| error("AI hull definition is unavailable"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let hull_ids =
            definition_ids(cx, &hull_defs.iter().map(|d| &d.record).collect::<Vec<_>>())?;
        for ((h, d), id) in receipts.hulls.iter().zip(hull_defs).zip(hull_ids) {
            let binding = cx
                .ids
                .hulls
                .iter()
                .find(|b| b.setup.hull == h.hull)
                .ok_or_else(|| error("AI hull has no earlier F3 binding"))?;
            let slot_desc = battle
                .vehicle_def
                .as_ref()
                .and_then(|v| v.get("spawns"))
                .and_then(|v| v.at(h.pad));
            facts.hulls.push(HullFacts {
                hull: h.hull,
                member: if h.prepared {
                    None
                } else {
                    binding.setup.member
                },
                def: id,
                definition: &d.record,
                name: &h.name,
                base: d.base.map(|name| defs.names.text(name)).unwrap_or("hover"),
                target_class: &d.record,
                category: binding.setup.category as f64,
                bounds: h.bounds,
                max_health: h.max_health,
                speed: h.speed,
                turn_rate: h.turn_rate,
                slot_desc,
                seats: d
                    .seats
                    .iter()
                    .map(|s| {
                        (
                            s.record.get("settings").unwrap_or(&s.record),
                            s.weapons.len() as u32,
                        )
                    })
                    .collect(),
            });
        }
        let owners = cx
            .returned_rows
            .iter()
            .filter(|(name, _)| *name == "ai_world_prop_owner")
            .map(|(_, row)| {
                if row.len() == F4Handle::WORDS {
                    Ok(F4Handle::take(row, &[]))
                } else {
                    Err(error("AI prop owner returned an invalid handle row"))
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        if owners.len() != cx.ids.props.len() {
            return Err(error(
                "AI prop owner answers do not match earlier building bindings",
            ));
        }
        let props = battle
            .props
            .as_ref()
            .and_then(|v| v.get("instances"))
            .and_then(Value::as_array)
            .unwrap_or(&[]);
        let bindings = cx.ids.props.clone();
        let descriptions = bindings
            .iter()
            .map(|b| {
                props
                    .get(b.source)
                    .ok_or_else(|| error("AI building source is unavailable"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let prop_ids = definition_ids(cx, &descriptions)?;
        for (((b, desc), id), owner) in bindings.iter().zip(descriptions).zip(prop_ids).zip(&owners)
        {
            let command = desc.get("base").and_then(Value::as_str) == Some("commandpost");
            let target_name = desc
                .get("class")
                .filter(|v| v.truthy())
                .or_else(|| desc.get("model").filter(|v| v.truthy()))
                .and_then(Value::as_str)
                .unwrap_or(if command { "commandpost" } else { "turret" });
            facts.props.push(PropFacts {
                desc,
                def: id,
                target: b.binding.target,
                owner: owner.clone(),
                member: b.binding.member,
                target_name: Some(target_name),
                target_class: b.binding.member.map(|_| {
                    if command {
                        desc.get("settings").unwrap_or(desc)
                    } else {
                        desc
                    }
                }),
                team: b.binding.team,
                armed: desc.get("weapon").is_some_and(Value::truthy),
                post: receipts
                    .posts
                    .iter()
                    .find(|p| match b.binding.target {
                        RepairTarget::Building(r) => p.building == Some(r),
                        _ => false,
                    })
                    .map(|p| p.reference),
            });
        }
        receipts.initial_classes = facts.classes.iter().map(|c| c.definition.clone()).collect();
        receipts.initial_items = items.len();
        content_cx(cx, &facts)
    } else {
        activate_cx(cx, &facts)
    }
}
