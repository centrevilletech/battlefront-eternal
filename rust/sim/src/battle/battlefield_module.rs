//! Live battlefield commands. The boundary is immutable configuration;
//! Characters leave the shared store while their countdowns run.
use std::cell::RefCell;
use std::rc::Rc;
use super::battlefield::{Battlefield, BattlefieldWorld, LeavingWarning};
use super::world::Body;
use crate::abi::sim;
use crate::ai::nav::boundary::Boundary;
use crate::gen::*;
use crate::store::Ref;

thread_local! { static FIELDS: RefCell<Vec<Rc<Battlefield>>> = const { RefCell::new(Vec::new()) }; }

fn swap(s: &mut Store) { std::mem::swap(&mut sim().store.character, &mut s.character); }

struct JsWorld { characters: Vec<Ref<Character>> }
impl JsWorld {
    fn countdowns(&self, s: &Store) -> Vec<BfCountdown> {
        self.characters.iter().map(|&character| BfCountdown { character, value: s.character.get(character).expect("a Character of the phase").out_of_bounds }).collect()
    }
    fn query(&self, op: BfQueryOp, unit: Ref<Unit>) -> BfQueryResult { import_battlefield_query(&BfQueryArgs { op, unit }) }
    fn action(&mut self, s: &mut Store, op: BfActionOp, unit: Option<Ref<Unit>>, vehicle: Option<Ref<Vehicle>>) {
        let countdowns = self.countdowns(s);
        swap(s);
        self.characters = import_battlefield_action(&BfActionArgs { op, unit, vehicle, countdowns }).characters;
        swap(s);
    }
}
impl BattlefieldWorld for JsWorld {
    fn character_at(&mut self, _: &Store, index: usize) -> Option<Ref<Character>> { self.characters.get(index).copied() }
    fn body(&mut self, unit: Ref<Unit>) -> Body {
        let b = self.query(BfQueryOp::Body, unit).body.expect("the Character's body");
        Body { removed: b.removed, alive: b.alive, combat_dead: b.combat_dead, place: b.place, vehicle: b.vehicle, droid: b.droid }
    }
    fn vehicle(&mut self, unit: Ref<Unit>) -> Option<Ref<Vehicle>> { self.query(BfQueryOp::Vehicle, unit).vehicle }
    fn combat_dead(&mut self, unit: Ref<Unit>) -> bool { self.query(BfQueryOp::CombatDead, unit).flag }
    fn kill(&mut self, s: &mut Store, unit: Ref<Unit>) { self.action(s, BfActionOp::Kill, Some(unit), None); }
    fn kill_droid(&mut self, s: &mut Store, unit: Ref<Unit>) { self.action(s, BfActionOp::KillDroid, Some(unit), None); }
    fn wreck(&mut self, s: &mut Store, vehicle: Ref<Vehicle>) { self.action(s, BfActionOp::Wreck, None, Some(vehicle)); }
}

impl From<BfWarning> for LeavingWarning {
    fn from(v: BfWarning) -> Self { Self { shown: v.shown, value: v.value, rate: v.rate, tick: v.tick } }
}
impl From<LeavingWarning> for BfWarning {
    fn from(v: LeavingWarning) -> Self { Self { shown: v.shown, value: v.value, rate: v.rate, tick: v.tick } }
}

#[no_mangle]
pub extern "C" fn battlefield_words(which: u32) -> u32 { (match which { 0 => BfInput::WORDS, 1 => BfResult::WORDS, 2 => BfConfig::WORDS, _ => 0 }) as u32 }

#[no_mangle]
pub extern "C" fn battlefield_config() -> u32 {
    let c = { let (row, heap) = sim().calls.args(); BfConfig::take(row, heap) };
    let edges: Vec<_> = c.polygons.into_iter().map(|p| p.edges).collect();
    FIELDS.with(|fields| {
        let mut fields = fields.borrow_mut();
        let index = fields.len();
        fields.push(Rc::new(Battlefield { boundary: Boundary::from_edges(&edges) }));
        index as u32
    })
}

#[no_mangle]
pub extern "C" fn battlefield_run() -> *const f64 {
    let input = { let (row, heap) = sim().calls.args(); BfInput::take(row, heap) };
    let field = FIELDS.with(|fields| fields.borrow()[input.index as usize].clone());
    let mut result = BfResult::default();
    match input.op {
        BfOperation::Outside => result.flag = field.outside(&input.position),
        BfOperation::ControlsOutside => result.flag = field.controls_outside(input.present, &input.position, input.droid.as_ref()),
        BfOperation::Update => {
            let mut s = Store::default();
            swap(&mut s);
            let mut world = JsWorld { characters: input.characters };
            field.update(input.dt, &mut s, &mut world);
            result.countdowns = world.countdowns(&s);
            swap(&mut s);
        }
        BfOperation::Warning | BfOperation::Alpha => {
            let mut warning: LeavingWarning = input.warning.expect("the page's warning").into();
            if input.op == BfOperation::Warning { result.flag = warning.update(input.tick, input.outside); }
            result.alpha = warning.alpha();
            result.warning = Some(warning.into());
        }
    }
    let sim = sim();
    sim.calls.result(BfResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}
