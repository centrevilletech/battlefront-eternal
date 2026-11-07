//! The vehicle agent base observes the active Core and canonical frame.
//! Its helper and machine are retained objects; posts and hints come from
//! reserved ordered buffers instead of a host service or copied world view.
use super::{arsenals::Mounted,weapons::Changes,rows::Rows,vehicle_frame::{Frame,Hooks},active::Active,world::{Services,World},scratch::{List,Inline}};
use crate::ai::{combat::binding::CombatAi,phase::RandomState,vehicles::{agents::AgentHost,world::{Obj,HintView}},nav::math::Vec3};
use three::Vector3;
pub trait AgentHooks<C:Changes+?Sized,M:Mounted>:Hooks<C,M> {
    fn update_target(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,agent:Obj);
    fn aim(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,agent:Obj,stance:f64);
    fn can_engage(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,agent:Obj,target:Obj)->bool;
    fn helper_failed(&self,rows:&Rows<'_,C,M>,agent:Obj)->bool;
}
fn index<C:Changes+?Sized,M:Mounted>(rows:&Rows<'_,C,M>,agent:Obj)->usize {rows.state.agents.get(&rows.state.entities,agent).expect("running native vehicle agent").core}
impl<C:Changes+?Sized,M:Mounted,S:Services<C,M>> AgentHooks<C,M> for Active<'_,S> {
    fn update_target(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,agent:Obj) {
        let index=index(rows,agent);let mut world=World {rows,random,combat,services:self.services,settings:self.settings,facts:self.facts};
        self.core.update_combat_target(&mut world,index);
    }
    fn aim(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,agent:Obj,stance:f64) {
        let index=index(rows,agent);let mut world=World {rows,random,combat,services:self.services,settings:self.settings,facts:self.facts};
        self.core.aim(&mut world,index,stance,None);
        // Agent.aim changes the shared look after its final combat query.
        // A later foreign-agent writeback must observe that accepted look.
        self.core.feed_combat(&mut world,index);
    }
    fn can_engage(&mut self,rows:&mut Rows<'_,C,M>,random:&mut RandomState,combat:&mut CombatAi,agent:Obj,target:Obj)->bool {
        let index=index(rows,agent);let mut world=World {rows,random,combat,services:self.services,settings:self.settings,facts:self.facts};
        self.core.can_engage(&mut world,index,target)
    }
    fn helper_failed(&self,rows:&Rows<'_,C,M>,agent:Obj)->bool {
        let helper=self.helper(rows,agent);
        match helper.kind {None=>false,Some(crate::ai::control::world::HelperKind::Direct)=>helper.direct.failed,_=>helper.obs.failed}
    }
}
impl<C:Changes+?Sized,M:Mounted,H:AgentHooks<C,M>> AgentHost for Frame<'_,'_,C,M,H> {
    fn interrupts_tick(&mut self,agent:Obj)->bool {super::frame::Hooks::interrupts_tick(self.hooks,self.rows,self.random,agent,self.combat)}
    fn update_combat_target(&mut self,agent:Obj) {self.hooks.update_target(self.rows,self.random,self.combat,agent);}
    fn aim(&mut self,agent:Obj,stance:f64) {AgentHooks::aim(self.hooks,self.rows,self.random,self.combat,agent,stance);}
    fn tick_helper(&mut self,agent:Obj,dt:f64) {super::frame::Hooks::helper_tick(self.hooks,self.rows,self.random,agent,dt);}
    fn can_engage(&mut self,agent:Obj,target:Obj)->bool {self.hooks.can_engage(self.rows,self.random,self.combat,agent,target)}
    fn has_helper(&self,agent:Obj)->bool {super::frame::Hooks::has_helper(self.hooks,self.rows,agent)}
    fn helper_arrived(&mut self,agent:Obj)->bool {super::frame::Hooks::helper_arrived(self.hooks,self.rows,self.random,agent)}
    fn helper_failed(&mut self,agent:Obj)->bool {self.hooks.helper_failed(self.rows,agent)}
    fn posts(&self)->List<Obj> {
        let mut posts=self.rows.state.buffers.posts.take();
        for i in 0..self.rows.state.population.len() {
            let post=self.rows.state.population.post(i).expect("prepared native vehicle post");
            // UnitFlyAgent.pickPost reads CommandPosts.posts, which excludes
            // mobile spawn hulls even though objective selection includes them.
            if !matches!(post,crate::gen::SpawnPost::Post(_)) {continue;}
            posts.push_reserved(self.rows.state.entities.find(super::objects::post_entity(post)).expect("native vehicle post identity"));
        }
        posts
    }
    fn query_hints(&mut self,center:&Vector3,requester:Obj,now:f64)->Option<Inline<HintView,1>> {
        if !self.navigation {return None;}
        let hints=self.rows.state.hints.as_ref().expect("prepared native hints");
        let nav=&self.rows.state.navigation.nav;
        let found=hints.query(nav,32,Vec3::new(center.x,center.y,center.z),80.,1.,Some(requester),now);
        let mut out=Inline::default();
        for index in found {
            let hint=&nav.hints.list[index];let row=hints.read(nav,index);
            let point=|p:Vec3|Vector3::new(p.x,p.y,p.z);
            let post=row.post.and_then(|i|self.rows.state.population.post(i)).and_then(|p|self.rows.state.entities.find(super::objects::post_entity(p)));
            out.push(HintView {obj:self.rows.state.entities.find(super::entities::Entity::Hint(index as u32)).expect("native landing hint identity"),position:point(hint.position),forward:point(hint.forward),up:Some(point(hint.up)),command_post:post});
        }
        Some(out)
    }
}
