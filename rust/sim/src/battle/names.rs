//! Bot names (src/bot-names.js), as the 2004 game makes them when a team's
//! Characters are created at the battle's start: each team draws from a
//! generator of its own, seeded with its side type (Rebels 1, Empire 2,
//! Republic 3, CIS 4); a name is a first name from the side's list (or two
//! random letters A-Z when it is empty), a space, then a last name from its
//! list (or three random digits), each a raw draw modulo the list's length.
//! Locals (side type 5) are all called the level's local name.

use jsrt::rng::ClassicRandom;

pub const LOCALS: f64 = 5.0;
const LIST_LIMIT: usize = 64;

/// A side's name lists (ai-mission.json `names`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NameLists {
    /// By side type 0..4: (first names, last names).
    pub sides: Vec<(Vec<String>, Vec<String>)>,
    /// The level's local name.
    pub local: Option<String>,
}

/// A team's name generator (botNames(lists, faction)'s closure).
#[derive(Clone, Debug, PartialEq)]
pub enum BotNames {
    Locals(String),
    Side { random: ClassicRandom, first: Vec<String>, last: Vec<String> },
}

impl BotNames {
    /// botNames(lists, faction) (bot-names.js:23).
    pub fn new(lists: Option<&NameLists>, faction: f64) -> BotNames {
        if faction == LOCALS {
            return BotNames::Locals(lists.and_then(|l| l.local.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| "LOCAL".into()));
        }
        let side = if (1.0..=4.0).contains(&faction) { faction } else { 0.0 };
        let list = |pick: fn(&(Vec<String>, Vec<String>)) -> &Vec<String>| -> Vec<String> {
            lists
                .and_then(|l| l.sides.get(side as usize))
                .map(|s| pick(s).iter().take(LIST_LIMIT).cloned().collect())
                .unwrap_or_default()
        };
        BotNames::Side { random: ClassicRandom::new(side), first: list(|s| &s.0), last: list(|s| &s.1) }
    }

    /// The team's next name.
    pub fn next(&mut self) -> String {
        match self {
            BotNames::Locals(name) => name.clone(),
            BotNames::Side { random, first, last } => {
                let mut pick = |count: usize| (random.next_f64() % count as f64) as usize;
                let given = if !first.is_empty() {
                    first[pick(first.len())].clone()
                } else {
                    let a = (65 + pick(26)) as u8 as char;
                    let b = (65 + pick(26)) as u8 as char;
                    format!("{a}{b}")
                };
                let family = if !last.is_empty() {
                    last[pick(last.len())].clone()
                } else {
                    let a = (48 + pick(10)) as u8 as char;
                    let b = (48 + pick(10)) as u8 as char;
                    let c = (48 + pick(10)) as u8 as char;
                    format!("{a}{b}{c}")
                };
                format!("{given} {family}")
            }
        }
    }
}

/// humanName(index) (bot-names.js:43): "Player n".
pub fn human_name(index: f64) -> String {
    format!("Player {}", jsrt::numfmt::number_to_string(index + 1.0))
}
