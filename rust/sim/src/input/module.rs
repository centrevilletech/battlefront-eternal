use super::{actions, fire, controls};
use crate::gen::{IaInput, IaResult, IfInput, IfResult, ImInput, ImResult, IqInput, IqResult, IcInput, IcResult, IkInput};
pub fn input_actions_words(which: u32) -> u32 {
    (match which { 0 => IaInput::WORDS, 1 => IaResult::WORDS, _ => 0 }) as u32
}

pub(crate) fn input_actions_run_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let sim = &mut w.sim;
    let (row, heap) = sim.calls.args();
    let input = IaInput::take(row, heap);
    let result = actions::run(input, &mut sim.strings);
    sim.calls.result(IaResult::WORDS, |row, heap| result.put(row, heap));
    Some(&sim.calls.block.words[..])
}
pub fn input_fire_words(which: u32) -> u32 {
    (match which { 0 => IfInput::WORDS, 1 => IfResult::WORDS, _ => 0 }) as u32
}

pub(crate) fn input_fire_run_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let sim = &mut w.sim;
    let (row, heap) = sim.calls.args();
    let input = IfInput::take(row, heap);
    let result = fire::run(input, &mut sim.strings);
    sim.calls.result(IfResult::WORDS, |row, heap| result.put(row, heap));
    Some(&sim.calls.block.words[..])
}

pub(crate) fn input_mailbox_owned_run_in(w: &mut crate::world::owner::World, handle:u32) -> Option<&[f64]> {
    let sim=&mut w.sim;let (row,heap)=sim.calls.args();
    if !super::owner::valid_call(row,heap) {
        if let Some(owner)=w.input.mailboxes.get_mut(handle as usize) {owner.rejected=owner.rejected.saturating_add(1);}
        return None;
    }
    let call=crate::gen::InputMailboxCall::take(row,heap);
    let input=ImInput {op:call.op,entry:call.entry,sequence:call.sequence,epoch_matches:call.epoch_matches,
        tick:call.tick,generation:call.generation,..Default::default()};
    let Some(owner)=w.input.mailboxes.get_mut(handle as usize) else {return None};
    let mut result=owner.run(input,&mut sim.strings);
    owner.lend_down(&mut result);
    owner.lend_consumed(&mut result);
    result.state=std::mem::take(&mut owner.state);
    // Queued commands stay with their owner. Only consumed values and fixed
    // counters return to the caller; inspection is a separate cold operation.
    let queue=std::mem::take(&mut result.state.queue);
    sim.calls.result(ImResult::WORDS,|row,heap|result.put(row,heap));
    result.state.queue=queue;owner.state=std::mem::take(&mut result.state);owner.recycle(&mut result);owner.return_down(&mut result);
    Some(&sim.calls.block.words[..])
}
pub fn input_queue_words(which: u32) -> u32 {
    (match which { 0 => IqInput::WORDS, 1 => IqResult::WORDS, _ => 0 }) as u32
}

pub(crate) fn input_queue_run_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let sim = &mut w.sim; let (row, heap) = sim.calls.args();
    let result = controls::queue(IqInput::take(row, heap), &mut sim.strings);
    sim.calls.result(IqResult::WORDS, |row, heap| result.put(row, heap));
    Some(&sim.calls.block.words[..])
}
pub fn input_controls_words(which: u32) -> u32 {
    (match which { 0 => IcInput::WORDS, 1 => IcResult::WORDS, 2 => IkInput::WORDS, _ => 0 }) as u32
}

pub(crate) fn input_controls_run_in(w: &mut crate::world::owner::World) -> Option<&[f64]> {
    let sim = &mut w.sim; let (row, heap) = sim.calls.args();
    let result = controls::controls(IcInput::take(row, heap));
    sim.calls.result(IcResult::WORDS, |row, heap| result.put(row, heap));
    Some(&sim.calls.block.words[..])
}

pub(crate) fn input_controls_axis_in(w: &mut crate::world::owner::World) -> f64 {
    let (row, heap) = w.sim.calls.args();
    let input = IkInput::take(row, heap);
    controls::axis(&input.down, &input.plus, &input.minus)
}

/// Mailboxes live until their connection is retired.
pub(crate) fn input_mailbox_create_in(w: &mut crate::world::owner::World) ->u32 {
    let sim=&mut w.sim;let(row,heap)=sim.calls.args();
    use crate::gen::InputMailboxSetup as S;
    if row.len()<S::WORDS {return 0;}
    let start=row[S::CODES];let count=row[S::CODES+1];
    if !start.is_finite() || start<0. || start.fract()!=0. || !count.is_finite() || count.fract()!=0. || !(0. ..=128.).contains(&count)
        || start+count>heap.len() as f64 {return 0;}
    if heap[start as usize..(start+count) as usize].iter().any(|id|!id.is_finite()||id.fract()!=0.||!(0. ..=u32::MAX as f64).contains(id)) {return 0;}
    let setup=S::take(row,heap);let capacity=setup.capacity;
    let Some(mailbox)=super::owner::Mailbox::new(setup.epoch,capacity,setup.codes,&mut sim.strings) else{return 0;};
    // InputMailbox.take can consume an entire paused burst. Reserve both
    // generated exchange vectors at admission so even its first result fits.
    let count=capacity as usize;
    let heap=count+7*crate::gen::ImField::WORDS+2048+2048*crate::gen::ImLook::WORDS
        +16*count+16*count*crate::gen::ImFire::WORDS;
    let block=&mut sim.calls.block;
    block.heap.reserve(heap.saturating_sub(block.heap.len()));
    let words=crate::store::block::HEADER+ImResult::WORDS+heap;
    block.words.reserve(words.saturating_sub(block.words.len()));
    w.input.mailboxes.insert(mailbox).unwrap_or(0) as u32
}

pub(crate) fn input_mailbox_drop_in(w: &mut crate::world::owner::World, handle:u32) {w.input.mailboxes.remove(handle as usize);}

pub(crate) fn input_mailbox_view_in(w: &mut crate::world::owner::World, handle:u32) {
    let sim=&mut w.sim;let (row,heap)=sim.calls.args();let input=crate::gen::InputViewUpdate::take(row,heap);
    if let Some(mailbox)=w.input.mailboxes.get_mut(handle as usize) {mailbox.edit_view(input.key as usize,input.present,&input.view);}
}
/// Shape failures cannot be projected into a generated command. Count them
/// beside native refusals so one connection owns all its rejected input.
pub(crate) fn input_mailbox_reject_in(w:&mut crate::world::owner::World,handle:u32) {
    if let Some(owner)=w.input.mailboxes.get_mut(handle as usize) {owner.rejected=owner.rejected.saturating_add(1);}
}
pub(crate) fn input_mailbox_rejected_in(w:&crate::world::owner::World,handle:u32)->u32 {
    w.input.mailboxes.get(handle as usize).map_or(0,|owner|owner.rejected)
}
/// Debug views do not run during an ordinary input or publication operation.
pub(crate) fn input_mailbox_inspect_in(w: &mut crate::world::owner::World, handle:u32) -> Option<&[f64]> {
    let sim=&mut w.sim;let Some(owner)=w.input.mailboxes.get(handle as usize) else {return None};
    sim.calls.result(crate::gen::ImState::WORDS,|row,heap|owner.state.put(row,heap));Some(&sim.calls.block.words[..])
}

/// Spawn-selection mutations enter once, independently of publication cadence.
pub(crate) fn input_mailbox_spawn_in(w: &mut crate::world::owner::World, handle:u32) {
    let sim=&mut w.sim;let (row,heap)=sim.calls.args();
    let spawn=crate::gen::WireSpawnFacts::take(row,heap);
    if let Some(owner)=w.input.mailboxes.get_mut(handle as usize) {owner.publication.spawn=spawn;}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{gen::InputMailboxSetup, world::owner::World};

    fn admit(w: &mut World, epoch: f64) -> u32 {
        let setup = InputMailboxSetup {epoch, capacity: 4., codes: Vec::new()};
        w.sim.calls.result(InputMailboxSetup::WORDS, |row,heap| setup.put(row,heap));
        input_mailbox_create_in(w)
}

    #[test]
    fn mailbox_admission_and_retirement_use_the_supplied_world() {
        let (mut a, mut b) = (World::new(), World::new());
        let ka = admit(&mut a, 3.);
        let kb = admit(&mut b, 9.);
        assert_ne!(ka, 0);
        assert_eq!(ka, kb);
        assert_eq!(a.input.mailboxes.get(ka as usize).unwrap().state.epoch, 3.);
        assert_eq!(b.input.mailboxes.get(kb as usize).unwrap().state.epoch, 9.);
        input_mailbox_drop_in(&mut a, ka);
        let replacement = admit(&mut a, 7.);
        assert_ne!(replacement, ka);
        assert!(a.input.mailboxes.get(ka as usize).is_none());
        input_mailbox_drop_in(&mut a, ka);
        assert!(a.input.mailboxes.get(replacement as usize).is_some());
        assert_eq!(b.input.mailboxes.get(kb as usize).unwrap().state.epoch, 9.);
    }
}

// The WASM boundary owns addresses; shared services borrow their owners.
#[cfg(target_arch = "wasm32")]
mod exports {
    use super::*;
    #[no_mangle]
    pub extern "C" fn input_actions_words(which: u32) -> u32 { super::input_actions_words(which) }
    #[no_mangle]
    pub extern "C" fn input_fire_words(which: u32) -> u32 { super::input_fire_words(which) }
    #[no_mangle]
    pub extern "C" fn input_queue_words(which: u32) -> u32 { super::input_queue_words(which) }
    #[no_mangle]
    pub extern "C" fn input_controls_words(which: u32) -> u32 { super::input_controls_words(which) }
    #[no_mangle]
    pub extern "C" fn input_actions_run() -> *const f64 { input_actions_run_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn input_fire_run() -> *const f64 { input_fire_run_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn input_mailbox_owned_run(handle:u32) -> *const f64 { input_mailbox_owned_run_in(crate::abi::facade::world(), handle).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn input_queue_run() -> *const f64 { input_queue_run_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn input_controls_run() -> *const f64 { input_controls_run_in(crate::abi::facade::world()).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn input_controls_axis() -> f64 { input_controls_axis_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn input_mailbox_create() ->u32 { input_mailbox_create_in(crate::abi::facade::world()) }
    #[no_mangle]
    pub extern "C" fn input_mailbox_drop(handle:u32) { input_mailbox_drop_in(crate::abi::facade::world(), handle) }
    #[no_mangle]
    pub extern "C" fn input_mailbox_view(handle:u32) { input_mailbox_view_in(crate::abi::facade::world(), handle) }
    #[no_mangle]
    pub extern "C" fn input_mailbox_reject(handle:u32) { input_mailbox_reject_in(crate::abi::facade::world(),handle) }
    #[no_mangle]
    pub extern "C" fn input_mailbox_rejected(handle:u32)->u32 { input_mailbox_rejected_in(crate::abi::facade::world(),handle) }
    #[no_mangle]
    pub extern "C" fn input_mailbox_inspect(handle:u32) ->*const f64 { input_mailbox_inspect_in(crate::abi::facade::world(), handle).map_or(std::ptr::null(), |b| b.as_ptr()) }
    #[no_mangle]
    pub extern "C" fn input_mailbox_spawn(handle:u32) { input_mailbox_spawn_in(crate::abi::facade::world(), handle) }
}
#[cfg(target_arch = "wasm32")]
pub use exports::{input_actions_run, input_fire_run, input_mailbox_owned_run, input_queue_run, input_controls_run, input_controls_axis, input_mailbox_create, input_mailbox_drop, input_mailbox_view, input_mailbox_inspect, input_mailbox_spawn, input_mailbox_reject, input_mailbox_rejected};
