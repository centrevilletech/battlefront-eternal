//! VehicleContent.pack and vehicle-phase-data.js json: retain authored order,
//! negative zero and the three unmodified binary spans. JSON's ordinary
//! serializer changes -0, so this encoder deliberately writes every number.
use super::{AdmitError, Call, Cx, Options, Stage};
use bfe_content::battle::Battle;
use bfe_content::value::{Object, Value};
use std::rc::Rc;

/// An export's scalar arguments are separate from its generated argument
/// block. Upload bytes live on the consuming export, after its earlier
/// allocation returned the input pointer, as the recorder captures them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExportCall {
    pub name: &'static str,
    pub call: Call,
}
impl ExportCall {
    pub fn scalar(name: &'static str, arguments: Vec<f64>) -> Self {
        Self { name, call: Call { args: arguments, ..Default::default() } }
    }
    pub fn into_row(self, stage: Stage) -> (Stage, &'static str, Call) { (stage, self.name, self.call) }
    /// Cx records the same intern operations the live sequence replays.
    /// String arguments occupy the leading scalar fields of these exports.
    pub fn in_context(mut self, cx: &mut Cx, stage: Stage) -> Result<(Stage, &'static str, Call), AdmitError> {
        let texts = std::mem::take(&mut self.call.strings);
        for (field, units) in texts.iter().enumerate() {
            let text = String::from_utf16(units).map_err(|_| error("vehicle string is not valid UTF-16"))?;
            let id = cx.intern(&mut self.call, &text).0 as f64;
            let argument = self.call.args.get_mut(field).ok_or_else(|| error("vehicle string has no scalar argument"))?;
            *argument = id;
        }
        Ok(self.into_row(stage))
    }
}

pub(crate) fn error(message: impl Into<String>) -> AdmitError {
    AdmitError::Content { stage: Stage::Fleet, message: message.into() }
}

/// A written object, with ECMAScript's integer-key ordering supplied by Value.
/// Missing properties are omitted, as json() omits undefined object values.
pub fn object(entries: impl IntoIterator<Item = (&'static str, Option<Value>)>) -> Value {
    Value::Object(Rc::new(Object::from_written(entries.into_iter()
        .filter_map(|(k, v)| v.map(|v| (Rc::from(k), v))).collect())))
}

pub fn array(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(Rc::new(values.into_iter().collect()))
}

/// vehicle-phase-data.js json(). Content has already rejected invalid UTF-8
/// and lone surrogates, so serde's string quoting matches JSON.stringify.
/// Nonfinite numbers become null; signed zero is the deliberate exception.
pub fn json(value: &Value) -> Result<String, AdmitError> {
    fn write(value: &Value, out: &mut String) -> Result<(), AdmitError> {
        match value {
            Value::Null => out.push_str("null"),
            Value::Bool(v) => out.push_str(if *v { "true" } else { "false" }),
            Value::Number(v) => {
                if v.to_bits() == (-0.0f64).to_bits() { out.push_str("-0"); }
                else { out.push_str(&jsrt::numfmt::json_number(*v)); }
            }
            Value::String(v) => out.push_str(&serde_json::to_string(&**v).map_err(|e| error(e.to_string()))?),
            Value::Array(values) => {
                out.push('[');
                for (i, value) in values.iter().enumerate() {
                    if i != 0 { out.push(','); }
                    write(value, out)?;
                }
                out.push(']');
            }
            Value::Object(values) => {
                out.push('{');
                for (i, (key, value)) in values.entries().enumerate() {
                    if i != 0 { out.push(','); }
                    out.push_str(&serde_json::to_string(key).map_err(|e| error(e.to_string()))?);
                    out.push(':'); write(value, out)?;
                }
                out.push('}');
            }
        }
        Ok(())
    }
    let mut out = String::new();
    write(value, &mut out)?;
    Ok(out)
}

/// Exact veh_bytes allocation and veh_pack parameters. Bytes are a cold
/// transfer, never floating point values: NaN payloads in buffers stay intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pack {
    pub text: String,
    pub bytes: Vec<u8>,
    pub text_bytes: u32,
    pub floats: u32,
    pub indices: u32,
    pub animations: u32,
}
impl Pack {
    /// VehicleContent.pack picks only these four properties in this order;
    /// extras, spawns and blockers do not belong to the rig parser's JSON.
    pub fn new(data: &Value, floats: &[u8], indices: &[u8], animations: &[u8]) -> Result<Self, AdmitError> {
        let text = json(&object(["models", "collision", "animations", "classes"].map(|k| (k, data.get(k).cloned()))))?;
        let text_bytes = u32::try_from(text.len()).map_err(|_| error("vehicle JSON exceeds the byte boundary"))?;
        let head = text.len().checked_add(3).ok_or_else(|| error("vehicle JSON alignment overflow"))? & !3;
        let mut counts = [0; 3];
        let mut total = head;
        for (i, bytes) in [floats, indices, animations].iter().enumerate() {
            if bytes.len() % 4 != 0 { return Err(error("vehicle binary span is not a whole word")); }
            counts[i] = u32::try_from(bytes.len() / 4).map_err(|_| error("vehicle binary span exceeds the word boundary"))?;
            total = total.checked_add(bytes.len()).ok_or_else(|| error("vehicle pack length overflow"))?;
        }
        u32::try_from(total).map_err(|_| error("vehicle pack exceeds the byte boundary"))?;
        let mut bytes = Vec::with_capacity(total);
        bytes.extend_from_slice(text.as_bytes()); bytes.resize(head, 0);
        bytes.extend_from_slice(floats); bytes.extend_from_slice(indices); bytes.extend_from_slice(animations);
        Ok(Self { text, bytes, text_bytes, floats: counts[0], indices: counts[1], animations: counts[2] })
    }

    pub fn from_battle(battle: &Battle) -> Result<Option<Self>, AdmitError> {
        match (&battle.vehicle_def, &battle.vehicle_bufs) {
            (None, None) => Ok(None),
            (Some(def), Some(bufs)) => Self::new(def, &bufs.models, &bufs.models_index,
                bufs.animations.as_deref().unwrap_or(&[])).map(Some),
            _ => Err(error("vehicle definition and binaries must be loaded together")),
        }
    }

    /// Call.args holds scalar export arguments. veh_bytes allocates first;
    /// veh_pack carries the exact span the host wrote to that allocation.
    /// Neither export uses a generated argument block.
    pub fn calls(&self) -> Vec<ExportCall> {
        vec![
            ExportCall::scalar("veh_bytes", vec![self.bytes.len() as f64]),
            ExportCall { name: "veh_pack", call: Call { args: vec![self.text_bytes as f64, self.floats as f64, self.indices as f64, self.animations as f64], scratch: self.bytes.clone(), ..Default::default() } },
        ]
    }
}

pub fn content_calls(battle: &Battle, _options: &Options) -> Result<Vec<ExportCall>, AdmitError> {
    Ok(Pack::from_battle(battle)?.map_or_else(Vec::new, |pack| pack.calls()))
}

/// The pack is the first fleet admission import. giveMapRules precedes the
/// allocation in attachF2, so keep that call with this prefix. Fleet's owner
/// constructs the retained hulls after this immutable pack has been loaded.
pub fn build(cx: &mut Cx) -> Result<super::Calls, AdmitError> {
    let Some(pack) = Pack::from_battle(cx.battle)? else { return Ok(Vec::new()); };
    // Plan against a copy: Cx::intern must see each first use before the
    // world replays it, rather than finding strings pre-interned by planning.
    let mut planned_strings = cx.strings.clone();
    let mut calls = vec![super::fleet::map_call(&cx.battle.world, cx.content.collision.kill_y, &mut planned_strings)?];
    calls.extend(pack.calls());
    calls.into_iter().map(|export| export.in_context(cx, Stage::Fleet)).collect()
}
