//! Connection command owners retain queues independently of facade mirrors.
//! Only admitted commands enter from the host; publication borrows the same
//! consumed held/view values rather than serializing every actor each time.
use crate::{gen::*,store::{handles::Pool,StrId,Strings}};
// WiView keeps protocol presence independent of numeric values and truthiness.
const INVERT_MOUSE:u16=1;const FIRST_PERSON:u16=2;const FLIGHT:u16=4;const FLIGHT_INVERT:u16=8;
const GAMEPAD:u16=16;const MOUSE_SCALE:u16=32;const ASPECT:u16=64;const VIEW_TICK:u16=128;
const SENSITIVITY:u16=256;const CURVE:u16=512;const BINDINGS:u16=1024;
pub struct Owner {pub mailboxes:Pool<Mailbox>}
impl Default for Owner {fn default()->Self {Self {mailboxes:Pool::new(15)}}}
pub struct Mailbox {pub state:ImState,pub publication:WireOwnFacts,names:[StrId;7],codes:Vec<StrId>,taken:ImTaken,retired:Vec<f64>}
impl Mailbox {
    pub fn new(epoch:f64,capacity:f64,strings:&mut Strings)->Self {
        let names=["mouseScale","invertMouse","firstPerson","aspect","viewTick","flightSettings","gamepad"].map(|s|strings.intern_str(s));
        let mut codes=Vec::with_capacity(112);
        for n in b'A'..=b'Z' {codes.push(strings.intern_str(&format!("Key{}",n as char)));}
        for n in 0..10 {codes.push(strings.intern_str(&format!("Digit{n}")));}
        for n in 1..=12 {codes.push(strings.intern_str(&format!("F{n}")));}
        for n in 0..10 {codes.push(strings.intern_str(&format!("Numpad{n}")));}
        for name in ["Space","Enter","Escape","Tab","Backquote","CapsLock","ShiftLeft","ShiftRight","ControlLeft","ControlRight","AltLeft","AltRight","ArrowUp","ArrowDown","ArrowLeft","ArrowRight","Backspace","Delete","Insert","Home","End","PageUp","PageDown","Minus","Equal","BracketLeft","BracketRight","Backslash","Semicolon","Quote","Comma","Period","Slash","NumpadEnter","NumpadAdd","NumpadSubtract","NumpadMultiply","NumpadDivide","NumpadDecimal","Mouse0","Mouse1","Mouse2","Mouse3","Mouse4","WheelUp","WheelDown"] {codes.push(strings.intern_str(name));}
        Self {state:ImState {epoch,capacity,queue:Vec::with_capacity(capacity.clamp(0.,4096.) as usize),view:Vec::with_capacity(7),..Default::default()},
            publication:WireOwnFacts {held_valid:true,recon_aspect:1.,view:WiView {profile_found:true,..Default::default()},spawn:WireSpawnFacts {valid:true,..Default::default()},..Default::default()},names,codes,
            taken:ImTaken {actions:Vec::with_capacity(2048),look:Vec::with_capacity(2048),human:Vec::with_capacity(16*capacity as usize),fire:Vec::with_capacity(16*capacity as usize)},retired:Vec::with_capacity(capacity as usize)}
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
    }
    true
}
