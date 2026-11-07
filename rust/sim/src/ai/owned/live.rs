//! Native owner scope for the AI phase and individual post-movement aimer.
//! Store and F2 seats are borrowed once. F4's readers acquire only the row
//! needed by an observation, so no F4 table borrow crosses recursive damage.
use super::{arsenals,rows::{Rows,BuildingOwner},roster::CharacterOwner,state::State,weapons};
use crate::{battle::{f4,f4_core,live_phase::Building,ownership},gen::{Character,F4Handle,Store},store::Ref};
struct Buildings;
impl BuildingOwner for Buildings {
    fn rows(&self)->&ownership::Rows<Building> {&f4::live().buildings}
    fn traps(&self)->Option<&ownership::Rows<crate::battle::live_phase::Trap>> {Some(&f4::live().traps)}
    fn trap_geometry(&self,target:crate::gen::RepairTarget)->Option<(crate::ai::combat::world::Sphere,three::Matrix4)> {
        crate::units::f3_props::trap_geometry(target)
    }
}
struct Characters;
impl CharacterOwner for Characters {
    fn resolve(&self,store:&Store,handle:&F4Handle)->Option<Ref<Character>> {f4_core::resolve_character(store,handle)}
    fn changed(&mut self,reference:Ref<Character>) {f4_core::mark_character(reference);}
}
pub fn with_rows<R>(w:&mut crate::world::owner::World,state:&mut State,store:&mut Store,
    run:impl FnOnce(&mut Rows<'_,weapons::F3,arsenals::Live>)->R)->R {
    with_rows_parts(&mut w.fleet,&mut w.combat,state,store,run)
}
/// Cold loaders borrow strings beside the owner parts. Hot physical callbacks
/// retain no string-table loan while they lend Store and seats onward.
pub(crate) fn with_rows_parts<R>(fleet:&mut crate::vehicles::module::Fleet,combat:&mut crate::combat::module::CombatOwner,
    state:&mut State,store:&mut Store,run:impl FnOnce(&mut Rows<'_,weapons::F3,arsenals::Live>)->R)->R {
    let definitions=crate::combat::module::defs_in(combat);
    let mut seats=std::mem::take(fleet.seats_owner());
    let buildings=Buildings;let mut characters=Characters;
    let mut changes=weapons::F3;let mut mounted=arsenals::Live;
    let mut rows=Rows {store,seats:&mut seats,buildings:&buildings,characters:&mut characters,state,
        changes:&mut changes,mounted:&mut mounted,definitions:&definitions.weapons};
    let result=run(&mut rows);
    *fleet.seats_owner()=seats;
    result
}
/// Navigation asks for these observations inside its existing owner scope.
pub fn clear_above(store:&mut Store,seats:&mut crate::vehicles::f2_seats::Seats,from:crate::ai::nav::math::Vec3,unit:&crate::gen::AiLifeHandle)->bool {
    super::sight::clear_above(store,seats,&mut super::sight::F3,&three::Vector3::new(from.x,from.y,from.z),unit)
}
pub fn seat_frame(store:&mut Store,_seats:&mut crate::vehicles::f2_seats::Seats,hull:crate::vehicles::f2_seats::HullLife,index:usize)->Option<(crate::ai::nav::math::Vec3,crate::ai::nav::math::Vec3)> {
        let origin=crate::vehicles::module::f2::seat_origin_owned(store,hull,index)?;
        let forward=crate::vehicles::module::f2::seat_forward_owned(store,hull,index)?;
        let point=|p:three::Vector3|crate::ai::nav::math::Vec3::new(p.x,p.y,p.z);
        Some((point(origin),point(forward)))
}

/// Lazy geometry shares the same canonical scope as scalar observations.
/// Each pose reader loans only the owner needed by that observation.
pub fn origin<C:weapons::Changes+?Sized,M:arsenals::Mounted>(rows:&mut Rows<'_,C,M>,unit:crate::ai::combat::world::Obj)->three::Vector3 {
    rows.origin(unit,&mut super::geometry::F2).expect("current native AI origin")
}
pub fn forward<C:weapons::Changes+?Sized,M:arsenals::Mounted>(rows:&mut Rows<'_,C,M>,unit:crate::ai::combat::world::Obj)->three::Vector3 {
    rows.forward(unit,&mut super::geometry::F2).expect("current native AI forward")
}
pub fn sphere<C:weapons::Changes+?Sized,M:arsenals::Mounted>(rows:&mut Rows<'_,C,M>,unit:crate::ai::combat::world::Obj)->crate::ai::combat::world::Sphere {
    rows.sphere(unit,&mut super::observe::F3).expect("current native AI sphere")
}
pub fn query<C:weapons::Changes+?Sized,M:arsenals::Mounted>(rows:&mut Rows<'_,C,M>,center:&three::Vector3,radius:f64,filter:crate::ai::combat::world::QueryFilter)->super::scratch::List<crate::ai::combat::world::Obj> {
    rows.state.queries.query_live(rows.store,rows.seats,&rows.state.entities,center,radius,filter)
}
pub fn living<C:weapons::Changes+?Sized,M:arsenals::Mounted>(rows:&mut Rows<'_,C,M>)->super::scratch::List<crate::ai::combat::world::Obj> {
    rows.state.queries.living_live(rows.store,rows.seats,&rows.state.entities)
}
pub fn los<C:weapons::Changes+?Sized,M:arsenals::Mounted>(rows:&mut Rows<'_,C,M>,from:&three::Vector3,to:&three::Vector3,a:Option<crate::ai::combat::world::Obj>,b:Option<crate::ai::combat::world::Obj>)->bool {
    rows.state.sight.los(rows.store,rows.seats,&rows.state.entities,&rows.state.births,&rows.state.queries,&mut super::sight::F3,from,to,a,b)
}
