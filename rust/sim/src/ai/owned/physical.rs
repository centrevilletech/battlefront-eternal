//! VehicleAI's immediate physical actions use canonical F2 commands. Their
//! arguments are resolved before lending the owners onward, so recursive
//! boarding and death callbacks cannot keep a stale body or seat borrow.
use super::rows::{row_buildings,row_seats,row_store};

use super::rows::row_store_mut;
use super::{arsenals::Mounted,entities::Entity,rows::Rows,weapons::Changes};
use crate::{ai::vehicles::world::{ExitOptions,Obj},gen::{F2SeatCommand,F2SeatOp,SeatReason}};
#[derive(Clone,Copy)]
pub enum SeatAction {
    Board {unit:Obj,hull:Obj},
    Man {unit:Obj,turret:Obj},
    RequestVehicle {unit:Option<Obj>,options:ExitOptions},
    RequestTurret {unit:Option<Obj>,options:ExitOptions},
    Unboard {unit:Option<Obj>,options:ExitOptions},
}
/// The source passes the physical soldier to F2, never its planner proxy.
/// Missing identities reject without redirecting to a replacement occupant.
pub fn seat_command<C:Changes+?Sized,M:Mounted>(rows:&Rows<'_,C,M>,action:SeatAction)->Option<F2SeatCommand> {
    let (object,op,options)=match action {
        SeatAction::Board {unit,..}=>(Some(unit),F2SeatOp::Board,ExitOptions::default()),
        SeatAction::Man {unit,..}=>(Some(unit),F2SeatOp::Man,ExitOptions::default()),
        SeatAction::RequestVehicle {unit,options}=>(unit,F2SeatOp::RequestExit,options),
        SeatAction::RequestTurret {unit,options}=>(unit,F2SeatOp::TurretRequestExit,options),
        SeatAction::Unboard {unit,options}=>(unit,F2SeatOp::Unboard,options),
    };
    let Entity::Unit(reference)=rows.state.entities.get(object?)? else{return None};
    let unit=row_store!(rows).unit.get(reference)?;
    let mut out=F2SeatCommand {op,unit:Some(reference),unit_life:unit.generation,
        placement:options.placement.unwrap_or(true),promote:options.promote,reason:SeatReason::Exit,index:-1.,..Default::default()};
    if let Some(Some(post))=options.post {
        let Entity::Post(post)=rows.state.entities.get(post)? else{return None};
        row_store!(rows).post.get(post)?;out.post=Some(post);
    }
    match action {
        SeatAction::Board {hull,..}=>{
            let Entity::Hull(hull)=rows.state.entities.get(hull)? else{return None};
            out.hull=Some(hull);out.hull_life=row_store!(rows).vehicle.get(hull)?.target_generation;
        },
        SeatAction::Man {turret,..}=>{
            let Entity::Turret(turret)=rows.state.entities.get(turret)? else{return None};
            let life=rows.state.turrets.life(row_seats!(rows),row_buildings!(rows),turret)?;
            out.index=row_seats!(rows).crews.turrets.iter().position(|t|t.turret==turret)? as f64;
            out.hull_life=life;
        },
        SeatAction::RequestVehicle {..}|SeatAction::RequestTurret {..}|SeatAction::Unboard {..}=>{
            let lease=unit.seat?;let current=row_store!(rows).seat_lease.get(lease)?;
            if current.released||current.unit!=Some(reference)||current.generation!=unit.generation {return None;}
            out.lease=Some(lease);
            if current.turret.is_some()&&matches!(action,SeatAction::Unboard {..}) {out.op=F2SeatOp::TurretLeave;}
        },
    }
    Some(out)
}

pub fn seat<F:super::callbacks::Factory>(factory:&F,core:&mut crate::ai::control::Core,
    rows:&mut Rows<'_,super::weapons::F3,super::arsenals::Live>,random:&mut crate::ai::phase::RandomState,
    combat:&mut crate::ai::combat::binding::CombatAi,settings:&super::frame::Settings,facts:&super::world::Facts,
    recon:&mut crate::recon::Authority,queries:&mut crate::battle::live::Queries,action:SeatAction)->bool {
    let Some(command)=seat_command(rows,action) else{return false};
    super::callbacks::with_services(factory,core,rows,random,combat,settings,facts,recon,queries,
        |w,services|crate::vehicles::module::f2::live_seats::ai_command_in(w,command,services).accepted)
}

pub fn damage<F:super::callbacks::Factory>(factory:&F,core:&mut crate::ai::control::Core,
    rows:&mut Rows<'_,super::weapons::F3,super::arsenals::Live>,random:&mut crate::ai::phase::RandomState,
    combat:&mut crate::ai::combat::binding::CombatAi,settings:&super::frame::Settings,facts:&super::world::Facts,
    recon:&mut crate::recon::Authority,queries:&mut crate::battle::live::Queries,hull:Obj,amount:f64) {
    let Some(Entity::Hull(reference))=rows.state.entities.get(hull) else{return};
    if !row_store_mut!(rows).vehicle.contains(reference) {return;}
    // VehicleAI decay uses Vehicle.damage with its default null attacker.
    // F3's impact owner keeps crew deaths and F4 hooks in that same call.
    super::callbacks::with_services(factory,core,rows,random,combat,settings,facts,recon,queries,
        |w,services|crate::units::f3_hulls::mutate_in(w,reference,amount,Some(crate::gen::OrdnanceOwner::Null),services));
}

/// Vehicles.eligibleSeat keeps F2's admission and team checks in source
/// order. Native AI lends only a relation reader to the shared seat chooser.
pub fn eligible<C:Changes+?Sized,M:Mounted>(rows:&Rows<'_,C,M>,unit:Obj,hull:Obj,relation:&mut impl FnMut(f64,f64)->f64)->f64 {
    let Some(Entity::Unit(unit))=rows.state.entities.get(unit) else{return -1.};
    let Some(Entity::Hull(hull))=rows.state.entities.get(hull) else{return -1.};
    let Some(body)=row_store!(rows).unit.get(unit) else{return -1.};
    let network=crate::seats::lease::network_human(body);
    if body.human&&!network {return -1.;}
    let no_enter=row_seats!(rows).hulls.riders.get(&unit.slot).map_or(0.,|facts|facts.no_enter);
    if !crate::seats::boarding::can_board(body,network,no_enter) {return -1.;}
    let Some(vehicle)=row_store!(rows).vehicle.get(hull) else{return -1.};
    let Some(side)=row_seats!(rows).hulls.hulls.iter().find(|side|side.vehicle==hull) else{return -1.};
    crate::seats::boarding::boarding_seat_with(relation,row_store!(rows),vehicle,side,body.team,network,-1) as f64
}
