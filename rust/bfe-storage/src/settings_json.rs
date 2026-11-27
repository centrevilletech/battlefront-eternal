//! JSON values in player files. A client built alone read every object key literally.
//! The packaged client shared the server's raw-value reader. Online reply parsing
//! also needs raw values now, independently of how the client reads saved settings.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

struct SavedValue(Value);

/// Read a value from a settings file supplied as bytes.
pub fn from_slice(bytes: &[u8]) -> serde_json::Result<Value> {
    serde_json::from_slice::<SavedValue>(bytes).map(|v| v.0)
}

/// Read a value from a settings file supplied as text.
pub fn from_str(text: &str) -> serde_json::Result<Value> {
    serde_json::from_str::<SavedValue>(text).map(|v| v.0)
}

#[cfg(feature = "server-json")]
mod reader {
    use super::*;

    impl<'de> Deserialize<'de> for SavedValue {
        fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
            Value::deserialize(de).map(Self)
        }
    }

    pub fn field<'de, T: Deserialize<'de>, D: Deserializer<'de>>(de: D) -> Result<T, D::Error> {
        T::deserialize(de)
    }
    pub use field as list;
    pub use field as map;
    pub use field as object;
    pub use field as optional;
}

#[cfg(not(feature = "server-json"))]
mod reader {
    use super::*;
    use serde::de::{MapAccess, SeqAccess, Visitor};
    use serde_json::Map;
    use std::{collections::BTreeMap, fmt};

    impl<'de> Deserialize<'de> for SavedValue {
        fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
            de.deserialize_any(Literal).map(Self)
        }
    }

    // The JSON deserializer reads strings and numbers as usual. Object keys
    // remain literal at every depth.
    struct Literal;
    impl<'de> Visitor<'de> for Literal {
        type Value = Value;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("any valid JSON value")
        }
        fn visit_unit<E>(self) -> Result<Value, E> {
            Ok(Value::Null)
        }
        fn visit_none<E>(self) -> Result<Value, E> {
            Ok(Value::Null)
        }
        fn visit_some<D: Deserializer<'de>>(self, de: D) -> Result<Value, D::Error> {
            SavedValue::deserialize(de).map(|value| value.0)
        }
        fn visit_bool<E>(self, v: bool) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_i64<E>(self, v: i64) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_u64<E>(self, v: u64) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_f64<E>(self, v: f64) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_str<E>(self, v: &str) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_string<E>(self, v: String) -> Result<Value, E> {
            Ok(v.into())
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Value, A::Error> {
            let mut values = Vec::new();
            while let Some(SavedValue(value)) = seq.next_element()? {
                values.push(value);
            }
            Ok(Value::Array(values))
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Value, A::Error> {
            let mut values = Map::new();
            while let Some((key, SavedValue(value))) = map.next_entry()? {
                values.insert(key, value);
            }
            Ok(Value::Object(values))
        }
    }

    pub fn optional<'de, D: Deserializer<'de>>(de: D) -> Result<Option<Value>, D::Error> {
        Ok(Option::<SavedValue>::deserialize(de)?.map(|v| v.0))
    }

    pub fn list<'de, D: Deserializer<'de>>(de: D) -> Result<Vec<Value>, D::Error> {
        Ok(Vec::<SavedValue>::deserialize(de)?
            .into_iter()
            .map(|v| v.0)
            .collect())
    }

    pub fn map<'de, D: Deserializer<'de>>(de: D) -> Result<BTreeMap<String, Value>, D::Error> {
        Ok(BTreeMap::<String, SavedValue>::deserialize(de)?
            .into_iter()
            .map(|(k, v)| (k, v.0))
            .collect())
    }

    pub fn object<'de, D: Deserializer<'de>>(de: D) -> Result<Map<String, Value>, D::Error> {
        let values = BTreeMap::<String, SavedValue>::deserialize(de)?;
        Ok(values.into_iter().map(|(k, v)| (k, v.0)).collect())
    }
}

pub use reader::{list, map, object, optional};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn saved_object_keys_keep_their_meaning_in_each_build() {
        let key = "$serde_json::private::RawValue";
        let number = br#"[{"\u0024serde_json::private::RawValue":7}]"#;
        let text = r#"{"item":{"$serde_json::private::RawValue":"42"}}"#;
        let ordinary =
            r#"{"items":[null,false,-7,1.25,18446744073709551615],"word":"value","word":"last"}"#;
        assert_eq!(
            from_str(ordinary).unwrap(),
            serde_json::from_str::<Value>(ordinary).unwrap()
        );
        if cfg!(feature = "server-json") {
            assert!(from_slice(number).is_err());
            assert_eq!(from_str(text).unwrap()["item"], json!(42));
        } else {
            assert_eq!(from_slice(number).unwrap()[0][key], json!(7));
            assert_eq!(from_str(text).unwrap()["item"][key], json!("42"));
        }
    }
}
