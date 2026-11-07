//! Vehicle agents keep their machine and flyer fields on the running object.
//! Handles borrow those fields only for scalar reads or edits, so a boarding
//! or damage callback can retire the object without borrowing its successor.
use crate::ai::vehicles::{agents::{AgentMachine,FlyAccess,FlyAgent,Fsm},world::Obj};
use std::{cell::RefCell,rc::Rc};
#[derive(Debug)]
struct Fields {fsm:Fsm,fly:FlyAgent,unit:Option<Obj>,pattern:Option<crate::ai::vehicles::patterns::PatternState>}
impl Default for Fields {
    fn default()->Self {
        let mut fsm=Fsm::default();let fly=FlyAgent::new(&mut fsm);
        Self {fsm:Fsm {state:1.,pending:0.},fly,unit:None,pattern:None}
    }
}
#[derive(Clone,Debug)]
pub struct VehicleAgent(Rc<RefCell<Fields>>);
impl VehicleAgent {
    /// Reserved once for every live controller and retained callback scope.
    pub fn new()->Self {Self(Rc::new(RefCell::new(Fields::default())))}
    pub fn reset(&mut self) {
        assert_eq!(Rc::strong_count(&self.0),1,"native vehicle agent scopes end before reuse");
        *self.0.borrow_mut()=Fields::default();
    }
    pub fn initialize(&self,kind:crate::ai::control::world::ForeignKind,unit:Option<Obj>) {
        use crate::ai::control::world::ForeignKind;
        let mut row=self.0.borrow_mut();row.unit=unit;
        match kind {
            ForeignKind::Board=>row.fsm.pending=2.,
            ForeignKind::Unboard=>row.fsm=Fsm {state:0.,pending:1.},
            ForeignKind::Fly=>{let fly=FlyAgent::new(&mut row.fsm);row.fly=fly;},
            ForeignKind::Combat=>unreachable!("combat owns its machine"),
        }
    }
    pub fn state(&self)->Fsm {self.0.borrow().fsm}
    pub fn unit(&self)->Option<Obj> {self.0.borrow().unit}
    pub fn transition(&mut self) {let mut row=self.0.borrow_mut();row.fsm.state=row.fsm.pending;row.fsm.pending=0.;}
    pub fn clear_pending(&mut self) {self.0.borrow_mut().fsm.pending=0.;}
    pub fn terminate(&mut self) {self.0.borrow_mut().fsm.state=crate::ai::vehicles::controllers::DONE;}
}
impl AgentMachine for VehicleAgent {
    fn state(&self)->f64 {self.0.borrow().fsm.state}
    fn request_state(&mut self,state:f64) {self.0.borrow_mut().fsm.pending=state;}
}
impl FlyAccess for VehicleAgent {
    fn fly(&self)->FlyAgent {self.0.borrow().fly}
    fn edit_fly(&mut self,edit:impl FnOnce(&mut FlyAgent)) {edit(&mut self.0.borrow_mut().fly);}
}

impl crate::ai::vehicles::patterns::PatternAccess for VehicleAgent {
    fn pattern(&self)->crate::ai::vehicles::patterns::PatternState {self.0.borrow().pattern.unwrap_or_default()}
    fn edit(&mut self,edit:impl FnOnce(&mut crate::ai::vehicles::patterns::PatternState)) {edit(self.0.borrow_mut().pattern.get_or_insert_with(Default::default));}
}
