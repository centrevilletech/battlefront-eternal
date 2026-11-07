//! SeatUnit mirrors carry structural identities only. Steering remains in
//! the native holder consumed by F2; the page needs the lease and controller
//! to resolve captured references, never a second live intent.
use crate::{gen::AiControlView,seats::lease::control_key,vehicles::f2_seats::Seats};
use super::{entities::Entity,state::State};
// Structural identities must cross before a lifecycle callback can resolve
// them. Ordinary steering never changes these identity links.
#[derive(Clone,Copy,PartialEq)]
struct Links {
    control:crate::store::Ref<crate::gen::SeatUnit>,
    lease:crate::store::Ref<crate::gen::SeatLease>,
    controller:Option<crate::store::Ref<crate::gen::Controller>>,
}
#[derive(Default)]
pub struct Publication {
    last:Vec<[f64;AiControlView::WORDS]>,
    dirty:Vec<bool>,
    links:Vec<Option<Links>>,
    words:Vec<f64>,
    version:u32,ack:[f64;1],count:u32,
}
impl Publication {
    pub fn new(capacity:usize)->Self {
        Self {last:vec![[0.;AiControlView::WORDS];capacity],dirty:vec![false;capacity],
            links:vec![None;capacity],words:Vec::with_capacity(capacity*AiControlView::WORDS),..Default::default()}
    }
    pub fn version(&self)->u32 {self.version}
    pub fn word_count(&self)->u32 {self.count*(AiControlView::WORDS as u32)}
    pub fn acknowledge(&mut self,version:u32) {self.ack[0]=version as f64;}
    pub fn ack_pointer(&mut self)->u32 {self.ack.as_mut_ptr() as usize as u32}
    /// Call only after every recursive Core loan has returned. Released
    /// controls whose controller has unbound can leave the native pool; weak targets resolve
    /// dead rather than accidentally following the next storage generation.
    pub fn publish(&mut self,state:&mut State,store:&crate::gen::Store,seats:&mut Seats,buildings:&crate::battle::ownership::Rows<crate::battle::live_phase::Building>)->[u32;4] {
        if self.count!=0&&self.ack[0]==self.version as f64 {self.dirty.fill(false);self.words.clear();self.count=0;}
        let mut changed=false;
        for slot in 0..state.births.slots().min(self.last.len() as u32) {
            let Some((reference,birth))=state.births.at(slot) else{continue};
            let holder=match birth.source {
                super::seats::Source::Hull(reference,life)=>store.vehicle.get(reference).is_some_and(|hull|hull.target_generation==life && hull.alive),
                super::seats::Source::Turret(reference,life)=>state.turrets.life(seats,buildings,reference)==Some(life),
            };
            let active=holder && store.seat_lease.get(birth.lease).is_some_and(|lease|
                !lease.released && lease.controllable==Some(reference) && lease.index==birth.index as f64 && crate::seats::lease::live(lease,birth.lease,&store.unit));
            // A live qualified lease retains its F2 control. Only released
            // controls need a table probe and the deferred Core unbind read.
            if !active {
                let key=control_key(reference);
                let present=match birth.source {
                    super::seats::Source::Hull(..)=>seats.hulls.controls.get(&key).is_some(),
                    super::seats::Source::Turret(..)=>seats.crews.controls.get(&key).is_some(),
                };
                if !present {continue;}
            }
            let controller_object=state.controls.controller_key(reference).or_else(||state.turrets.controller_key(reference));
            let controller=active.then_some(controller_object).flatten()
                .and_then(|object|if let Some(Entity::Controller(reference))=state.entities.get(object) {Some(reference)}else{None});
            let at=slot as usize;let links=Links {control:reference,lease:birth.lease,controller};
            if self.links[at]!=Some(links) {
                let row=AiControlView {control:reference,lease:birth.lease,controller};
                row.put(&mut self.last[at],&mut Vec::new());
                self.links[at]=Some(links);self.dirty[at]=true;changed=true;
            }
            // F2 release may precede the next Core sync. Keep its qualified
            // object until Core::unbind has disposed the old agent and nav
            // helper; both still observe the captured lease during cleanup.
            if !active {
                let bound=controller_object.and_then(|controller|state.controllers.read(&state.entities,controller))
                    .is_some_and(|controller|controller.unit==state.entities.find(Entity::Control(reference)));
                if !bound {
                    state.controls.retire(reference);state.turrets.retire_control(reference);
                    state.births.retire(seats,&mut state.entities,reference);
                }
            }
        }
        // Rebuild the changed prefix only when a new value arrives. Repeated
        // mirror barriers keep the same pointer, version and unread prefix.
        if changed {
            self.words.clear();self.count=0;
            for slot in 0..self.last.len() {if self.dirty[slot] {self.words.extend_from_slice(&self.last[slot]);self.count+=1;}}
            self.version=self.version.wrapping_add(1);
        }
        [self.words.as_ptr() as usize as u32,self.count,self.version,0]
    }
}
