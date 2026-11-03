//! The battle phase (src/game.js updateBattle) on a server world: who
//! counts toward the posts, the posts' tick, each post event (a capture's
//! points for the first 64 registry members in its region, a destructible
//! post's kill region), the command vehicles' posts, then the spawn
//! manager's clock, the battle's rules and each Character's respawn. The
//! local player's spawn screen, his corpse and the HUD are a page's.

use super::objectives::{kill_region, KillRegionHost};
use super::posts::{Capturer, Change, CommandPosts, PostEvent};
use super::rules::{Battle, Character};
use super::spawning::{ClassicSpawning, Draws};
use super::world::{BattleWorld, Victim};
use crate::battle::mobile::MobileEvent;
use three::Vector3;

fn v3(p: [f64; 3]) -> Vector3 {
    Vector3::new(p[0], p[1], p[2])
}

/// updateBattle(dt) (game.js:2310). `ai`: the spawn manager when the
/// Classic AI runs; `random`: its generator A; `gameplay`: the gameplay
/// stream (a kill region's draws).
#[allow(clippy::too_many_arguments)]
pub fn update_battle(
    dt: f64,
    battle: &mut Battle,
    posts: &mut CommandPosts,
    mut ai: Option<&mut ClassicSpawning>,
    stats_active: impl Fn(usize, &Character) -> bool,
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
    posts.update(dt, &capturers, world);
    let events: Vec<PostEvent> = posts.events.drain(..).collect();
    for event in events {
        // The native award pass revisits the capture region's first 64
        // objects, crediting the living Characters among them.
        if let Change::Captured { .. } = event.change {
            let mut seen = 0;
            for m in world.members() {
                if m.removed || !posts.posts[event.post].contains(&v3(m.position)) {
                    continue;
                }
                seen += 1;
                if seen > 64 {
                    break;
                }
                if m.alive {
                    battle.point(m.character);
                }
            }
        }
        if let Change::Destroyed { from } = event.change {
            objective_destroyed(event.post, from, battle, ai.as_deref_mut(), posts, world, gameplay);
        }
        world.post_event(&event, &mut battle.characters);
    }
    // The command vehicles' posts: one whose vehicle died is lost, a point
    // for whoever hurt it last unless he is its owner's friend.
    for event in posts.mobile_events(world) {
        if let MobileEvent::Lost { vehicle, from, .. } = event {
            let destroyer = vehicle.and_then(|v| world.vehicle_attacker(v));
            if let Some(d) = destroyer {
                let relation = world.relation(battle.characters[d as usize].team, from);
                battle.post_destroyed(Some(d), relation);
            }
        }
        world.mobile_event(&event, &mut battle.characters);
    }
    if let Some(sp) = ai.as_deref_mut() {
        sp.clock(dt, posts, world);
    }
    battle.update(dt, posts, &stats_active, world);
    if let Some(sp) = ai {
        sp.update_characters(dt, battle, posts, world, random);
    }
}

/// A destructible post's kill region as the battle runs it: the gameplay
/// stream's draws, the page's effects, the living soldiers and hulls.
struct Region<'w> {
    world: &'w mut dyn BattleWorld,
    gameplay: &'w mut dyn Draws,
    battle: &'w mut Battle,
    spawning: Option<&'w mut ClassicSpawning>,
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
        self.world.kill_victim(self.victims[victim], self.battle, self.spawning.as_deref_mut());
    }
}

/// objectiveDestroyed(event) (game.js:2370): a post point for whoever hurt
/// the building last unless he is its owner's friend, then its kill region.
fn objective_destroyed(post: usize, from: f64, battle: &mut Battle, spawning: Option<&mut ClassicSpawning>, posts: &CommandPosts, world: &mut dyn BattleWorld, gameplay: &mut dyn Draws) {
    let destroyer = posts.posts[post].building.and_then(|b| world.building_attacker(b));
    if let Some(d) = destroyer {
        let relation = world.relation(battle.characters[d as usize].team, from);
        battle.post_destroyed(Some(d), relation);
    }
    let mut host = Region { world, gameplay, battle, spawning, victims: Vec::new() };
    kill_region(posts.posts[post].kill_region.as_ref(), &mut host);
}
