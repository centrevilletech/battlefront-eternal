//! Reliable gameplay delivery shares the changed-field journal with snapshot
//! delivery. Borrow JSON strings and decode into its fixed payload scratch;
//! neither an event vector nor a per-packet string survives the callback.
use super::publications::Publications;
use base64::{engine::general_purpose::STANDARD, Engine};
use bfe_presentation::network_layout::{net_kind as k, net_limits as l, net_record as r};
use serde::{
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Deserialize,
};
use std::fmt;
#[derive(Deserialize)]
struct Record<'a> {
    kind: &'a str,
    id: u32,
    generation: u32,
    #[serde(default)]
    tick: f64,
    schema: u32,
    payload: &'a str,
}
struct Records<'a>(&'a mut Publications);
impl<'de> DeserializeSeed<'de> for Records<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_seq(self)
    }
}
impl<'de> Visitor<'de> for Records<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("at most 256 gameplay records")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        let mut count = 0;
        let mut bytes = [0; l::FIELD_PAYLOAD];
        while let Some(record) = seq.next_element::<Record>()? {
            count += 1;
            if count > 256 {
                return Err(de::Error::custom("gameplay event count exceeds bounds"));
            }
            if record.kind != "combat"
                || record.id == 0
                || record.generation == 0
                || !record.tick.is_finite()
                || record.tick < 0.
                || record.tick.fract() != 0.
            {
                return Err(de::Error::custom("invalid gameplay record"));
            }
            let length = STANDARD.decode_slice(record.payload, &mut bytes).map_err(de::Error::custom)?;
            let mut row = [0.; r::STRIDE];
            row[r::KIND] = k::COMBAT as f64;
            row[r::ID] = record.id as f64;
            row[r::GENERATION] = record.generation as f64;
            row[r::TICK] = record.tick;
            row[r::SCHEMA] = record.schema as f64;
            self.0.push(&row, &bytes[..length]);
        }
        Ok(())
    }
}
struct Envelope<'a>(&'a mut Publications);
impl<'de> Visitor<'de> for Envelope<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a gameplay event envelope")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        let mut seen = false;
        while let Some(key) = map.next_key::<&str>()? {
            if key == "records" {
                if seen {
                    return Err(de::Error::duplicate_field("records"));
                }
                map.next_value_seed(Records(self.0))?;
                seen = true;
            } else {
                map.next_value::<de::IgnoredAny>()?;
            }
        }
        if !seen {
            return Err(de::Error::missing_field("records"));
        }
        Ok(())
    }
}
pub fn receive(bytes: &[u8], journal: &mut Publications) -> Result<(), &'static str> {
    let mut decoder = serde_json::Deserializer::from_slice(bytes);
    de::Deserializer::deserialize_map(&mut decoder, Envelope(journal)).map_err(|_| "invalid reliable gameplay events")?;
    decoder.end().map_err(|_| "trailing reliable gameplay events")
}

// serde_json's IgnoredAny reserves a nesting stack when it skips an unknown
// array of objects. The envelope classifier must skip records without that
// per-packet allocation before the typed journal visitor consumes them.
pub fn ignore<'de, D: de::Deserializer<'de>>(decoder: D) -> Result<(), D::Error> {
    decoder.deserialize_any(Ignore)
}
struct Ignore;
impl<'de> Visitor<'de> for Ignore {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value")
    }
    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }
    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }
    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }
    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }
    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }
    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while seq.next_element_seed(Ignore)?.is_some() {}
        Ok(())
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while map.next_key::<&str>()?.is_some() {
            map.next_value_seed(Ignore)?;
        }
        Ok(())
    }
}
impl<'de> DeserializeSeed<'de> for Ignore {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        d.deserialize_any(self)
    }
}
