//! Cold recording of the selected mode and the native instances it can address.
use crate::battle::rules::{events::ObjectHandle, objects::{NamedObject, RuleError, RuleResult}, ModeParameters, Selection};
use crate::gen::{F4Handle, F4ObjectKind, SpawnPost};
use crate::store::{Ref, StrId};
use crate::world::{admit::Call, owner::World};
use bfe_family::GameFamily;

pub(crate) const EXPORT: &str = "f4_select_rules";
// F4 kinds occupy the nonnegative values. Fleet hulls have a separate owner.
const DOOR: f64 = -3.0;
const HULL: f64 = -2.0;
const ABSENT: f64 = -1.0;

pub(crate) fn encode(request: &Selection, objects: Vec<NamedObject>, regions: Option<&[bfe_content::Value]>, mut intern: impl FnMut(&mut Call, &str) -> StrId) -> Call {
    let mut c = Call::default();
    let family = intern(&mut c, request.family.as_str());
    let mode = intern(&mut c, &request.mode);
    let p = request.parameters;
    c.args.extend([family.0.into(), mode.0.into(), p.command_limit.into(), p.timer_limit.into(),
        p.score_limit.map_or(ABSENT, f64::from), p.time_limit.unwrap_or(ABSENT)]);
    for row in objects {
        let (kind, slot, generation, life) = match row.object {
            ObjectHandle::Door(instance) => (DOOR, instance, 0, ABSENT),
            ObjectHandle::Prop(h) => (h.kind as u8 as f64, h.slot, h.generation, h.life),
            ObjectHandle::Vehicle { vehicle, life } => (HULL, vehicle.slot, vehicle.gen, life),
            ObjectHandle::Post(SpawnPost::Post(r)) => (F4ObjectKind::Post as u8 as f64, r.slot, r.gen, ABSENT),
            ObjectHandle::Post(SpawnPost::MobilePost(r)) => (F4ObjectKind::MobilePost as u8 as f64, r.slot, r.gen, ABSENT),
        };
        let name = row.name.as_ref().map_or(ABSENT, |n| f64::from(intern(&mut c, n).0));
        let class = row.class.as_ref().map_or(ABSENT, |n| f64::from(intern(&mut c, n).0));
        c.args.extend([kind, slot.into(), generation.into(), life, name, class]);
    }
    if let Some(records) = regions {
        // A present, empty index is distinct from an older recording without one.
        c.row.push(0.0);
        for record in records {
            let Some(name) = record.get("name").and_then(bfe_content::Value::as_str) else { continue };
            let Some(p) = crate::ai::goals::regions::region_center(record) else { continue };
            let id = intern(&mut c, name);
            c.row.extend([f64::from(id.0), p.x, p.y, p.z]);
        }
        c.row[0] = ((c.row.len() - 1) / 4) as f64;
    }
    c
}

fn uint(n: f64) -> RuleResult<u32> {
    (f64::from(n as u32) == n).then_some(n as u32).ok_or(RuleError::InvalidValue)
}

pub(crate) fn apply(w: &mut World, c: &Call) -> RuleResult<()> {
    let invalid = RuleError::InvalidValue;
    if w.tick.running { return Err(invalid); }
    let [family, mode, commands, timers, score, time, rows @ ..] = c.args.as_slice() else { return Err(invalid) };
    if !rows.len().is_multiple_of(6) { return Err(invalid); }
    let label = |n: f64| {
        let utf16 = w.sim.strings.list.get(uint(n)? as usize).ok_or(invalid)?;
        String::from_utf16(utf16).map_err(|_| invalid)
    };
    let optional = |n| if n == ABSENT { Ok(None) } else { label(n).map(Some) };
    let family = GameFamily::parse(&label(*family)?).ok_or(invalid)?;
    let mode = label(*mode)?;
    if family != GameFamily::Swbf2 || mode.is_empty() { return Err(invalid); }
    if *time != ABSENT && (!time.is_finite() || *time < 0.0) { return Err(invalid); }
    let parameters = ModeParameters {
        command_limit: uint(*commands)?, timer_limit: uint(*timers)?,
        score_limit: (*score != ABSENT).then(|| uint(*score)).transpose()?,
        time_limit: (*time != ABSENT).then_some(*time),
    };
    let mut objects = Vec::with_capacity(rows.len() / 6);
    for &[kind, slot, generation, life, name, class] in rows.as_chunks::<6>().0 {
        let (slot, generation) = (uint(slot)?, uint(generation)?);
        let object = if kind == DOOR && generation == 0 && life == ABSENT {
            ObjectHandle::Door(slot)
        } else if kind == F4ObjectKind::Post as u8 as f64 && life == ABSENT {
            ObjectHandle::Post(SpawnPost::Post(Ref::new(slot, generation)))
        } else if kind == F4ObjectKind::MobilePost as u8 as f64 && life == ABSENT {
            ObjectHandle::Post(SpawnPost::MobilePost(Ref::new(slot, generation)))
        } else {
            if !life.is_finite() || life < 0.0 { return Err(invalid); }
            if kind == HULL {
                ObjectHandle::Vehicle { vehicle: Ref::new(slot, generation), life }
            } else {
                let kind = [F4ObjectKind::Building, F4ObjectKind::Trap, F4ObjectKind::Animated].into_iter()
                    .find(|k| *k as u8 as f64 == kind).ok_or(invalid)?;
                ObjectHandle::Prop(F4Handle { kind, slot, generation, life })
            }
        };
        objects.push(NamedObject { object, name: optional(name)?, class: optional(class)? });
    }
    let Some((&count, coords)) = c.row.split_first() else {
        return crate::battle::f4_tail::select_rules(w, family, &mode, parameters, objects);
    };
    let (rows, remainder) = coords.as_chunks::<4>();
    if !remainder.is_empty() || count != rows.len() as f64 { return Err(invalid); }
    let mut centres = Vec::with_capacity(rows.len());
    for &[name, x, y, z] in rows {
        if !(x.is_finite() && y.is_finite() && z.is_finite()) { return Err(invalid); }
        centres.push((label(name)?.into_bytes().into_boxed_slice(), crate::ai::nav::math::Vec3::new(x, y, z)));
    }
    let previous = w.battle.map_goal_regions.replace(crate::ai::goals::GoalRegions::new(centres));
    let result = crate::battle::f4_tail::select_rules(w, family, &mode, parameters, objects);
    if result.is_err() { w.battle.map_goal_regions = previous; }
    result
}
