//! Where a server's reply or listing names the game of its battle, read one way
//! by every reader: the client before it joins, and the server browser.
//!
//! A released server names nothing but its battle query and its rotation, and
//! nothing it publishes for a battle a released client can join names another
//! game. What can name a game:
//!
//! - the namespace of a map id ([`namespace`]): the text before its first `/`
//!   when it is a family id, in any case and with surrounding space
//!   (`swbf2/tat2`, `SWBF1/hot1`). An id without `/` names nothing (a released
//!   2004 id), nor does a namespace that is no family (`mods/hot1`). The 2004
//!   content loader refuses every map id with a `/`.
//! - a battle query's first `map`, decoded as the content code decodes the
//!   query ([`query_map_namespace`]). No other key of the query is read: a
//!   released server copies an entry's side text into its query verbatim, so a
//!   playable 2004 battle can carry any key there.
//! - a rotation slug ([`slug_owner`]): the 2005 game when it starts with the
//!   literal `swbf2/` (a server reads every such entry as a 2005 entry,
//!   whatever follows), else the family a released join reads in it;
//! - the `family` members of a structured object, beside the namespace of its
//!   `map`.
//!
//! Every game a reply names is collected ([`Owners`], [`Stated`]): repeated
//! identical statements are one, and two different ones are a disagreement that
//! every reader refuses ([`Verdict::Two`]).

use std::fmt;

use serde::{Serialize, Serializer};

use crate::{FamilyTag, GameFamily};

/// What the namespace of a map id names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace {
    /// No `/`: an id as released 2004 builds write it.
    Bare,
    /// A family id before its first `/` (`swbf1/…`, `SWBF2/…`).
    Family(GameFamily),
    /// A `/` after text that names no family (`mods/…`, `swbf9/…`).
    Other,
}

/// Maximum byte length of a family name after surrounding space is removed.
pub const MAX_NAMESPACE: usize = 32;

/// The namespace of a map id or a rotation slug.
pub fn namespace(id: &str) -> Namespace {
    match id.split_once('/') {
        None => Namespace::Bare,
        Some((namespace, _)) => family_namespace(namespace),
    }
}

fn family_namespace(text: &str) -> Namespace {
    let text = text.trim();
    if text.len() > MAX_NAMESPACE {
        return Namespace::Other;
    }
    match GameFamily::parse(text) {
        Some(family) => Namespace::Family(family),
        None => Namespace::Other,
    }
}

/// What a rotation slug names: the 2005 game for a slug that starts with the
/// literal `swbf2/`, else the family a released join reads in it, a family
/// namespace before `<map>:<side>` with both present (`swbf1/tat2:all`,
/// `SWBF2/tat2:gcw:eli`). Any other slug names nothing, an unqualified one
/// (`tat2:all:conquest`) included.
pub fn slug_owner(slug: &str) -> Option<GameFamily> {
    if crate::swbf2::is_entry(slug) {
        return Some(GameFamily::Swbf2);
    }
    let (namespace, rest) = slug.split_once('/')?;
    let family = GameFamily::parse(namespace)?;
    let mut parts = rest.split(':');
    parts.next().filter(|map| !map.is_empty())?;
    parts.next().filter(|side| !side.is_empty())?;
    Some(family)
}

/// The value of a battle query's first `map`, before decoding, or `None` when
/// the query has none ([`query_value`]).
pub fn query_map(query: &str) -> Option<&str> {
    query_value(query, "map")
}

/// The value of a battle query's first `key`, before decoding, or `None` when
/// the query has none. The query is read as the content code reads it
/// (`bfe_content::mission::Query`): an optional leading `?`, pairs separated by
/// `&` (empty ones skipped) and split at their first `=` (a pair with none has
/// an empty value), each key compared after decoding.
pub fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    let text = query.strip_prefix('?').unwrap_or(query);
    text.split('&').filter(|part| !part.is_empty()).find_map(|part| {
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        Decoded::new(name).eq(key.bytes()).then_some(value)
    })
}

/// The namespace of a battle query's first `map` ([`query_map`]), decoded as
/// the content code decodes it (`+` is a space, `%XX` a byte), without
/// allocating.
pub fn query_map_namespace(query: &str) -> Namespace {
    let Some(value) = query_map(query) else {
        return Namespace::Bare;
    };
    let mut bytes = Decoded::new(value);
    loop {
        let start = bytes.at;
        match bytes.next() {
            Some(b'/') => return encoded_namespace(&value[..start]),
            None => return Namespace::Bare,
            _ => {}
        }
    }
}

fn encoded_namespace(raw: &str) -> Namespace {
    if !raw.contains(['%', '+']) {
        return family_namespace(raw);
    }
    let mut bytes = Decoded::new(raw);
    let mut name = [0; MAX_NAMESPACE];
    let mut len = 0;
    let mut trailing_space = false;
    while let Some(byte) = bytes.next() {
        let space = if byte.is_ascii() {
            char::from(byte).is_whitespace()
        } else {
            // Only whitespace outside the ASCII family name can match.
            let width = byte.leading_ones() as usize;
            if !(2..=4).contains(&width) { return Namespace::Other; }
            let mut scalar = [byte; 4];
            for part in &mut scalar[1..width] {
                let Some(next) = bytes.next() else { return Namespace::Other; };
                *part = next;
            }
            if !std::str::from_utf8(&scalar[..width]).is_ok_and(|s| s.trim().is_empty()) {
                return Namespace::Other;
            }
            true
        };
        if space {
            trailing_space |= len != 0;
        } else {
            if trailing_space || len == name.len() { return Namespace::Other; }
            name[len] = byte;
            len += 1;
        }
    }
    family_namespace(std::str::from_utf8(&name[..len]).expect("ASCII family name"))
}

/// The bytes of a query key or value after the content code's decoding.
struct Decoded<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Decoded<'a> {
    fn new(text: &'a str) -> Self {
        Self { bytes: text.as_bytes(), at: 0 }
    }
}

impl Iterator for Decoded<'_> {
    type Item = u8;

    fn next(&mut self) -> Option<u8> {
        let byte = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(match byte {
            b'+' => b' ',
            b'%' => {
                let hex = |at: usize| self.bytes.get(at).and_then(|c| (*c as char).to_digit(16));
                match (hex(self.at), hex(self.at + 1)) {
                    (Some(high), Some(low)) => {
                        self.at += 2;
                        (high * 16 + low) as u8
                    }
                    _ => b'%',
                }
            }
            other => other,
        })
    }
}

/// One game a reply names, borrowed from where it is named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner<'a> {
    /// A family this build knows.
    Known(GameFamily),
    /// A `family` text this build does not know, as written.
    Unknown(&'a str),
}

impl<'a> From<&'a FamilyTag> for Owner<'a> {
    fn from(tag: &'a FamilyTag) -> Self {
        match tag {
            FamilyTag::Known(family) => Self::Known(*family),
            FamilyTag::Unknown(text) => Self::Unknown(text),
        }
    }
}

impl Owner<'_> {
    /// The statement, kept.
    pub fn to_tag(self) -> FamilyTag {
        match self {
            Self::Known(family) => FamilyTag::Known(family),
            Self::Unknown(text) => FamilyTag::Unknown(text.into()),
        }
    }
}

impl fmt::Display for Owner<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Known(family) => f.write_str(family.title()),
            Self::Unknown(text) => write!(f, "{text:?}"),
        }
    }
}

/// What everything a reply names comes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict<'a> {
    /// Nothing names a game: the 2004 game, as every released battle.
    Unstated,
    /// One game, named once or more.
    One(Owner<'a>),
    /// Two different games named, the earliest two.
    Two(Owner<'a>, Owner<'a>),
}

/// Every game a reply names, collected without allocating.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Owners<'a> {
    first: Option<Owner<'a>>,
    other: Option<Owner<'a>>,
}

impl<'a> Owners<'a> {
    /// Adds one statement.
    pub fn add(&mut self, owner: Owner<'a>) {
        match self.first {
            None => self.first = Some(owner),
            Some(first) if first != owner && self.other.is_none() => self.other = Some(owner),
            Some(_) => {}
        }
    }

    /// Adds what a namespace names (only a family names anything).
    pub fn add_namespace(&mut self, namespace: Namespace) {
        if let Namespace::Family(family) = namespace {
            self.add(Owner::Known(family));
        }
    }

    /// Adds what a map id names by its namespace.
    pub fn add_id(&mut self, id: &str) {
        self.add_namespace(namespace(id));
    }

    /// Adds what a rotation slug names ([`slug_owner`]).
    pub fn add_slug(&mut self, slug: &str) {
        if let Some(family) = slug_owner(slug) {
            self.add(Owner::Known(family));
        }
    }

    /// Adds every statement of `stated`.
    pub fn add_stated(&mut self, stated: &'a Stated) {
        for tag in stated.tags() {
            self.add(tag.into());
        }
    }

    /// The statements collected, in the order they were made.
    pub fn iter(&self) -> impl Iterator<Item = Owner<'a>> {
        self.first.into_iter().chain(self.other)
    }

    /// What the statements come to.
    pub fn verdict(&self) -> Verdict<'a> {
        match (self.first, self.other) {
            (None, _) => Verdict::Unstated,
            (Some(one), None) => Verdict::One(one),
            (Some(first), Some(other)) => Verdict::Two(first, other),
        }
    }
}

/// The games something names, kept: an object's `family` members, or all a
/// reply names. Repeated identical statements are one; once two different
/// ones are kept nothing more is, since two already disagree.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stated {
    first: Option<FamilyTag>,
    other: Option<FamilyTag>,
}

impl Stated {
    /// Adds one statement.
    pub fn add(&mut self, tag: FamilyTag) {
        match &self.first {
            None => self.first = Some(tag),
            Some(first) if *first != tag && self.other.is_none() => self.other = Some(tag),
            Some(_) => {}
        }
    }

    /// Adds what a map id names by its namespace.
    pub fn add_id(&mut self, id: &str) {
        if let Namespace::Family(family) = namespace(id) {
            self.add(FamilyTag::Known(family));
        }
    }

    /// Adds what a rotation slug names ([`slug_owner`]).
    pub fn add_slug(&mut self, slug: &str) {
        if let Some(family) = slug_owner(slug) {
            self.add(FamilyTag::Known(family));
        }
    }

    /// Adds every statement of `owners`.
    pub fn add_owners(&mut self, owners: Owners<'_>) {
        for owner in owners.iter() {
            self.add(owner.to_tag());
        }
    }

    /// Whether nothing is named.
    pub fn is_empty(&self) -> bool {
        self.first.is_none()
    }

    /// The statements kept, in the order they were made.
    pub fn tags(&self) -> impl Iterator<Item = &FamilyTag> {
        self.first.iter().chain(self.other.iter())
    }

    /// The statements, to combine with others.
    pub fn owners(&self) -> Owners<'_> {
        let mut owners = Owners::default();
        owners.add_stated(self);
        owners
    }

    /// The value written back as a `family` member: none when nothing or only
    /// the 2004 game is named (released objects carry no such member), else the
    /// first other game named.
    pub fn written(&self) -> Option<&FamilyTag> {
        self.tags().find(|tag| **tag != FamilyTag::Known(GameFamily::Swbf1))
    }

    /// Whether [`Stated::written`] writes nothing, for
    /// `skip_serializing_if`.
    pub fn writes_nothing(&self) -> bool {
        self.written().is_none()
    }
}

impl From<FamilyTag> for Stated {
    fn from(tag: FamilyTag) -> Self {
        let mut stated = Self::default();
        stated.add(tag);
        stated
    }
}

/// Writes [`Stated::written`] (a caller skips the member when that is none).
impl Serialize for Stated {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.written() {
            Some(tag) => tag.serialize(serializer),
            None => serializer.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_namespace_names_a_family_in_any_case_and_nothing_else_names_one() {
        assert_eq!(namespace("tat2"), Namespace::Bare);
        assert_eq!(namespace("swbf2/tat2"), Namespace::Family(GameFamily::Swbf2));
        assert_eq!(namespace("SWBF2/tat2:gcw:eli"), Namespace::Family(GameFamily::Swbf2));
        assert_eq!(namespace(" swbf1 /hot1"), Namespace::Family(GameFamily::Swbf1));
        assert_eq!(namespace("swbf1/hot1/cw"), Namespace::Family(GameFamily::Swbf1));
        for other in ["mods/hot1", "swbf9/tat2", "/tat2", "tat2:all/x:conquest", "swbf2:tat2/x"] {
            assert_eq!(namespace(other), Namespace::Other, "{other}");
        }
        let padded = format!("{}swbf2/tat2", " ".repeat(MAX_NAMESPACE));
        assert_eq!(namespace(&padded), Namespace::Family(GameFamily::Swbf2));
    }

    #[test]
    fn a_slug_names_what_the_server_or_a_released_join_reads_in_it() {
        for (slug, family) in [
            ("swbf2/tat2:gcw:eli", Some(GameFamily::Swbf2)),
            ("swbf2/tat2::eli", Some(GameFamily::Swbf2)),
            ("swbf2/tat2", Some(GameFamily::Swbf2)),
            ("SWBF2/tat2:gcw:eli", Some(GameFamily::Swbf2)),
            (" swbf2/tat2:gcw", Some(GameFamily::Swbf2)),
            ("swbf1/tat2:all:conquest", Some(GameFamily::Swbf1)),
            ("SWBF2/tat2", None),
            (" swbf2/nab2/rep", None),
            ("SWBF2/tat2::eli", None),
            ("swbf1/tat2", None),
            ("tat2:all:conquest", None),
            ("mods/tat2:all:conquest", None),
        ] {
            assert_eq!(slug_owner(slug), family, "{slug}");
        }
    }

    #[test]
    fn the_query_map_is_the_first_decoded_map_key() {
        assert_eq!(query_map("?map=tat2&side=all"), Some("tat2"));
        assert_eq!(query_map("map=tat2"), Some("tat2"));
        assert_eq!(query_map("?side=all"), None);
        assert_eq!(query_map("?&&m%61p=tat2&map=swbf2/tat2"), Some("tat2"));
        assert_eq!(query_map("?map&map=swbf2/tat2"), Some(""));
        assert_eq!(query_map("?maps=x&ma+p=y&map=z"), Some("z"));
        assert_eq!(query_map("??map=x"), None);
        assert_eq!(query_map_namespace("?map=swbf2%2Ftat2"), Namespace::Family(GameFamily::Swbf2));
        assert_eq!(query_map_namespace("?m%61p=tat2&map=swbf2/tat2"), Namespace::Bare);
        assert_eq!(query_map_namespace("?map=%53WBF2/tat2"), Namespace::Family(GameFamily::Swbf2));
        assert_eq!(query_map_namespace("?map=+swbf2+/tat2"), Namespace::Family(GameFamily::Swbf2));
        assert_eq!(query_map_namespace("?map=swbf9/tat2"), Namespace::Other);
        assert_eq!(query_map_namespace("?map=%zzswbf2/tat2"), Namespace::Other);
        assert_eq!(query_map_namespace("?map=sw%ffbf2/tat2"), Namespace::Other);
        assert_eq!(query_map_namespace("?map=tat2&map=swbf2/tat2"), Namespace::Bare);
        for padding in ["+".repeat(MAX_NAMESPACE), "%E2%80%83".repeat(34)] {
            let query = format!("?map={padding}SwBf2{padding}%2Fmoon");
            assert_eq!(query_map_namespace(&query), Namespace::Family(GameFamily::Swbf2));
        }
        assert_eq!(query_map_namespace("?map=sw+bf2/one"), Namespace::Other);
        assert_eq!(query_map_namespace(&format!("?map={}", "a".repeat(100))), Namespace::Bare);
    }

    #[test]
    fn repeated_statements_are_one_and_different_ones_disagree() {
        let swbf9 = FamilyTag::read("swbf9");
        let mut owners = Owners::default();
        assert_eq!(owners.verdict(), Verdict::Unstated);
        owners.add_id("tat2");
        owners.add_id("mods/tat2");
        assert_eq!(owners.verdict(), Verdict::Unstated);
        owners.add(Owner::Known(GameFamily::Swbf1));
        owners.add_id("SWBF1/tat2");
        assert_eq!(owners.verdict(), Verdict::One(Owner::Known(GameFamily::Swbf1)));
        owners.add_id("swbf2/tat2");
        owners.add((&swbf9).into());
        assert_eq!(owners.verdict(), Verdict::Two(Owner::Known(GameFamily::Swbf1), Owner::Known(GameFamily::Swbf2)));

        let mut stated = Stated::default();
        assert!(stated.is_empty() && stated.writes_nothing());
        stated.add(FamilyTag::read("swbf1"));
        stated.add(FamilyTag::read("SWBF1"));
        assert_eq!(stated.owners().verdict(), Verdict::One(Owner::Known(GameFamily::Swbf1)));
        assert!(stated.writes_nothing(), "the 2004 game is never written back");
        stated.add(swbf9.clone());
        stated.add(FamilyTag::read("swbf2"));
        assert_eq!(stated.tags().count(), 2);
        assert_eq!(stated.written(), Some(&swbf9));
        assert_eq!(Stated::from(FamilyTag::read("swbf2")).written(), Some(&FamilyTag::Known(GameFamily::Swbf2)));
    }
}
