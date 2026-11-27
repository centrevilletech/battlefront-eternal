//! Which game the battle a server names belongs to, read once per join from
//! the server's fresh reply and before any content is opened.
//!
//! The reply's released fields are read by the released reader (`info::Server`).
//! What the reply names beside its query is read by a pass of its own over the
//! same bytes (`bfe_online::declared::Statements`), and a listed server's from
//! its explicitly stated current family. Every reader applies the
//! rules of `bfe_family::owner`:
//!
//! - every `family` member of a `battle` summary or of a structured first
//!   rotation entry, and the namespace of their `map` (`swbf2/tat2`,
//!   `SWBF1/hot1`; an unqualified map names nothing);
//! - the namespace of `rotation[0]` when it is a slug;
//! - the namespace of the query's first `map`, decoded as the content code
//!   decodes it.
//!
//! A `family=` key inside the query is not read: a released server copies an
//! entry's side field into its query verbatim, so a playable 2004 battle can
//! carry one. Repeated identical statements are one; different ones are
//! refused, as is a game this build does not know. A 2004 battle (nothing
//! names another game) then runs exactly the released join: the query's bytes
//! are fingerprinted, sent and loaded as they are. A 2005 battle, current or
//! announced, is joined either way: the client plays it when its gate opens
//! the 2005 game and it has the battle's map, and sits it out otherwise
//! (`info::join_hello`).

use bfe_family::{GameFamily, DEFAULT_MODE};
pub use bfe_online::declared::{check_next, UNUSABLE_NEXT};
use bfe_online::declared::Statements;

/// One battle a client is about to join, resolved once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// The game: the 2004 game unless something named another.
    pub family: GameFamily,
    /// Whether the reply named the game (a released 2004 battle names none, or
    /// the 2004 game).
    pub named: bool,
    /// The map as the content code reads it: for a 2004 battle the query's
    /// first `map`, decoded (empty when the query names none).
    pub map: String,
    /// The side and era in the fresh query, before any launch defaults.
    pub side: String,
    pub era: String,
    /// The mode: a 2004 battle's from the fresh summary, a 2005 battle's from
    /// its query; conquest when neither names one.
    pub mode: String,
    /// The battle query's bytes, unchanged, for the fingerprint and the network.
    pub query: String,
    /// For a 2005 battle, the match the join asks for (mode, qualified map,
    /// match token and this client's content digest), set when its hello is
    /// made and checked against the server's signed answer; none for 2004.
    pub join: Option<super::match2005::Expected>,
    /// For a 2005 battle this join plays, the files its content digest was
    /// computed from, as they were read: the battle's scene, and with it the
    /// prediction, is loaded from them, so that what the client offered is
    /// what it plays. Taken once the scene is loaded.
    pub pinned: Option<std::sync::Arc<bfe_content::files::Pinned>>,
}

impl Selection {
    /// Whether this join sits a 2005 battle out: its hello offered no content,
    /// so the server gives it the map-free connection.
    pub fn sits_out(&self) -> bool {
        self.family == GameFamily::Swbf2 && self.join.as_ref().is_some_and(|join| join.content.is_none())
    }
}

/// Resolves the battle of a fresh reply, refusing a game this build does not
/// know, statements that disagree and a next battle it cannot load. Reads no
/// content.
pub fn select(query: &str, statements: &Statements) -> Result<Selection, String> {
    select_reply(query, statements, None)
}

pub(super) fn select_reply(query: &str, statements: &Statements, next: Option<&str>) -> Result<Selection, String> {
    let (family, named) = statements.family(query, next)?;
    Ok(Selection::read(query, family, named, statements.mode.as_deref()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field<'a> {
    Query(&'a str),
    Launch(&'a str),
    Default(&'static str),
    Side(&'static str),
}

impl<'a> Field<'a> {
    pub(crate) fn value(self) -> &'a str {
        match self {
            Self::Query(s) | Self::Launch(s) | Self::Default(s) | Self::Side(s) => s,
        }
    }
}

impl Selection {
    fn read(query: &str, family: GameFamily, named: bool, mode: Option<&str>) -> Self {
        let parsed = bfe_content::mission::Query::parse(query);
        let map = match family {
            GameFamily::Swbf2 => parsed.get("map").split_once('/').map_or("", |(_, map)| map),
            GameFamily::Swbf1 => parsed.get("map"),
        };
        let mode = match family {
            // A 2005 battle's query names its mode (`mode=`, absent for Conquest): the
            // server writes it with the battle, so it is as fresh as the query itself.
            GameFamily::Swbf2 => Some(parsed.get("mode")).filter(|m| !m.is_empty()),
            GameFamily::Swbf1 => mode.filter(|m| !m.is_empty()),
        }.unwrap_or(DEFAULT_MODE);
        Self {
            family,
            named,
            map: map.to_string(),
            side: parsed.get("side").to_string(),
            era: parsed.get("era").to_string(),
            mode: mode.to_string(),
            query: query.to_string(),
            join: None,
            pinned: None,
        }
    }

    /// A battle announced next: its query is all there is (`check_next`). A
    /// 2005 one is joined as a current one is, played or sat out.
    pub(crate) fn announced(query: &str) -> Result<Self, &'static str> {
        let family = check_next(query)?;
        Ok(Self::read(query, family, family == GameFamily::Swbf2, None))
    }

    /// The released content and fingerprint readers use their own default.
    pub(crate) fn content_map(&self) -> Field<'_> {
        if self.family == GameFamily::Swbf1 && self.map.is_empty() {
            Field::Default("nab2")
        } else {
            Field::Query(&self.map)
        }
    }

    /// The released HUD can also have a map and era in its launch descriptor.
    pub(crate) fn display<'a>(&'a self, map: &'a str, era: &'a str) -> (Field<'a>, Field<'a>) {
        if self.family == GameFamily::Swbf2 {
            let era = if self.era.is_empty() {
                bfe_content::mission::era_of_side(&self.side).map_or(Field::Query(""), Field::Side)
            } else {
                Field::Query(&self.era)
            };
            return (self.content_map(), era);
        }
        let field = |fresh: &'a str, launch: &'a str, default: &'static str| {
            if !fresh.is_empty() { Field::Query(fresh) }
            else if !launch.is_empty() { Field::Launch(launch) }
            else { Field::Default(default) }
        };
        (field(&self.map, map, "nab2"), field(&self.era, era, "cw"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(body: &str) -> Statements {
        Statements::read(body.as_bytes())
    }

    #[test]
    fn a_2005_battle_uses_its_decoded_map_and_its_own_era_and_mode() {
        let nothing = read("{}");
        for (query, map, era, mode) in [
            ("?map=swbf2%2Fcourt&map=stale&side=imp&mode=hunt", "court", "gcw", "hunt"),
            ("?map=swbf2/yard&side=rep&era=gcw&mode=duel", "yard", "gcw", "duel"),
            ("?map=swbf2/yard&side=cis", "yard", "cw", "conquest"),
        ] {
            let selected = select(query, &nothing).unwrap();
            let (shown_map, shown_era) = selected.display("", "");
            assert_eq!((selected.family, shown_map.value(), shown_era.value(), selected.mode.as_str()),
                (GameFamily::Swbf2, map, era, mode));
            assert_eq!(selected.query, query);
        }
        // The summary's mode does not override the one the 2005 query names.
        let selected = Selection::read("?map=swbf2/yard&side=rep&mode=hunt", GameFamily::Swbf2, true, Some("duel"));
        assert_eq!(selected.mode, "hunt");
    }

    fn family(query: &str, extra: &str) -> Result<GameFamily, String> {
        let body = if extra.is_empty() { format!(r#"{{"query":"{query}"}}"#) } else { format!(r#"{{"query":"{query}",{extra}}}"#) };
        select(query, &read(&body)).map(|chosen| chosen.family)
    }

    #[test]
    fn admitted_fields_keep_the_released_display_and_content_defaults() {
        let fresh = select("?map=co%75rt&era=gcw&side=all&map=old", &Statements::default()).unwrap();
        assert_eq!(fresh.content_map(), Field::Query("court"));
        assert_eq!(fresh.display("stale", "cw"), (Field::Query("court"), Field::Query("gcw")));
        assert_eq!(fresh.side, "all");
        assert_eq!(fresh.query, "?map=co%75rt&era=gcw&side=all&map=old");

        let missing = select("?side=all", &read(r#"{"battle":{"map":"hint","mode":"duel"}}"#)).unwrap();
        assert_eq!(missing.content_map(), Field::Default("nab2"));
        assert_eq!(missing.display("stale", "gcw"), (Field::Launch("stale"), Field::Launch("gcw")));
        assert_eq!(missing.display("", ""), (Field::Default("nab2"), Field::Default("cw")));
        assert_eq!(missing.mode, "duel");

        let later = Selection::read("?map=swbf2%2Fcourt&side=imp&mode=hunt", GameFamily::Swbf2, true, None);
        assert_eq!(later.content_map(), Field::Query("court"));
        assert_eq!(later.display("stale", "cw"), (Field::Query("court"), Field::Side("gcw")));
        assert_eq!(later.mode, "hunt");
        assert_eq!(Selection::announced(&later.query).map(|next| next.family), Ok(GameFamily::Swbf2));
    }

    #[test]
    fn a_2004_battle_is_the_query_as_written() {
        let first = read(r#"{"rotation":["swbf1/kam1:all:duel"]}"#);
        for (query, map) in [
            ("?map=kam1&side=all&family=swbf2&ai=enhanced&seed=1", "kam1"),
            ("?map=kam1&side=rep&ai=enhanced&seed=1&family=swbf2&mode=hunt", "kam1"),
            ("?side=all", ""),
            ("?map=swbf1/kam1", "swbf1/kam1"),
            ("?map=k%61m1&side=rep", "kam1"),
            ("?m%61p=kam1&map=swbf2/kam1", "kam1"),
        ] {
            let chosen = select(query, &first).unwrap();
            assert_eq!((chosen.family, chosen.map.as_str(), chosen.query.as_str()), (GameFamily::Swbf1, map, query));
            assert!(chosen.named);
        }
        // The released summary of a 2004 server, with JSON nulls and its mode.
        let released = read(r#"{"battle":{"map":"kam1","mapName":null,"side":"cw","era":null,"mode":"duel"}}"#);
        let chosen = select("?map=kam1&side=cw", &released).unwrap();
        assert_eq!((chosen.family, chosen.named, chosen.mode.as_str()), (GameFamily::Swbf1, false, "duel"));
        // What cannot be read names nothing.
        for odd in [r#""battle":null"#, r#""battle":7"#, r#""battle":"text""#, r#""battle":[1]"#, r#""battle":{"map":7,"mode":null,"family":7}"#, r#""battle":{"family":{"x":1}}"#] {
            assert_eq!(family("?map=kam1", odd), Ok(GameFamily::Swbf1), "{odd}");
        }
    }

    // A 2005 battle resolves to the 2005 game whatever the gate: the join
    // then plays or sits it out. Unknown games and disagreements are refused
    // before content.
    #[test]
    fn the_2005_game_resolves_and_disagreements_are_refused_before_content() {
        let swbf2 = Ok(GameFamily::Swbf2);
        assert_eq!(family("?map=kam1", r#""battle":{"family":"swbf2"}"#), swbf2);
        assert_eq!(family("?map=swbf2/kam1", ""), swbf2);
        assert_eq!(family("?map=%73wbf2%2Fkam1", ""), swbf2);
        assert_eq!(family("?map=kam1", r#""rotation":["swbf2/kam1:gcw:hunt"]"#), swbf2);
        assert_eq!(family("?map=kam1", r#""rotation":["SWBF2/kam1:gcw:hunt"]"#), swbf2);
        assert_eq!(family("?map=kam1", r#""rotation":["swbf2/kam1::eli"]"#), swbf2);
        assert_eq!(family("?map=kam1", r#""rotation":[{"family":"swbf2","map":"kam1"}]"#), swbf2);
        assert_eq!(family("?map=kam1", r#""rotation":[{"map":"SWBF2/kam1"}]"#), swbf2);
        assert!(family("?map=kam1", r#""battle":{"family":"swbf9"}"#).unwrap_err().contains("does not know"));
        for two in [
            r#""battle":{"family":"swbf1"},"rotation":["swbf2/kam1:gcw:hunt"]"#,
            r#""rotation":[{"family":"swbf1","map":"swbf2/kam1","mode":"eli"}]"#,
            r#""rotation":[{"map":"swbf2/kam1","family":"swbf1"}]"#,
            r#""battle":{"map":"kam1","family":"swbf2","family":"swbf1"}"#,
            r#""battle":{"map":"kam1","family":"swbf1","family":"swbf2"}"#,
            r#""rotation":[{"map":"kam1","family":"swbf2","family":"swbf1"}]"#,
            r#""battle":{"family":"swbf2"},"battle":{"family":"swbf1"}"#,
        ] {
            assert!(family("?map=kam1", two).unwrap_err().contains("two games"), "{two}");
        }
        assert!(family("?map=swbf2/kam1", r#""battle":{"family":"swbf1"}"#).unwrap_err().contains("two games"));
        // Repeated identical statements are one.
        assert_eq!(family("?map=kam1", r#""battle":{"family":"swbf1","family":"SWBF1"},"rotation":["swbf1/kam1:all:conquest"],"rotation":["swbf1/kam1:all:conquest"]"#), Ok(GameFamily::Swbf1));

        assert_eq!(check_next("?map=swbf2/kam1&side=all&match=00"), Ok(GameFamily::Swbf2));
        assert_eq!(check_next("?map=swbf2%2Fkam1&match=00"), Ok(GameFamily::Swbf2));
        assert_eq!(check_next("?map=kam1&side=all&family=swbf2&match=00"), Ok(GameFamily::Swbf1));
        for unusable in ["?map=swbf9/tat2&match=00", "?map=swbf1/tat2", "?map=mods/tat2"] {
            assert_eq!(check_next(unusable), Err(UNUSABLE_NEXT), "{unusable}");
        }
    }
}
