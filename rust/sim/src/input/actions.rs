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

fn outcome(s: &mut IaState, id: f64, status: StrId) -> Result<(), &'static str> {
    if let Some(outcomes) = s.outcomes.as_mut() {
        if outcomes.len() as f64 >= s.capacity * 2.0 { return Err("action outcome backlog exceeded"); }
        outcomes.push(IaOutcome { id, status });
    }
    Ok(())
}

fn push(s: &mut IaState, entry: Option<IaEntry>, same_epoch: bool, same_generation: bool,
    payload_valid: bool, humans_valid: bool, humans: Vec<IaHuman>, tick: f64, strings: &Strings) -> Result<bool, &'static str> {
    let Some(mut entry) = entry else { return Ok(false) };
    if !same_epoch || !same_generation || !safe_integer(entry.id) || entry.id <= s.received ||
        !safe_integer(entry.tick) || entry.tick < 1.0 || entry.tick > tick + 120.0 { return Ok(false); }
    if !payload_valid || entry.actions.len() > 64 || entry.actions.iter().any(|id| {
        let value = strings.units(*id);
        value.len() > 64 || value.iter().copied().eq("firePrimary".encode_utf16()) || value.iter().copied().eq("fireSecondary".encode_utf16())
    }) { return Err("invalid reliable actions"); }
    if !humans_valid || humans.len() > 16 || humans.iter().any(|h| !human_valid(h, strings)) { return Err("invalid human input"); }
    if s.queue.len() as f64 >= s.capacity { return Ok(false); }
    if s.queue.last().is_some_and(|last| entry.tick < last.tick) { return Err("action tick moved backwards"); }
    entry.human = humans.into_iter().map(|h| h.command).collect();
    s.received = entry.id;
    s.queue.push(entry);
    Ok(true)
}

fn take(s: &mut IaState, tick: f64, generation: f64, strings: &mut Strings) -> Result<(Vec<StrId>, Vec<HsCommand>), &'static str> {
    while let Some(entry) = s.queue.first() {
        if entry.generation != generation || entry.tick < tick - 120.0 {
            let id = entry.id;
            let status = if entry.generation != generation { "life-cancelled" } else { "expired" };
            outcome(s, id, strings.intern_str(status))?;
            s.discarded = v8math::max(s.discarded, id);
            s.queue.remove(0);
            continue;
        }
        if entry.tick > tick { break; }
        let entry = &mut s.queue[0];
        let mut actions = Vec::new();
        for &action in &entry.actions { if !actions.contains(&action) { actions.push(action); } }
        let human = std::mem::take(&mut entry.human);
        for action in &actions {
            let at = entry.actions.iter().position(|a| a == action).expect("an action from this entry");
            entry.actions.remove(at);
        }
        if entry.actions.is_empty() {
            let id = entry.id;
            outcome(s, id, strings.intern_str("consumed"))?;
            s.ack = id;
            s.queue.remove(0);
        }
        return Ok((actions, human));
    }
    Ok((Vec::new(), Vec::new()))
}

pub fn run(input: IaInput, strings: &mut Strings) -> IaResult {
    let mut out = IaResult { state: input.state, ..Default::default() };
    let result = match input.op {
        IaOperation::Push => push(&mut out.state, input.entry, input.same_epoch, input.same_generation,
            input.payload_valid, input.human_valid, input.human, input.tick, strings).map(|v| out.accepted = v),
        IaOperation::Take => take(&mut out.state, input.tick, input.generation, strings).map(|(actions, human)| { out.actions = actions; out.human = human; }),
        IaOperation::Clear => (|| {
            for i in 0..out.state.queue.len() {
                let id = out.state.queue[i].id;
                outcome(&mut out.state, id, strings.intern_str("cleared"))?;
            }
            out.state.discarded = v8math::max(out.state.discarded, out.state.queue.last().map_or(0.0, |e| e.id));
            out.state.queue.clear(); Ok(())
        })(),
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
