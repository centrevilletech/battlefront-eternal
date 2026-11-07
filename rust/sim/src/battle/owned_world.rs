//! Battle-tail observations read the canonical Store. Only actions belonging
//! to another owner cross Services; membership, statistics and post queries
//! stay inside the phase and reuse prepared class/pad tables.
use super::{battlefield::BattlefieldWorld, fleet_posts::Pads, human::Command,
    mobile::{self, MobileEvent, Pad}, observation::{Mounts, Observations},
    phase::PhaseWorld, posts, registry::{Registry, Targets}, rules::RulesWorld,
    spawning, stats::Seen, tail, world::{Body, Member, PostsWorld, SpawningWorld, Victim}};
use crate::gen::{Building, Character, MobilePost, PostEvent, SpawnPost, Store, Unit, Vehicle};
use crate::store::{Ref, StrId};

/// F2/F3 implement these actions with their existing owner services. The
/// factory and player operations finish synchronously: a deferred spawn or
/// death would change the next post's observations and the spawn queues.
pub trait Services: Mounts + Targets {
    fn changed_battle(&mut self, s: &Store);
    fn building_alive(&mut self, building: Ref<Building>) -> bool;
    fn building_attacker(&mut self, s: &Store, building: Ref<Building>) -> Option<Ref<Character>>;
    fn vehicle_attacker(&mut self, s: &Store, vehicle: Ref<Vehicle>) -> Option<Ref<Character>>;
    fn piloted(&mut self, s: &Store, unit: Ref<Unit>) -> Option<[f64;3]>;
    fn relation(&mut self, a: f64, b: f64) -> f64;
    fn prepare_record(&mut self, s: &mut Store, character: Ref<Character>);
    fn point(&mut self, s: &mut Store, character: Option<Ref<Character>>);
    fn kill(&mut self, s: &mut Store, unit: Ref<Unit>);
    fn kill_droid(&mut self, s: &mut Store, unit: Ref<Unit>);
    fn wreck(&mut self, s: &mut Store, hull: Ref<Vehicle>);
    fn set_hull_health(&mut self, s: &mut Store, hull: Ref<Vehicle>, value: f64);
    fn spawn(&mut self, s: &mut Store, character: Ref<Character>, class_name: Option<StrId>, post: SpawnPost, at: [f64;3], yaw: Option<f64>) -> Option<Ref<Unit>>;
    fn request_objective(&mut self, s: &mut Store, character: Ref<Character>);
    fn republish(&mut self, s: &mut Store, character: Ref<Character>);
    fn cast_spawn(&mut self, from: three::Vector3, direction: three::Vector3, span: f64, body: Ref<crate::gen::Body>) -> Option<three::Vector3>;
    fn effect(&mut self, s: &mut Store, name: &str, at: [f64;3]);
    fn post_event(&mut self, s: &mut Store, event: &PostEvent);
    fn mobile_event(&mut self, s: &mut Store, event: &MobileEvent);
    fn battle_over(&mut self, s: &mut Store, loser: f64);
    fn human_tail(&mut self, s: &mut Store, dt: f64, commands: &[Command], prepared: &mut Prepared);
    fn corpse_tail(&mut self, s: &mut Store);
}
#[derive(Default)]
pub struct Prepared {
    pub enhanced: bool,
    pub local_character:Option<Ref<Character>>,
    pub registry: Registry,
    pub observations: Observations,
    pub pads: Pads,
    pub captures: Vec<Option<f64>>,
    pub hero_names: Vec<Option<StrId>>,
}
pub struct World<'a, S: Services> {
    pub prepared: &'a mut Prepared,
    pub services: &'a mut S,
    pub local: Option<Ref<Unit>>,
    pub classic: bool,
}
impl<S: Services> World<'_, S> {
    fn mounted(s: &Store, r: Option<Ref<Unit>>) -> bool {
        r.and_then(|r| s.unit.get(r)).and_then(|u| u.seat).is_some()
    }
    /// Battlefield observes what a Character controls, even when that is a
    /// stationary gun. The body remains the death target and keeps its flags.
    pub fn body(&mut self, s: &Store, r: Ref<Unit>) -> Body {
        let u = s.unit.get(r).expect("canonical battlefield body");
        let lease = u.seat.and_then(|r| s.seat_lease.get(r).filter(|l| !l.released).map(|l| (r,l.vehicle)));
        let mount = lease.and_then(|(r,_)| self.services.mount(s,r));
        let p = u.position();
        Body { removed:u.removed, alive:u.alive, combat_dead:u.combat.dead,
            place:mount.map_or([p.x,p.y,p.z],|m|m.position), vehicle:lease.and_then(|(_,v)|v),
            droid:self.services.piloted(s,r) }
    }
}
impl<S: Services> PostsWorld for World<'_, S> {
    fn capture_team(&mut self, team: f64) -> f64 {
        if team >= 0.0 && team.is_finite() && team.fract() == 0.0 {
            self.prepared.captures.get(team as usize).copied().flatten().unwrap_or(team)
        } else { team }
    }
    fn building_alive(&mut self, b: Ref<Building>) -> bool { self.services.building_alive(b) }
    fn pad(&mut self, s: &Store, slot: f64) -> Pad { self.prepared.pads.get(s,slot as usize).clone() }
    fn with_pad(&mut self, s: &mut Store, slot: f64, visit: &mut dyn FnMut(&mut Store,&Pad)) {
        let pad = self.prepared.pads.get(s,slot as usize); visit(s,pad);
    }
    fn vehicle_position(&mut self, s: &Store, r: Ref<Vehicle>) -> [f64;3] { s.vehicle.get(r).expect("canonical post hull").position }
    fn class_spawn_world(&mut self, s: &Store, r: Ref<MobilePost>) -> Vec<[f64;4]> {
        let mut into = Vec::new(); self.class_spawn_world_into(s,r,&mut into); into
    }
    fn class_spawn_world_into(&mut self, s: &Store, r: Ref<MobilePost>, into: &mut Vec<[f64;4]>) {
        super::fleet_posts::spawn_points(s,r,into,|from,direction,span,body| self.services.cast_spawn(from,direction,span,body));
    }
}
impl<S: Services> RulesWorld for World<'_, S> {
    fn stats_active(&mut self, s: &Store, _: Ref<Character>, c: &Character, _: bool) -> bool {
        !c.human || matches!(s.human_spawn.phase, crate::gen::HumanPhase::Alive | crate::gen::HumanPhase::Dying)
    }
    fn prepare_record(&mut self, s: &mut Store, r: Ref<Character>) { self.services.prepare_record(s,r); }
    fn relation(&mut self, a: f64, b: f64) -> f64 { self.services.relation(a,b) }
    fn unit_alive(&mut self, s: &Store, r: Ref<Unit>) -> bool { s.unit.get(r).is_some_and(|u|u.alive) }
    fn observe(&mut self, s: &Store, _: Ref<Character>, unit: Option<Ref<Unit>>) -> Option<Seen> {
        self.prepared.observations.observe(s,unit,self.services)
    }
    fn living(&mut self, s: &Store, team: f64) -> Option<f64> {
        self.classic.then(||s.spawning.teams.iter().find(|t|t.index == team).map(|t|t.living)).flatten()
    }
    fn owned_values(&mut self, s: &mut Store, team: f64) -> Vec<Option<f64>> {
        posts::owned(s,team,self).into_iter().map(|p|posts::value_bleed(s,p)).collect()
    }
    /// Rules count/sum in the same placed-then-mobile order as Posts.owned,
    /// without constructing that convenience method's result vector.
    fn owned_summary(&mut self, s: &mut Store, team: f64) -> (usize,f64) {
        mobile::refresh_all(s,self);
        let mut count = 0; let mut sum = 0.0;
        for &r in &s.command_posts.posts {
            let p = s.post.get(r).expect("placed post");
            if p.active && p.team == team { count += 1; sum += p.value_bleed.unwrap_or(10.0); }
        }
        for &r in &s.mobile_posts.posts {
            let p = s.mobile_post.get(r).expect("mobile post");
            if p.active && p.team == team { count += 1; sum += p.desc.value_bleed; }
        }
        (count,sum)
    }
    fn battle_over(&mut self, s: &mut Store, loser: f64) { self.services.battle_over(s,loser); }
}
impl<S: Services> SpawningWorld for World<'_, S> {
    fn spawn_goal(&mut self, s: &Store, r: Ref<Character>) -> Option<[f64;3]> {
        if self.prepared.enhanced {spawning::objective_position(s,r,self)}else{None}
    }
    fn health(&mut self, s: &Store, r: Ref<Unit>) -> f64 { s.unit.get(r).map_or(0.0,|u|u.combat.health) }
    fn force_kill(&mut self, s: &mut Store, _: Ref<Character>, r: Ref<Unit>) { self.services.kill(s,r); }
    fn request_objective(&mut self, s: &mut Store, r: Ref<Character>) { super::fleet_posts::refresh_posts(s,&mut self.prepared.pads);self.services.request_objective(s,r); }
    fn republish(&mut self, s: &mut Store, r: Ref<Character>) { super::fleet_posts::refresh_posts(s,&mut self.prepared.pads);self.services.republish(s,r); }
    fn hero_name(&mut self, class: StrId) -> Option<StrId> { self.prepared.hero_names.get(class.0 as usize).copied().flatten() }
    fn spawn_unit(&mut self, s: &mut Store, r: Ref<Character>, class: Option<StrId>, post: SpawnPost, at: [f64;3], yaw: Option<f64>) -> Option<Ref<Unit>> {
        super::fleet_posts::refresh_posts(s,&mut self.prepared.pads);
        self.services.spawn(s,r,class,post,at,yaw)
    }
}
impl<S: Services> PhaseWorld for World<'_, S> {
    fn visit_members(&mut self, s: &mut Store, visit: &mut dyn FnMut(&mut Store,Member)) {
        let mounted = Self::mounted(s,self.local);
        self.prepared.registry.visit(s,self.local,mounted,self.services,visit);
    }
    fn building_attacker(&mut self, s: &Store, building: Ref<Building>) -> Option<Ref<Character>> {
        self.services.building_attacker(s,building)
    }
    fn award_capture(&mut self, s: &mut Store, post: Ref<crate::gen::Post>) {
        let mounted = Self::mounted(s,self.local); let mut seen = 0;
        self.prepared.registry.visit_with(s,self.local,mounted,self.services,&mut |s,services,m| {
            let at = three::Vector3::new(m.position[0],m.position[1],m.position[2]);
            if seen >= 64 || m.removed || !posts::contains(s.post.get(post).expect("capture post"),&at) { return; }
            seen += 1;
            if m.alive { services.point(s,m.character); }
        });
    }
    fn post_destroyed(&mut self, s: &mut Store, r: Option<Ref<Character>>, relation: f64) {
        if relation < 1.0 { self.services.point(s,r); }
    }
    fn vehicle_attacker(&mut self, s: &Store, r: Ref<Vehicle>) -> Option<Ref<Character>> { self.services.vehicle_attacker(s,r) }
    fn victims_into(&mut self, s: &Store, into: &mut Vec<Victim>) { Registry::victims(s,into); }
    fn victim_position(&mut self, s: &Store, victim: Victim) -> [f64;3] { victim.position_in(s) }
    fn kill_victim(&mut self, s: &mut Store, victim: Victim) {
        match victim { Victim::Unit{id,..}=>self.services.kill(s,id), Victim::Hull{id,..}=>self.services.set_hull_health(s,id,0.0) }
    }
    fn effect(&mut self, s: &mut Store, name: &str, at: [f64;3]) { self.services.effect(s,name,at); }
    fn post_event(&mut self, s: &mut Store, event: &PostEvent) { super::fleet_posts::refresh_posts(s,&mut self.prepared.pads);self.services.post_event(s,event); }
    fn mobile_event(&mut self, s: &mut Store, event: &MobileEvent) { self.services.mobile_event(s,event); }
}
impl<S: Services> BattlefieldWorld for World<'_, S> {
    fn body(&mut self, s: &Store, r: Ref<Unit>) -> Body { self.body(s,r) }
    fn vehicle(&mut self, s: &Store, r: Ref<Unit>) -> Option<Ref<Vehicle>> {
        s.unit.get(r).and_then(|u|u.seat).and_then(|r|s.seat_lease.get(r)).and_then(|l|l.vehicle)
    }
    fn combat_dead(&mut self, s: &Store, r: Ref<Unit>) -> bool { s.unit.get(r).expect("canonical body").combat.dead }
    fn kill(&mut self, s: &mut Store, r: Ref<Unit>) { self.services.kill(s,r); }
    fn kill_droid(&mut self, s: &mut Store, r: Ref<Unit>) { self.services.kill_droid(s,r); }
    fn wreck(&mut self, s: &mut Store, r: Ref<Vehicle>) { self.services.wreck(s,r); }
}
impl<S: Services> tail::World for World<'_, S> {
    fn human_tail(&mut self, s: &mut Store, dt: f64, commands: &[Command]) { self.services.human_tail(s,dt,commands,self.prepared); }
    fn corpse_tail(&mut self, s: &mut Store) { self.services.corpse_tail(s); }
}

/// The two tail schedulers and their retained buffers form one ownership span.
/// Human statistics intentionally follow the local spawn phase for every human,
/// including server humans, matching Game's existing statsActive closure.
pub struct Owner {
    pub tail: tail::Tail,
    pub prepared: Prepared,
    pub local: Option<Ref<Unit>>,
    pub classic: bool,
}
impl Owner {
    pub fn new(s: &mut Store, battlefield: super::battlefield::Battlefield, local: Option<Ref<Unit>>, classic: bool,
        members: usize, victims: usize) -> Self {
        let posts = s.command_posts.posts.len(); let mobile = s.mobile_posts.posts.len();
        if classic { spawning::prepare_runtime(s,posts+mobile,s.battle.characters.len()); }
        s.command_posts.events.reserve(posts.saturating_sub(s.command_posts.events.len()));
        for &r in &s.command_posts.posts { s.post.get_mut(r).expect("prepared post").counts.reserve(s.battle.team_ids.len()); }
        Self {tail:tail::Tail::new(battlefield,members,posts,mobile,victims), prepared:Prepared::default(),local,classic}
    }
    pub fn step<S: Services>(&mut self, input: &crate::gen::F4TailInput, s: &mut Store,
        strings: &dyn for<'w> super::stats::ContextText<World<'w, S>>, commands: &[Command], services: &mut S,
        random: &mut dyn for<'w> spawning::ContextDraws<World<'w, S>>, gameplay: &mut dyn for<'w> spawning::ContextDraws<World<'w, S>>) {
        self.tail.step(input,s,strings,self.classic,|_,_|true,commands,
            &mut World {prepared:&mut self.prepared,services,local:self.local,classic:self.classic}, random,gameplay);
        services.changed_battle(s);
    }
}

impl<S: Services + super::stats::TextWorld> super::stats::TextWorld for World<'_, S> { fn text_nonempty(&self,id:StrId)->bool { self.services.text_nonempty(id) } }

impl<S: Services + spawning::RandomWorld> spawning::RandomWorld for World<'_, S> {
    fn random_next(&mut self, stream: u32) -> f64 { self.services.random_next(stream) }
}
