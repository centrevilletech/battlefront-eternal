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
pub fn clear_above_unit(w:&mut crate::world::owner::World,from:crate::ai::nav::math::Vec3,unit:Ref<crate::gen::Unit>)->bool {
    super::sight::clear_above_unit(w,&mut super::sight::F3,&three::Vector3::new(from.x,from.y,from.z),unit)
}
pub(crate) fn collision_snapshot(w:&mut crate::world::owner::World)->Option<((u64,u64), &[u32], &[u8])>{
    let id=crate::units::f3::collision_world_in(&w.units);
    let state=&w.collision.state;
    let scene=state.worlds.get(id)?;
    let count=scene.order.len().min(crate::collision::MAX_BODIES);
    Some(((scene.version,state.fixed_version),&scene.order[..count],&w.movement.flags_mut()[..count]))
}
pub(crate) fn observe_navigation(w:&mut crate::world::owner::World,nav:&mut crate::ai::nav::route::MapNavigation)->bool {
    let id=crate::units::f3::collision_world_in(&w.units);
    let geometry=&w.collision.state;
    let Some(scene)=geometry.worlds.get(id) else {return false};
    let count=scene.order.len().min(crate::collision::MAX_BODIES);
    let order=&scene.order[..count];
    let flags=&w.movement.flags_mut()[..count];
    nav.observe_collision_classified(
        (scene.version,geometry.fixed_version),
        order,
        flags,
        |body| geometry.bodies.get(body as usize).is_some_and(|b| b.moving),
        |i| Some(geometry.bodies.get(order[i] as usize)?.bounds),
    )
}
pub(crate) fn supported_floor(w:&mut crate::world::owner::World, p:crate::ai::nav::math::Vec3, above:f64, below:f64)->Option<crate::ai::nav::math::Vec3> {
    super::walk::support(w,p,above,below)
}
/// Navigation already reserved the mover work for this segment.
pub fn crosses(w: &mut crate::world::owner::World, a: crate::ai::nav::math::Vec3, b: crate::ai::nav::math::Vec3, movement:u8) -> bool {
    try_crosses(w,a,b,movement,crate::ai::nav::swbf2::ProbeKind::Turn).unwrap_or(false)
}
/// Ground clearance within the tick's work. `None` is neither clear nor
/// blocked: the tick had no work left for it.
pub(crate) fn try_crosses(
    w: &mut crate::world::owner::World,
    a: crate::ai::nav::math::Vec3,
    b: crate::ai::nav::math::Vec3,
    movement: u8,
    kind: crate::ai::nav::swbf2::ProbeKind,
) -> Option<bool> {
    super::walk::try_crosses(w, a, b, movement, kind)
}
/// Ground clearance for a caller that has already reserved the physical work.
pub fn line_of_walk(w: &mut crate::world::owner::World, from: crate::ai::nav::math::Vec3, to: crate::ai::nav::math::Vec3) -> bool {
    crosses(w, from, to, 0)
}
pub use super::walk::line_of_walk as direct_walk_clear;
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
