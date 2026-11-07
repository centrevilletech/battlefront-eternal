//! predictionCommands and EntityInputSender.replayActions shared sequence
//! identity but rebuilt Maps and cloned command objects on every page frame.
//! Merge their raw transport rows here, in the existing prediction call.
use super::span;
use crate::gen::{ClientCommand as C, ClientCommandSource as Source, ClientFrame};
const LIMIT: usize = bfe_presentation::network_layout::net_limits::PREDICTION_COMMANDS;
// The reservation belongs to actor, hull or recon configuration. A long
// transport pause keeps a bounded prefix during assembly; it never asks the
// allocator to expand a live replay. Direct native hosts can supply already
// ordered canonical commands without constructing an action decoration row.
pub(super) fn reserve(rows: &mut Vec<f64>) {
    if rows.capacity() < LIMIT * C::WORDS {
        rows.reserve(LIMIT * C::WORDS - rows.len());
    }
}
pub(super) fn assemble(words: &[f64], frame: &ClientFrame, out: &mut Vec<f64>) -> Result<(), &'static str> {
    let input = span(words, frame.commands_at, frame.commands_count, C::WORDS)?;
    out.clear();
    // Movement wins a duplicate sequence, then the first reliable action's
    // original stamp. Fire presses alone also keep a replay command alive.
    // Other outcomes decorate existing commands without creating movement.
    for source in [Source::Movement, Source::Action, Source::Fire] {
        let source = source as u32 as f64;
        for row in input.chunks_exact(C::WORDS) {
            if row[C::SOURCE] != source {
                continue;
            }
            let sequence = row[C::SEQUENCE];
            if out.chunks_exact(C::WORDS).any(|old| old[C::SEQUENCE] == sequence) {
                continue;
            }
            if out.len() == LIMIT * C::WORDS {
                continue;
            }
            out.extend_from_slice(row);
            let start = out.len() - C::WORDS;
            out[start + C::ACTIONS] = 0.;
            out[start + C::FLIGHT_ACTIONS] = 0.;
            if source != Source::Movement as u32 as f64 {
                out[start + C::HELD] = frame.held as f64;
                out[start + C::LOOK_COUNT] = 0.;
            }
        }
    }
    // Sorting is stable and in place. At most the two-second command window
    // crosses this boundary; no comparison allocates or observes host objects.
    for n in 1..out.len() / C::WORDS {
        let mut at = n;
        while at > 0 && out[at * C::WORDS + C::SEQUENCE] < out[(at - 1) * C::WORDS + C::SEQUENCE] {
            for k in 0..C::WORDS {
                out.swap(at * C::WORDS + k, (at - 1) * C::WORDS + k);
            }
            at -= 1;
        }
    }
    // Actions decorate the winning movement row after duplicate removal.
    // Multiple reliable presses retain their separate cosmetic counts in
    // that row, while boolean controller actions combine as bit groups.
    // An ordinary action without a retained command cannot resurrect movement.
    for row in input.chunks_exact(C::WORDS) {
        if row[C::SOURCE] < Source::Outcome as u32 as f64 {
            continue;
        }
        for command in out.chunks_exact_mut(C::WORDS) {
            if command[C::SEQUENCE] == row[C::SEQUENCE] {
                command[C::ACTIONS] = ((command[C::ACTIONS] as u32) | (row[C::ACTIONS] as u32)) as f64;
                command[C::FLIGHT_ACTIONS] = ((command[C::FLIGHT_ACTIONS] as u32) | (row[C::FLIGHT_ACTIONS] as u32)) as f64;
                break;
            }
        }
    }
    Ok(())
}
