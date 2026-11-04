//! Battle rules and tallies through the live store. Only these owned
//! kinds leave Sim during a command; bodies and posts can run their own
//! native commands from imports without aliasing this command's state.
use super::rules::{self, RulesWorld};
use super::stats::{Seen, Text};
use crate::abi::sim;
use crate::gen::*;
use crate::store::{Ref, StrId};

fn swap(s: &mut Store) {
    let sim = sim();
    std::mem::swap(&mut sim.store.battle, &mut s.battle);
    std::mem::swap(&mut sim.store.character, &mut s.character);
}

struct ModuleText;
impl Text for ModuleText {
    fn nonempty(&self, id: StrId) -> bool { !sim().strings.units(id).is_empty() }
}

fn query(op: BrQueryOp, a: f64, b: f64, unit: Option<Ref<Unit>>, character: Option<Ref<Character>>) -> BrQueryResult {
    import_battle_query(&BrQueryArgs { op, a, b, unit, character })
}

struct JsRulesWorld;
impl RulesWorld for JsRulesWorld {
    fn relation(&mut self, a: f64, b: f64) -> f64 { query(BrQueryOp::Relation, a, b, None, None).number }
    fn unit_alive(&mut self, unit: Ref<Unit>) -> bool { query(BrQueryOp::UnitAlive, 0.0, 0.0, Some(unit), None).flag }
    fn observe(&mut self, character: Ref<Character>, unit: Option<Ref<Unit>>) -> Option<Seen> {
        query(BrQueryOp::Observe, 0.0, 0.0, unit, Some(character)).seen.map(|s| Seen {
            position: s.position, health: s.health, class_label: s.class_label,
            weapon: s.weapon.map(|v| (v.key, v.label)), vehicle: s.vehicle.map(|v| (v.key, v.label)),
        })
    }
    fn living(&mut self, team: f64) -> Option<f64> { query(BrQueryOp::Living, team, 0.0, None, None).optional_number }
    fn owned_values(&mut self, _: &mut Store, team: f64) -> Vec<Option<f64>> { query(BrQueryOp::Owned, team, 0.0, None, None).values }
    fn battle_over(&mut self, s: &mut Store, loser: f64) {
        swap(s);
        import_battle_over(&BrNumber { value: loser });
        swap(s);
    }
}

#[no_mangle]
pub extern "C" fn battle_rules_words(which: u32) -> u32 {
    (match which { 0 => BrInput::WORDS, 1 => BrNumber::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn battle_rules_run() -> *const f64 {
    let input = { let (row, heap) = sim().calls.args(); BrInput::take(row, heap) };
    let mut s = Store::default();
    // Rules only inspect this list for the no-post early return. The
    // posts' query owns refresh and the values of active posts.
    s.command_posts.posts = sim().store.command_posts.posts.clone();
    swap(&mut s);
    let mut world = JsRulesWorld;
    let mut value = 0.0;
    match input.op {
        BrOperation::Update => rules::update(&mut s, input.dt, &ModuleText,
            |character, _| query(BrQueryOp::StatsActive, 0.0, 0.0, None, Some(character)).flag, &mut world),
        BrOperation::Spend => rules::spend(&mut s.battle, input.team, input.count),
        BrOperation::Credit => rules::credit(&mut s, input.victim, input.killer, input.relation),
        BrOperation::Headshot => rules::headshot(&mut s, input.killer),
        BrOperation::VehicleDestroyed => rules::vehicle_destroyed(&mut s, input.killer),
        BrOperation::Point => rules::point(&mut s, input.killer),
        BrOperation::PostDestroyed => rules::post_destroyed(&mut s, input.killer, input.relation),
        BrOperation::Score => value = rules::score(&mut s, input.team, &mut world),
        BrOperation::UpdateTeam => rules::update_team(&mut s, input.team, input.dt, &mut world),
        BrOperation::UpdateLocals => rules::update_locals(&mut s, input.team, &mut world),
    }
    swap(&mut s);
    let sim = sim();
    sim.calls.result(BrNumber::WORDS, |row, heap| BrNumber { value }.put(row, heap));
    sim.calls.block.ptr()
}
