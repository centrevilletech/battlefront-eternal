//! Native combat calls the core's acceptance rules with shared weapon policy
//! and current canonical world facts. It never calls back into a borrowed Core.
use crate::ai::{
    combat::{
        agent::can_engage_shared,
        weapons_ai::Shared,
        world::{Obj, QueryFilter, World},
    },
    control::{controller::EnhancedMemory, policy::Scene as PolicyScene, world::UKey},
    nav::math::Vec3,
};
pub trait Facts {
    fn policy_post_contains(&self, index: usize, position: Vec3) -> bool;
    fn policy_hint_forward(&self, index: usize) -> Vec3;
    fn policy_memory(&mut self, controller: Obj) -> EnhancedMemory;
}
pub struct Scene<'a, W: World + Facts + ?Sized> {
    world: &'a mut W,
    shared: Shared,
    controller: Obj,
    agent: Obj,
}
impl<'a, W: World + Facts + ?Sized> Scene<'a, W> {
    pub fn new(world: &'a mut W, shared: &Shared, controller: Obj, agent: Obj) -> Self {
        Self {
            world,
            shared: shared.share(),
            controller,
            agent,
        }
    }
}
impl<W: World + Facts + ?Sized> PolicyScene for Scene<'_, W> {
    fn time(&self) -> f64 {
        self.world.time()
    }
    fn kind(&self, unit: UKey) -> f64 {
        self.world.unit(unit).kind
    }
    fn position(&self, unit: UKey) -> Vec3 {
        let p = self.world.unit(unit).position;
        Vec3::new(p.x, p.y, p.z)
    }
    fn identity(&self, unit: UKey) -> (u64, u64) {
        let u = self.world.unit(unit);
        (u.id.unwrap_or(f64::NAN).to_bits(), u.generation.to_bits())
    }
    fn post_contains(&self, post: usize, position: Vec3) -> bool {
        self.world.policy_post_contains(post, position)
    }
    fn hint_forward(&self, hint: usize) -> Vec3 {
        self.world.policy_hint_forward(hint)
    }
    fn can_engage(&mut self, target: UKey) -> bool {
        can_engage_shared(self.world, &mut self.shared, self.controller, Some(target))
    }
    fn board_policy(&mut self, candidate: UKey) -> bool {
        self.world.agent_board_policy(self.agent, candidate)
    }
    fn near_fighting(&mut self, unit: UKey) -> f64 {
        let u = self.world.unit(unit);
        let found = self.world.query(
            &u.position,
            7.,
            QueryFilter::Ally {
                except: unit,
                team: u.team,
            },
        );
        let mut fighting = 0.;
        for other in found.into_iter().take(30) {
            if fighting > 2. {
                break;
            }
            let u = self.world.unit(other);
            if u.kind != 0. {
                continue;
            }
            let Some(controller) = u.controller else {
                continue;
            };
            if controller != self.controller && self.world.ctl(controller).controller_state == 2. {
                fighting += 1.;
            }
        }
        fighting
    }
    fn emit_charge(&mut self, unit: UKey) {
        self.world.emit_vo(unit, "charge", "ChargeSound")
    }
    fn new_memory(&mut self) -> EnhancedMemory {
        self.world.policy_memory(self.controller)
    }
}
