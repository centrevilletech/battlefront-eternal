//! Battle-tail readers over the shared native registry. Captures and awards
//! revisit its insertion sequence; unit facts are scalar reads, never feeds.
use super::world::{Member, Victim};
use crate::gen::{Member as Key, Store, Unit};
use crate::store::{DefId, Nullable, Ref};

/// Non-soldier registry entries belong to the native hull/building owners.
/// They still count toward the 64-member award cap when inside the region.
pub trait Targets { fn member(&mut self, s: &Store, key: Key) -> Member; }
#[derive(Default)]
pub struct Registry { captures: Vec<Option<bool>> }
impl Registry {
    pub fn class(&mut self, def: DefId, captures: bool) {
        self.captures.resize(self.captures.len().max(def.0 as usize + 1), None);
        self.captures[def.0 as usize] = Some(captures);
    }
    /// No Store or unit borrow survives the visitor. A preceding capture or
    /// death can change the next member, including the registry's live length.
    pub fn visit(&self, s: &mut Store, player: Option<Ref<Unit>>, mounted: bool, targets: &mut impl Targets,
        visit: &mut dyn FnMut(&mut Store, Member)) {
        self.visit_with(s, player, mounted, targets, &mut |s, _, m| visit(s, m));
    }
    /// The callback can use a short reborrow of the same owner services after
    /// observing a member. No target-table borrow survives that action.
    pub fn visit_with<T: Targets>(&self, s: &mut Store, player: Option<Ref<Unit>>, mounted: bool, targets: &mut T,
        visit: &mut dyn FnMut(&mut Store, &mut T, Member)) {
        let mut i = 0;
        while let Some(&key) = s.units.list.get(i) {
            i += 1;
            let member = match key {
                Key::Unit(r) => {
                    let u = s.unit.get(r).expect("registry soldier"); let p = u.position();
                    Member { soldier: true, alive: u.alive, removed: u.removed, seated: u.seat.is_some(),
                        held: matches!(u.held, Nullable::Value(_)),
                        captures: self.captures[u.class_def.0 as usize].expect("prepared capture class"),
                        player_mounted: player == Some(r) && mounted, team: u.team,
                        position: [p.x, p.y, p.z], character: u.character }
                }
                _ => targets.member(s, key),
            };
            visit(s, targets, member);
        }
    }
    /// KillRegion builds the candidate array after its effects, soldiers
    /// before hulls. Later containment reads use Victim::position_in.
    pub fn victims(s: &Store, into: &mut Vec<Victim>) {
        into.clear();
        for &key in &s.units.list {
            let Key::Unit(r) = key else { continue };
            let u = s.unit.get(r).expect("kill-region soldier");
            if u.alive && !u.removed {
                let p = u.position(); into.push(Victim::Unit { id: r, position: [p.x, p.y, p.z] });
            }
        }
        for &r in &s.vehicles.vehicles {
            let v = s.vehicle.get(r).expect("kill-region hull");
            if v.alive { into.push(Victim::Hull { id: r, position: v.position }); }
        }
    }
}
