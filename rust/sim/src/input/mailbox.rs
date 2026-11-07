//! Replaceable movement commands (runtime/transport.js InputMailbox).
//! Foreign values keep opaque identities until their consumers migrate;
//! this queue owns stamps, coalescing, cumulative look and acknowledgement.
use super::actions::safe_integer;
use crate::gen::{ImInput, ImLook, ImOperation, ImResult, ImState, ImTaken};
use crate::store::Strings;

fn admits(s: &ImState, epoch_matches: bool, sequence: f64) -> bool {
    epoch_matches && safe_integer(sequence) && sequence > s.received
}
fn truthy(n: f64) -> bool { n != 0.0 && !n.is_nan() }

pub fn run(input: ImInput, strings: &mut Strings) -> ImResult {run_with(input,strings,|_|{})}
// The owner observes each consumed command at the same point as down/view
// replacement. It retains publication values without decoding host mirrors.
pub fn run_with(input: ImInput, strings: &mut Strings, consumed:impl FnMut(&crate::gen::ImEntry)) -> ImResult {
    run_reusing(input,strings,ImTaken::default(),Vec::new(),consumed).0
}
/// InputMailbox.take lends reserved result vectors across each exchange.
/// Admission can allocate owned packet values; consumption only moves them
/// into this fixed-capacity scratch, preserving paused-burst order.
pub fn run_reusing(input:ImInput,strings:&mut Strings,mut command:ImTaken,mut retired:Vec<f64>,
    mut consumed:impl FnMut(&crate::gen::ImEntry))->(ImResult,ImTaken) {
    command.actions.clear();command.look.clear();command.human.clear();command.fire.clear();retired.clear();
    let mut out = ImResult { state: input.state, retired, ..Default::default() };
    let result: Result<(), &'static str> = (|| {
        let s = &mut out.state;
        match input.op {
            ImOperation::Admit => out.accepted = admits(s, input.epoch_matches, input.sequence),
            ImOperation::Push => {
                let entry = input.entry.ok_or("invalid movement command")?;
                if !admits(s, input.epoch_matches, entry.sequence) { return Ok(()); }
                if s.queue.len() as f64 >= s.capacity { return Ok(()); }
                if s.queue.last().is_some_and(|last| entry.tick < last.tick) { return Err("input tick moved backwards"); }
                s.received = entry.sequence; s.queue.push(entry); out.accepted = true;
            }
            ImOperation::Take => {
                if input.generation != s.generation {
                    s.down = None; s.generation = input.generation; s.look_total = [0.0; 2]; s.state_tick = 0.0;
                    out.replaced_look_total = true;
                }
                while s.queue.first().is_some_and(|c| c.tick <= input.tick) {
                    let next=&s.queue[0];
                    let looks=next.look_total.map_or(next.look.len(),|total|usize::from(
                        truthy(total[0]-s.look_total[0])||truthy(total[1]-s.look_total[1])));
                    let oversized=next.actions.len()>2048 || looks>2048 || next.human.len() as f64>16.0*s.capacity;
                    // InputMailbox.take defers a valid burst, but must retire an
                    // individually invalid record rather than retry it forever.
                    if next.generation==input.generation && !oversized && (command.actions.len()+next.actions.len()>2048
                        || command.look.len()+looks>2048 || (command.human.len()+next.human.len()) as f64>16.0*s.capacity) {break;}
                    let entry = s.queue.remove(0);
                    out.retired.push(entry.identity);
                    if entry.generation != input.generation {
                        s.discarded = v8math::max(s.discarded, entry.sequence); continue;
                    }
                    consumed(&entry);
                    s.ack = entry.sequence; s.ack_tick = input.tick; s.state_tick = entry.tick;
                    if entry.tick < input.tick { s.late += 1.0; }
                    s.down = Some(entry.down);
                    if !oversized { command.actions.extend(entry.actions); }
                    if let Some(total) = entry.look_total {
                        let point = [total[0] - s.look_total[0], total[1] - s.look_total[1]];
                        if !oversized && (truthy(point[0]) || truthy(point[1])) { command.look.push(ImLook { point, object: None }); }
                        s.look_total = total;
                    } else if !oversized { command.look.extend(entry.look); }
                    if !oversized { command.human.extend(entry.human); command.fire.extend(entry.fire); }
                    for field in entry.view {
                        if let Some(old) = s.view.iter_mut().find(|v| v.key == field.key) { old.value = field.value; }
                        else if s.view.len()<7 { s.view.push(field); }
                    }
                    // Rejection preserves the original consumption side effects,
                    // including held/view and cumulative look, without appending
                    // oversized payloads to the reserved command scratch.
                    if oversized { return Err("input batch overflow"); }
                }
                out.command = Some(std::mem::take(&mut command));
            }
            ImOperation::Clear => {
                let last = s.queue.last().map_or(0.0, |c| c.sequence);
                s.discarded = v8math::max(s.discarded, if truthy(last) { last } else { 0.0 });
                s.queue.clear(); s.down = None; s.state_tick = 0.0;
                s.view.retain(|field| {
                    let key = strings.units(field.key);
                    !key.iter().copied().eq("gamepad".encode_utf16()) && !key.iter().copied().eq("viewTick".encode_utf16())
                });
            }
        }
        Ok(())
    })();
    if let Err(error) = result { out.error = Some(strings.intern_str(error)); }
    (out,command)
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
        let out = run(input, &mut strings);
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
            let scratch=ImTaken {actions:Vec::with_capacity(2048),look:Vec::with_capacity(2048),
                human:Vec::with_capacity(16),fire:Vec::with_capacity(16)};
            let capacities=(scratch.actions.capacity(),scratch.look.capacity(),scratch.human.capacity(),scratch.fire.capacity());
            let mut consumed=0;
            let (out,spare)=run_reusing(input,&mut strings,scratch,Vec::with_capacity(1),|_|consumed+=1);
            assert_eq!(strings.units(out.error.unwrap()),"input batch overflow".encode_utf16().collect::<Vec<_>>());
            assert!(out.command.is_none());assert!(out.state.queue.is_empty());assert_eq!(out.retired,[7.0]);
            assert_eq!(out.state.ack,1.0);assert_eq!(out.state.ack_tick,2.0);assert_eq!(out.state.down,Some(17.0));
            assert_eq!(out.state.look_total.map(f64::to_bits),total.unwrap_or([0.0;2]).map(f64::to_bits));
            assert_eq!(consumed,1);
            assert_eq!((spare.actions.capacity(),spare.look.capacity(),spare.human.capacity(),spare.fire.capacity()),capacities);
            let recovered=run(ImInput {op:ImOperation::Take,state:out.state,tick:3.0,generation:1.0,..Default::default()},&mut strings);
            assert!(recovered.error.is_none());
        }
    }

}
