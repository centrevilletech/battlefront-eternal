//! Connection command owners retain queues independently of facade mirrors.
//! Only admitted commands enter from the host; publication borrows the same
//! consumed held/view values rather than serializing every actor each time.
use crate::{gen::*,store::{handles::Pool,StrId,Strings}};
// WiView keeps protocol presence independent of numeric values and truthiness.
/// The view record's presence bits (VIEW).
pub mod view_flags {
    pub const INVERT_MOUSE:u16=1;pub const FIRST_PERSON:u16=2;pub const FLIGHT:u16=4;pub const FLIGHT_INVERT:u16=8;
    pub const GAMEPAD:u16=16;pub const MOUSE_SCALE:u16=32;pub const ASPECT:u16=64;pub const VIEW_TICK:u16=128;
    pub const SENSITIVITY:u16=256;pub const CURVE:u16=512;pub const BINDINGS:u16=1024;
}
use view_flags::*;
pub struct Owner {pub mailboxes:Pool<Mailbox>,
    /// The match's connected players, whose commands these mailboxes carry.
    pub players:crate::players::owner::Players}
impl Owner {pub const fn new()->Self {Self {mailboxes:Pool::new(15),players:crate::players::owner::Players::new()}}}
impl Default for Owner {fn default()->Self {Self::new()}}
/// The command the players' phase reads is the last take's lists plus the
/// held codes of the last consumed entry (`InputMailbox.take`'s `down`).
/// Controls a second-game life sends beside its commands ride the same queue:
/// each waits beside the command of its sequence and is taken with it, and
/// only the life it was sent for reads it.
struct LifeInputs<T> {
    life: Option<crate::units::infantry::contact::LifeId>,
    pending: std::collections::VecDeque<(f64, crate::units::infantry::contact::LifeId, T)>,
    current: T,
}
impl<T: Copy + Default> LifeInputs<T> {
    fn new(capacity: usize) -> Self {
        Self { life: None, pending: std::collections::VecDeque::with_capacity(capacity), current: T::default() }
    }
    /// Beside the newest queued command only, when it is this life's.
    fn queue(&mut self, queue: &[ImEntry], sequence: f64, life: crate::units::infantry::contact::LifeId, input: T) -> bool {
        if !queue.last().is_some_and(|entry| entry.sequence == sequence && entry.generation == f64::from(life.generation)) {
            return false;
        }
        // Old generations can retire without being consumed.
        while self.pending.front().is_some_and(|e| queue.first().is_none_or(|q| q.sequence > e.0)) {
            self.pending.pop_front();
        }
        if self.pending.len() >= self.pending.capacity() { return false; }
        self.pending.push_back((sequence, life, input));
        true
    }
    /// The consumed command's controls become the current ones; `carry` keeps
    /// the presses of the commands consumed before it in the same take.
    fn consume(&mut self, sequence: f64, carry: impl Fn(&mut T, &T)) {
        while self.pending.front().is_some_and(|e| e.0 <= sequence) {
            let Some((seq, life, mut input)) = self.pending.pop_front() else { break };
            if seq == sequence {
                if self.life != Some(life) { self.life = Some(life); self.current = T::default(); }
                carry(&mut input, &self.current);
                self.current = input;
            }
        }
    }
    fn clear(&mut self) {
        self.pending.clear();
        self.current = T::default();
    }
}
pub struct Mailbox {infantry: Option<LifeInputs<crate::units::infantry::Controls>>,
    /// A Space pilot's controls (`space::local`), enabled when the seat driver connects him.
    pilot: Option<LifeInputs<crate::space::local::PilotControls>>,pub rejected:u32,pub state:ImState,pub publication:WireOwnFacts,names:[StrId;7],codes:Vec<StrId>,taken:ImTaken,retired:Vec<f64>,down:Vec<StrId>,down_out:Vec<StrId>,
    /// The packets the players' phase consumed since the host's last exchange:
    /// the host frees their lent producer objects when it hears of them.
    consumed:Vec<f64>,
    /// InputMailbox.push borrows one admitted bank, returned on every retirement.
    entries:Vec<ImEntry>}
impl Mailbox {
    pub fn new(epoch:f64,capacity:f64,codes:Vec<StrId>,strings:&mut Strings)->Option<Self> {
        // The host supplies registry order, which is also the wire bit order.
        // Check identities and bounded names before retaining a connection.
        if !capacity.is_finite() || capacity.fract()!=0. || !(0. ..=4096.).contains(&capacity)
            || codes.len()>128 || codes.iter().enumerate().any(|(i,id)|codes[..i].contains(id)
                || strings.list.get(id.0 as usize).is_none_or(|s|s.is_empty()||s.len()>64)) {return None;}
        let mut entries=Vec::new();
        entries.try_reserve_exact(capacity as usize).ok()?;
        for _ in 0..capacity as usize {entries.push(super::records::reserved_entry()?);}
        let names=["mouseScale","invertMouse","firstPerson","aspect","viewTick","flightSettings","gamepad"].map(|s|strings.intern_str(s));
        Some(Self {infantry:None,pilot:None,rejected:0,state:ImState {epoch,capacity,queue:Vec::with_capacity(capacity.clamp(0.,4096.) as usize),view:Vec::with_capacity(7),..Default::default()},
            publication:WireOwnFacts {held_valid:true,recon_aspect:1.,view:WiView {profile_found:true,..Default::default()},spawn:WireSpawnFacts {valid:true,..Default::default()},..Default::default()},names,codes,
            taken:ImTaken {actions:Vec::with_capacity(2048),look:Vec::with_capacity(2048),human:Vec::with_capacity(16*capacity as usize),fire:Vec::with_capacity(16*capacity as usize)},retired:Vec::with_capacity(2*capacity as usize),down:Vec::with_capacity(64),down_out:Vec::with_capacity(64),consumed:Vec::with_capacity(capacity as usize),entries})
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
    // InputMailbox.view edits keep their own native token until another
    // command replaces the field. Tokens below eight name the borrowed
    // consumer command itself, rather than retaining a producer packet.
    pub fn edit_view(&mut self,key:usize,present:bool,value:&WiView) {
        let Some(&name)=self.names.get(key) else {return};
        self.view(key,present,value);
        if present {
            if let Some(field)=self.state.view.iter_mut().find(|field|field.key==name) {field.value=key as f64;}
            else if self.state.view.len()<7 {self.state.view.push(ImField {key:name,value:key as f64});}
        } else {self.state.view.retain(|field|field.key!=name);}
    }
    pub(crate) fn enable_infantry(&mut self) {
        self.infantry = Some(LifeInputs::new(self.state.capacity as usize));
    }
    pub(crate) fn queue_infantry(&mut self, sequence: f64, life: crate::units::infantry::contact::LifeId, input: crate::units::infantry::Controls) {
        if let Some(extra) = &mut self.infantry {
            extra.queue(&self.state.queue, sequence, life, input);
        }
    }
    /// A pilot connected again keeps the controls already queued.
    pub(crate) fn enable_pilot(&mut self) {
        if self.pilot.is_none() { self.pilot = Some(LifeInputs::new(self.state.capacity as usize)); }
    }
    /// Controls with a number that is not finite never ride.
    pub(crate) fn queue_pilot(&mut self, sequence: f64, life: crate::units::infantry::contact::LifeId, input: crate::space::local::PilotControls) -> bool {
        input.is_finite() && self.pilot.as_mut().is_some_and(|extra| extra.queue(&self.state.queue, sequence, life, input))
    }
    /// This tick's pilot controls for this life (none for another).
    pub(crate) fn pilot_controls(&self, life: crate::units::infantry::contact::LifeId) -> crate::space::local::PilotControls {
        self.pilot.as_ref().filter(|extra| extra.life == Some(life)).map_or_else(Default::default, |extra| extra.current)
    }
    pub(crate) fn infantry_controls(&mut self, life: crate::units::infantry::contact::LifeId) -> crate::units::infantry::Controls {
        let Some(extra) = self.infantry.as_mut().filter(|x| x.life == Some(life)) else { return Default::default() };
        let sample = extra.current;
        extra.current.block_pressed = false;
        extra.current.offhand_pressed = false;
        extra.current.lock_pressed = false;
        extra.current.melee_pressed = 0;
        extra.current.roll = false;
        sample
    }
    fn consumed(&mut self,entry:&ImEntry) {
        if let Some(extra) = &mut self.infantry {
            extra.consume(entry.sequence, |input, current| {
                input.block_pressed |= current.block_pressed;
                input.offhand_pressed |= current.offhand_pressed;
                input.lock_pressed |= current.lock_pressed;
                input.roll |= current.roll;
                input.melee_pressed |= current.melee_pressed;
            });
        }
        if let Some(extra) = &mut self.pilot { extra.consume(entry.sequence, |input, current| input.carry_edges(*current)); }
        self.down.clear();self.down.extend(entry.codes.iter().copied().take(64));
        self.publication.held.0.fill(0);self.publication.held_valid=true;
        for code in &entry.codes {if let Some(bit)=self.codes.iter().position(|id|id==code).filter(|&bit|bit<128) {self.publication.held.0[bit>>3]|=1<<(bit&7);} else {self.publication.held_valid=false;}}
        for field in &entry.view {if let Some(key)=self.names.iter().position(|&id|id==field.key) {self.view(key,true,&entry.publication);}}
    }
    /// The generated exchange borrows results only until it has encoded them.
    /// Return those vectors before the next consume instead of warming anew.
    pub fn recycle(&mut self,result:&mut ImResult) {
        if let Some(taken)=result.command.take() {self.taken=taken;}
        // An early refusal never borrowed this buffer. Keep its reservation
        // rather than replacing it with the empty default error result.
        if result.retired.capacity()>0 {self.retired=std::mem::take(&mut result.retired);}
    }
    /// The players' phase takes this tick's command (InputMailbox.take):
    /// true when it was admitted; a refused burst leaves an empty command,
    /// where the host threw and lost the tick.
    pub fn take_command(&mut self,tick:f64,generation:f64,strings:&mut Strings)->bool {
        if let Some(extra) = &mut self.infantry {
            if self.state.generation != generation { extra.current = Default::default(); }
            extra.current.roll = false;
            extra.current.block_pressed = false;
            extra.current.offhand_pressed = false;
            extra.current.melee_pressed = 0;
        }
        if let Some(extra) = &mut self.pilot {
            if self.state.generation != generation { extra.current = Default::default(); }
            extra.current.clear_edges();
        }
        let mut result=self.run(ImInput {op:ImOperation::Take,tick,generation,..Default::default()},strings);
        let ok=result.error.is_none()&&result.command.is_some();
        self.keep_consumed(&result);
        self.recycle(&mut result);
        if !ok {self.taken.actions.clear();self.taken.look.clear();self.taken.human.clear();self.taken.fire.clear();}
        ok
    }
    /// InputMailbox.clear.
    pub fn clear_command(&mut self,strings:&mut Strings) {
        if let Some(extra) = &mut self.infantry { extra.clear(); }
        if let Some(extra) = &mut self.pilot { extra.clear(); }
        let mut result=self.run(ImInput {op:ImOperation::Clear,..Default::default()},strings);
        self.keep_consumed(&result);
        self.recycle(&mut result);
    }
    /// A native take's retired identities, kept for the host (bounded by the
    /// queue: a packet retires once, and only pushes refill the queue).
    fn keep_consumed(&mut self,result:&ImResult) {
        for &id in &result.retired {if self.consumed.len()<self.consumed.capacity() {self.consumed.push(id);}}
    }
    /// The host's exchange learns of the natively consumed packets too
    /// (run reads `retired`).
    pub fn lend_consumed(&mut self,result:&mut ImResult) {result.retired.extend(self.consumed.drain(..));}
    /// The host's result lends this retained list for the held codes.
    pub fn lend_down(&mut self,result:&mut ImResult) {result.down=std::mem::take(&mut self.down_out);result.down.clear();result.down.extend_from_slice(&self.down);}
    pub fn return_down(&mut self,result:&mut ImResult) {self.down_out=std::mem::take(&mut result.down);}
    /// The last take's lists.
    pub fn taken(&self)->&ImTaken {&self.taken}
    /// The held codes of the last consumed entry.
    pub fn down(&self)->&[StrId] {&self.down}
    /// Whether the command carries the view field (its key's index in
    /// InputMailbox's `keys`: mouseScale, invertMouse, firstPerson, aspect,
    /// viewTick, flightSettings, gamepad).
    pub fn present(&self,key:usize)->bool {
        self.names.get(key).is_some_and(|name|self.state.view.iter().any(|field|field.key==*name))
    }
    /// InputMailbox's generated exchange lends the same banks for push and
    /// take. Decode only after validation, and never grow a full queue.
    pub fn exchange<'a>(&mut self,calls:&'a mut crate::store::calls::Calls,strings:&mut Strings)->Option<&'a [f64]> {
        let (row,heap)=calls.args();
        if !valid_call(row,heap) {self.rejected=self.rejected.saturating_add(1);return None;}
        let mut input=super::records::call(row);
        let mut result=if matches!(input.op,ImOperation::Push) && row[InputMailboxCall::ENTRY]==0. {
            if let Some(mut entry)=self.entries.pop() {
                super::records::read_entry(&mut entry,&row[InputMailboxCall::ENTRY+1..],heap);
                input.entry=Some(entry);self.run(input,strings)
            } else {
                self.refuse("input queue full",strings)
            }
        } else {self.run(input,strings)};
        self.lend_down(&mut result);self.lend_consumed(&mut result);
        result.state=std::mem::take(&mut self.state);
        // Inspection owns queue serialization; an ordinary exchange publishes
        // only consumed values and counters, just as InputMailbox.run does.
        let queue=std::mem::take(&mut result.state.queue);
        calls.result(ImResult::WORDS,|row,heap|result.put(row,heap));
        result.state.queue=queue;self.state=std::mem::take(&mut result.state);
        self.recycle(&mut result);self.return_down(&mut result);
        Some(&calls.block.words)
    }
    // A refusal can still return identities consumed by the native phase.
    // Lend its retained receipt bank too, even though no command was queued.
    fn refuse(&mut self,reason:&str,strings:&mut Strings)->ImResult {
        self.rejected=self.rejected.saturating_add(1);
        let mut retired=std::mem::take(&mut self.retired);retired.clear();
        ImResult {retired,error:Some(strings.intern_str(reason)),..Default::default()}
    }
    pub fn run(&mut self,mut input:ImInput,strings:&mut Strings)->ImResult {
        // Transport validation precedes string interning; native callers must
        // still pass only registered identities before the queue retains them.
        // InputMailbox.push refuses each bad record at this connection, before
        // its queue or publication can change. A later valid command still runs.
        if matches!(input.op,ImOperation::Push) {
            let reason=input.entry.as_ref().map_or(Some("invalid movement command"),|entry| {
                super::mailbox::invalid_entry(entry,&self.codes,strings).or_else(|| {
                    if self.state.queue.last().is_some_and(|last|entry.tick<last.tick) {Some("input tick moved backwards")}
                    else if self.state.queue.len() as f64>=self.state.capacity {Some("input queue full")}
                    else {None}
                })
            });
            if let Some(reason)=reason {
                if let Some(entry)=input.entry.take() {super::records::recycle(&mut self.entries,entry);}
                return self.refuse(reason,strings);
            }
        }
        if matches!(input.op,ImOperation::Take) && (!super::actions::safe_integer(input.tick)||input.tick<0.
            ||!super::actions::safe_integer(input.generation)||input.generation<0.) {
            self.rejected=self.rejected.saturating_add(1);input.tick=self.state.ack_tick;input.generation=self.state.generation;
        }
        if matches!(input.op,ImOperation::Clear)||matches!(input.op,ImOperation::Take)&&input.generation!=self.state.generation {self.publication.held.0.fill(0);self.publication.held_valid=true;}
        if matches!(input.op,ImOperation::Clear) {self.publication.view.flags=((self.publication.view.flags as u16)&!(GAMEPAD|VIEW_TICK)) as f64;}
        let input_op=input.op;
        input.state=std::mem::take(&mut self.state);
        let taken=std::mem::take(&mut self.taken);let retired=std::mem::take(&mut self.retired);
        let mut entries=std::mem::take(&mut self.entries);
        let (mut result,spare)=super::mailbox::run_reusing(input,strings,taken,retired,&mut entries,|entry|self.consumed(entry));
        self.entries=entries;
        self.taken=spare;
        if matches!(input_op,ImOperation::Take) && self.publication.view.aspect.is_finite() && self.publication.view.aspect>0. {self.publication.recon_aspect=self.publication.view.aspect;}
        self.publication.discarded=result.state.discarded;self.state=std::mem::take(&mut result.state);
        // A clear or a new life forgets the held keys with the consumed entry.
        if self.state.down.is_none() {self.down.clear();}
        result
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
        (E::VIEW,ImField::WORDS,7),(E::HUMAN,HsCommand::WORDS,16),(E::FIRE,ImFire::WORDS,16)] {
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
    fn infantry_taps_share_one_sample_only_for_the_addressed_life() {
        use crate::units::infantry::{contact::LifeId, Controls};
        let mut strings = Strings::default();
        let mut mailbox = Mailbox::new(2., 4., Vec::new(), &mut strings).unwrap();
        mailbox.enable_infantry();
        let life = LifeId {slot: 6, slab: 2, generation: 5};
        for (sequence, mask, held) in [(1., 1, 1), (2., 4, 0)] {
            let entry = ImEntry {sequence, generation: 5., ..Default::default()};
            mailbox.state.queue.push(entry.clone());
            mailbox.queue_infantry(sequence, life, Controls {
                melee_pressed: mask, melee_held: held, block_pressed: true, lock_pressed: sequence == 1., ..Default::default()
            });
            mailbox.consumed(&entry);
        }
        assert_eq!(mailbox.infantry_controls(LifeId {slab: 3, ..life}).melee_pressed,0);
        let sample = mailbox.infantry_controls(life);
        assert_eq!((sample.melee_pressed,sample.melee_held,sample.block_pressed,sample.lock_pressed),(5,0,true,true));
        let next = mailbox.infantry_controls(life);
        assert_eq!((next.melee_pressed,next.lock_pressed),(0,false));
        assert!(!mailbox.infantry_controls(LifeId {generation: 6, ..life}).block_pressed);
    }

    #[test]
    fn pilot_controls_ride_beside_their_command_for_their_life_and_lose_their_presses_at_the_next_take() {
        use crate::{space::local::PilotControls, units::infantry::contact::LifeId};
        let mut strings = Strings::default();
        let mut mailbox = Mailbox::new(2., 4., Vec::new(), &mut strings).unwrap();
        let life = LifeId {slot: 6, slab: 2, generation: 5};
        let entry = |sequence| ImEntry {sequence, generation: 5., ..Default::default()};
        let press = PilotControls {use_pressed: true, mouse: [0.5, 0.], ..Default::default()};
        mailbox.state.queue.push(entry(1.));
        assert!(!mailbox.queue_pilot(1., life, press), "not connected");
        mailbox.enable_pilot();
        assert!(!mailbox.queue_pilot(2., life, press), "not the newest command");
        assert!(!mailbox.queue_pilot(1., LifeId {generation: 6, ..life}, press), "another life's command");
        assert!(!mailbox.queue_pilot(1., life, PilotControls {mouse: [f64::NAN, 0.], ..press}), "not finite");
        assert!(mailbox.queue_pilot(1., life, press));
        mailbox.state.queue.push(entry(2.));
        let mut held = PilotControls {mouse: [0.25, 0.], ..Default::default()};
        held.flight.throttle = 1.;
        assert!(mailbox.queue_pilot(2., life, held));
        mailbox.consumed(&entry(1.));
        mailbox.consumed(&entry(2.));
        // Both commands reached this tick: the later one's held controls, both presses and movement.
        let sample = mailbox.pilot_controls(life);
        assert_eq!((sample.use_pressed, sample.mouse, sample.flight.throttle), (true, [0.75, 0.], 1.));
        assert!(!mailbox.pilot_controls(LifeId {slab: 3, ..life}).use_pressed, "another lease of the slot");
        // The next take keeps what is held and drops the presses.
        mailbox.state.queue.clear();
        mailbox.state.generation = 5.;
        mailbox.take_command(1., 5., &mut strings);
        let next = mailbox.pilot_controls(life);
        assert_eq!((next.use_pressed, next.mouse, next.flight.throttle), (false, [0.; 2], 1.));
        // A new life forgets them, and so does a clear.
        mailbox.take_command(2., 6., &mut strings);
        assert_eq!(mailbox.pilot_controls(life).flight.throttle, 0.);
        mailbox.state.queue.push(entry(3.));
        assert!(mailbox.queue_pilot(3., life, held));
        mailbox.clear_command(&mut strings);
        mailbox.consumed(&entry(3.));
        assert_eq!(mailbox.pilot_controls(life).flight.throttle, 0.);
    }

    #[test]
    fn pilot_controls_stay_bound_to_the_addressed_life() {
        use crate::space::local::PilotControls;
        use crate::units::infantry::contact::LifeId;
        let mut strings = Strings::default();
        let mut mailbox = Mailbox::new(2., 4., Vec::new(), &mut strings).unwrap();
        mailbox.enable_pilot();
        let life = LifeId { slot: 6, slab: 2, generation: 5 };
        for (sequence, throttle, use_pressed) in [(1., 0.5, true), (2., 1.0, false)] {
            let entry = ImEntry { sequence, generation: 5., ..Default::default() };
            mailbox.state.queue.push(entry.clone());
            let mut input = PilotControls { use_pressed, ..Default::default() };
            input.flight.throttle = throttle;
            assert!(mailbox.queue_pilot(sequence, life, input));
            mailbox.consumed(&entry);
        }
        // Different slab, slot, or generation must never inherit the old life's controls.
        for other in [LifeId { slab: 3, ..life }, LifeId { slot: 7, ..life }, LifeId { generation: 6, ..life }] {
            let sample = mailbox.pilot_controls(other);
            assert!(!sample.use_pressed && sample.flight.throttle == 0., "{other:?}");
        }
        let sample = mailbox.pilot_controls(life);
        assert!(sample.use_pressed);
        assert_eq!(sample.flight.throttle, 1.0);
    }

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

    // Exercise the native admission boundary without the host shape checker.
    // Every refusal preserves queue stamps and permits a later valid record.
    #[test]
    fn malformed_commands_are_counted_without_advancing_or_poisoning_the_queue() {
        let mut strings=Strings::default();let code=strings.intern_str("KeyW");
        let mut owner=Mailbox::new(1.,2.,vec![code],&mut strings).unwrap();
        let good=ImEntry {identity:1.,down:1.,sequence:1.,tick:3.,generation:2.,codes:vec![code],..Default::default()};
        let mut bad=Vec::new();
        for tick in [0.,-1.,f64::NAN,f64::INFINITY,0.5] {bad.push(ImEntry {tick,..good.clone()});}
        for generation in [-1.,f64::NAN,f64::INFINITY,0.5] {bad.push(ImEntry {generation,..good.clone()});}
        bad.push(ImEntry {codes:vec![code;65],..good.clone()});
        bad.push(ImEntry {codes:vec![StrId(u32::MAX)],..good.clone()});
        bad.push(ImEntry {actions:vec![code;2049],..good.clone()});
        bad.push(ImEntry {look:vec![ImLook {point:[f64::NAN,0.],object:None}],..good.clone()});
        bad.push(ImEntry {look_total:Some([f64::INFINITY,0.]),..good.clone()});
        bad.push(ImEntry {human:vec![HsCommand {kind:HsCommandKind::Suicide,..Default::default()};17],..good.clone()});
        bad.push(ImEntry {human:vec![HsCommand {kind:HsCommandKind::Team,team:8.,..Default::default()}],..good.clone()});
        bad.push(ImEntry {human:vec![HsCommand {kind:HsCommandKind::Spawn,..Default::default()}],..good.clone()});
        bad.push(ImEntry {fire:vec![ImFire {id:0.,button:0.,pressed:true}],..good.clone()});
        for entry in bad {
            let count=owner.rejected;
            let result=owner.run(ImInput {op:ImOperation::Push,entry:Some(entry),epoch_matches:true,..Default::default()},&mut strings);
            assert!(!result.accepted);assert!(result.error.is_some());assert_eq!(owner.rejected,count+1);
            assert!(owner.state.queue.is_empty());assert_eq!(owner.state.received,0.);
        }
        let push=|entry|ImInput {op:ImOperation::Push,entry:Some(entry),epoch_matches:true,..Default::default()};
        assert!(owner.run(push(good.clone()),&mut strings).accepted);
        assert!(!owner.run(push(ImEntry {sequence:2.,tick:2.,..good.clone()}),&mut strings).accepted);
        assert_eq!(owner.state.received,1.);assert_eq!(owner.state.queue.len(),1);
        assert!(owner.run(push(ImEntry {sequence:2.,..good.clone()}),&mut strings).accepted);
        assert!(!owner.run(push(ImEntry {sequence:3.,..good}),&mut strings).accepted);
        assert_eq!(owner.state.queue.len(),2);
        let mut result=owner.run(ImInput {op:ImOperation::Take,tick:3.,generation:2.,..Default::default()},&mut strings);
        assert!(result.error.is_none());assert_eq!(owner.state.ack,2.);assert!(owner.state.queue.is_empty());
        owner.recycle(&mut result);
        // Non-finite consume stamps clamp to the last safe state, never loop.
        assert!(owner.run(ImInput {op:ImOperation::Take,tick:f64::INFINITY,generation:f64::NAN,..Default::default()},&mut strings).error.is_none());
    }

}
