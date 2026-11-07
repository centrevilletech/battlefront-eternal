//! JetHintService keeps the authored helper job through the combat pattern.
//! Queries borrow prepared hint scopes and consume A in the source's random
//! removal order. Launches retain the authored vector, including zero length.
use super::{hints::Hints,navigation::{Actor,HelperKey,Command,Environment,Navigation}};
use crate::{ai::{combat::world::Obj,nav::{goto::GotoMode,jet_hints::{JetHintJob,jet_tick},math::Vec3},phase::RandomState},gen::Store};

#[derive(Clone,Copy)]
struct Job {controller:Obj,actor:Actor,helper:HelperKey,value:JetHintJob}
pub struct Jets {jobs:Vec<Option<Job>>}
impl Jets {
    pub fn new(identities:usize)->Self {Self {jobs:vec![None;identities]}}
    pub fn find(&self,nav:&Navigation,hints:&Hints,unit:Obj,position:Vec3,now:f64,random:&mut RandomState)->Option<usize> {
        let mut candidates=hints.query(&nav.nav,8,position,40.,30.,Some(unit),now);
        let mut count=candidates.len();let mut best=None;let mut score=f64::INFINITY;
        while count>0 {
            let i=random.a_mut().int(0.,count as f64-1.) as usize;
            let index=candidates[i];count-=1;candidates[i]=candidates[count];
            let hint=&nav.nav.hints.list[index];let dx=hint.position.x-position.x;let dz=hint.position.z-position.z;
            let next=2.25*(dx*dx+dz*dz);
            if next<score {best=Some(index);score=next;}
        }
        best
    }
    pub fn enter(&mut self,controller:Obj,actor:Option<Actor>,hint:Option<usize>,nav:&mut Navigation,
        store:&mut Store,seats:&mut crate::vehicles::f2_seats::Seats,random:&mut RandomState,env:&mut dyn Environment) {
        let (Some(actor),Some(hint))=(actor,hint) else {return};
        if nav.helper_observation(store,actor).is_none() {return;}
        let Some(helper)=nav.helper_key(actor) else{return};
        self.exit(controller,nav);
        assert!(nav.retain_helper(helper));
        let position=nav.nav.hints.list[hint].position;
        self.jobs[controller as u32 as usize]=Some(Job {controller,actor,helper,value:JetHintJob {helper:actor.0,hint,state:2.,started:0.}});
        nav.command_for(store,seats,random,env,helper,Command::Goto(position,GotoMode::Route));
    }
    pub fn tick(&mut self,controller:Obj,nav:&mut Navigation,store:&mut Store,seats:&mut crate::vehicles::f2_seats::Seats,random:&mut RandomState,env:&mut dyn Environment)->bool {
        let Some(job)=self.jobs.get_mut(controller as u32 as usize).and_then(Option::as_mut).filter(|j|j.controller==controller) else {return true};
        let Some(observation)=nav.helper_observation_for(store,job.helper) else {return true};
        let jetting=nav.control(job.actor).is_some_and(|c|c.jetting);
        let status=observation.status!=0.&&!observation.status.is_nan();
        let result=jet_tick(&mut job.value,env.time(),observation.arrived,status||observation.failed,jetting);
        if result.launch {
            let Some(position)=nav.position(store,job.actor) else {return true};
            let forward=nav.nav.hints.list[job.value.hint].forward;
            let point=Vec3::new(position.x+forward.x*80.,position.y+forward.y*80.,position.z+forward.z*80.);
            nav.command_for(store,seats,random,env,job.helper,Command::Goto(point,GotoMode::Route));
            nav.command_for(store,seats,random,env,job.helper,Command::Jet(forward));
        }
        result.done
    }
    pub fn exit(&mut self,controller:Obj,nav:&mut Navigation) {
        if let Some(slot)=self.jobs.get_mut(controller as u32 as usize) {
            if slot.is_some_and(|j|j.controller==controller) {nav.release_helper(slot.take().unwrap().helper);}
        }
    }
}
