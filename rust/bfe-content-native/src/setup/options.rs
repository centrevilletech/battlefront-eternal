//! What a battle is asked for, as tools/world-hash.mjs and the page's battle
//! URL say it: the map, the side (and so the era), the seed, the AI flavour,
//! the heroes and the player's class. The query these make is the one
//! selectMission and spawnPolicy read (rust/content/src/mission.rs).
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
    /// world-hash.mjs: `--ai classic|enhanced`, anything else refused.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "classic" => Ok(Ai::Classic),
            "enhanced" => Ok(Ai::Enhanced),
            other => Err(format!("--ai {other}: classic or enhanced")),
        }
    }
}

/// The options world-hash.mjs reads from its arguments, with its defaults:
/// nab2, rep, seed 1, Classic AI, no heroes, the side's first class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleOptions {
    pub map: String,
    pub side: String,
    pub seed: u32,
    pub ai: Ai,
    pub heroes: bool,
    pub unit: Option<String>,
}
impl Default for BattleOptions {
    fn default() -> Self {
        Self { map: "nab2".into(), side: "rep".into(), seed: 1, ai: Ai::Classic, heroes: false, unit: None }
    }
}
impl BattleOptions {
    /// `--map`, `--side`, `--seed`, `--ai`, `--heroes`, `--unit`, in any
    /// order; other arguments are left to the caller untouched.
    pub fn from_args<S: AsRef<str>>(args: &[S]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut at = 0;
        while at < args.len() {
            let arg = args[at].as_ref();
            let value = |at: usize| args.get(at + 1).map(|v| v.as_ref().to_string()).ok_or_else(|| format!("{arg} needs a value"));
            match arg {
                "--map" => { options.map = value(at)?; at += 1; }
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
        options.check()?;
        Ok(options)
    }
    /// Names reach file paths and the query: keep them to what prepared
    /// content uses, so a stray argument cannot leave the content folder.
    pub fn check(&self) -> Result<(), String> {
        let plain = |text: &str| !text.is_empty() && text.len() <= 64 && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
        if !plain(&self.map) { return Err(format!("invalid map name {:?}", self.map)); }
        if !plain(&self.side) { return Err(format!("invalid side {:?}", self.side)); }
        if let Some(unit) = &self.unit { if !plain(unit) { return Err(format!("invalid class {unit:?}")); } }
        Ok(())
    }
    /// world-hash.mjs: `['rep', 'cis'].includes(side) ? 'cw' : 'gcw'`.
    pub fn era(&self) -> &'static str {
        if self.side == "rep" || self.side == "cis" { "cw" } else { "gcw" }
    }
    /// The battle's query, word for word as world-hash.mjs composes it.
    pub fn query(&self) -> String {
        let mut query = format!("?map={}&side={}&era={}&ai={}&seed={}", self.map, self.side, self.era(), self.ai.name(), self.seed);
        if self.heroes { query.push_str("&heroes=1"); }
        if let Some(unit) = &self.unit { query.push_str("&unit="); query.push_str(unit); }
        query
    }
}
impl fmt::Display for BattleOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.query())
    }
}
