//! Connection command owners retain queues independently of facade mirrors.
//! Only admitted commands enter from the host; publication borrows the same
//! consumed held/view values rather than serializing every actor each time.
use crate::{gen::*,store::{handles::Pool,StrId,Strings}};
// WiView keeps protocol presence independent of numeric values and truthiness.
const INVERT_MOUSE:u16=1;const FIRST_PERSON:u16=2;const FLIGHT:u16=4;const FLIGHT_INVERT:u16=8;
const GAMEPAD:u16=16;const MOUSE_SCALE:u16=32;const ASPECT:u16=64;const VIEW_TICK:u16=128;
const SENSITIVITY:u16=256;const CURVE:u16=512;const BINDINGS:u16=1024;
pub struct Owner {pub mailboxes:Pool<Mailbox>}
impl Owner {pub const fn new()->Self {Self {mailboxes:Pool::new(15)}}}
impl Default for Owner {fn default()->Self {Self::new()}}
pub struct Mailbox {pub state:ImState,pub publication:WireOwnFacts,names:[StrId;7],codes:Vec<StrId>,taken:ImTaken,retired:Vec<f64>}
impl Mailbox {
    pub fn new(epoch:f64,capacity:f64,codes:Vec<StrId>,strings:&mut Strings)->Option<Self> {
        // The host supplies registry order, which is also the wire bit order.
        // Check identities and bounded names before retaining a connection.
        if !capacity.is_finite() || capacity.fract()!=0. || !(0. ..=4096.).contains(&capacity)
            || codes.len()>128 || codes.iter().enumerate().any(|(i,id)|codes[..i].contains(id)
                || strings.list.get(id.0 as usize).is_none_or(|s|s.is_empty()||s.len()>64)) {return None;}
        let names=["mouseScale","invertMouse","firstPerson","aspect","viewTick","flightSettings","gamepad"].map(|s|strings.intern_str(s));
        Some(Self {state:ImState {epoch,capacity,queue:Vec::with_capacity(capacity.clamp(0.,4096.) as usize),view:Vec::with_capacity(7),..Default::default()},
            publication:WireOwnFacts {held_valid:true,recon_aspect:1.,view:WiView {profile_found:true,..Default::default()},spawn:WireSpawnFacts {valid:true,..Default::default()},..Default::default()},names,codes,
            taken:ImTaken {actions:Vec::with_capacity(2048),look:Vec::with_capacity(2048),human:Vec::with_capacity(16*capacity as usize),fire:Vec::with_capacity(16*capacity as usize)},retired:Vec::with_capacity(capacity as usize)})
    }
    // WiView's presence flags remain independent from numeric values. In
    // particular a viewTick of zero and a signed-zero axis stay observable.
    pub fn view(&mut self,key:usize,present:bool,value:&WiView) {
        let out=&mut self.publication.view;
        let mask=match key {0=>MOUSE_SCALE,1=>INVERT_MOUSE,2=>FIRST_PERSON,3=>ASPECT,4=>VIEW_TICK,5=>FLIGHT|FLIGHT_INVERT|SENSITIVITY|CURVE|BINDINGS,6=>GAMEPAD,_=>return};
        let flags=out.flags as u16;let incoming=value.flags as u16;
        out.flags=((flags&!mask)|if present {incoming&mask}else {0}) as f64;
        match key {0=>out.mouse_scale=value.mouse_scale,3=>out.aspect=value.aspect,4=>out.view_tick=value.view_tick,
            5=>{out.sensitivity=value.sensitivity;out.curve=value.curve;out.profile=value.profile;out.profile_found=value.profile_found;},
            6=>{out.axes=value.axes;out.buttons=value.buttons;},_=>{}}
    }
    fn consumed(&mut self,entry:&ImEntry) {
        self.publication.held.0.fill(0);self.publication.held_valid=true;
        for code in &entry.codes {if let Some(bit)=self.codes.iter().position(|id|id==code).filter(|&bit|bit<128) {self.publication.held.0[bit>>3]|=1<<(bit&7);} else {self.publication.held_valid=false;}}
        for field in &entry.view {if let Some(key)=self.names.iter().position(|&id|id==field.key) {self.view(key,true,&entry.publication);}}
    }
    /// The generated exchange borrows results only until it has encoded them.
    /// Return those vectors before the next consume instead of warming anew.
    pub fn recycle(&mut self,result:&mut ImResult) {
        if let Some(taken)=result.command.take() {self.taken=taken;}
        self.retired=std::mem::take(&mut result.retired);
    }
    pub fn run(&mut self,mut input:ImInput,strings:&mut Strings)->ImResult {
        // Transport validation precedes string interning; native callers must
        // still pass only registered identities before the queue retains them.
        if input.entry.as_ref().is_some_and(|entry| entry.codes.len()>64 || entry.codes.iter().any(|code|
            !self.codes.contains(code) || strings.list.get(code.0 as usize).is_none_or(|units|units.len()>64))) {
            return ImResult {error:Some(strings.intern_str("invalid input payload")),..Default::default()};
        }
        if matches!(input.op,ImOperation::Clear)||matches!(input.op,ImOperation::Take)&&input.generation!=self.state.generation {self.publication.held.0.fill(0);self.publication.held_valid=true;}
        if matches!(input.op,ImOperation::Clear) {self.publication.view.flags=((self.publication.view.flags as u16)&!(GAMEPAD|VIEW_TICK)) as f64;}
        let input_op=input.op;
        input.state=std::mem::take(&mut self.state);
        let taken=std::mem::take(&mut self.taken);let retired=std::mem::take(&mut self.retired);
        let (mut result,spare)=super::mailbox::run_reusing(input,strings,taken,retired,|entry|self.consumed(entry));
        self.taken=spare;
        if matches!(input_op,ImOperation::Take) && self.publication.view.aspect.is_finite() && self.publication.view.aspect>0. {self.publication.recon_aspect=self.publication.view.aspect;}
        self.publication.discarded=result.state.discarded;self.state=std::mem::take(&mut result.state);result
    }
}

/// Check the owned command's list descriptors before generated decoding.
/// Only admitted packet values have variable storage; a take has no entity
/// state to copy in, and malformed spans cannot index module memory.
pub fn valid_call(row:&[f64],heap:&[f64])->bool {
    use crate::gen::{InputMailboxCall as C,ImEntry as E};
    let integer=|n:f64,max:usize|n.is_finite()&&n>=0.&&n.fract()==0.&&n<=max as f64;
    if row.len()<C::WORDS || !integer(row[C::OP],3) {return false;}
    if row[C::ENTRY]==1. {return true;}
    if row[C::ENTRY]!=0. {return false;}
    let entry=&row[C::ENTRY+1..];
    for (field,width,max) in [(E::CODES,1,64),(E::ACTIONS,1,2048),(E::LOOK,ImLook::WORDS,2048),
        (E::VIEW,ImField::WORDS,7),(E::HUMAN,1,16),(E::FIRE,ImFire::WORDS,16)] {
        if !integer(entry[field],heap.len())||!integer(entry[field+1],max) {return false;}
        let end=(entry[field] as usize).saturating_add((entry[field+1] as usize).saturating_mul(width));
        if end>heap.len() {return false;}
        if field==E::CODES && heap[entry[field] as usize..end].iter().any(|id|!integer(*id,u32::MAX as usize)) {return false;}
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gen::ImEntry;

    #[test]
    fn cumulative_look_and_ack_use_only_consumed_matching_lives() {
        let mut strings = Strings::default();
        let input = ImInput { op: ImOperation::Take, tick: 5.0, generation: 2.0,
            state: ImState { epoch: 1.0, capacity: 256.0, queue: vec![
                ImEntry { sequence: 1.0, tick: 1.0, generation: 1.0, look_total: Some([100.0, 50.0]), ..Default::default() },
                ImEntry { sequence: 2.0, tick: 3.0, generation: 2.0, look_total: Some([12.0, -4.0]), ..Default::default() },
                ImEntry { sequence: 3.0, tick: 9.0, generation: 2.0, look_total: Some([20.0, 8.0]), ..Default::default() },
            ], ..Default::default() }, ..Default::default() };
        let mut owner=Mailbox::new(1.,256.,Vec::new(),&mut strings).unwrap();
        owner.state=input.state;
        let mut out=owner.run(ImInput {state:ImState::default(),..input},&mut strings);
        out.state=std::mem::take(&mut owner.state);
        assert!(out.error.is_none()); assert_eq!(out.state.ack, 2.0); assert_eq!(out.state.discarded, 1.0);
        assert_eq!(out.state.ack_tick, 5.0); assert_eq!(out.state.state_tick, 3.0); assert_eq!(out.state.late, 1.0);
        assert_eq!(out.state.queue.len(), 1); assert_eq!(out.command.unwrap().look[0].point, [12.0, -4.0]);
    }
    #[test]
    fn oversized_records_retire_with_partial_state_without_growing_scratch() {
        for (actions,looks,humans) in [(2049,0,0),(0,2049,0),(0,0,17)] {
            let mut strings=Strings::default();let jump=strings.intern_str("jump");
            let total=(looks==0).then_some([-0.0,12.5]);
            let input=ImInput {op:ImOperation::Take,tick:2.0,generation:1.0,
                state:ImState {capacity:1.0,generation:1.0,queue:vec![ImEntry {
                    identity:7.0,sequence:1.0,tick:1.0,generation:1.0,down:17.0,
                    actions:vec![jump;actions],look:vec![ImLook::default();looks],
                    human:vec![1.0;humans],look_total:total,..Default::default()
                }],..Default::default()},..Default::default()};
            let mut owner=Mailbox::new(1.,1.,Vec::new(),&mut strings).unwrap();
            owner.state=input.state;
            let capacities=(owner.taken.actions.capacity(),owner.taken.look.capacity(),owner.taken.human.capacity(),owner.taken.fire.capacity());
            let mut out=owner.run(ImInput {state:ImState::default(),..input},&mut strings);
            out.state=std::mem::take(&mut owner.state);
            assert_eq!(strings.units(out.error.unwrap()),"input batch overflow".encode_utf16().collect::<Vec<_>>());
            assert!(out.command.is_none());assert!(out.state.queue.is_empty());assert_eq!(out.retired,[7.0]);
            assert_eq!(out.state.ack,1.0);assert_eq!(out.state.ack_tick,2.0);assert_eq!(out.state.down,Some(17.0));
            assert_eq!(out.state.look_total.map(f64::to_bits),total.unwrap_or([0.0;2]).map(f64::to_bits));
            assert_eq!((owner.taken.actions.capacity(),owner.taken.look.capacity(),owner.taken.human.capacity(),owner.taken.fire.capacity()),capacities);
            owner.state=std::mem::take(&mut out.state);owner.recycle(&mut out);
            let recovered=owner.run(ImInput {op:ImOperation::Take,tick:3.0,generation:1.0,..Default::default()},&mut strings);
            assert!(recovered.error.is_none());
        }
    }

}
