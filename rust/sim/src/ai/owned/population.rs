//! Objective requests on canonical Characters, hull crews and command posts.
//! Mission weights are immutable policies; no per-tick roster/post copy exists.
use crate::gen::{AiPostPolicy, Character, F4Handle, PostRef, SpawnPost, Store, Vehicle};
use crate::store::Ref;
use super::scratch::List;
use crate::battle::{commands::Commands, posts::{post_ref,spawn_post}};
use super::super::{control::{goals::{ObjectivePost,ObjectiveScratch,select_objective_rows},world::{Draws,TeamTable}},nav::math::Vec3};

pub trait Facts {
    fn building_alive(&self, building: Ref<crate::gen::Building>) -> bool;
    fn vehicle_mask(&self, vehicle: Ref<Vehicle>) -> f64;
}
/// Registry queries preserve mixed-member order and perform their original
/// lazy sphere test. Entries without a body controller still consume the
/// first-ten budget before recruitment rejects them.
pub trait Recruitment {
    fn query(&mut self,store:&mut Store,center:Vec3,radius:f64,team:f64,teams:&TeamTable)->List<crate::gen::Member>;
    fn mask(&self,unit:&crate::gen::Unit)->f64;
}
#[derive(Clone,Copy,Debug,PartialEq)]
pub struct Publication { pub character: Ref<Character>, pub before: PostRef, pub after: PostRef, pub goal: bool }
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum Stale { Character, Post }

pub struct Population {
    policies: Vec<AiPostPolicy>,
    scratch: ObjectiveScratch,
    publications: Vec<Publication>,
    enhanced: bool,
    threat_at: Vec<Option<f64>>,
    poll_at: f64,
    candidates: Vec<usize>, weights: Vec<f64>, distances: Vec<f64>, spawns: Vec<usize>,
}
impl Population {
    pub fn new(posts: usize, characters: usize, enhanced: bool) -> Self {
        let mut scratch=ObjectiveScratch::default();scratch.reserve(posts);
        Self { policies:Vec::with_capacity(posts),scratch,publications:Vec::with_capacity(characters.saturating_mul(characters).saturating_add(35)), enhanced,
            threat_at:Vec::with_capacity(posts),poll_at:0.,candidates:Vec::with_capacity(posts),
            weights:Vec::with_capacity(posts),distances:Vec::with_capacity(posts),spawns:Vec::with_capacity(posts) }
    }
    pub fn add(&mut self, store: &Store, policy: AiPostPolicy) -> Result<usize,Stale> {
        if !present(store,policy.post) { return Err(Stale::Post); }
        self.policies.push(policy);self.threat_at.push(None);Ok(self.policies.len()-1)
    }
    pub fn view<'a>(&'a self,store:&'a Store,index:usize,facts:&dyn Facts)->Option<ObjectivePost<'a>> {
        Some(row(store,self.policies.get(index)?,facts))
    }
    pub fn len(&self)->usize {self.policies.len()}
    pub fn is_empty(&self)->bool {self.policies.is_empty()}
    pub fn post(&self,index:usize)->Option<SpawnPost> {self.policies.get(index).map(|p|p.post)}
    pub fn index(&self, post: PostRef) -> Option<usize> {
        let post=spawn_post(post)?;self.policies.iter().position(|p| p.post==post)
    }
    pub fn publications(&self) -> &[Publication] { &self.publications }
    pub fn select(&mut self, store: &Store, character: Ref<Character>, random: &mut dyn Draws,
        teams: &TeamTable, facts: &dyn Facts, now: f64, vehicle: Option<Ref<Vehicle>>,
        exclude_previous: bool, previous: PostRef) -> Option<SpawnPost> {
        let c=store.character.get(character)?;let team=c.team;let faction=teams.faction(team);
        let previous=self.index(previous);let driver=vehicle.is_some();
        let mask=vehicle.map(|v|facts.vehicle_mask(v)).unwrap_or(0.);
        if self.enhanced {
            if let Some(index)=self.enhanced_select(store,character,random,teams,facts,now,driver,mask,exclude_previous,previous) {
                return Some(self.policies[index].post);
            }
            // Enhanced only falls back when its candidate list is empty.
            if !self.candidates.is_empty() { return None; }
        }
        let policies=&self.policies;
        let index=select_objective_rows(policies.len(),&|i| row(store,&policies[i],facts),&mut self.scratch,
            team,faction,random,&|a,b|teams.relation(a,b),driver,mask,exclude_previous,previous)?;
        Some(self.policies[index].post)
    }
    /// ClassicObjectives.request: a bot passenger inherits a bot driver's
    /// objective without drawing. A driver publishes to bot occupants in seat
    /// order; human passengers retain their own objective.
    pub fn request(&mut self, store: &mut Store, commands: &mut Commands, character: &F4Handle,
        random: &mut dyn Draws, teams: &TeamTable, facts: &dyn Facts, now:f64,
        exclude_previous:bool, previous:PostRef) -> Result<Option<SpawnPost>,Stale> {
        self.publications.clear();
        let me=commands.resolve(store,character).ok_or(Stale::Character)?;
        Ok(self.request_inner(store,commands,me,random,teams,facts,now,exclude_previous,previous))
    }
    fn request_inner(&mut self,store:&mut Store,commands:&mut Commands,me:Ref<Character>,
        random:&mut dyn Draws,teams:&TeamTable,facts:&dyn Facts,now:f64,
        exclude_previous:bool,previous:PostRef)->Option<SpawnPost> {
        let crew=crew(store,me);
        if let Some((vehicle,false))=crew {
            if let Some(driver)=occupant(store,vehicle,0) {
                if !store.character.get(driver).unwrap().human {
                    let post=spawn_post(store.character.get(driver).unwrap().objective);
                    self.publish(store,commands,me,post);return post;
                }
            }
        }
        let vehicle=crew.and_then(|(v,driver)|driver.then_some(v));
        let post=self.select(store,me,random,teams,facts,now,vehicle,exclude_previous,previous);
        self.publish(store,commands,me,post);
        if let Some(vehicle)=vehicle {
            let count=store.vehicle.get(vehicle).unwrap().seats.len();
            for index in 0..count {
                if let Some(other)=occupant(store,vehicle,index) {
                    if other!=me && !store.character.get(other).unwrap().human { self.publish(store,commands,other,post); }
                }
            }
        }
        post
    }
    fn publish(&mut self, store:&mut Store, commands:&mut Commands, character:Ref<Character>, post:Option<SpawnPost>) {
        let row=store.character.get_mut(character).unwrap();let before=row.objective;let after=post_ref(post);
        // Publishing the same non-null objective still overwrites the queued
        // goal. Dirty facade publication is independent of that gameplay effect.
        assert!(self.publications.len()<self.publications.capacity(),"AI objective effects exceed reserved roster bound");
        self.publications.push(Publication { character,before,after,goal:row.unit.is_some() && post.is_some() });
        if before!=after {
            row.objective=after;
            if !commands.dirty.contains(&character) { commands.dirty.push(character); }
        }
    }
    /// Post callbacks run after F4 changes ownership/activity, in source team
    /// order. Driver requests can change later roster entries immediately.
    pub fn owner_changed(&mut self,store:&mut Store,commands:&mut Commands,roster:&[F4Handle],
        random:&mut dyn Draws,teams:&TeamTable,facts:&dyn Facts,recruitment:&mut dyn Recruitment,
        now:f64,post:SpawnPost,old:f64,new:f64)->Result<(),Stale> {
        self.publications.clear();
        let index=self.index(post_ref(Some(post))).ok_or(Stale::Post)?;
        if !present(store,post) {return Err(Stale::Post);}
        if old==new {return Ok(());}
        for team in 1..8 {
            let team=team as f64;let before=teams.relation(team,old);let after=teams.relation(team,new);
            if (before<1. && after>0.)||(before>=1. && after<1.) {
                self.reassign(store,commands,roster,random,teams,facts,now,post,team);
                if before>0. && after==0. {self.recruit(store,commands,teams,facts,recruitment,index,team);}
            }
        }
        Ok(())
    }
    pub fn deactivated(&mut self,store:&mut Store,commands:&mut Commands,roster:&[F4Handle],
        random:&mut dyn Draws,teams:&TeamTable,facts:&dyn Facts,now:f64,post:SpawnPost)->Result<(),Stale> {
        self.publications.clear();
        if self.index(post_ref(Some(post))).is_none()||!present(store,post) {return Err(Stale::Post);}
        for team in 1..8 {self.reassign(store,commands,roster,random,teams,facts,now,post,team as f64);}
        Ok(())
    }
    fn reassign(&mut self,store:&mut Store,commands:&mut Commands,roster:&[F4Handle],
        random:&mut dyn Draws,teams:&TeamTable,facts:&dyn Facts,now:f64,post:SpawnPost,team:f64) {
        for handle in roster {
            let Some(character)=commands.resolve(store,handle) else {continue};
            let c=store.character.get(character).unwrap();
            if c.team==team && !c.human && spawn_post(c.objective)==Some(post) {
                self.request_inner(store,commands,character,random,teams,facts,now,false,PostRef::Null);
            }
        }
    }
    fn recruit(&mut self,store:&mut Store,commands:&mut Commands,teams:&TeamTable,facts:&dyn Facts,
        recruitment:&mut dyn Recruitment,index:usize,team:f64) {
        let post=self.policies[index].post;let view=row(store,&self.policies[index],facts);
        let position=view.position;let bans=v8math::to_int32(view.bans);
        let found=recruitment.query(store,position,60.,team,teams);let mut sent=0;
        for member in found.into_iter().take(10) {
            if sent>=5 {break;}
            let crate::gen::Member::Unit(reference)=member else {continue};
            let Some(u)=store.unit.get(reference) else {continue};
            if u.controller.is_none()||u.human||u.kind==5. {continue;}
            if bans&v8math::to_int32(recruitment.mask(u))!=0 {continue;}
            let Some(character)=u.character else {continue};
            if !store.character.contains(character) {continue;}
            self.publish(store,commands,character,Some(post));sent+=1;
        }
    }
    pub fn tick(&mut self, store:&Store, facts:&dyn Facts, now:f64) {
        if !self.enhanced || now<self.poll_at { return; } self.poll_at=now+1.;
        for (i,p) in self.policies.iter().enumerate() {
            let view=row(store,p,facts);
            if view.active && view.owner()!=0. && neutralize(store,p.post)>0. { self.threat_at[i]=Some(now); }
        }
    }
    fn enhanced_select(&mut self, store:&Store, character:Ref<Character>, random:&mut dyn Draws,
        teams:&TeamTable,facts:&dyn Facts,now:f64,driver:bool,mask:f64,exclude:bool,previous:Option<usize>) -> Option<usize> {
        let c=store.character.get(character)?;let team=c.team;let faction=teams.faction(team);
        self.candidates.clear();self.weights.clear();self.distances.clear();self.spawns.clear();
        for (i,p) in self.policies.iter().enumerate() {
            let view=row(store,p,facts);if !view.active { continue; }
            let owner=view.owner();
            let defend=owner!=0. && teams.relation(team,owner)>0.
                && (neutralize(store,p.post)>0. || self.threat_at[i].is_some_and(|at|now-at<8.));
            if view.friendly(team,&|a,b|teams.relation(a,b)) && !defend { continue; }
            if driver && v8math::to_int32(view.bans)&v8math::to_int32(mask)!=0 { continue; }
            if exclude && previous==Some(i) && !defend { continue; }
            let mut weight=view.weight(team,faction);
            if defend { weight=(if weight>0. {weight}else{100.})*2.; }
            if !(weight>0.) {continue;}
            if !defend && owner==0. {weight*=1.5;}
            if view.building_alive==Some(true) && teams.relation(team,owner)<0. {weight*=2.;}
            let crowd=store.character.iter().filter(|(r,c)|*r!=character && c.team==team && spawn_post(c.objective)==Some(p.post)
                && c.unit.and_then(|u|store.unit.get(u)).is_some_and(|u|u.alive)).count() as f64;
            if crowd>6. {weight/=1.+(crowd-6.)/6.;}
            self.candidates.push(i);self.weights.push(weight);
        }
        if self.candidates.is_empty() {return None;}
        let body=c.unit.and_then(|r|store.unit.get(r)).filter(|u|u.alive).map(|u| {
            u.seat.and_then(|r|store.seat_lease.get(r)).and_then(|l|l.vehicle).and_then(|v|store.vehicle.get(v))
                .map(|v|Vec3::new(v.position[0],v.position[1],v.position[2])).unwrap_or_else(||{let p=u.position();Vec3::new(p.x,p.y,p.z)})
        });
        if body.is_none() {
            for (i,p) in self.policies.iter().enumerate() {
                let view=row(store,p,facts);if view.active && view.friendly(team,&|a,b|teams.relation(a,b)) {self.spawns.push(i);}
            }
        }
        let flat=|a:Vec3,b:Vec3|v8math::hypot(a.x-b.x,a.z-b.z);
        for &i in &self.candidates {
            let pos=row(store,&self.policies[i],facts).position;
            let distance=match body {Some(body)=>flat(body,pos),None=>{
                let mut best=f64::INFINITY;
                for &spawn in &self.spawns {if spawn!=i {best=v8math::min(best,flat(row(store,&self.policies[spawn],facts).position,pos));}}
                if best.is_finite(){best}else{0.}
            }};
            self.distances.push(v8math::max(distance,20.));
        }
        let nearest=self.distances.iter().copied().fold(f64::INFINITY,v8math::min);let mut total=0.;
        for i in 0..self.candidates.len() {let ratio=nearest/self.distances[i];self.weights[i]*=ratio*ratio;total+=self.weights[i];}
        let mut draw=random.a_uniform()*total;
        for i in 0..self.candidates.len() {draw-=self.weights[i];if draw<0. {return Some(self.candidates[i]);}}
        self.candidates.last().copied()
    }
}
fn present(store:&Store, post:SpawnPost)->bool {match post {SpawnPost::Post(r)=>store.post.contains(r),SpawnPost::MobilePost(r)=>store.mobile_post.contains(r)}}
fn neutralize(store:&Store, post:SpawnPost)->f64 {match post {SpawnPost::Post(r)=>store.post.get(r).map(|p|p.neutralize).unwrap_or(0.),SpawnPost::MobilePost(_)=>0.}}
fn row<'a>(store:&'a Store, policy:&'a AiPostPolicy, facts:&dyn Facts)->ObjectivePost<'a> {
    let mut view=ObjectivePost {active:false,team:0.,position:Vec3::default(),spawn:policy.spawn,ally:policy.ally,
        class_spawn_points:policy.class_spawn_points,building_alive:None,bans:policy.bans,attack:&policy.attack,defend:&policy.defend};
    match policy.post {
        SpawnPost::Post(r)=>if let Some(post)=store.post.get(r) {
            view.active=post.active;view.team=post.team;view.position=Vec3::new(post.position[0],post.position[1],post.position[2]);
            view.building_alive=post.building.map(|r|facts.building_alive(r));
            if let Some(ai)=&post.ai {view.spawn=ai.spawn_path.as_ref().is_some_and(|p|p.present);view.ally=ai.ally_path.as_ref().is_some_and(|p|p.present);view.class_spawn_points=ai.class_spawn_points;}
        },
        SpawnPost::MobilePost(r)=>if let Some(post)=store.mobile_post.get(r) {
            view.active=post.active;view.team=post.team;view.attack=&post.ai.atk;view.defend=&post.ai.def;
            let p=post.vehicle.filter(|_|post.active).and_then(|r|store.vehicle.get(r)).map(|v|v.position).unwrap_or(post.last_position);
            view.position=Vec3::new(p[0],p[1],p[2]);view.class_spawn_points=post.ai.class_spawn_points;
            view.spawn=false;view.ally=false;
        },
    } view
}
fn crew(store:&Store, character:Ref<Character>)->Option<(Ref<Vehicle>,bool)> {
    let unit=store.character.get(character)?.unit?;let lease=store.unit.get(unit)?.seat?;let l=store.seat_lease.get(lease)?;
    if !crate::seats::lease::live(l,lease,&store.unit) {return None;}
    Some((l.vehicle?,l.index==0.))
}
fn occupant(store:&Store,vehicle:Ref<Vehicle>,index:usize)->Option<Ref<Character>> {
    let lease=store.vehicle.get(vehicle)?.seats.get(index)?.lease?;let l=store.seat_lease.get(lease)?;
    if !crate::seats::lease::live(l,lease,&store.unit) {return None;}
    l.unit.and_then(|u|store.unit.get(u)).and_then(|u|u.character).filter(|r|store.character.contains(*r))
}
