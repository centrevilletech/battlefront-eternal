//! flight/ai.js reads rounds from the canonical ordnance sequence. A dead
//! guided round stays in that sequence until its owner reaps it; it must not
//! trigger a pilot's evasive turn during that intervening observation.

use super::rows::row_store;
use crate::gen::Store;
use three::Vector3;
pub fn incoming(store:&Store,target_id:f64,position:&Vector3,range:f64)->bool {
    store.ordnance.rounds.iter().any(|reference| {
        let Some(round)=store.round.get(*reference) else{return false};
        !round.dead&&round.target_id==Some(target_id)
            &&Vector3::new(round.position[0],round.position[1],round.position[2]).distance_to(position)<range
    })
}

mod live {
    use super::*;
    use crate::vehicles::BodyId;
    use crate::{ai::{owned::{arsenals::Mounted,entities::Entity,rows::Rows,seats::Source,weapons::Changes},vehicles::world::{Obj,PilotEnv,Request,Stick}},flight::ai::{self,PilotWorld},vehicles::{module::{self as fleet,f2},Hit}};
    struct Native<'a> {w:&'a mut crate::world::owner::World}
    impl PilotWorld for Native<'_> {
        fn cast(&mut self,origin:&Vector3,direction:&Vector3,far:f64,ignore:Option<BodyId>)->Option<Hit> {
            fleet::collision_owner(&mut self.w.fleet).expect("native flight collision world")
                .cast_in(&mut self.w.collision.state,&*self.w.movement.flags_mut(),origin,direction,far,ignore)?
        }
        fn terrain(&mut self,x:f64,z:f64)->Option<f64> {
            fleet::collision_owner(&mut self.w.fleet).expect("native flight collision world").ground(&mut self.w.collision.state,x,z,10000.).map(|hit|hit.height)
        }
        fn water(&mut self,x:f64,z:f64)->Option<f64> {
            let map=fleet::map_ref(&self.w.fleet).expect("native flight map rules");
            map.water.as_ref().filter(|water|water.over(x,z)).map(|_|map.level)
        }
        fn incoming(&mut self,target_id:f64,position:&Vector3,range:f64)->bool {super::incoming(&self.w.sim.store,target_id,position,range)}
    }
    pub fn input<C:Changes+?Sized,M:Mounted>(rows:&mut Rows<'_,C,M>,control:Obj,request:Option<&Request>,aim:Option<&Vector3>,env:&PilotEnv)->Stick {
        let Some(Entity::Control(reference))=rows.state.entities.get(control) else{return Stick::default()};
        let Some(birth)=rows.state.births.read(reference) else{return Stick::default()};
        let Source::Hull(hull,life)=birth.source else{return Stick::default()};
        let Some(body)=row_store!(rows).vehicle.get(hull).filter(|body|body.target_generation==life) else{return Stick::default()};
        let Some(lease)=row_store!(rows).seat_lease.get(birth.lease).filter(|lease|!lease.released&&lease.controllable==Some(reference)) else{return Stick::default()};
        if lease.vehicle!=Some(hull)||!body.flying {return Stick::default();}
        let target_id=body.target_id;let class=body.flight_class.unwrap_or_default();
        let steer=rows.state.controls.steer(reference).flatten().unwrap_or_else(crate::ai::vehicles::world::Steer::empty);
        let request=request.map(|request|ai::Request {valid:request.valid,steering_target:Some(request.steering_target),mode:request.mode,blend:request.blend,throttle:request.throttle.unwrap_or(f64::NAN)});
        let steer=ai::Steer {land:steer.land.unwrap_or(false),loop_:steer.loop_.unwrap_or(false),path_altitude:steer.path_altitude.unwrap_or(false)};
        // pilotInput observes the canonical flight row directly. Collision
        // probes borrow their own owner, and incoming rounds read this same
        // store; no global store exchange or solver scratch is needed.
        let Some(config)=f2::flight_config_in(rows.w,hull) else{return Stick::default()};
        let Some(body)=rows.w.sim.store.vehicle.get(hull) else{return Stick::default()};
        let Ok(pilot)=ai::Pilot::from_vehicle(body,&rows.w.sim.strings,config) else{return Stick::default()};
        let tuning=fleet::flight_tuning_in(rows.w,class);
        let env=ai::PilotEnv {has_path:env.has_path,min_fly_height:env.min_fly_height,max_fly_height:Some(env.max_fly_height),space:fleet::flight_environment_in(rows.w).space};
        let stick=pilot.pilot_input(&mut Native {w:rows.w},&env,&tuning,request.as_ref(),aim,&steer,target_id);
        Stick {forward:stick.forward,right:stick.right,turn:stick.turn,pitch:stick.pitch,up:stick.up,down:stick.down,brake:stick.brake,trick:stick.trick,immelmann:stick.immelmann,boost:stick.boost}
    }
}

pub use live::input;
