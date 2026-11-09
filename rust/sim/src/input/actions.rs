//! The reliable action lane (runtime/input-actions.js). Its watermark is
//! independent of replaceable movement, and duplicate presses span ticks.
use crate::gen::{HsCommand, HsCommandKind, IaEntry, IaHuman, IaInput, IaOperation, IaOutcome, IaResult, IaState};
use crate::store::{StrId, Strings};

pub(super) fn safe_integer(n: f64) -> bool { n.is_finite() && n.abs() <= 9007199254740991.0 && n.trunc() == n }

fn human_valid(value: &IaHuman, strings: &Strings) -> bool {
    if !value.valid { return false; }
    let HsCommand { kind, team, class_name, post } = &value.command;
    let name = |s: Option<StrId>| s.is_some_and(|s| (1..=64).contains(&strings.units(s).len()));
    match kind {
        HsCommandKind::Team => team.is_finite() && team.trunc() == *team && (1.0..=7.0).contains(team),
        HsCommandKind::Spawn => name(*class_name) && (post.is_none() || name(*post)),
        HsCommandKind::Suicide | HsCommandKind::Cancel => true,
    }
}

// A receipt backlog of twice the queue threw in JavaScript and would have
// ended the server's tick. A full backlog now drops the receipt, which the
// client treats as any lost receipt.
fn outcome(s: &mut IaState, id: f64, status: StrId) -> Result<(), &'static str> {
    if let Some(outcomes) = s.outcomes.as_mut() {
        if outcomes.len() as f64 >= s.capacity * 2.0 { return Ok(()); }
        outcomes.push(IaOutcome { id, status });
    }
    Ok(())
}

pub(crate) fn recycle_entry(entries: &mut Vec<IaEntry>, mut entry: IaEntry) {
    entry.actions.clear();
    entry.human.clear();
    if entries.len() < entries.capacity() {
        entries.push(entry);
    }
}

pub(crate) fn push_reusing(s: &mut IaState, entry: Option<IaEntry>, same_epoch: bool, same_generation: bool,
    payload_valid: bool, humans_valid: bool, humans: &[IaHuman], tick: f64, strings: &Strings,
    entries: &mut Vec<IaEntry>) -> Result<bool, &'static str> {
    let Some(mut entry) = entry else { return Ok(false) };
    if !same_epoch || !same_generation || !safe_integer(entry.id) || entry.id <= s.received ||
        !safe_integer(entry.tick) || entry.tick < 1.0 || entry.tick > tick + 120.0 {
        recycle_entry(entries, entry);
        return Ok(false);
    }
    if !payload_valid || entry.actions.len() > 64 || entry.actions.iter().any(|id| {
        let value = strings.units(*id);
        value.len() > 64 || value.iter().copied().eq("firePrimary".encode_utf16()) || value.iter().copied().eq("fireSecondary".encode_utf16())
    }) {
        recycle_entry(entries, entry);
        return Err("invalid reliable actions");
    }
    if !humans_valid || humans.len() > 16 || humans.iter().any(|h| !human_valid(h, strings)) {
        recycle_entry(entries, entry);
        return Err("invalid human input");
    }
    if s.queue.len() as f64 >= s.capacity {
        recycle_entry(entries, entry);
        return Ok(false);
    }
    if s.queue.last().is_some_and(|last| entry.tick < last.tick) {
        recycle_entry(entries, entry);
        return Err("action tick moved backwards");
    }
    entry.human.clear();
    entry.human.extend(humans.iter().map(|h| h.command.clone()));
    s.received = entry.id;
    s.queue.push(entry);
    Ok(true)
}

pub(crate) fn push(s: &mut IaState, entry: Option<IaEntry>, same_epoch: bool, same_generation: bool,
    payload_valid: bool, humans_valid: bool, humans: Vec<IaHuman>, tick: f64, strings: &Strings) -> Result<bool, &'static str> {
    let mut entries = Vec::new();
    push_reusing(s, entry, same_epoch, same_generation, payload_valid, humans_valid, &humans, tick, strings, &mut entries)
}

/// InputActions.take into the caller's retained lists: the first due
/// message's distinct presses and its choices; repeated presses in one
/// message remain separate ticks.
pub(crate) fn take_into_reusing(s: &mut IaState, tick: f64, generation: f64, actions: &mut Vec<StrId>,
    human: &mut Vec<HsCommand>, strings: &mut Strings, entries: &mut Vec<IaEntry>) -> Result<(), &'static str> {
    while let Some(entry) = s.queue.first() {
        if entry.generation != generation || entry.tick < tick - 120.0 {
            let id = entry.id;
            let status = if entry.generation != generation { "life-cancelled" } else { "expired" };
            outcome(s, id, strings.intern_str(status))?;
            s.discarded = v8math::max(s.discarded, id);
            let removed = s.queue.remove(0);
            recycle_entry(entries, removed);
            continue;
        }
        if entry.tick > tick { break; }
        let entry = &mut s.queue[0];
        for &action in &entry.actions { if !actions.contains(&action) { actions.push(action); } }
        human.append(&mut entry.human);
        for action in actions.iter() {
            let at = entry.actions.iter().position(|a| a == action).expect("an action from this entry");
            entry.actions.remove(at);
        }
        if entry.actions.is_empty() {
            let id = entry.id;
            outcome(s, id, strings.intern_str("consumed"))?;
            s.ack = id;
            let removed = s.queue.remove(0);
            recycle_entry(entries, removed);
        }
        return Ok(());
    }
    Ok(())
}

pub(crate) fn take_into(s: &mut IaState, tick: f64, generation: f64, actions: &mut Vec<StrId>, human: &mut Vec<HsCommand>, strings: &mut Strings) -> Result<(), &'static str> {
    let mut entries = Vec::new();
    take_into_reusing(s, tick, generation, actions, human, strings, &mut entries)
}

fn take(s: &mut IaState, tick: f64, generation: f64, strings: &mut Strings) -> Result<(Vec<StrId>, Vec<HsCommand>), &'static str> {
    let (mut actions, mut human) = (Vec::new(), Vec::new());
    take_into(s, tick, generation, &mut actions, &mut human, strings)?;
    Ok((actions, human))
}

/// InputActions.clear: queued messages are receipted as cleared and the
/// discard watermark moves past them.
pub(crate) fn clear_reusing(s: &mut IaState, strings: &mut Strings, entries: &mut Vec<IaEntry>) -> Result<(), &'static str> {
    for i in 0..s.queue.len() {
        let id = s.queue[i].id;
        outcome(s, id, strings.intern_str("cleared"))?;
    }
    s.discarded = v8math::max(s.discarded, s.queue.last().map_or(0.0, |e| e.id));
    for entry in s.queue.drain(..) {
        recycle_entry(entries, entry);
    }
    Ok(())
}

pub(crate) fn clear(s: &mut IaState, strings: &mut Strings) -> Result<(), &'static str> {
    let mut entries = Vec::new();
    clear_reusing(s, strings, &mut entries)
}

pub fn run(input: IaInput, strings: &mut Strings) -> IaResult {
    let mut out = IaResult { state: input.state, ..Default::default() };
    let result = match input.op {
        IaOperation::Push => push(&mut out.state, input.entry, input.same_epoch, input.same_generation,
            input.payload_valid, input.human_valid, input.human, input.tick, strings).map(|v| out.accepted = v),
        IaOperation::Take => take(&mut out.state, input.tick, input.generation, strings).map(|(actions, human)| { out.actions = actions; out.human = human; }),
        IaOperation::Clear => clear(&mut out.state, strings),
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

    #[test]
    fn repeated_presses_keep_humans_on_the_first_tick() {
        let mut strings = Strings::default();
        let jump = strings.intern_str("jump");
        let reload = strings.intern_str("reload");
        let mut s = IaState { epoch: 1.0, capacity: 128.0, outcomes: Some(vec![]), ..Default::default() };
        let entry = IaEntry { id: 1.0, tick: 10.0, generation: 2.0, actions: vec![jump, jump, reload], human: vec![] };
        let human = IaHuman { valid: true, command: HsCommand { kind: HsCommandKind::Suicide, ..Default::default() } };
        assert!(push(&mut s, Some(entry), true, true, true, true, vec![human], 10.0, &strings).unwrap());
        let (actions, human) = take(&mut s, 10.0, 2.0, &mut strings).unwrap();
        assert_eq!(actions, vec![jump, reload]); assert_eq!(human.len(), 1); assert_eq!(s.ack, 0.0);
        let (actions, human) = take(&mut s, 11.0, 2.0, &mut strings).unwrap();
        assert_eq!(actions, vec![jump]); assert!(human.is_empty()); assert_eq!(s.ack, 1.0);
        assert_eq!(s.outcomes.unwrap().len(), 1);
    }

    #[test]
    fn expired_and_old_life_records_do_not_acknowledge_input() {
        let mut strings = Strings::default();
        let mut s = IaState { capacity: 2.0, outcomes: Some(vec![]), queue: vec![
            IaEntry { id: 1.0, tick: 1.0, generation: 2.0, ..Default::default() },
            IaEntry { id: 2.0, tick: 122.0, generation: 1.0, ..Default::default() },
        ], ..Default::default() };
        assert!(take(&mut s, 122.0, 2.0, &mut strings).unwrap().0.is_empty());
        assert_eq!(s.ack, 0.0); assert_eq!(s.discarded, 2.0);
        assert!(s.queue.is_empty()); assert_eq!(s.outcomes.unwrap().len(), 2);
    }
}
