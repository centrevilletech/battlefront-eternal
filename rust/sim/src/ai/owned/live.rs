//! Native owner scope for the AI phase and individual post-movement aimer.
//! Rows retains the owning world. F4 readers borrow only during each
//! observation, so no F4 table borrow crosses recursive damage.
use super::{arsenals,rows::{Rows,BuildingOwner},roster::CharacterOwner,state::State,weapons};
use crate::{battle::{f4_core,live_phase::Building,ownership},gen::{Character,F4Handle},store::Ref};
struct Buildings;
impl BuildingOwner for Buildings {
    fn rows_in<'a>(&'a self,live:&'a crate::battle::live_phase::Live)->&'a ownership::Rows<Building> {&live.buildings}
    fn traps_in<'a>(&'a self,live:&'a crate::battle::live_phase::Live)->Option<&'a ownership::Rows<crate::battle::live_phase::Trap>> {Some(&live.traps)}
    fn trap_geometry(&self,w:&mut crate::world::owner::World,target:crate::gen::RepairTarget)->Option<(crate::ai::combat::world::Sphere,three::Matrix4)> {
        crate::units::f3_props::trap_geometry_in(w,target)
    }
}
struct Characters;
impl CharacterOwner for Characters {
    fn resolve(&self,w:super::rows::ReadContext<'_>,handle:&F4Handle)->Option<Ref<Character>> {f4_core::resolve_character_ref(&w.battle,&w.sim.store,handle)}
    fn changed_in(&mut self,w:&mut crate::world::owner::World,reference:Ref<Character>) {f4_core::mark_character_in(&mut w.battle,reference);}
}
pub fn with_rows<R>(w:&mut crate::world::owner::World,state:&mut State,
    run:impl FnOnce(&mut Rows<'_,weapons::F3,arsenals::Live>)->R)->R {
    let definitions=crate::combat::module::defs_in(&mut w.combat);
    w.fleet.seats_owner();
    let buildings=Buildings;let mut characters=Characters;
    let mut changes=weapons::F3;let mut mounted=arsenals::Live;
    run(&mut Rows {w,buildings:&buildings,characters:&mut characters,state,
        changes:&mut changes,mounted:&mut mounted,definitions:&definitions.weapons})
}
/// Navigation asks for these observations inside its existing owner scope.
pub fn clear_above(w:&mut crate::world::owner::World,from:crate::ai::nav::math::Vec3,unit:&crate::gen::AiLifeHandle)->bool {
    super::sight::clear_above(w,&mut super::sight::F3,&three::Vector3::new(from.x,from.y,from.z),unit)
}
pub fn seat_frame(w:&mut crate::world::owner::World,hull:crate::vehicles::f2_seats::HullLife,index:usize)->Option<(crate::ai::nav::math::Vec3,crate::ai::nav::math::Vec3)> {
        let origin=crate::vehicles::module::f2::seat_origin_in(w,hull,index)?;
        let forward=crate::vehicles::module::f2::seat_forward_in(w,hull,index)?;
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
    rows.state.queries.query_live(rows.w,&rows.state.entities,center,radius,filter)
}
pub fn living<C:weapons::Changes+?Sized,M:arsenals::Mounted>(rows:&mut Rows<'_,C,M>)->super::scratch::List<crate::ai::combat::world::Obj> {
    rows.state.queries.living_live(rows.w,&rows.state.entities)
}
pub fn los<C:weapons::Changes+?Sized,M:arsenals::Mounted>(rows:&mut Rows<'_,C,M>,from:&three::Vector3,to:&three::Vector3,a:Option<crate::ai::combat::world::Obj>,b:Option<crate::ai::combat::world::Obj>)->bool {
    rows.state.sight.los(rows.w,&rows.state.entities,&rows.state.births,&rows.state.queries,&mut super::sight::F3,from,to,a,b)
}
