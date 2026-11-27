// What a listing row or a `/info` reply names as the game of its battle, read
// beside the released readers of those objects.
//
// Released builds never wrote a `family` member, and their readers ignore one
// whatever its value. Reading it must never make a listing or a reply
// unreadable, nor change what a released reader makes of anything else:
//
// - A listing row (`ServerEntry`) keeps its released reader for its battle
//   summary and rotation entries. Their `family` members are taken aside
//   before that reader sees the object ([`battle`], [`rotation`]), each read
//   from its raw JSON text, which serde_json skips with no limit on depth or
//   numbers; every one is kept ([`Stated`]).
// - A `/info` reply keeps its released join reader. What it names is read by a
//   pass of its own over the same bytes ([`Statements::read`]) that never
//   fails: a member it cannot read names nothing.
//
// A member that is not text (JSON `null` included) names nothing. The rules
// for what a map id, a slug or a query names are `bfe_family::owner`'s.

mod fields;
pub(crate) use fields::{master_shape, read_row};

use std::borrow::Cow;
use std::fmt;

use bfe_family::owner::{self, Namespace, Owner, Stated, Verdict};
use bfe_family::{FamilyTag, GameFamily};
use serde::de::value::{BorrowedStrDeserializer, StringDeserializer};
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;
use serde_json::value::RawValue;

use crate::browser::{BattleSummary, RotationItem};

/// What one `family` member's raw JSON value names: its text, read as a
/// family, or nothing when the value is not text.
pub fn member_tag(raw: &RawValue) -> Option<FamilyTag> {
    let text = raw.get();
    if !text.starts_with('"') {
        return None;
    }
    serde_json::from_str::<String>(text).ok().map(|text| FamilyTag::read(&text))
}

/// The text of a raw JSON value, when it is text.
fn member_text(raw: &RawValue) -> Option<String> {
    let text = raw.get();
    text.starts_with('"').then(|| serde_json::from_str::<String>(text).ok()).flatten()
}

// A map access that takes every `family` member aside into `family` and hands
// every other member to the released reader, unchanged.
struct Aside<'a, A> {
    map: A,
    family: &'a mut Stated,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Aside<'_, A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(&mut self, seed: K) -> Result<Option<K::Value>, A::Error> {
        loop {
            let Some(key) = self.map.next_key::<Key<'de>>()? else {
                return Ok(None);
            };
            if key.0 == "family" {
                let raw = self.map.next_value::<Box<RawValue>>()?;
                if let Some(tag) = member_tag(&raw) {
                    self.family.add(tag);
                }
                continue;
            }
            return match key.0 {
                Cow::Borrowed(text) => seed.deserialize(BorrowedStrDeserializer::new(text)),
                Cow::Owned(text) => seed.deserialize(StringDeserializer::new(text)),
            }
            .map(Some);
        }
    }

    fn next_value_seed<V: DeserializeSeed<'de>>(&mut self, seed: V) -> Result<V::Value, A::Error> {
        self.map.next_value_seed(seed)
    }

    fn size_hint(&self) -> Option<usize> {
        self.map.size_hint()
    }
}

// A map key as written (borrowed when the input allows).
struct Key<'de>(Cow<'de, str>);

impl<'de> Deserialize<'de> for Key<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Text;
        impl<'de> Visitor<'de> for Text {
            type Value = Key<'de>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a key")
            }
            fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<Key<'de>, E> {
                Ok(Key(v.into()))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Key<'de>, E> {
                Ok(Key(v.to_string().into()))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Key<'de>, E> {
                Ok(Key(v.into()))
            }
        }
        deserializer.deserialize_str(Text)
    }
}

/// A listing row's `battle`: the released summary reader, with the summary's
/// `family` members taken aside into [`BattleSummary::family`].
pub fn battle<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<BattleSummary>, D::Error> {
    struct Summary;
    impl<'de> Visitor<'de> for Summary {
        type Value = Option<BattleSummary>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("option")
        }
        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
            deserializer.deserialize_struct("BattleSummary", &[], Fields).map(Some)
        }
    }
    struct Fields;
    impl<'de> Visitor<'de> for Fields {
        type Value = BattleSummary;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("struct BattleSummary")
        }
        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<BattleSummary, A::Error> {
            let mut family = Stated::default();
            let mut summary = BattleSummary::deserialize(de::value::MapAccessDeserializer::new(Aside { map, family: &mut family }))?;
            summary.family = family;
            Ok(summary)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<BattleSummary, A::Error> {
            BattleSummary::deserialize(de::value::SeqAccessDeserializer::new(seq))
        }
    }
    deserializer.deserialize_option(Summary)
}

/// A listing row's `rotation`: the released entry reader for each entry, with
/// a structured entry's `family` members taken aside into its `family`.
pub fn rotation<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<RotationItem>, D::Error> {
    struct Entries;
    impl<'de> Visitor<'de> for Entries {
        type Value = Vec<RotationItem>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a sequence")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<RotationItem>, A::Error> {
            let mut entries = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
            while let Some(entry) = seq.next_element_seed(Entry)? {
                entries.push(entry);
            }
            Ok(entries)
        }
    }
    struct Entry;
    impl<'de> DeserializeSeed<'de> for Entry {
        type Value = RotationItem;
        fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<RotationItem, D::Error> {
            deserializer.deserialize_any(EntryValue)
        }
    }
    // Each kind of value goes on to the released (untagged) entry reader as
    // the value it was, so its answers and errors are the released ones.
    struct EntryValue;
    impl<'de> Visitor<'de> for EntryValue {
        type Value = RotationItem;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a rotation entry")
        }
        fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<RotationItem, A::Error> {
            let mut stated = Stated::default();
            let mut entry = RotationItem::deserialize(de::value::MapAccessDeserializer::new(Aside { map, family: &mut stated }))?;
            if let RotationItem::Entry { family, .. } = &mut entry {
                *family = stated;
            }
            Ok(entry)
        }
        fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<RotationItem, A::Error> {
            RotationItem::deserialize(de::value::SeqAccessDeserializer::new(seq))
        }
        fn visit_borrowed_str<E: de::Error>(self, v: &'de str) -> Result<RotationItem, E> {
            RotationItem::deserialize(BorrowedStrDeserializer::<E>::new(v))
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<RotationItem, E> {
            RotationItem::deserialize(de::value::StrDeserializer::<E>::new(v))
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<RotationItem, E> {
            RotationItem::deserialize(StringDeserializer::<E>::new(v))
        }
        fn visit_bool<E: de::Error>(self, v: bool) -> Result<RotationItem, E> {
            RotationItem::deserialize(de::value::BoolDeserializer::<E>::new(v))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<RotationItem, E> {
            RotationItem::deserialize(de::value::I64Deserializer::<E>::new(v))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<RotationItem, E> {
            RotationItem::deserialize(de::value::U64Deserializer::<E>::new(v))
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<RotationItem, E> {
            RotationItem::deserialize(de::value::F64Deserializer::<E>::new(v))
        }
        fn visit_unit<E: de::Error>(self) -> Result<RotationItem, E> {
            RotationItem::deserialize(de::value::UnitDeserializer::<E>::new())
        }
        fn visit_none<E: de::Error>(self) -> Result<RotationItem, E> {
            RotationItem::deserialize(de::value::UnitDeserializer::<E>::new())
        }
    }
    deserializer.deserialize_seq(Entries)
}

/// What a `/info` reply names beside its battle query: every `battle`
/// member's `family` members and map, and entry 0 of every `rotation`
/// member. Also the summary's mode, its first `mode` text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Statements {
    /// The games named.
    pub stated: Stated,
    /// The fresh summary's mode, when it names one.
    pub mode: Option<String>,
}

impl Statements {
    /// Reads a reply's bytes, which a released reader has accepted. Never
    /// fails: a member this pass cannot read names nothing. Text that is not
    /// UTF-8 is read with its bad bytes replaced, as only text can hold them.
    pub fn read(body: &[u8]) -> Self {
        let text = String::from_utf8_lossy(body);
        let mut statements = Self::default();
        let mut reply = serde_json::Deserializer::from_str(&text);
        let _ = reply.deserialize_map(Reply(&mut statements));
        statements
    }

    /// Admit the pending battle, if present, and return the current game and
    /// whether it was named, refusing a game this build, as compiled, cannot
    /// play: the listing's verdict. A client's join reads [`Self::family`] and
    /// plays or sits out the battle as its gate decides.
    pub fn game(&self, query: &str, next: Option<&str>) -> Result<(GameFamily, bool), String> {
        let (family, named) = self.family(query, next)?;
        family.require_available().map_err(|e| e.to_string())?;
        Ok((family, named))
    }

    /// [`Self::game`] without the availability gate: the game the reply names
    /// for its battle, refusing a game this build does not know and two games.
    pub fn family(&self, query: &str, next: Option<&str>) -> Result<(GameFamily, bool), String> {
        if let Some(next) = next {
            check_next(next)?;
        }
        let mut owners = self.stated.owners();
        owners.add_namespace(owner::query_map_namespace(query));
        let verdict = owners.verdict();
        let family = match verdict {
            Verdict::Unstated => GameFamily::Swbf1,
            Verdict::One(Owner::Known(family)) => family,
            Verdict::One(Owner::Unknown(text)) | Verdict::Two(Owner::Unknown(text), _) | Verdict::Two(_, Owner::Unknown(text)) => {
                return Err(unknown(text))
            }
            Verdict::Two(first, other) => {
                return Err(format!("The server names two games for its battle ({first} and {other}). Refresh and try again."))
            }
        };
        Ok((family, verdict != Verdict::Unstated))
    }

    fn summary(&mut self, raw: &RawValue) {
        let mut summary = serde_json::Deserializer::from_str(raw.get());
        let _ = summary.deserialize_any(Object { statements: self, mode: true });
    }

    fn rotation(&mut self, raw: &RawValue) {
        let mut rotation = serde_json::Deserializer::from_str(raw.get());
        let _ = rotation.deserialize_seq(First(self));
    }

    fn entry(&mut self, raw: &RawValue) {
        if let Some(slug) = member_text(raw) {
            self.stated.add_slug(&slug);
            return;
        }
        let mut entry = serde_json::Deserializer::from_str(raw.get());
        let _ = entry.deserialize_map(Object { statements: self, mode: false });
    }
}

// Read only the ASCII field names used by the supplemental pass. A key may
// contain a lone surrogate escape in an object the released reader ignored.
#[derive(Clone, Copy)]
enum Field { Battle, Rotation, Family, Map, Mode, Other }

impl<'de> Deserialize<'de> for Field {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Name;
        impl Visitor<'_> for Name {
            type Value = Field;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object key")
            }
            fn visit_bytes<E: de::Error>(self, bytes: &[u8]) -> Result<Field, E> {
                Ok(match bytes {
                    b"battle" => Field::Battle,
                    b"rotation" => Field::Rotation,
                    b"family" => Field::Family,
                    b"map" => Field::Map,
                    b"mode" => Field::Mode,
                    _ => Field::Other,
                })
            }
        }
        deserializer.deserialize_bytes(Name)
    }
}

// The reply object: its `battle` and `rotation` members, each read alone from
// its raw text.
struct Reply<'a>(&'a mut Statements);

impl<'de> Visitor<'de> for Reply<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a reply")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(field) = map.next_key::<Field>()? {
            match field {
                Field::Battle => self.0.summary(&map.next_value::<Box<RawValue>>()?),
                Field::Rotation => self.0.rotation(&map.next_value::<Box<RawValue>>()?),
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

// A summary or a structured rotation entry: its `family` members, the
// namespace of its `map` members, and (for a summary) its first `mode` text.
struct Object<'a> {
    statements: &'a mut Statements,
    mode: bool,
}

impl<'de> Visitor<'de> for Object<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("an object")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        // The released summary also accepts its fields in declaration order.
        if let Some(raw) = seq.next_element::<Box<RawValue>>()? {
            if let Some(map) = member_text(&raw) {
                self.statements.stated.add_id(&map);
            }
        }
        for _ in 0..3 {
            if seq.next_element::<IgnoredAny>()?.is_none() {
                return Ok(());
            }
        }
        if let Some(raw) = seq.next_element::<Box<RawValue>>()? {
            if self.statements.mode.is_none() {
                self.statements.mode = member_text(&raw);
            }
        }
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(field) = map.next_key::<Field>()? {
            match field {
                Field::Family => {
                    if let Some(tag) = member_tag(&map.next_value::<Box<RawValue>>()?) {
                        self.statements.stated.add(tag);
                    }
                }
                Field::Map => {
                    if let Some(map_id) = member_text(&map.next_value::<Box<RawValue>>()?) {
                        self.statements.stated.add_id(&map_id);
                    }
                }
                Field::Mode if self.mode => {
                    let mode = member_text(&map.next_value::<Box<RawValue>>()?);
                    if self.statements.mode.is_none() {
                        self.statements.mode = mode;
                    }
                }
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}

// A rotation: its first entry.
struct First<'a>(&'a mut Statements);

impl<'de> Visitor<'de> for First<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a rotation")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        if let Some(first) = seq.next_element::<Box<RawValue>>()? {
            self.0.entry(&first);
        }
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(())
    }
}

/// The announced successor in a fresh information reply.
#[derive(Clone, Debug, serde::Deserialize)]
pub struct NextBattle {
    pub query: String,
}

/// The game of a battle announced next (`/info` `next`, a signalling answer's
/// `next`, an in-session `match`): its query is all there is. Its first `map`
/// (decoded) names it: a map without a namespace is the 2004 game, a `swbf2/`
/// map the 2005 game, which the join of that battle plays or sits out as the
/// client's gate decides, exactly as a current battle's; any other namespaced
/// map cannot be loaded.
pub fn check_next(query: &str) -> Result<GameFamily, &'static str> {
    match owner::query_map_namespace(query) {
        Namespace::Bare => Ok(GameFamily::Swbf1),
        Namespace::Family(GameFamily::Swbf2) => Ok(GameFamily::Swbf2),
        Namespace::Family(GameFamily::Swbf1) | Namespace::Other => Err(UNUSABLE_NEXT),
    }
}

/// Why an announced next battle whose map has a namespace other than the
/// 2005 game's is refused.
pub const UNUSABLE_NEXT: &str = "The server's next battle names a map this build cannot load.";

fn unknown(text: &str) -> String {
    format!("This server runs a game this build does not know ({text:?}).")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bfe_family::owner::{Owner, Verdict};
    use bfe_family::GameFamily;

    fn verdict(stated: &Stated) -> Verdict<'_> {
        stated.owners().verdict()
    }

    #[test]
    fn unreadable_extension_keys_do_not_hide_later_declarations() {
        for body in [
            r#"{"battle":{"family":"swbf1","\ud800":1,"family":"swbf2"}}"#,
            r#"{"rotation":[{"\ud800":null,"family":"swbf1","map":"swbf2/moon"}]}"#,
        ] {
            assert!(matches!(verdict(&Statements::read(body.as_bytes()).stated), Verdict::Two(..)));
        }
    }

    #[test]
    fn a_reply_names_every_game_its_summary_and_first_entry_name() {
        let read = |extra: &str| Statements::read(format!(r#"{{"query":"?map=tat2&side=all",{extra}}}"#).as_bytes());
        assert_eq!(verdict(&read(r#""battle":{"map":"tat2"},"rotation":["swbf1/tat2:all:conquest"]"#).stated), Verdict::One(Owner::Known(GameFamily::Swbf1)));
        assert_eq!(verdict(&read(r#""battle":{"map":"tat2"},"rotation":["tat2:all:conquest"]"#).stated), Verdict::Unstated);
        // An object's family members and the namespace of its map are all read.
        for object in [r#"{"family":"swbf1","map":"swbf2/tat2","mode":"eli"}"#, r#"{"map":"swbf2/tat2","family":"swbf1"}"#, r#"{"map":"tat2","family":"swbf2","family":"swbf1"}"#] {
            for extra in [format!(r#""battle":{object}"#), format!(r#""rotation":[{object}]"#)] {
                assert!(matches!(verdict(&read(&extra).stated), Verdict::Two(..)), "{extra}");
            }
        }
        // Repeated members, repeated identical statements.
        let twice = read(r#""battle":{"map":"tat2","family":"swbf2"},"battle":{"family":"swbf2"},"rotation":["swbf2/tat2:gcw:eli"],"rotation":[{"family":"SWBF2"}]"#);
        assert_eq!(verdict(&twice.stated), Verdict::One(Owner::Known(GameFamily::Swbf2)));
        assert!(matches!(verdict(&read(r#""battle":{"family":"swbf2"},"battle":{"family":"swbf1"}"#).stated), Verdict::Two(..)));
        // What cannot be read names nothing; text that is no family is kept.
        for odd in [r#""battle":{"family":7}"#, r#""battle":{"family":null}"#, r#""battle":{"family":{"x":[1]}}"#, r#""battle":1e999"#, r#""rotation":{"x":1}"#, r#""rotation":[7,"swbf2/tat2:gcw:eli"]"#, r#""battle":{"map":7}"#] {
            assert_eq!(verdict(&read(odd).stated), Verdict::Unstated, "{odd}");
        }
        assert_eq!(verdict(&read(r#""battle":{"family":"swbf9"}"#).stated), Verdict::One(Owner::Unknown("swbf9")));
        assert_eq!(read(r#""battle":{"mode":"duel"},"battle":{"mode":"hunt"}"#).mode.as_deref(), Some("duel"));
        assert_eq!(read(r#""rotation":[{"mode":"duel"}]"#).mode, None);
    }

    #[test]
    fn a_listing_row_takes_its_family_members_aside() {
        let row: crate::browser::ServerEntry = serde_json::from_str(
            r#"{"id":"a","battle":{"map":"tat2","family":"swbf2","family":"swbf1","extra":1e999,"deep":[[[[[[1]]]]]]},
                "rotation":[{"family":"swbf9","map":"tat2","family":7},"swbf1/tat2:all:conquest"]}"#,
        )
        .unwrap();
        let battle = row.battle.as_ref().unwrap();
        assert_eq!(battle.map, "tat2");
        assert_eq!(battle.family.tags().map(ToString::to_string).collect::<Vec<_>>(), ["swbf2", "swbf1"]);
        match &row.rotation[..] {
            [RotationItem::Entry { family, map, .. }, RotationItem::Slug(slug)] => {
                assert_eq!((family.tags().map(ToString::to_string).collect::<Vec<_>>(), map.as_str()), (vec!["swbf9".to_string()], "tat2"));
                assert_eq!(slug, "swbf1/tat2:all:conquest");
            }
            other => panic!("{other:?}"),
        }
        // What the released reader refuses stays refused: a duplicate released
        // field, an entry that is neither text nor an object.
        for refused in [r#"{"battle":{"map":"a","map":"b"}}"#, r#"{"rotation":[["hot1","cw","conquest"]]}"#, r#"{"rotation":[7]}"#, r#"{"rotation":null}"#] {
            assert!(serde_json::from_str::<crate::browser::ServerEntry>(refused).is_err(), "{refused}");
        }
        // And what it reads, it reads alike.
        let released: crate::browser::ServerEntry =
            serde_json::from_str(r#"{"battle":{"map":"kam1","extra":[1e999]},"rotation":[{"map":"kam1","era":"cw","mode":"conquest"}]}"#).unwrap();
        assert_eq!(released.battle.map(|b| b.map).as_deref(), Some("kam1"));
        assert_eq!(released.rotation[0].map_id(), Some("kam1"));
        assert!(serde_json::from_str::<crate::browser::ServerEntry>(r#"{"battle":null}"#).unwrap().battle.is_none());
    }
}
