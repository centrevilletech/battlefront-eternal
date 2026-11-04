//! Optional page debris commands. There is no authoritative tick dispatch.
use super::debris::{Hit, Piece, World};
use super::debris_commands::{a3, a4, v3};
use crate::abi::sim;
use crate::gen::{import_db_action, DbAction, DbActionKind, DbInput, DbState};
use crate::store::StrId;
use three::{Quaternion, Vector3};

struct JsWorld;
fn action(kind: DbActionKind) -> DbAction { DbAction { kind, ..DbAction::default() } }
impl World for JsWorld {
    fn add(&mut self, id: u32, p: &Vector3, q: &Quaternion, shift: Option<&Vector3>) {
        import_db_action(&DbAction { id: id as f64, point: a3(*p), turn: a4(*q), shift: shift.copied().map(a3), ..action(DbActionKind::Add) });
    }
    fn publish(&mut self, p: &Piece) { import_db_action(&DbAction { piece: Some(p.into()), ..action(DbActionKind::Publish) }); }
    fn out(&mut self, p: &Vector3) { import_db_action(&DbAction { point: a3(*p), ..action(DbActionKind::Out) }); }
    fn vectors(&mut self, ray: &Vector3, start: &Vector3) { import_db_action(&DbAction { point: a3(*start), direction: a3(*ray), ..action(DbActionKind::Vectors) }); }
    fn remove(&mut self, p: &Piece) { import_db_action(&DbAction { piece: Some(p.into()), ..action(DbActionKind::Remove) }); }
    fn cast(&mut self, start: &Vector3, ray: &Vector3, far: f64, ignore: Option<u32>) -> Option<Hit> {
        let r = import_db_action(&DbAction { point: a3(*start), direction: a3(*ray), far, ignore: ignore.map(f64::from), ..action(DbActionKind::Cast) });
        r.hit.then(|| Hit { point: v3(r.point), normal: v3(r.normal) })
    }
    fn low(&mut self, p: &Piece, q: &Quaternion) -> f64 {
        import_db_action(&DbAction { piece: Some(p.into()), turn: a4(*q), ..action(DbActionKind::Low) }).low
    }
    fn play(&mut self, piece: &Piece, name: StrId, p: &Vector3, velocity: Option<&Vector3>, impact: bool) -> Option<u32> {
        import_db_action(&DbAction { piece: Some(piece.into()), impact, name: Some(name), point: a3(*p), velocity: velocity.copied().map(a3), ..action(DbActionKind::Play) }).effect.map(|n| n as u32)
    }
    fn move_effect(&mut self, p: &Piece, id: u32, velocity: bool) {
        import_db_action(&DbAction { id: id as f64, piece: Some(p.into()), point: a3(p.position), velocity: velocity.then_some(a3(p.velocity)), ..action(DbActionKind::Move) });
    }
    fn stop_effect(&mut self, id: u32) { import_db_action(&DbAction { id: id as f64, ..action(DbActionKind::Stop) }); }
}
#[no_mangle]
pub extern "C" fn debris_words(which: u32) -> u32 { (match which { 0 => DbInput::WORDS, 1 => DbState::WORDS, _ => 0 }) as u32 }
#[no_mangle]
pub extern "C" fn debris_run() -> *const f64 {
    let (row, heap) = sim().calls.args(); let input = DbInput::take(row, heap);
    let result = super::debris_commands::run(input, &mut JsWorld);
    let sim = sim(); sim.calls.result(DbState::WORDS, |row, heap| result.put(row, heap)); sim.calls.block.ptr()
}
