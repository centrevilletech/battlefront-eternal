//! A family named by a structured field (`"family": "swbf2"` in a listing).
//!
//! Released builds never wrote such a field, so its presence is a declaration
//! of its own: absent means nothing was said, a family this build knows is
//! that family, and anything else is an explicit family this build does not
//! know. The last is never taken for the 2004 game. A listing row reads the
//! field once, when it arrives; checking it afterwards allocates nothing.

use std::fmt;

use serde::{de, Deserialize, Deserializer, Serialize, Serializer};

use crate::GameFamily;

/// The value of a structured `family` field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FamilyTag {
    /// A family this build knows (read ignoring case and surrounding space).
    Known(GameFamily),
    /// An explicit value this build does not know, kept as written. A value
    /// that is not a string keeps a short description of its JSON type.
    Unknown(Box<str>),
}

impl FamilyTag {
    /// Reads a field's text.
    pub fn read(text: &str) -> Self {
        match GameFamily::parse(text) {
            Some(family) => Self::Known(family),
            None => Self::Unknown(text.into()),
        }
    }

    /// The family, when this build knows it.
    pub fn family(&self) -> Option<GameFamily> {
        match self {
            Self::Known(family) => Some(*family),
            Self::Unknown(_) => None,
        }
    }

    /// Whether this build can play the named family.
    pub fn is_available(&self) -> bool {
        self.family().is_some_and(GameFamily::is_available)
    }
}

impl fmt::Display for FamilyTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Known(family) => f.write_str(family.as_str()),
            Self::Unknown(text) => f.write_str(text),
        }
    }
}

impl Serialize for FamilyTag {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Known(family) => serializer.serialize_str(family.as_str()),
            Self::Unknown(text) => serializer.serialize_str(text),
        }
    }
}

// Any JSON value reads: a field this build cannot interpret must never make the
// whole listing (or the rest of its row) unreadable.
impl<'de> Deserialize<'de> for FamilyTag {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Tag;
        impl<'de> de::Visitor<'de> for Tag {
            type Value = FamilyTag;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("any value")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<FamilyTag, E> {
                Ok(FamilyTag::read(v))
            }
            fn visit_bool<E: de::Error>(self, _: bool) -> Result<FamilyTag, E> {
                Ok(FamilyTag::Unknown("<boolean>".into()))
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<FamilyTag, E> {
                Ok(FamilyTag::Unknown("<number>".into()))
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<FamilyTag, E> {
                Ok(FamilyTag::Unknown("<number>".into()))
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<FamilyTag, E> {
                Ok(FamilyTag::Unknown("<number>".into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<FamilyTag, E> {
                Ok(FamilyTag::Unknown("<null>".into()))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<FamilyTag, A::Error> {
                while seq.next_element::<de::IgnoredAny>()?.is_some() {}
                Ok(FamilyTag::Unknown("<array>".into()))
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<FamilyTag, A::Error> {
                while map.next_entry::<de::IgnoredAny, de::IgnoredAny>()?.is_some() {}
                Ok(FamilyTag::Unknown("<object>".into()))
            }
        }
        deserializer.deserialize_any(Tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, Serialize, PartialEq)]
    struct Row {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        family: Option<FamilyTag>,
        #[serde(default)]
        map: String,
    }

    #[test]
    fn every_value_reads_and_an_unknown_family_is_its_own_state() {
        let read = |text: &str| serde_json::from_str::<Row>(text).unwrap().family;
        assert_eq!(read(r#"{"map":"x"}"#), None);
        assert_eq!(read(r#"{"family":null}"#), None);
        assert_eq!(read(r#"{"family":"swbf1"}"#), Some(FamilyTag::Known(GameFamily::Swbf1)));
        assert_eq!(read(r#"{"family":" SWBF2 "}"#), Some(FamilyTag::Known(GameFamily::Swbf2)));
        assert_eq!(read(r#"{"family":"swbf9"}"#), Some(FamilyTag::Unknown("swbf9".into())));
        assert_eq!(read(r#"{"family":""}"#), Some(FamilyTag::Unknown("".into())));
        for other in ["7", "true", "[1,{}]", r#"{"name":"x"}"#] {
            let tag = read(&format!(r#"{{"family":{other},"map":"kept"}}"#)).unwrap();
            assert_eq!(tag.family(), None, "{other}");
        }
        assert!(!FamilyTag::read("swbf2").is_available());
        assert!(FamilyTag::read("swbf1").is_available());
        assert!(!FamilyTag::read("mods").is_available());
        // Absent stays absent when written back; a written value keeps its text.
        assert_eq!(serde_json::to_string(&Row { family: None, map: "x".into() }).unwrap(), r#"{"map":"x"}"#);
        let row: Row = serde_json::from_str(r#"{"family":"swbf9","map":"x"}"#).unwrap();
        assert_eq!(serde_json::to_string(&row).unwrap(), r#"{"family":"swbf9","map":"x"}"#);
    }
}
