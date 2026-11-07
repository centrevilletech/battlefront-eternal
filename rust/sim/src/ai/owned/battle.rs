//! Prepared mappings from authored post/vehicle handles to native F4 and F2
//! facts. Building rebuilds advance F4 life without changing the authored post.
use crate::{gen::{Building,F4Handle,F4ObjectKind,Vehicle},store::Ref,
    battle::{live_phase,ownership::Rows}};

pub struct Bindings {
    buildings:Vec<Option<(Ref<Building>,F4Handle)>>,
    vehicles:Vec<Option<(Ref<Vehicle>,f64)>>,
}
impl Bindings {
    pub fn new()->Self {Self {buildings:Vec::new(),vehicles:Vec::new()}}
    pub fn building(&mut self,reference:Ref<Building>,native:F4Handle) {
        assert_eq!(native.kind,F4ObjectKind::Building);
        let at=reference.slot as usize;
        if self.buildings.len()<=at {self.buildings.resize(at+1,None);}
        self.buildings[at]=Some((reference,native));
    }
    pub fn vehicle(&mut self,reference:Ref<Vehicle>,mask:f64) {
        let at=reference.slot as usize;
        if self.vehicles.len()<=at {self.vehicles.resize(at+1,None);}
        self.vehicles[at]=Some((reference,mask));
    }
    pub fn facts<'a>(&'a self,buildings:&'a Rows<live_phase::Building>)->Facts<'a> {Facts {bindings:self,buildings}}
}
pub struct Facts<'a> {bindings:&'a Bindings,buildings:&'a Rows<live_phase::Building>}
impl super::population::Facts for Facts<'_> {
    fn building_alive(&self,reference:Ref<Building>)->bool {
        let (authored,native)=self.bindings.buildings.get(reference.slot as usize).and_then(Option::as_ref).expect("prepared AI building binding");
        if *authored!=reference {return false;}
        self.buildings.current(native).and_then(|current|self.buildings.get(&current)).is_some_and(|b|b.state.building.alive())
    }
    fn vehicle_mask(&self,reference:Ref<Vehicle>)->f64 {
        let (registered,mask)=self.bindings.vehicles.get(reference.slot as usize).and_then(Option::as_ref).expect("prepared AI vehicle traversal");
        assert_eq!(*registered,reference,"current vehicle traversal binding");*mask
    }
}
