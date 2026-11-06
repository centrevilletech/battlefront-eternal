//! The battle phase (src/game.js updateBattle) on a server world: who
//! counts toward the posts, the posts' tick, each post event (a capture's
//! points for the first 64 registry members in its region, a destructible
//! post's kill region), the command vehicles' posts, then the spawn
//! manager's clock, the battle's rules and each Character's respawn. The
//! local player's spawn screen, his corpse and the HUD are a page's.

use super::mobile::{self, MobileEvent};
use super::objectives::{kill_region_built, KillRegionHost};
use super::regions::build_region;
use super::posts::{self, Capturer, Change};
use super::rules::{self, RulesWorld};
use super::stats::Text;
use super::spawning::{self, Draws};
use super::world::{BattleWorld, Member, SpawningWorld, Victim};
use crate::gen::{Building, Character, Post, PostChange, PostEvent, Store, Vehicle};
use crate::store::{Nullable, Ref};
use three::Vector3;

/// Services outside the battle phase. Its rules, Characters, posts and
/// spawning state stay together while a callback temporarily publishes
/// them for another native command or a foreign body service.
pub trait PhaseWorld: SpawningWorld + RulesWorld {
    fn visit_members(&mut self, s: &mut Store, visit: &mut dyn FnMut(&mut Store, Member));
    fn award_capture(&mut self, s: &mut Store, post: Ref<Post>) {
        let mut seen = 0;
        self.visit_members(s, &mut |s, m| {
            if seen >= 64 || m.removed || !posts::contains(s.post.get(post).expect("a post"), &v3(m.position)) { return; }
            seen += 1;
            if m.alive { rules::point(s, m.character); }
        });
    }
    fn building_attacker(&mut self, s: &Store, building: Ref<Building>) -> Option<Ref<Character>>;
    fn vehicle_attacker(&mut self, s: &Store, vehicle: Ref<Vehicle>) -> Option<Ref<Character>>;
    fn victims_into(&mut self, s: &Store, into: &mut Vec<Victim>);
    fn victim_position(&mut self, _s: &Store, victim: Victim) -> [f64; 3] {
        match victim { Victim::Unit { position, .. } | Victim::Hull { position, .. } => position }
    }
    fn kill_victim(&mut self, s: &mut Store, victim: Victim);
    fn post_destroyed(&mut self, s: &mut Store, r: Option<Ref<Character>>, relation: f64) { rules::post_destroyed(s, r, relation); }
    fn effect(&mut self, s: &mut Store, name: &str, at: [f64; 3]);
    fn post_event(&mut self, s: &mut Store, event: &PostEvent);
    fn mobile_event(&mut self, s: &mut Store, event: &MobileEvent);
}

impl<T: BattleWorld + ?Sized> PhaseWorld for T {
    fn visit_members(&mut self, s: &mut Store, visit: &mut dyn FnMut(&mut Store, Member)) {
        for member in BattleWorld::members(self) { visit(s, member); }
    }
    fn building_attacker(&mut self, _s: &Store, building: Ref<Building>) -> Option<Ref<Character>> { BattleWorld::building_attacker(self, building) }
    fn vehicle_attacker(&mut self, _s: &Store, vehicle: Ref<Vehicle>) -> Option<Ref<Character>> { BattleWorld::vehicle_attacker(self, vehicle) }
    fn victims_into(&mut self, _: &Store, into: &mut Vec<Victim>) { into.clear(); into.extend(BattleWorld::kill_region_victims(self)); }
    fn kill_victim(&mut self, s: &mut Store, victim: Victim) { BattleWorld::kill_victim(self, s, victim); }
    fn effect(&mut self, _: &mut Store, name: &str, at: [f64; 3]) { BattleWorld::effect(self, name, at); }
    fn post_event(&mut self, s: &mut Store, event: &PostEvent) { BattleWorld::post_event(self, s, event); }
    fn mobile_event(&mut self, s: &mut Store, event: &MobileEvent) { BattleWorld::mobile_event(self, s, event); }
}

fn v3(p: [f64; 3]) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

/// Reused phase work, reserved at construction from the registry and post
/// capacities. Neither publication nor a nested callback owns these buffers.
#[derive(Default)]
pub struct Scratch {
    capturers: Vec<Capturer>,
    events: Vec<PostEvent>,
    mobile: Vec<MobileEvent>,
    victims: Vec<Victim>,
    points: Vec<Vector3>,
    spawning: spawning::Scratch,
}
impl Scratch {
    pub fn reserve_spawn_points(&mut self,points:usize){self.spawning.reserve_points(points);}
    pub fn new(members: usize, posts: usize, mobile: usize, victims: usize) -> Self {
        Self { capturers: Vec::with_capacity(members), events: Vec::with_capacity(posts),
            mobile: Vec::with_capacity(mobile), victims: Vec::with_capacity(victims), points: Vec::with_capacity(victims), spawning: spawning::Scratch::new(posts + mobile, 0) }
    }
}

/// updateBattle(dt) (game.js:2310). `ai`: whether the Classic AI runs (its
/// spawn manager is the store's `spawning`); `random`: its generator A;
/// `gameplay`: the gameplay stream (a kill region's draws).
#[allow(clippy::too_many_arguments)]
pub fn update_battle<W: PhaseWorld + ?Sized>(
    dt: f64,
    s: &mut Store,
    strings: &dyn Text,
    ai: bool,
    stats_active: impl Fn(Ref<Character>, &Character) -> bool,
    world: &mut W,
    random: &mut dyn Draws,
    gameplay: &mut dyn Draws,
) {
    update_battle_with_scratch(dt, s, strings, ai, stats_active, world, random, gameplay, &mut Scratch::default());
}

/// The owned battle-tail span. All observations and selected events retain
/// their source order while callbacks can change the next observation.
#[allow(clippy::too_many_arguments)]
pub fn update_battle_with_scratch<W: PhaseWorld + ?Sized>(
    dt: f64, s: &mut Store, strings: &dyn Text, ai: bool,
    stats_active: impl Fn(Ref<Character>, &Character) -> bool, world: &mut W,
    random: &mut dyn Draws, gameplay: &mut dyn Draws, scratch: &mut Scratch,
) {
    // Who counts toward a post: living soldiers on foot whose class captures;
    // never anyone seated, held, or the player in a hull or at a gun.
    scratch.capturers.clear();
    world.visit_members(s, &mut |_s, m| {
        if m.soldier && m.alive && !m.seated && !m.held && m.captures && !m.player_mounted {
            scratch.capturers.push(Capturer { team: m.team, position: v3(m.position) });
        }
    });
    posts::update(s, dt, &scratch.capturers, world);
    std::mem::swap(&mut s.command_posts.events, &mut scratch.events);
    for i in 0..scratch.events.len() {
        let event = &scratch.events[i];
        let post = event.post;
        // The native award pass revisits the capture region's first 64
        // objects, crediting the living Characters among them.
        if event.kind == PostChange::Captured {
            world.award_capture(s, post);
        }
        if event.destroyed == Some(true) {
            objective_destroyed(post, event.from.unwrap_or(f64::NAN), s, world, gameplay, &mut scratch.victims, &mut scratch.points);
        }
        world.post_event(s, event);
    }
    // The command vehicles' posts: one whose vehicle died is lost, a point
    // for whoever hurt it last unless he is its owner's friend.
    scratch.events.clear();
    mobile::events_into(s, world, &mut scratch.mobile);
    for i in 0..scratch.mobile.len() {
        let event = scratch.mobile[i];
        if let MobileEvent::Lost { vehicle, from, .. } = event {
            let destroyer = vehicle.and_then(|v| world.vehicle_attacker(s, v));
            if let Some(d) = destroyer {
                let team = s.character.get(d).expect("a Character").team;
                let relation = world.relation(team, from);
                world.post_destroyed(s, Some(d), relation);
            }
        }
        world.mobile_event(s, &event);
    }
    if ai {
        spawning::clock(s, dt, world);
    }
    rules::update(s, dt, strings, &stats_active, world);
    if ai {
        spawning::update_characters_with_scratch(s, dt, strings, world, random, &mut scratch.spawning);
    }
}

/// A destructible post's kill region as the battle runs it: the gameplay
/// stream's draws, the page's effects, the living soldiers and hulls.
struct Region<'w, W: PhaseWorld + ?Sized> {
    world: &'w mut W,
    gameplay: &'w mut dyn Draws,
    store: &'w mut Store,
    victims: &'w mut Vec<Victim>,
}

impl<W: PhaseWorld + ?Sized> KillRegionHost for Region<'_, W> {
    fn random(&mut self) -> f64 {
        self.gameplay.uniform()
    }

    fn play(&mut self, name: &str, point: Vector3) {
        self.world.effect(self.store, name, [point.x, point.y, point.z]);
    }

    fn victims(&mut self) -> Vec<Vector3> {
        let mut out = Vec::new(); self.victims_into(&mut out); out
    }
    fn victims_into(&mut self, into: &mut Vec<Vector3>) {
        self.world.victims_into(self.store, self.victims);
        into.clear();
        for victim in self.victims.iter() {
            let at = match victim { Victim::Unit { position, .. } | Victim::Hull { position, .. } => *position };
            into.push(v3(at));
        }
    }

    fn position(&mut self, victim: usize, _: Vector3) -> Vector3 {
        v3(self.world.victim_position(self.store, self.victims[victim]))
    }
    fn kill(&mut self, victim: usize) {
        self.world.kill_victim(self.store, self.victims[victim]);
    }
}

/// objectiveDestroyed(event) (game.js:2370): a post point for whoever hurt
/// the building last unless he is its owner's friend, then its kill region.
fn objective_destroyed<W: PhaseWorld + ?Sized>(post: Ref<Post>, from: f64, s: &mut Store, world: &mut W, gameplay: &mut dyn Draws, victims: &mut Vec<Victim>, points: &mut Vec<Vector3>) {
    let building = s.post.get(post).expect("a post").building;
    let destroyer = building.and_then(|b| world.building_attacker(s, b));
    if let Some(d) = destroyer {
        let team = s.character.get(d).expect("a Character").team;
        let relation = world.relation(team, from);
        world.post_destroyed(s, Some(d), relation);
    }
    let region = match &s.post.get(post).expect("a post").kill_region {
        Nullable::Value(r) => Some(build_region(r)),
        Nullable::Null | Nullable::Undefined => None,
    };
    if let Some(region) = region {
        let mut host = Region { world, gameplay, store: s, victims };
        kill_region_built(&region, &mut host, points);
    }
}

/// The event's change, as Post.update and destroy() made it.
pub fn change_of(event: &crate::gen::PostEvent) -> Change {
    match event.kind {
        PostChange::Captured => Change::Captured { by: event.by.unwrap_or(f64::NAN) },
        PostChange::Neutralized => Change::Neutralized { from: event.from.unwrap_or(f64::NAN) },
        PostChange::Lost => Change::Destroyed { from: event.from.unwrap_or(f64::NAN) },
    }
}
