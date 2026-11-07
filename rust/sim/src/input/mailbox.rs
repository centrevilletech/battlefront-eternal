//! Replaceable movement commands (runtime/transport.js InputMailbox).
//! Foreign values keep opaque identities until their consumers migrate;
//! this queue owns stamps, coalescing, cumulative look and acknowledgement.
use super::actions::safe_integer;
use crate::gen::{HsCommandKind, ImInput, ImLook, ImOperation, ImResult, ImState, ImTaken};
use crate::store::Strings;

fn admits(s: &ImState, epoch_matches: bool, sequence: f64) -> bool {
    epoch_matches && safe_integer(sequence) && sequence > s.received
}
fn truthy(n: f64) -> bool { n != 0.0 && !n.is_nan() }

/// InputMailbox.push checks projected numeric values again for native callers.
/// Shape checks in the host cannot protect a shared memory caller on their own.
pub(super) fn invalid_entry(e:&crate::gen::ImEntry,codes:&[crate::store::StrId],strings:&Strings)->Option<&'static str> {
    if !safe_integer(e.identity)||e.identity<0.||!safe_integer(e.down)||e.down<0.
        ||!safe_integer(e.sequence)||e.sequence<1.||!safe_integer(e.tick)||e.tick<1.
        ||!safe_integer(e.generation)||e.generation<0. {return Some("invalid input stamp");}
    if e.codes.len()>64||e.codes.iter().any(|id|!codes.contains(id))||e.actions.len()>2048
        ||e.actions.iter().any(|id|strings.list.get(id.0 as usize).is_none_or(|s|s.len()>64))
        ||e.look.len()>2048||e.look.iter().any(|v|v.point.iter().any(|n|!n.is_finite()))
        ||e.look.iter().any(|v|v.object.is_some_and(|n|!safe_integer(n)||n<0.))
        ||e.view.len()>7||e.view.iter().any(|field|strings.list.get(field.key.0 as usize).is_none_or(|key|
            !["mouseScale","invertMouse","firstPerson","aspect","viewTick","flightSettings","gamepad"].iter().any(|name|key.iter().copied().eq(name.encode_utf16())))
            ||!safe_integer(field.value)||field.value<0.)
        ||e.human.len()>16||e.fire.len()>16 {return Some("invalid input payload");}
    // Choices are records (src/sim/wasm/runtime/input-mailbox.js invalidCommand's
    // human rules): a team of 1 to 7, a named class with an optional post.
    let named=|id:Option<crate::store::StrId>|id.is_some_and(|id|strings.list.get(id.0 as usize).is_some_and(|s|!s.is_empty()&&s.len()<=64));
    if e.human.iter().any(|h|match h.kind {
        HsCommandKind::Team=>!safe_integer(h.team)||!(1. ..=7.).contains(&h.team),
        HsCommandKind::Spawn=>!named(h.class_name)||h.post.is_some()&&!named(h.post),
        _=>false}) {return Some("invalid human input");}
    if e.look_total.is_some_and(|v|v.iter().any(|n|!n.is_finite()||n.abs()>2_f64.powi(48))) {return Some("invalid cumulative look");}
    if e.fire.iter().any(|v|!safe_integer(v.id)||v.id<1.||(v.button!=0.&&v.button!=1.)) {return Some("invalid trigger edges");}
    let v=&e.publication;
    if !safe_integer(v.flags)||!(0. ..=2047.).contains(&v.flags) {return Some("invalid view input");}
    let flags=v.flags as u16;
    if flags&32!=0&&(!v.mouse_scale.is_finite()||v.mouse_scale<=0.)
        ||flags&64!=0&&(!v.aspect.is_finite()||v.aspect<=0.) {return Some("invalid view input");}
    if flags&128!=0&&(!v.view_tick.is_finite()||!(0. ..=u32::MAX as f64).contains(&v.view_tick)) {return Some("invalid view tick");}
    if flags&256!=0&&(!v.sensitivity.is_finite()||v.sensitivity<=0.||v.sensitivity>10.)
        ||flags&512!=0&&(!v.curve.is_finite()||!(0.1..=4.).contains(&v.curve)) {return Some("invalid flight settings");}
    if flags&16!=0&&(v.axes.iter().any(|n|!n.is_finite()||n.abs()>1.)||!safe_integer(v.buttons)||!(0. ..=65535.).contains(&v.buttons)) {return Some("invalid gamepad input");}
    None
}

/// InputMailbox.take lends reserved result vectors across each exchange.
/// Admission can allocate owned packet values; consumption only moves them
/// into this fixed-capacity scratch, preserving paused-burst order.
pub fn run_reusing(input:ImInput,strings:&mut Strings,mut command:ImTaken,mut retired:Vec<f64>,
    mut consumed:impl FnMut(&crate::gen::ImEntry))->(ImResult,ImTaken) {
    command.actions.clear();command.look.clear();command.human.clear();command.fire.clear();retired.clear();
    let mut out = ImResult { state: input.state, retired, ..Default::default() };
    (|| {
        let s = &mut out.state;
        match input.op {
            ImOperation::Admit => out.accepted = admits(s, input.epoch_matches, input.sequence),
            ImOperation::Push => {
                let Some(entry) = input.entry else { return; };
                if !admits(s, input.epoch_matches, entry.sequence) { return; }
                if s.queue.len() as f64 >= s.capacity { return; }
                if s.queue.last().is_some_and(|last| entry.tick < last.tick) { return; }
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
                    let oversized=next.actions.len()>2048 || looks>2048 || next.human.len() as f64>16.0*s.capacity || next.fire.len() as f64>16.0*s.capacity;
                    // InputMailbox.take defers a valid burst, but must retire an
                    // individually invalid record rather than retry it forever.
                    if next.generation==input.generation && !oversized && (command.actions.len()+next.actions.len()>2048
                        || command.look.len()+looks>2048 || (command.human.len()+next.human.len()) as f64>16.0*s.capacity || (command.fire.len()+next.fire.len()) as f64>16.0*s.capacity) {break;}
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
                    // Even corrupted retained records retire with their original
                    // held/view watermarks, without growing command scratch or
                    // leaving a record that every later tick retries forever.
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
    })();
    (out,command)
}
