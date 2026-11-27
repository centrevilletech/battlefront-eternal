//! How a new entry names a Battlefront II (2005) battle.
//!
//! The 2004 game's playlist and rotation text keeps the readers each boundary
//! always had (the shell's playlist parser, the server's rotation builder and
//! menu check, the desktop host's check). Those readers are case and space
//! sensitive in different ways, and their behaviour on 2004 text does not
//! change. A 2005 battle is named by this one grammar instead, and only where a
//! released build could not have read the text as a 2004 battle:
//!
//! - **Complete form** (server rotation, host playlist, server menu):
//!   `swbf2/<map>:<era>:<mode>`, all three fields present; the era may be empty
//!   (`swbf2/tat2::eli`).
//! - **Short form** (Instant Action text, after the shell's trim and lower
//!   case): `swbf2/<map>`, `swbf2/<map>:<era>` or `swbf2/<map>:<era>:<mode>`. A
//!   missing era stays unset and a missing or empty mode is conquest.
//!
//! Both forms share the field rules: the map and the mode are 1 to
//! [`MAX_FIELD`] characters of `a-z`, `0-9` and `_`; the era is `cw`, `gcw` or
//! empty. The prefix is the literal lower case `swbf2/`.
//!
//! In a battle query and in browser listings the 2005 map id is the qualified
//! id `swbf2/<map>` ([`Entry::qualified_map`]). A released client refuses such
//! a map before it reads any content, and a released server never publishes
//! one for a battle it can run, so the qualified id never collides with a 2004
//! map.

use crate::{GameFamily, DEFAULT_MODE};

/// The literal prefix of a 2005 entry and of a 2005 map id.
pub const PREFIX: &str = "swbf2/";

/// Longest map or mode field of a 2005 entry, in bytes.
pub const MAX_FIELD: usize = 32;

/// A map or mode field: 1 to [`MAX_FIELD`] of `a-z`, `0-9` and `_`.
pub fn valid_field(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_FIELD
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// An era field: `cw`, `gcw` or empty.
pub fn valid_era(text: &str) -> bool {
    matches!(text, "" | "cw" | "gcw")
}

/// Whether a server rotation or host playlist entry is a 2005 entry: the
/// literal prefix [`PREFIX`]. Released servers and hosts read every other
/// entry, whatever its spelling, as a 2004 entry, and still do.
pub fn is_entry(text: &str) -> bool {
    text.starts_with(PREFIX)
}

/// The bare map of a qualified 2005 map id (`"swbf2/tat2"` gives `"tat2"`).
/// Any other id, including a 2004 id, gives `None`.
pub fn bare_map(id: &str) -> Option<&str> {
    id.strip_prefix(PREFIX)
}

/// The query a 2005 battle's content is read with: the battle query with its
/// first `map` ([`crate::owner::query_map`]) naming `bare`, the decoded map
/// the battle's selection resolved, because the 2005 content folder holds 2005
/// maps under their bare ids; and with the era its side fights in, when the
/// query names none, so the loader, the digest and the script host all read
/// the mission of that era. The network keeps the query as written. `None`
/// when the query names no map.
pub fn content_query(query: &str, bare: &str) -> Option<String> {
    use crate::owner::{query_map, query_value};
    let value = query_map(query)?;
    let start = value.as_ptr() as usize - query.as_ptr() as usize;
    let mut content = format!("{}{}{}", &query[..start], bare, &query[start + value.len()..]);
    if query_value(query, "era").is_none() {
        let era = match query_value(query, "side") {
            Some("rep" | "cis") => "cw",
            Some("all" | "imp") => "gcw",
            _ => return Some(content),
        };
        content.push_str("&era=");
        content.push_str(era);
    }
    Some(content)
}

/// Whether a map id read from a battle query or a listing names the 2005 game:
/// its namespace (the text before its first `/`) is `swbf2` in any case. A released
/// client refuses every map id with a `/` in it, so this reading never takes a
/// battle a released client could join.
pub fn names_swbf2(id: &str) -> bool {
    id.split_once('/')
        .is_some_and(|(namespace, _)| namespace.eq_ignore_ascii_case(GameFamily::Swbf2.as_str()))
}

/// One validated 2005 battle: map, era and mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The bare map id (`"tat2"`).
    pub map: String,
    /// `Some("cw")`, `Some("gcw")`, or `None` when the entry left it empty.
    pub era: Option<String>,
    /// The mode (`"eli"`, `"ass"`, `"conquest"`).
    pub mode: String,
}

impl Entry {
    /// Builds an entry from separate fields under the shared field rules. An
    /// empty era is unset; an empty or missing mode is conquest.
    pub fn from_fields(map: &str, era: Option<&str>, mode: Option<&str>) -> Option<Self> {
        let era = era.unwrap_or("");
        let mode = mode.filter(|m| !m.is_empty()).unwrap_or(DEFAULT_MODE);
        (valid_field(map) && valid_era(era) && valid_field(mode)).then(|| Self {
            map: map.to_string(),
            era: (!era.is_empty()).then(|| era.to_string()),
            mode: mode.to_string(),
        })
    }

    /// Reads the complete form `swbf2/<map>:<era>:<mode>`.
    pub fn parse_complete(text: &str) -> Option<Self> {
        let mut fields = text.strip_prefix(PREFIX)?.split(':');
        let (map, era, mode) = (fields.next()?, fields.next()?, fields.next()?);
        if fields.next().is_some() || mode.is_empty() {
            return None;
        }
        Self::from_fields(map, Some(era), Some(mode))
    }

    /// Reads the short form `swbf2/<map>[:<era>[:<mode>]]`.
    pub fn parse_short(text: &str) -> Option<Self> {
        let mut fields = text.strip_prefix(PREFIX)?.split(':');
        let (map, era, mode) = (fields.next()?, fields.next(), fields.next());
        if fields.next().is_some() {
            return None;
        }
        Self::from_fields(map, era, mode)
    }

    /// The complete form, which [`Entry::parse_complete`] and
    /// [`Entry::parse_short`] both read back unchanged: `swbf2/tat2:gcw:eli`, or
    /// `swbf2/tat2::eli` with no era.
    pub fn format(&self) -> String {
        format!("{PREFIX}{}:{}:{}", self.map, self.era.as_deref().unwrap_or(""), self.mode)
    }

    /// The qualified map id used in battle queries and listings: `swbf2/tat2`.
    pub fn qualified_map(&self) -> String {
        format!("{PREFIX}{}", self.map)
    }

    /// The side a battle of this entry starts the player on: the Alliance for
    /// `gcw`, the Republic otherwise (the rule the 2004 entries use).
    pub fn side(&self) -> &'static str {
        if self.era.as_deref() == Some("gcw") {
            "all"
        } else {
            "rep"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_forms_share_the_field_rules() {
        let complete = Entry::parse_complete("swbf2/tat2:gcw:eli").unwrap();
        assert_eq!((complete.map.as_str(), complete.era.as_deref(), complete.mode.as_str()), ("tat2", Some("gcw"), "eli"));
        assert_eq!(Entry::parse_short("swbf2/tat2:gcw:eli"), Some(complete.clone()));
        assert_eq!(complete.format(), "swbf2/tat2:gcw:eli");
        assert_eq!(complete.side(), "all");
        assert_eq!(complete.qualified_map(), "swbf2/tat2");

        let no_era = Entry::parse_complete("swbf2/spa6::ass").unwrap();
        assert_eq!(no_era.era, None);
        assert_eq!(no_era.format(), "swbf2/spa6::ass");
        assert_eq!(Entry::parse_complete(&no_era.format()), Some(no_era.clone()));
        assert_eq!(Entry::parse_short(&no_era.format()), Some(no_era));

        // The short form fills what the complete form must say.
        let short = Entry::parse_short("swbf2/kam9").unwrap();
        assert_eq!((short.era, short.mode.as_str()), (None, "conquest"));
        assert_eq!(Entry::parse_short("swbf2/kam9:cw").unwrap().era.as_deref(), Some("cw"));
        assert!(Entry::parse_complete("swbf2/kam9").is_none());
        assert!(Entry::parse_complete("swbf2/kam9:cw").is_none());
        assert!(Entry::parse_complete("swbf2/kam9:cw:").is_none());

        let longest = "a".repeat(MAX_FIELD);
        assert!(Entry::parse_complete(&format!("swbf2/{longest}:cw:{longest}")).is_some());
        for bad in [
            format!("swbf2/{longest}a:cw:eli"),
            format!("swbf2/tat2:cw:{longest}a"),
            "swbf2/tat2:rep:eli".to_string(),
            "swbf2/tat2:CW:eli".to_string(),
            "swbf2/Tat2:cw:eli".to_string(),
            "swbf2/tat-2:cw:eli".to_string(),
            "swbf2/tat2:cw:eli:more".to_string(),
            "swbf2/:cw:eli".to_string(),
            "SWBF2/tat2:cw:eli".to_string(),
            " swbf2/tat2:cw:eli".to_string(),
            "swbf1/tat2:cw:eli".to_string(),
            "tat2:cw:eli".to_string(),
        ] {
            assert!(Entry::parse_complete(&bad).is_none(), "{bad}");
            assert!(Entry::parse_short(&bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn a_qualified_map_id_names_the_2005_game() {
        assert_eq!(bare_map("swbf2/tat2"), Some("tat2"));
        assert_eq!(bare_map("tat2"), None);
        assert!(names_swbf2("swbf2/tat2") && names_swbf2("SWBF2/tat2"));
        assert!(!names_swbf2("tat2") && !names_swbf2("swbf1/tat2") && !names_swbf2("mods/tat2"));
    }
}
