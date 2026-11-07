use super::{actions, fire, controls};
use crate::abi::sim;
use crate::gen::{IaInput, IaResult, IfInput, IfResult, ImInput, ImResult, IqInput, IqResult, IcInput, IcResult, IkInput};

#[no_mangle]
pub extern "C" fn input_actions_words(which: u32) -> u32 {
    (match which { 0 => IaInput::WORDS, 1 => IaResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn input_actions_run() -> *const f64 {
    let sim = sim();
    let (row, heap) = sim.calls.args();
    let input = IaInput::take(row, heap);
    let result = actions::run(input, &mut sim.strings);
    sim.calls.result(IaResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn input_fire_words(which: u32) -> u32 {
    (match which { 0 => IfInput::WORDS, 1 => IfResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn input_fire_run() -> *const f64 {
    let sim = sim();
    let (row, heap) = sim.calls.args();
    let input = IfInput::take(row, heap);
    let result = fire::run(input, &mut sim.strings);
    sim.calls.result(IfResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn input_mailbox_owned_run(handle:u32) -> *const f64 {
    let sim=sim();let (row,heap)=sim.calls.args();
    if !super::owner::valid_call(row,heap) {return std::ptr::null();}
    let call=crate::gen::InputMailboxCall::take(row,heap);
    let input=ImInput {op:call.op,entry:call.entry,sequence:call.sequence,epoch_matches:call.epoch_matches,
        tick:call.tick,generation:call.generation,..Default::default()};
    let Some(owner)=crate::abi::world().input.mailboxes.get_mut(handle as usize) else {return std::ptr::null()};
    let mut result=owner.run(input,&mut sim.strings);
    result.state=std::mem::take(&mut owner.state);
    // Queued commands stay with their owner. Only consumed values and fixed
    // counters return to the caller; inspection is a separate cold operation.
    let queue=std::mem::take(&mut result.state.queue);
    sim.calls.result(ImResult::WORDS,|row,heap|result.put(row,heap));
    result.state.queue=queue;owner.state=std::mem::take(&mut result.state);owner.recycle(&mut result);
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn input_queue_words(which: u32) -> u32 {
    (match which { 0 => IqInput::WORDS, 1 => IqResult::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn input_queue_run() -> *const f64 {
    let sim = sim(); let (row, heap) = sim.calls.args();
    let result = controls::queue(IqInput::take(row, heap), &mut sim.strings);
    sim.calls.result(IqResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn input_controls_words(which: u32) -> u32 {
    (match which { 0 => IcInput::WORDS, 1 => IcResult::WORDS, 2 => IkInput::WORDS, _ => 0 }) as u32
}

#[no_mangle]
pub extern "C" fn input_controls_run() -> *const f64 {
    let sim = sim(); let (row, heap) = sim.calls.args();
    let result = controls::controls(IcInput::take(row, heap));
    sim.calls.result(IcResult::WORDS, |row, heap| result.put(row, heap));
    sim.calls.block.ptr()
}

#[no_mangle]
pub extern "C" fn input_controls_axis() -> f64 {
    let (row, heap) = sim().calls.args();
    let input = IkInput::take(row, heap);
    controls::axis(&input.down, &input.plus, &input.minus)
}

/// Mailboxes live until their connection is retired.
#[no_mangle]
pub extern "C" fn input_mailbox_create()->u32 {
    let sim=sim();let(row,heap)=sim.calls.args();
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
    crate::abi::world().input.mailboxes.insert(mailbox).unwrap_or(0) as u32
}

#[no_mangle]
pub extern "C" fn input_mailbox_drop(handle:u32) {crate::abi::world().input.mailboxes.remove(handle as usize);}

#[no_mangle]
pub extern "C" fn input_mailbox_view(handle:u32) {
    let sim=sim();let (row,heap)=sim.calls.args();let input=crate::gen::InputViewUpdate::take(row,heap);
    if let Some(mailbox)=crate::abi::world().input.mailboxes.get_mut(handle as usize) {mailbox.view(input.key as usize,input.present,&input.view);}
}
/// Debug views do not run during an ordinary input or publication operation.
#[no_mangle]
pub extern "C" fn input_mailbox_inspect(handle:u32)->*const f64 {
    let sim=sim();let Some(owner)=crate::abi::world().input.mailboxes.get(handle as usize) else {return std::ptr::null()};
    sim.calls.result(crate::gen::ImState::WORDS,|row,heap|owner.state.put(row,heap));sim.calls.block.ptr()
}

/// Spawn-selection mutations enter once, independently of publication cadence.
#[no_mangle]
pub extern "C" fn input_mailbox_spawn(handle:u32) {
    let sim=sim();let (row,heap)=sim.calls.args();
    let spawn=crate::gen::WireSpawnFacts::take(row,heap);
    if let Some(owner)=crate::abi::world().input.mailboxes.get_mut(handle as usize) {owner.publication.spawn=spawn;}
}
