//! HumanSpawn's world actions (Game.spawnHuman/readyHuman/setHumanTeam).
//! Class availability and rosters are prepared at load. The body service
//! acquires, places or kills a native viewpoint; this module owns the choice,
//! queue, team and HumanSpawn transitions around those actions.
use super::{human::{Command,Event}, human_owned, human_posts::{Choices,Key}, spawning::{self,Draws,Scratch}, stats::Text,
    world::SpawningWorld};
use crate::gen::{Character, HumanPick, SpawnPost, Store, Unit};
use crate::store::{Ref,StrId};
use three::Vector3;

pub trait PlayerServices: SpawningWorld {
    fn floor(at:Vector3,body:Ref<crate::gen::Body>)->Option<(Vector3,Vector3)>;
    fn relation(a:f64,b:f64)->f64;
    fn local(&self)->Option<Ref<Unit>>;
    /// The low-level factory operation changes body/class/life only. The
    /// caller links the Character and emits HumanSpawn's own spawned event.
    fn spawn_player(&mut self,s:&mut Store,class:StrId,post:Option<StrId>,point:[f64;4])->Option<Ref<Unit>>;
    fn linked(&mut self,s:&mut Store,unit:Ref<Unit>,character:Option<Ref<Character>>);
    fn retarget(&mut self,s:&mut Store,team:f64);
    fn kill_player(&mut self,s:&mut Store,events:&mut Vec<Event>);
    fn changed(&mut self,s:&Store,character:Option<Ref<Character>>);
    fn rejected(&mut self,class:StrId,team:f64);
}
pub struct Prepared {
    pub character:Option<Ref<Character>>,
    pub classic:bool,
    pub rosters:Vec<Vec<StrId>>,
    pub classes:Vec<StrId>,
}
pub struct Human {
    state:human_owned::Human,
    pub prepared:Prepared,
    pub choices:Choices,
    scratch:Scratch,
}
impl Human {
    pub fn new(prepared:Prepared,choices:Choices,commands:usize,posts:usize,points:usize)->Self {
        Self{state:human_owned::Human::new(commands),prepared,choices,scratch:Scratch::new(posts,points)}
    }
    pub fn step<S:PlayerServices>(&mut self,s:&mut Store,dt:f64,commands:&[Command],services:&mut S,
        strings:&dyn Text,random:&mut dyn Draws,gameplay:&mut dyn Draws) {
        self.state.step(s,dt,commands,&mut Context{prepared:&self.prepared,choices:&mut self.choices,
            scratch:&mut self.scratch,services,strings,random,gameplay});
    }
    /// Game.onRespawn runs from dead-body combat, before the live and tail
    /// slots. Classic confirms its queue; without AI it places immediately.
    /// The caller retains the URL index and the first authored world path.
    pub fn auto_respawn<S:PlayerServices>(&mut self,s:&mut Store,index:&mut f64,fallback:&[[f64;4]],services:&mut S,
        strings:&dyn Text,gameplay:&mut dyn Draws) {
        let Some(auto)=&s.human_spawn.auto else{return};
        let class=auto.class_name;
        self.choices.auto_posts(s,s.battle.own,services);
        if self.prepared.classic {
            let Some(r)=self.prepared.character.filter(|r|s.character.contains(*r)) else{return};
            if !spawning::awaiting_human(s.character.get(r).unwrap()) {return;}
            let post=self.choices.auto_post(*index);
            spawning::on_human_ready(s,r,spawning::HumanChoice{class_name:Some(class),post:post.map(Some),..Default::default()},strings,services);
            services.changed(s,Some(r));
            return;
        }
        let Some((post,point))=self.choices.auto_point(index,fallback,||gameplay.uniform()) else{return};
        let Some(unit)=services.spawn_player(s,class,post,point) else{return};
        let character=self.prepared.character.filter(|r|s.character.contains(*r));
        if let Some(r)=character {let c=s.character.get_mut(r).unwrap();c.unit=Some(unit);c.class_name=Some(class);}
        s.unit.get_mut(unit).expect("auto viewpoint").character=character;
        services.linked(s,unit,character);services.changed(s,character);
        let mut cx=Context{prepared:&self.prepared,choices:&mut self.choices,scratch:&mut self.scratch,
            services,strings,random:&mut NoDraws,gameplay};
        self.state.spawned(s,class,post,&mut cx);
    }
    pub fn events(&self)->&[Event] {self.state.events()}
    pub fn published(&mut self) {self.state.published();}
    pub fn journal(&mut self,events:&mut crate::store::calls::Events) {self.state.journal(events);}
}
struct Context<'a,S:PlayerServices> {
    prepared:&'a Prepared,choices:&'a mut Choices,scratch:&'a mut Scratch,services:&'a mut S,
    strings:&'a dyn Text,random:&'a mut dyn Draws,gameplay:&'a mut dyn Draws,
}
impl<S:PlayerServices> Context<'_,S> {
    fn character(&self,s:&Store)->Option<Ref<Character>> {self.prepared.character.filter(|r|s.character.contains(*r))}
    fn begin(&mut self,s:&mut Store) {self.choices.begin(s,self.services,S::floor);}
    fn named_post(s:&Store,name:Option<StrId>)->Option<SpawnPost> {
        for &r in &s.command_posts.posts {if Some(s.post.get(r).unwrap().name)==name {return Some(SpawnPost::Post(r));}}
        for &r in &s.mobile_posts.posts {if Some(s.mobile_post.get(r).unwrap().name)==name {return Some(SpawnPost::MobilePost(r));}}
        None
    }
}
impl<S:PlayerServices> human_owned::Services for Context<'_,S> {
    fn team(&mut self,s:&Store)->f64 {s.battle.own}
    fn dead(&mut self,s:&Store)->bool {self.services.local().and_then(|r|s.unit.get(r)).is_none_or(|u|u.combat.dead)}
    fn over(&mut self,s:&Store)->bool {s.battle.over}
    fn roster(&mut self,_:&Store,team:f64)->&[StrId] {
        if team>=0.0 && team.is_finite() && team.fract()==0.0 {self.prepared.rosters.get(team as usize).map_or(&[],Vec::as_slice)} else {&[]}
    }
    fn classic(&mut self,_:&Store)->bool {self.prepared.classic}
    fn character_spawn(&mut self,s:&Store)->Option<(bool,f64)> {
        let c=s.character.get(self.character(s)?)?;c.spawn.as_ref().map(|sp|(c.unit.is_some(),sp.timer))
    }
    fn can_spawn(&mut self,_:&Store,class:StrId)->bool {self.prepared.classes.contains(&class)}
    fn set_team(&mut self,s:&mut Store,events:&mut Vec<Event>,team:f64) {
        let previous=s.battle.own;
        if (team!=1.0 && team!=2.0) || team==previous || !s.battle.team_ids.contains(&team) || !self.dead(s) {return;}
        let character=self.character(s);
        if self.prepared.classic {
            let Some(r)=character else{return};
            if !spawning::switch_team_with_scratch(s,r,team,self.strings,self.services,self.random,self.scratch) {return;}
        } else if let Some(r)=character {s.character.get_mut(r).unwrap().team=team;}
        s.battle.own=team;s.battle.foe=3.0-team;
        if let Some(r)=self.services.local() {let u=s.unit.get_mut(r).expect("viewpoint team");u.team=team;u.combat.team=team;}
        self.services.retarget(s,team);self.services.changed(s,character);
        human_owned::team_changed(s,events,team,previous,self);
    }
    fn spawn(&mut self,s:&mut Store,events:&mut Vec<Event>,choice:&HumanPick) {
        self.begin(s);let team=s.battle.own;
        let picked=self.choices.choose(team,choice.post,S::relation,||self.gameplay.uniform()).or_else(||self.choices.fallback(team));
        let Some(index)=picked else{return};
        let name=self.choices.rows[index].name;
        let Some(point)=self.choices.point(s,index,team,self.services,S::relation,||self.gameplay.uniform()) else{return};
        let Some(unit)=self.services.spawn_player(s,choice.class_name,Some(name),point) else{return};
        let character=self.character(s);
        if let Some(r)=character {let c=s.character.get_mut(r).unwrap();c.unit=Some(unit);c.class_name=Some(choice.class_name);}
        s.unit.get_mut(unit).expect("spawned viewpoint").character=character;
        self.services.linked(s,unit,character);self.services.changed(s,character);
        human_owned::spawned(s,events,choice.class_name,Some(name),self);
    }
    fn ready(&mut self,s:&mut Store,events:&mut Vec<Event>,choice:&HumanPick)->bool {
        let Some(r)=self.character(s) else{return false};
        let c=s.character.get(r).unwrap();let Some(sp)=c.spawn.as_ref() else{return false};if c.unit.is_some() {return false;}
        let timer=sp.timer;self.begin(s);let team=s.battle.own;
        let transport=self.choices.rows.iter().any(|p|Some(p.name)==choice.post && matches!(p.key,Key::Transport(_)) && p.facts.owner==team);
        if transport && timer<=0.0 {
            self.spawn(s,events,choice);return s.character.get(r).unwrap().unit.is_some();
        }
        s.character.get_mut(r).unwrap().spawn.as_mut().unwrap().ready=false;
        let post=Self::named_post(s,choice.post);
        if spawning::awaiting_human(s.character.get(r).unwrap()) {
            spawning::on_human_ready(s,r,spawning::HumanChoice{class_name:Some(choice.class_name),post:post.map(Some),..spawning::HumanChoice::default()},self.strings,self.services);
        }
        self.services.changed(s,Some(r));s.character.get(r).unwrap().spawn.as_ref().unwrap().ready
    }
    fn kill(&mut self,s:&mut Store,events:&mut Vec<Event>) {if !self.dead(s) {self.services.kill_player(s,events);}}
    fn cancel(&mut self,s:&mut Store,_:&mut Vec<Event>) {
        if !self.prepared.classic {return;}let Some(r)=self.character(s) else{return};
        let c=s.character.get_mut(r).unwrap();if c.unit.is_none() {if let Some(sp)=&mut c.spawn {sp.ready=false;}}
        self.services.changed(s,Some(r));
    }
    fn rejected(&mut self,_:&mut Store,_:&mut Vec<Event>,class:StrId,team:f64) {self.services.rejected(class,team);}
}

struct NoDraws;
impl Draws for NoDraws {fn next(&mut self)->f64 {panic!("auto respawn must not draw from AI")}}
