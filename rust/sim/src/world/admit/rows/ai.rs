//! Cold AI facts and the admission program from ai-world-content.js,
//! ai-world-load.js and AiWorld in ai-world.js. Identities come from the
//! earlier native owners; row construction never allocates a second owner.
mod content;
mod context;
mod facts;
mod drive;
mod load;
mod mobile;
mod observe;
mod posts;
mod program;
mod receipts;
mod stage;
mod vehicle;
pub use content::*;
pub use context::*;
pub use facts::*;
pub(crate) use drive::*;
pub use load::*;
pub use mobile::*;
pub use observe::*;
pub use posts::*;
pub use program::*;
pub use receipts::*;
pub use stage::*;
pub use vehicle::*;

use super::Call;
use crate::world::admit::{AdmitError, Stage};
use bfe_content::value::Value;

pub(crate) fn error(message: impl Into<String>) -> AdmitError {
    AdmitError::Content {
        stage: Stage::Ai,
        message: message.into(),
    }
}

/// String(value) at the few authored limits that AiWorld stringifies. In
/// particular an array joins with commas, while a numeric -0 becomes "0".
pub fn string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(v)) => v.to_string(),
        Some(Value::Number(v)) => bfe_content::js::number_to_string(*v),
        Some(Value::String(v)) => v.to_string(),
        Some(Value::Object(_)) => "[object Object]".into(),
        Some(Value::Array(v)) => v
            .iter()
            .map(|v| {
                if v.is_null() {
                    String::new()
                } else {
                    string(Some(v))
                }
            })
            .collect::<Vec<_>>()
            .join(","),
    }
}

pub(crate) fn present(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| !v.is_null())
}

pub(crate) fn numeric<const N: usize>(value: Option<&Value>) -> Result<[f64; N], AdmitError> {
    let values = value
        .and_then(Value::as_array)
        .ok_or_else(|| error("expected an AI numeric array"))?;
    // A generated fixed array writes undefined for missing entries, which a
    // Float64Array converts to NaN. Extra authored entries are not written.
    Ok(std::array::from_fn(|i| {
        bfe_content::js::to_number(values.get(i))
    }))
}

/// Use the generated record's writer, including its nested heap order. The
/// bridge reserves parent records before appending their indirect members.
pub(crate) fn encode(words: usize, put: impl FnOnce(&mut [f64], &mut Vec<f64>)) -> Call {
    let mut call = Call {
        row: vec![0.0; words],
        ..Default::default()
    };
    put(&mut call.row, &mut call.heap);
    call
}

/// The shared context must include the earlier owners' typed receipts.
/// Individual builders below accept those receipts and marshal through Cx.
pub fn build(cx: &mut super::Cx) -> Result<super::Calls, AdmitError> {
    build_next(cx)
}

/// Both standalone fixtures and the enclosing context use the same field
/// walk. Cx-backed interning also records each string on its export call.
pub trait AiInterner {
    fn intern_str(&mut self, text: &str) -> crate::gen::StrId;
}
impl AiInterner for crate::store::Strings {
    fn intern_str(&mut self, text: &str) -> crate::gen::StrId {
        crate::store::Strings::intern_str(self, text)
    }
}
