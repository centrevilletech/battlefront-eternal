//! The canonical row fixture disables effects outside its prepared hull.
use super::canonical_combat_fixture::Callbacks;
use bfe_sim::{ai::{owned::{vehicle_frame::Hooks,rows::Rows,weapons::Changes,arsenals::Mounted},vehicles::world::*,combat::binding::CombatAi,phase::RandomState},vehicles::{BodyId,Hit}};
use three::Vector3;
#[allow(unused_variables)]
impl<C:Changes+?Sized,M:Mounted> Hooks<C,M> for Callbacks {
fn support_holds(&self,rows:&Rows<'_,C,M>, agent: Obj) -> Option<f64> {panic!("unprepared native vehicle frame effect support_holds") }
fn cast(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, origin: &Vector3, direction: &Vector3, far: f64, ignore: Option<BodyId>) -> Option<Hit> {panic!("unprepared native vehicle frame effect cast") }
fn mount_origin(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, origin: Origin) -> Vector3 {panic!("unprepared native vehicle frame effect mount_origin") }
fn eligible_seat(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> f64 {panic!("unprepared native vehicle frame effect eligible_seat") }
fn board(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, hull: Obj) -> bool {panic!("unprepared native vehicle frame effect board") }
fn man(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Obj, turret: Obj) -> bool {panic!("unprepared native vehicle frame effect man") }
fn request_exit_vehicle(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool {panic!("unprepared native vehicle frame effect request_exit_vehicle") }
fn request_exit_turret(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool {panic!("unprepared native vehicle frame effect request_exit_turret") }
fn unboard(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, unit: Option<Obj>, options: ExitOptions) -> bool {panic!("unprepared native vehicle frame effect unboard") }
fn damage(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, hull: Obj, amount: f64) {panic!("unprepared native vehicle frame effect damage") }
fn pilot_input(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, control: Obj, request: Option<&Request>, aim: Option<&Vector3>, dt: f64, env: &PilotEnv) -> Stick {panic!("unprepared native vehicle frame effect pilot_input") }
fn nav_attach(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>, unit: Obj) {panic!("unprepared native vehicle frame effect nav_attach") }
fn nav_update(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>, dt: f64) {panic!("unprepared native vehicle frame effect nav_update") }
fn nav_request(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Option<Obj>) -> Option<Request> {panic!("unprepared native vehicle frame effect nav_request") }
fn request_objective(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, exclude: bool) {panic!("unprepared native vehicle frame effect request_objective") }
fn controller_request_exit(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) {panic!("unprepared native vehicle frame effect controller_request_exit") }
fn set_reboard_at(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, t: f64) {panic!("unprepared native vehicle frame effect set_reboard_at") }
fn fire_a(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) {panic!("unprepared native vehicle frame effect fire_a") }
fn invalidate_request(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj) {panic!("unprepared native vehicle frame effect invalidate_request") }
fn add_vehicle_note(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, controller: Obj, note: Ref) {panic!("unprepared native vehicle frame effect add_vehicle_note") }
fn set_objective(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, character: Obj, post: Obj) {panic!("unprepared native vehicle frame effect set_objective") }
fn set_support_holds(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, holds: f64) {panic!("unprepared native vehicle frame effect set_support_holds") }
fn set_patrol_expires(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, patrol: Obj, t: f64) {panic!("unprepared native vehicle frame effect set_patrol_expires") }
fn patrol_request_state(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, patrol: Obj, state: f64) {panic!("unprepared native vehicle frame effect patrol_request_state") }
fn face(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, patterns: Obj, posture: f64) {panic!("unprepared native vehicle frame effect face") }
fn helper_goto(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, point: &Vector3, direct: bool) {panic!("unprepared native vehicle frame effect helper_goto") }
fn helper_stop(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj) {panic!("unprepared native vehicle frame effect helper_stop") }
fn helper_follow_path(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, points: &[[f64; 3]], over: bool, altitude: bool) {panic!("unprepared native vehicle frame effect helper_follow_path") }
fn helper_land(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi, agent: Obj, on: bool) {panic!("unprepared native vehicle frame effect helper_land") }
}
