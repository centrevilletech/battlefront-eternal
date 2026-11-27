//! Limits on prepared barrier and hint records, accumulated across the active packages.
use bfe_content::Value;

const MAX_RECORDS: usize = u16::MAX as usize;
const MAX_PROPERTIES: usize = 262_144;
const RECORD_BUDGET: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct RecordLimits {
    barriers: usize,
    hints: usize,
    properties: usize,
    bytes: usize,
}

impl RecordLimits {
    pub(crate) fn add(&mut self, key: &str, list: &[Value]) -> Result<(), &'static str> {
        let count = if key == "barriers" {
            &mut self.barriers
        } else {
            &mut self.hints
        };
        *count = count
            .checked_add(list.len())
            .filter(|n| *n <= MAX_RECORDS)
            .ok_or("too many navigation records of one kind")?;
        for row in list {
            if row.as_object().is_none() {
                return Err("navigation record must be an object");
            }
            if let Some(props) = row.get("properties") {
                let props = props
                    .as_array()
                    .ok_or("navigation properties must be an array")?;
                self.properties = self
                    .properties
                    .checked_add(props.len())
                    .filter(|n| *n <= MAX_PROPERTIES)
                    .ok_or("too many navigation properties")?;
            }
            check_prepared_row(key, row)?;
            self.measure(row, 0)?;
        }
        Ok(())
    }

    pub(crate) fn add_layer_name(&mut self, layer: &str) -> Result<(), &'static str> {
        self.charge(16 + "sourceLayer".len())?;
        self.charge(layer.len())
    }

    fn charge(&mut self, size: usize) -> Result<(), &'static str> {
        self.bytes = self
            .bytes
            .checked_add(size)
            .filter(|n| *n <= RECORD_BUDGET)
            .ok_or("navigation records exceed storage budget")?;
        Ok(())
    }

    // Charge every node, object key and string before copying the record. This also
    // bounds unfamiliar properties retained for consumers of the original records.
    fn measure(&mut self, value: &Value, depth: usize) -> Result<(), &'static str> {
        if depth > 4 {
            return Err("navigation record exceeds nesting limit");
        }
        self.charge(16)?;
        match value {
            Value::String(s) => self.charge(s.len())?,
            Value::Array(items) => {
                for item in items.iter() {
                    self.measure(item, depth + 1)?;
                }
            }
            Value::Object(fields) => {
                for (key, item) in fields.entries() {
                    self.charge(key.len())?;
                    self.measure(item, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(crate) fn check_index(row: &Value, index: usize) -> Result<(), &'static str> {
        if row.get("sourceIndex").and_then(Value::as_f64) != Some(index as f64) {
            return Err("navigation source index differs from its package ordinal");
        }
        Ok(())
    }

    pub(super) fn check_composed(json: &Value) -> Result<(), &'static str> {
        let mut limits = Self::default();
        for key in ["barriers", "hints"] {
            let Some(value) = json.get(key) else { continue };
            let rows = value
                .as_array()
                .ok_or("navigation records must be an array")?;
            limits.add(key, rows)?;
            let mut package = None;
            let mut first = 0;
            for (i, row) in rows.iter().enumerate() {
                let layer = row
                    .get("sourceLayer")
                    .map(|v| v.as_str().ok_or("invalid navigation source layer"))
                    .transpose()?;
                if layer != package {
                    first = i;
                    package = layer;
                }
                Self::check_index(row, i - first)?;
            }
        }
        Ok(())
    }
}

fn check_prepared_row(key: &str, row: &Value) -> Result<(), &'static str> {
    if row.get("sourceIndex").and_then(Value::as_f64).is_none() {
        return Err("navigation record requires a numeric source index");
    }
    let vector = |field: &str, len: usize, positive: bool| {
        row.get(field)
            .and_then(Value::as_array)
            .is_some_and(|numbers| {
                numbers.len() == len
                    && numbers.iter().all(|v| {
                        v.as_f64()
                            .is_some_and(|v| (v as f32).is_finite() && (!positive || v >= 0.))
                    })
            })
    };
    if !vector("xfrm", 12, false) {
        return Err("invalid navigation transform");
    }
    if key == "barriers" {
        let valid_flags = row
            .get("flags")
            .and_then(Value::as_f64)
            .is_some_and(|v| v.fract() == 0. && (0. ..=f64::from(u32::MAX)).contains(&v));
        if !vector("size", 3, true)
            || !valid_flags
            || !matches!(row.get("enabled"), Some(Value::Bool(_)))
        {
            return Err("invalid navigation barrier size, flags or enabled state");
        }
    } else if row.get("type").and_then(Value::as_str).is_none() {
        return Err("navigation hint type must be a string");
    }
    Ok(())
}

#[cfg(test)]
mod navigation_record_limits {
    use super::*;
    use std::rc::Rc;

    #[test]
    fn refuses_large_counts_before_copy_and_malformed_package_indices() {
        let missing = bfe_content::json::parse_bytes(br#"{"type":"3"}"#).unwrap();
        assert_eq!(
            RecordLimits::default().add("hints", &[missing]),
            Err("navigation record requires a numeric source index")
        );
        let many = vec![Value::Null; MAX_RECORDS + 1];
        assert!(RecordLimits::default().add("hints", &many).is_err());
        let nested = (0..6).fold(Value::Null, |v, _| Value::Array(Rc::new(vec![v])));
        assert!(RecordLimits::default().measure(&nested, 0).is_err());
        let row = bfe_content::json::parse_bytes(br#"{"sourceIndex":0.5}"#).unwrap();
        assert!(RecordLimits::check_index(&row, 0).is_err());
        let row = bfe_content::json::parse_bytes(br#"{"sourceIndex":2}"#).unwrap();
        assert!(RecordLimits::check_index(&row, 0).is_err());
        assert!(RecordLimits::check_index(&row, 2).is_ok());
        assert!(check_prepared_row("hints", &row).is_err());
        let mut limits = RecordLimits {
            properties: MAX_PROPERTIES,
            bytes: RECORD_BUDGET,
            ..Default::default()
        };
        let row =
            bfe_content::json::parse_bytes(br#"{"properties":[["abc",null,"def"]]}"#).unwrap();
        assert!(limits.add("barriers", &[row]).is_err());
        assert!(limits.add_layer_name("annex").is_err());
    }
}
