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

pub fn run(input: ImInput, strings: &mut Strings) -> ImResult {
    let mut out = ImResult { state: input.state, ..Default::default() };
    let result: Result<(), &'static str> = (|| {
        let s = &mut out.state;
        match input.op {
            ImOperation::Admit => out.accepted = admits(s, input.epoch_matches, input.sequence),
            ImOperation::Push => {
                let entry = input.entry.expect("a validated movement command");
                if !admits(s, input.epoch_matches, entry.sequence) { return Ok(()); }
                if s.queue.len() as f64 >= s.capacity { return Err("input mailbox overflow"); }
                if s.queue.last().is_some_and(|last| entry.tick < last.tick) { return Err("input tick moved backwards"); }
                s.received = entry.sequence; s.queue.push(entry); out.accepted = true;
            }
            ImOperation::Take => {
                if input.generation != s.generation {
                    s.down = None; s.generation = input.generation; s.look_total = [0.0; 2]; s.state_tick = 0.0;
                    out.replaced_look_total = true;
                }
                let mut command = ImTaken::default();
                while s.queue.first().is_some_and(|c| c.tick <= input.tick) {
                    let entry = s.queue.remove(0);
                    if entry.generation != input.generation {
                        s.discarded = v8math::max(s.discarded, entry.sequence); continue;
                    }
                    s.ack = entry.sequence; s.ack_tick = input.tick; s.state_tick = entry.tick;
                    if entry.tick < input.tick { s.late += 1.0; }
                    s.down = Some(entry.down); command.actions.extend(entry.actions);
                    if let Some(total) = entry.look_total {
                        let point = [total[0] - s.look_total[0], total[1] - s.look_total[1]];
                        if truthy(point[0]) || truthy(point[1]) { command.look.push(ImLook { point, object: None }); }
                        s.look_total = total;
                    } else { command.look.extend(entry.look); }
                    command.human.extend(entry.human); command.fire.extend(entry.fire);
                    for field in entry.view {
                        if let Some(old) = s.view.iter_mut().find(|v| v.key == field.key) { old.value = field.value; }
                        else { s.view.push(field); }
                    }
                    if command.actions.len() > 2048 || command.look.len() > 2048 || command.human.len() as f64 > 16.0 * s.capacity {
                        return Err("input batch overflow");
                    }
                }
                out.command = Some(command);
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
    out
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
}
