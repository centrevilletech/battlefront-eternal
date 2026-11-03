//! The factory's pools (src/units.js UnitFactory): a unit's body goes back
//! to its class's pool when it is removed, and a spawn of that class takes
//! the last one returned (a stack), or a new one built when none is free.

use crate::store::StrId;

/// Each class's free units, by class name, in the order the JavaScript's
/// Map holds them (each class's list in the order bodies came back).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pools {
    pub free: Vec<(StrId, Vec<u32>)>,
}

impl Pools {
    /// acquire(className): the last body returned of that class, if any
    /// (the caller builds one when none is free).
    pub fn acquire(&mut self, class: StrId) -> Option<u32> {
        self.free.iter_mut().find(|(c, _)| *c == class)?.1.pop()
    }

    /// release(unit): the body back in its class's pool, unless it is there.
    pub fn release(&mut self, class: StrId, unit: u32) {
        let at = match self.free.iter().position(|(c, _)| *c == class) {
            Some(at) => at,
            None => {
                self.free.push((class, Vec::new()));
                self.free.len() - 1
            }
        };
        let list = &mut self.free[at].1;
        if !list.contains(&unit) {
            list.push(unit);
        }
    }
}
