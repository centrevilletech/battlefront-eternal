//! Process-local battle sequencing from tools/world-hash.mjs `battle(index)`.
//! Each admission owns a fresh Context. Corpus frames retain the process
//! clock, so a later battle must not synthesize its time from its tick.
use super::{admission, Args};
use bfe_sim::abi::context::Context;
use serde_json::Value;

pub fn frame_battle(frame: &Value) -> usize {
    frame["battle"].as_u64().unwrap_or(0) as usize
}

/// The oracle prefixes only later battles, including their first tick.
/// This is output formatting, independent of the world's clock reset.
pub fn prefix(battle: usize) -> String {
    if battle == 0 {
        String::new()
    } else {
        format!("{}:", battle + 1)
    }
}

pub(super) fn run(args: &Args) -> Result<(), String> {
    if args.battles == 0 {
        return Err("--battles must be at least 1".into());
    }
    let frames = super::read_frames(&args.inputs, &args.label)?;
    for battle in 0..args.battles {
        // A missing later corpus must fail rather than certify an empty run.
        if !frames.iter().any(|f| frame_battle(f) == battle) {
            return Err(format!("corpus has no battle {}", battle + 1));
        }
        super::run_battle(args, battle)?;
    }
    Ok(())
}

/// Recorded admissions include every battle, separated by `battle` markers.
/// Keep its own checkpoint and trailing setup calls, never the prior teardown.
/// A new native world has no pointer identities from the previous admission.
pub fn admission_lines(lines: Vec<Value>, battle: usize) -> Result<Vec<Value>, String> {
    let start = lines
        .iter()
        .position(|v| v["type"] == "battle" && frame_battle(v) == battle)
        .ok_or_else(|| format!("recording has no battle {}", battle + 1))?;
    let end = lines[start + 1..]
        .iter()
        .position(|v| v["type"] == "battle")
        .map_or(lines.len(), |n| start + 1 + n);
    Ok(lines.into_iter().skip(start).take(end - start).collect())
}

pub(super) fn admit(
    context: &mut Context,
    args: &Args,
    battle: usize,
) -> Result<admission::Handles, String> {
    if battle == 0 || bfe_sim::world::admit::provided() {
        admission::admit_world(
            context,
            &args.label,
            &args.options,
            &args.admissions,
            &args.content,
        )
    } else {
        admission::replay_recorded(context, &args.label, &args.admissions, battle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn later_admission_excludes_prior_calls_and_teardown() {
        let lines = vec![
            json!({"type":"battle","battle":0}),
            json!({"type":"call","export":"dispose"}),
            json!({"type":"battle","battle":1}),
            json!({"type":"call","export":"create"}),
            json!({"type":"end"}),
        ];
        let selected = admission_lines(lines, 1).unwrap();
        assert_eq!(selected.len(), 3);
        assert_eq!(selected[1]["export"], "create");
        assert_eq!(prefix(0), "");
        assert_eq!(prefix(1), "2:");
    }

    #[test]
    fn missing_admission_is_an_error() {
        assert!(admission_lines(vec![json!({"type":"battle","battle":0})], 1).is_err());
    }
}
