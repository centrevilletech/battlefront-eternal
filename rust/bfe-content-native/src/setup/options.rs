//! What a battle is asked for, as tools/world-hash.mjs and the client's battle
//! URL say it: the map, the side (and so the era), the seed, the AI flavour,
//! the heroes and the player's class. The query these make is the one
//! selectMission and spawnPolicy read (rust/content/src/mission.rs).
//!
//! The game the battle belongs to is the 2004 game unless the arguments name
//! the 2005 game: `--family swbf2`, or a qualified map `--map swbf2/<map>`. A
//! 2004 battle reads, checks and writes exactly what it always did (the mode is
//! carried but never checked or written). Declarations that disagree are
//! refused, whatever their order.
use bfe_family::{swbf2, GameFamily, DEFAULT_MODE};
use std::fmt;

/// `?ai=` (src/ai): the 2004 game's bots or the Enhanced ones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ai {
    #[default]
    Classic,
    Enhanced,
}
impl Ai {
    pub fn name(self) -> &'static str {
        match self {
            Ai::Classic => "classic",
            Ai::Enhanced => "enhanced",
        }
    }
    /// `--ai classic|enhanced`, anything else refused.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "classic" => Ok(Ai::Classic),
            "enhanced" => Ok(Ai::Enhanced),
            other => Err(format!("--ai {other}: classic or enhanced")),
        }
    }
}

/// The options world-hash.mjs reads from its arguments, with its defaults:
/// the 2004 game, nab2, rep, conquest, seed 1, Classic AI, no heroes, the
/// side's first class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleOptions {
    pub family: GameFamily,
    pub map: String,
    pub side: String,
    pub mode: String,
    pub seed: u32,
    pub ai: Ai,
    pub heroes: bool,
    pub unit: Option<String>,
}
impl Default for BattleOptions {
    fn default() -> Self {
        Self { family: GameFamily::Swbf1, map: "nab2".into(), side: "rep".into(), mode: DEFAULT_MODE.into(), seed: 1, ai: Ai::Classic, heroes: false, unit: None }
    }
}
impl BattleOptions {
    /// `--map`, `--side`, `--seed`, `--ai`, `--heroes`, `--unit`, `--family`,
    /// `--mode`, in any order; other arguments are left to the caller untouched.
    pub fn from_args<S: AsRef<str>>(args: &[S]) -> Result<Self, String> {
        let mut options = Self::default();
        // The games the arguments name: every `--family`, and the last `--map`
        // when it is qualified (a later `--map` replaces an earlier one).
        let mut named: Option<(GameFamily, String)> = None;
        let mut map_names: Option<String> = None;
        let mut at = 0;
        while at < args.len() {
            let arg = args[at].as_ref();
            let value = |at: usize| args.get(at + 1).map(|v| v.as_ref().to_string()).ok_or_else(|| format!("{arg} needs a value"));
            match arg {
                "--map" => {
                    let text = value(at)?;
                    match swbf2::bare_map(&text) {
                        Some(map) => {
                            options.map = map.to_string();
                            map_names = Some(text);
                        }
                        None => {
                            options.map = text;
                            map_names = None;
                        }
                    }
                    at += 1;
                }
                "--family" => {
                    let text = value(at)?;
                    let family = GameFamily::parse(&text).ok_or_else(|| format!("--family {text}: swbf1 or swbf2"))?;
                    if let Some((first, said)) = &named {
                        if *first != family { return Err(format!("{said} and --family {text} name different games")); }
                    }
                    named.get_or_insert((family, format!("--family {text}")));
                    at += 1;
                }
                "--mode" => { options.mode = value(at)?; at += 1; }
                "--side" => { options.side = value(at)?; at += 1; }
                "--seed" => {
                    let text = value(at)?;
                    options.seed = text.parse().map_err(|_| format!("--seed {text}: a whole number"))?;
                    at += 1;
                }
                "--ai" => { options.ai = Ai::parse(&value(at)?)?; at += 1; }
                "--heroes" => options.heroes = true,
                "--unit" => { options.unit = Some(value(at)?); at += 1; }
                _ => {}
            }
            at += 1;
        }
        options.family = match (named, map_names) {
            (Some((GameFamily::Swbf1, said)), Some(map)) => return Err(format!("{said} and --map {map} name different games")),
            (Some((family, _)), _) => family,
            (None, Some(_)) => GameFamily::Swbf2,
            (None, None) => GameFamily::Swbf1,
        };
        options.check()?;
        Ok(options)
    }
    /// Names reach file paths and the query: keep them to what prepared
    /// content uses, so a stray argument cannot leave the content folder.
    pub fn check(&self) -> Result<(), String> {
        let plain = |text: &str| !text.is_empty() && text.len() <= 64 && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if self.family == GameFamily::Swbf2 {
            if !swbf2::valid_field(&self.map) { return Err(format!("invalid map name {:?}", self.map)); }
            if !swbf2::valid_field(&self.mode) { return Err(format!("invalid mode {:?}", self.mode)); }
        }
        if !plain(&self.map) { return Err(format!("invalid map name {:?}", self.map)); }
        if !plain(&self.side) { return Err(format!("invalid side {:?}", self.side)); }
        if let Some(unit) = &self.unit { if !plain(unit) { return Err(format!("invalid class {unit:?}")); } }
        Ok(())
    }
    /// `['rep', 'cis'].includes(side) ? 'cw' : 'gcw'`.
    pub fn era(&self) -> &'static str {
        if self.side == "rep" || self.side == "cis" { "cw" } else { "gcw" }
    }
    /// The battle's query, word for word as world-hash.mjs composes it. A 2005
    /// battle adds `family=swbf2` and a mode other than conquest.
    pub fn query(&self) -> String {
        let mut query = format!("?map={}&side={}&era={}&ai={}&seed={}", self.map, self.side, self.era(), self.ai.name(), self.seed);
        if self.heroes { query.push_str("&heroes=1"); }
        if let Some(unit) = &self.unit { query.push_str("&unit="); query.push_str(unit); }
        if self.family == GameFamily::Swbf2 {
            query.push_str("&family=swbf2");
            if self.mode != DEFAULT_MODE { query.push_str("&mode="); query.push_str(&self.mode); }
        }
        query
    }
}
impl fmt::Display for BattleOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.query())
    }
}
