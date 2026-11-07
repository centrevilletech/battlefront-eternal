//! Vehicle services acquire their AI state only for pure field reads/edits.
//! No borrow remains while boarding, damage or a Core callback reenters them.
use crate::ai::vehicles::{VehicleAI,service::VehicleStorage};
use std::{cell::RefCell,rc::Rc};

#[derive(Clone)]
pub struct Authority(Rc<RefCell<VehicleAI>>);
impl Authority {
    pub fn new(value:VehicleAI)->Self {Self(Rc::new(RefCell::new(value)))}
    /// Battle teardown follows the last native frame and callback scope.
    pub fn finish(self)->VehicleAI {
        Rc::try_unwrap(self.0).unwrap_or_else(|_|panic!("native vehicle frames must end before battle teardown")).into_inner()
    }
}
impl VehicleStorage for Authority {
    fn read<R>(&self,read:impl FnOnce(&VehicleAI)->R)->R {read(&self.0.borrow())}
    fn write<R>(&mut self,write:impl FnOnce(&mut VehicleAI)->R)->R {write(&mut self.0.borrow_mut())}
}

/// Cold capability facts travel with the shared native vehicle authority.
/// Cloning a binding shares state without copying reservations or policies.
#[derive(Clone)]
pub struct Binding {
    pub authority:Authority,
    pub settings:crate::ai::vehicles::world::Settings,
    pub navigation:bool,
    pub population:bool,
}
