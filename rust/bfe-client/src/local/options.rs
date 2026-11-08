//! Native battle launch arguments. BattleOptions owns mission policy; this
//! parser only separates device choices and the content directory from the
//! arguments forwarded to that cold builder. No recorded setup is a fallback.
use std::{ffi::OsString, path::PathBuf, time::Duration};

pub struct Options {
    pub root: PathBuf,
    pub battle: Vec<String>,
    pub audio: bool,
    pub duration: Option<Duration>,
}

impl Options {
    // Keep query and startup policy in the content owner's BattleOptions.
    // The Context uses the same validated values; this host only translates
    // their enum representation at the cold admission boundary.
    pub fn world(&self) -> Result<bfe_sim::world::admit::Options, String> {
        use bfe_content_native::setup::{Ai, BattleOptions};
        let options = BattleOptions::from_args(&self.battle)?;
        Ok(bfe_sim::world::admit::Options { map: options.map, side: options.side,
            enhanced: options.ai == Ai::Enhanced, seed: options.seed, heroes: options.heroes, unit: options.unit })
    }
    // session_window::Options::read selects its mode before making a window.
    // Keep the existing online/archive parser authoritative for other modes;
    // a local launch must never consume one of its arguments accidentally.
    pub fn parse(args: &[String], content: Option<OsString>) -> Result<Option<Self>, String> {
        if args.first().map(String::as_str) != Some("--local") { return Ok(None); }
        let map = args.get(1).filter(|s| !s.starts_with('-')).ok_or("missing local battle map")?;
        let mut battle = vec!["--map".into(), map.clone()];
        let mut root = content.map(PathBuf::from);
        let mut audio = true;
        let mut duration = None;
        let mut at = 2;
        while at < args.len() {
            let name = &args[at];
            match name.as_str() {
                "--side" | "--seed" | "--ai" | "--unit" => {
                    // Validate mission values in BattleOptions, after this
                    // parser has separated them from device/content choices.
                    let value = args.get(at + 1).filter(|s| !s.starts_with('-')).ok_or_else(|| format!("missing value for {name}"))?;
                    battle.push(name.clone()); battle.push(value.clone()); at += 1;
                }
                "--heroes" => battle.push(name.clone()),
                "--no-audio" => audio = false,
                "--seconds" => {
                    // Timed desktop gates close through ordinary teardown.
                    // Reject nonfinite durations before Duration can panic.
                    let value: f64 = args.get(at + 1).ok_or("missing battle duration")?.parse().map_err(|_| "invalid battle duration")?;
                    if !value.is_finite() || !(0.1..=86400.).contains(&value) { return Err("invalid battle duration".into()); }
                    duration = Some(Duration::from_secs_f64(value)); at += 1;
                }
                _ if name.starts_with('-') => return Err(format!("unknown local battle option {name}")),
                // An explicit content directory takes precedence over the
                // environment, matching the other native client modes.
                _ => root = Some(PathBuf::from(name)),
            }
            at += 1;
        }
        Ok(Some(Self { root: root.ok_or("content directory required (argument or BFE_CONTENT_DIR)")?, battle, audio, duration }))
    }
}
