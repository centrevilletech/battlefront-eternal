//! Reliable trigger edges (runtime/fire-commands.js). Each button consumes
//! at most one edge per tick, preserving short clicks and future ordering.
use super::actions::safe_integer;
use crate::gen::{IaOperation, IaOutcome, IfEdge, IfInput, IfMovement, IfResult, IfState};
use crate::store::{StrId, Strings};

// A receipt backlog of 64 threw in JavaScript and would have ended the
// server's tick. A full backlog now drops the receipt: the client repeats
// the edge or reports it expired, as it does for any lost receipt.
fn outcome(s: &mut IfState, id: f64, status: StrId) -> Result<(), &'static str> {
    if let Some(outcomes) = s.outcomes.as_mut() {
        if outcomes.len() >= 64 { return Ok(()); }
        outcomes.push(IaOutcome { id, status });
    }
    Ok(())
}

pub(crate) fn push(s: &mut IfState, edge: Option<IfEdge>, epoch_one: bool, pressed_valid: bool, same_generation: bool, tick: f64) -> bool {
    let Some(mut edge) = edge else { return false };
    if !epoch_one || !pressed_valid || !same_generation || !safe_integer(edge.id) || edge.id <= s.received ||
        !safe_integer(edge.tick) || edge.tick < 1.0 || edge.tick > tick + 120.0 || !safe_integer(edge.generation) ||
        (edge.button != 0.0 && edge.button != 1.0) { return false; }
    if edge.pressed && edge.tick < tick - 30.0 { return false; }
    for value in [edge.aim_yaw, edge.aim_pitch].into_iter().flatten() {
        if !value.is_finite() || value.abs() > 1000000.0 { return false; }
    }
    if edge.view_tick.is_some_and(|v| !v.is_finite() || v < 0.0 || v > tick + 2.0) { return false; }
    if edge.external_id.is_some_and(|v| !safe_integer(v) || v < 1.0 || v > 4294967295.0) { return false; }
    if s.queue.len() >= 32 { return false; }
    edge.aim_yaw = edge.aim_yaw.map(|v| v8math::atan2(v8math::sin(v), v8math::cos(v)));
    edge.aim_pitch = edge.aim_pitch.map(|v| v8math::max(-std::f64::consts::PI / 2.0, v8math::min(std::f64::consts::PI / 2.0, v)));
    s.received = edge.id;
    s.queue.push(edge);
    true
}

/// FireCommands.clear: every queued edge is receipted as cleared.
pub(crate) fn clear(s: &mut IfState, strings: &mut Strings) -> Result<(), &'static str> {
    for i in 0..s.queue.len() {
        let id = s.queue[i].id;
        outcome(s, id, strings.intern_str("cleared"))?;
    }
    s.queue.clear(); s.buttons = [0.0; 2]; s.press_ticks = [0.0; 2]; Ok(())
}

pub(crate) fn take(out: &mut IfResult, tick: f64, generation: f64, movement: Option<IfMovement>, strings: &mut Strings) -> Result<(), &'static str> {
    let s = &mut out.state;
    if s.generation != Some(generation) {
        s.buttons = [0.0; 2]; s.press_ticks = [0.0; 2]; s.generation = Some(generation);
        out.replaced_buttons = true;
        // Array.filter publishes its new array only after every callback.
        // An outcome overflow keeps the old queue but retains prior receipts.
        for i in 0..s.queue.len() {
            let edge = &s.queue[i];
            if edge.generation != generation { let id = edge.id; outcome(s, id, strings.intern_str("life-cancelled"))?; }
        }
        s.queue.retain(|edge| edge.generation == generation);
        out.replaced_queue = true;
    }
    for button in 0..2 {
        if s.buttons[button] != 0.0 && movement.as_ref().is_some_and(|state| state.tick >= s.press_ticks[button] && state.buttons[button] == 0.0) {
            s.buttons[button] = 0.0;
        }
    }
    let mut used = [false; 2];
    let mut i = 0;
    while i < s.queue.len() {
        let edge = &s.queue[i];
        if edge.generation != generation || (edge.pressed && edge.tick < tick - 30.0) {
            let id = edge.id;
            let status = if edge.generation != generation { "life-cancelled" } else { "expired" };
            outcome(s, id, strings.intern_str(status))?;
            s.queue.remove(i); continue;
        }
        let button = edge.button as usize;
        if used[button] { i += 1; continue; }
        if edge.tick > tick { used[button] = true; i += 1; continue; }
        let id = edge.id;
        outcome(s, id, strings.intern_str("consumed"))?;
        let edge = s.queue.remove(i);
        used[button] = true;
        if edge.pressed && s.buttons[button] == 0.0 {
            let mut shot = edge.clone();
            if let Some(id) = shot.external_id { shot.id = id; }
            out.shots.push(shot);
        }
        if edge.pressed { s.press_ticks[button] = edge.tick; }
        s.buttons[button] = if edge.pressed { 1.0 } else { 0.0 };
    }
    Ok(())
}

pub fn run(input: IfInput, strings: &mut Strings) -> IfResult {
    let mut out = IfResult { state: input.state, ..Default::default() };
    let result = match input.op {
        IaOperation::Push => { out.accepted = push(&mut out.state, input.edge, input.epoch_one, input.pressed_valid, input.same_generation, input.tick); Ok(()) },
        IaOperation::Take => take(&mut out, input.tick, input.generation, input.movement, strings),
        IaOperation::Clear => { let cleared = clear(&mut out.state, strings); out.replaced_buttons = true; cleared },
        IaOperation::Enable => { out.state.outcomes.get_or_insert_with(Vec::new); Ok(()) },
        IaOperation::Drain => { if let Some(v) = out.state.outcomes.as_mut() { out.outcomes = std::mem::take(v); } Ok(()) },
        IaOperation::Outcome => outcome(&mut out.state, input.id, input.status.expect("an outcome status")),
    };
    if let Err(error) = result { out.error = Some(strings.intern_str(error)); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn edge(id: f64, tick: f64, button: f64, pressed: bool) -> IfEdge {
        IfEdge { id, tick, button, pressed, generation: 1.0, ..Default::default() }
    }
    #[test]
    fn a_future_press_blocks_only_its_button() {
        let mut strings = Strings::default();
        let mut s = IfState::default();
        assert!(push(&mut s, Some(edge(1.0, 15.0, 0.0, true)), true, true, true, 10.0));
        assert!(push(&mut s, Some(edge(2.0, 12.0, 0.0, false)), true, true, true, 10.0));
        assert!(push(&mut s, Some(edge(3.0, 12.0, 1.0, true)), true, true, true, 10.0));
        let mut out = IfResult { state: s, ..Default::default() };
        take(&mut out, 12.0, 1.0, None, &mut strings).unwrap(); assert_eq!(out.state.buttons, [0.0, 1.0]);
        take(&mut out, 15.0, 1.0, None, &mut strings).unwrap(); assert_eq!(out.state.buttons, [1.0, 1.0]);
        take(&mut out, 16.0, 1.0, None, &mut strings).unwrap(); assert_eq!(out.state.buttons, [0.0, 1.0]);
    }
    #[test]
    fn a_full_receipt_backlog_drops_the_receipt_and_still_filters_the_old_life() {
        let mut strings = Strings::default();
        let status = strings.intern_str("consumed");
        let mut out = IfResult { state: IfState { generation: Some(1.0), buttons: [1.0, 1.0],
            queue: vec![edge(1.0, 1.0, 0.0, true)], outcomes: Some(vec![IaOutcome { id: 0.0, status }; 64]), ..Default::default() }, ..Default::default() };
        assert_eq!(take(&mut out, 2.0, 2.0, None, &mut strings), Ok(()));
        assert_eq!(out.state.generation, Some(2.0)); assert_eq!(out.state.buttons, [0.0; 2]);
        assert!(out.state.queue.is_empty()); assert!(out.replaced_queue); assert!(out.replaced_buttons);
        assert_eq!(out.state.outcomes.as_ref().unwrap().len(), 64);
    }
}
