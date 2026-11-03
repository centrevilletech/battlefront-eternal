//! The battle phase (src/game.js updateBattle) on a server world: who
//! counts toward the posts, the posts' tick, each post event (a capture's
//! points for the first 64 registry members in its region, a destructible
//! post's kill region), the command vehicles' posts, then the spawn
//! manager's clock, the battle's rules and each Character's respawn. The
//! local player's spawn screen, his corpse and the HUD are a page's.

use super::mobile::MobileEvent;
use super::objectives::{kill_region, KillRegionHost};
use super::posts::{self, Capturer, Change};
use super::rules;
use super::spawning::{self, Draws};
use super::world::{BattleWorld, Victim};
use crate::gen::{Character, Post, PostChange, Store};
use crate::store::{Nullable, Ref, Strings};
use three::Vector3;

fn v3(p: [f64; 3]) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

/// updateBattle(dt) (game.js:2310). `ai`: whether the Classic AI runs (its
/// spawn manager is the store's `spawning`); `random`: its generator A;
/// `gameplay`: the gameplay stream (a kill region's draws).
#[allow(clippy::too_many_arguments)]
pub fn update_battle(
    dt: f64,
    s: &mut Store,
    strings: &mut Strings,
    ai: bool,
    stats_active: impl Fn(Ref<Character>, &Character) -> bool,
    world: &mut dyn BattleWorld,
    random: &mut dyn Draws,
    gameplay: &mut dyn Draws,
) {
    // Who counts toward a post: living soldiers on foot whose class captures;
    // never anyone seated, held, or the player in a hull or at a gun.
    let capturers: Vec<Capturer> = world
        .members()
        .iter()
        .filter(|m| m.soldier && m.alive && !m.seated && !m.held && m.captures && !m.player_mounted)
        .map(|m| Capturer { team: m.team, position: v3(m.position) })
        .collect();
    posts::update(s, dt, &capturers, world);
    let events = std::mem::take(&mut s.command_posts.events);
    for event in events {
        let post = event.post;
        // The native award pass revisits the capture region's first 64
        // objects, crediting the living Characters among them.
        if event.kind == PostChange::Captured {
            let mut seen = 0;
            for m in world.members() {
                if m.removed || !posts::contains(s.post.get(post).expect("a post"), &v3(m.position)) {
                    continue;
                }
                seen += 1;
                if seen > 64 {
                    break;
                }
                if m.alive {
                    rules::point(s, m.character);
                }
            }
        }
        if event.kind == PostChange::Lost && event.destroyed == Some(true) {
            objective_destroyed(post, event.from.unwrap_or(f64::NAN), s, world, gameplay);
        }
        world.post_event(s, &event);
    }
    // The command vehicles' posts: one whose vehicle died is lost, a point
    // for whoever hurt it last unless he is its owner's friend.
    for event in posts::mobile_events(s, world) {
        if let MobileEvent::Lost { vehicle, from, .. } = event {
            let destroyer = vehicle.and_then(|v| world.vehicle_attacker(v));
            if let Some(d) = destroyer {
                let team = s.character.get(d).expect("a Character").team;
                let relation = world.relation(team, from);
                rules::post_destroyed(s, Some(d), relation);
            }
        }
        world.mobile_event(s, &event);
    }
    if ai {
        spawning::clock(s, dt, world);
    }
    rules::update(s, dt, strings, &stats_active, world);
    if ai {
        spawning::update_characters(s, dt, strings, world, random);
    }
}

/// A destructible post's kill region as the battle runs it: the gameplay
/// stream's draws, the page's effects, the living soldiers and hulls.
struct Region<'w> {
    world: &'w mut dyn BattleWorld,
    gameplay: &'w mut dyn Draws,
    store: &'w mut Store,
    victims: Vec<Victim>,
}

impl KillRegionHost for Region<'_> {
    fn random(&mut self) -> f64 {
        self.gameplay.uniform()
    }

    fn play(&mut self, name: &str, point: Vector3) {
        self.world.effect(name, [point.x, point.y, point.z]);
    }

    fn victims(&mut self) -> Vec<Vector3> {
        self.victims = self.world.kill_region_victims();
        self.victims
            .iter()
            .map(|v| match v {
                Victim::Unit { position, .. } | Victim::Hull { position, .. } => v3(*position),
            })
            .collect()
    }

    fn kill(&mut self, victim: usize) {
        self.world.kill_victim(self.store, self.victims[victim]);
    }
}

/// objectiveDestroyed(event) (game.js:2370): a post point for whoever hurt
/// the building last unless he is its owner's friend, then its kill region.
fn objective_destroyed(post: Ref<Post>, from: f64, s: &mut Store, world: &mut dyn BattleWorld, gameplay: &mut dyn Draws) {
    let building = s.post.get(post).expect("a post").building;
    let destroyer = building.and_then(|b| world.building_attacker(b));
    if let Some(d) = destroyer {
        let team = s.character.get(d).expect("a Character").team;
        let relation = world.relation(team, from);
        rules::post_destroyed(s, Some(d), relation);
    }
    let region = match &s.post.get(post).expect("a post").kill_region {
        Nullable::Value(r) => Some(r.clone()),
        Nullable::Null | Nullable::Undefined => None,
    };
    let mut host = Region { world, gameplay, store: s, victims: Vec::new() };
    kill_region(region.as_ref(), &mut host);
}

/// The event's change, as Post.update and destroy() made it.
pub fn change_of(event: &crate::gen::PostEvent) -> Change {
    match event.kind {
        PostChange::Captured => Change::Captured { by: event.by.unwrap_or(f64::NAN) },
        PostChange::Neutralized => Change::Neutralized { from: event.from.unwrap_or(f64::NAN) },
        PostChange::Lost => Change::Destroyed { from: event.from.unwrap_or(f64::NAN) },
    }
}
