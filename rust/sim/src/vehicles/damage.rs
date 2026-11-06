//! Vehicle.damage, setHealth and dyingTick use the same hooks as movement,
//! without requiring the collision and flight interface for an incoming hit.
//! Each recursive death commits its hull before crew damage or an explosion;
//! resuming reads the live row rather than restoring an earlier health value.
use super::{BodyId, Hull, VehicleWorld};
use crate::gen::OrdnanceOwner;
use three::Vector3;

/// The state-sensitive surface of Vehicle.damage and Vehicle.setHealth.
/// Native combat can pass its existing world-service borrow through these
/// hooks, while ordinary hull ticks and replay worlds retain their interface.
pub trait HullDamageWorld {
    fn accepts_damage(&self) -> bool { true }
    fn commit_hull(&mut self, hull: &Hull);
    fn resume_hull(&mut self, hull: &mut Hull);
    fn damaged_hull(&mut self, hull: usize, amount: f64, attacker: &OrdnanceOwner);
    fn set_damage_body(&mut self, body: BodyId, enabled: bool);
    fn start_respawn(&mut self, hull: usize);
    fn destroy_hull_crew(&mut self, hull: usize);
    fn destroyed_hull(&mut self, hull: usize);
    fn release_hull_crew(&mut self, hull: usize);
    fn explode_hull(&mut self, hull: usize, final_explosion: bool, position: &Vector3);
    fn hull_effect(&mut self, name: &str, position: &Vector3);
}

// VehiclesWorld's existing callbacks keep their exact order and arguments.
// This forwarding performs no import, row copy or allocation of its own.
impl<W: VehicleWorld + ?Sized> HullDamageWorld for W {
    fn accepts_damage(&self) -> bool {
        self.damage_allowed()
    }
    fn commit_hull(&mut self, hull: &Hull) {
        self.current(hull);
    }
    fn resume_hull(&mut self, hull: &mut Hull) {
        self.reload(hull);
    }
    fn damaged_hull(&mut self, hull: usize, amount: f64, attacker: &OrdnanceOwner) {
        self.damaged(hull, amount, attacker);
    }
    fn set_damage_body(&mut self, body: BodyId, enabled: bool) {
        self.enable_body(body, enabled);
    }
    fn start_respawn(&mut self, hull: usize) {
        self.respawn_clock(hull);
    }
    fn destroy_hull_crew(&mut self, hull: usize) {
        self.destroyed_crew(hull);
    }
    fn destroyed_hull(&mut self, hull: usize) {
        self.destroyed(hull);
    }
    fn release_hull_crew(&mut self, hull: usize) {
        self.release_crew(hull);
    }
    fn explode_hull(&mut self, hull: usize, final_explosion: bool, position: &Vector3) {
        self.explosion(hull, final_explosion, position);
    }
    fn hull_effect(&mut self, name: &str, position: &Vector3) {
        self.effect(name, position);
    }
}
