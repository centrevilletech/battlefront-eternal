//! Native cold admission on the context (world/admit): the native client's
//! local battle hands over its content folder and options; the replay
//! driver hands over a recording's assets and options. Both end in the same
//! owner construction, inside the owning borrow, so a panic poisons the
//! context like any other owning entry.
use super::super::context::{Context, Error, ErrorKind};
use crate::world::admit::{self, AdmitError, Admitted, Call, Options, Returned, Stage};
use bfe_content::files::{Files, MemFiles};
use serde_json::Value;

/// Admission's refusal as a context error: a missing body is Unavailable
/// naming its stage; content the stage cannot play is a host error with the
/// loader's message; a refused row says which call refused.
pub(crate) fn admit_error(error: AdmitError) -> Error {
    match error {
        AdmitError::Unavailable(stage) => Error::new(ErrorKind::Unavailable, format!("world_admit {stage:?}")),
        other => Error::host(other.to_string()),
    }
}

impl Context {
    /// One admission or spawn-time export as the module's host calls it,
    /// against the same body the WebAssembly export wraps (world/admit/dispatch):
    /// a host that marshals its own rows (the dedicated server's JavaScript
    /// through the addon) admits a native world call by call. The call's
    /// scalar arguments are the export's; pool keys stand where the module
    /// passes addresses; its argument block, scratch bytes and interned
    /// strings travel with it. An owning entry: a panic poisons the context.
    pub fn invoke_export(&self, export: &str, call: &Call) -> Result<Returned, Error> {
        let outcome = self.owned(|w| -> Result<Returned, AdmitError> {
            admit::sequence::args(w, call)?;
            admit::dispatch::call(w, Stage::Host, export, call)
        })?;
        match outcome {
            Ok(Returned::Refused) => Err(Error::host(format!("{export} refused its arguments"))),
            Ok(answer) => Ok(answer),
            Err(AdmitError::NoBody { call, .. }) => Err(Error::new(ErrorKind::Unavailable, format!("invoke_export {call}"))),
            Err(other) => Err(Error::host(other.to_string())),
        }
    }

    /// preparePhase's foreign-row count for the next tick
    /// (`WorldTickInput.units`): the built bodies still in play that neither
    /// a network player nor a native intent drives. A host reads it before
    /// each tick, as the admission's `Admitted.units` gives the first.
    pub fn world_foreign_units(&self) -> Result<u32, Error> {
        self.owned(|w| w.foreign_unit_rows())
    }

    /// Native preparePhase writes the generated foreign rows and returns their
    /// count for the immediately following tick. The row bank is preallocated.
    pub fn world_prepare_units(&self) -> Result<u32, Error> {
        self.owned(|w| w.prepare_foreign_unit_rows())?
            .ok_or_else(|| Error::host("foreign unit row capacity exceeded"))
    }

    /// Builds the battle's owners from `files` (the content folder, or the
    /// recording's bytes) for `options`, then admits the local human and the
    /// publication host. One call a battle, before `world_step_init`.
    pub fn world_admit(&self, files: &dyn Files, options: &Options) -> Result<Admitted, Error> {
        let admitted = self.owned(|w| admit::admit(w, files, options))?.map_err(admit_error)?;
        self.world_host_admitted_random(options.seed, admitted.handles.player)?;
        Ok(admitted)
    }
}

/// The replay driver's admission line (tools/world-native.mjs):
/// `{kind:'admit', setup:{options:{map,side,ai,seed,heroes,unit,..}, ..}, assets:[{url,hex|null}]}`.
/// The recorded bytes become the battle's files; a null asset is a 404.
pub(crate) fn recorded(admission: &Value) -> Result<(MemFiles, Options), Error> {
    let invalid = |what: &str| Error::host(format!("invalid recorded admission: {what}"));
    if admission["kind"] != "admit" { return Err(invalid("kind")); }
    let options = &admission["setup"]["options"];
    let text = |key: &str| options[key].as_str().map(str::to_string).ok_or_else(|| invalid(key));
    let options = Options {
        map: text("map")?,
        side: text("side")?,
        enhanced: match options["ai"].as_str() { Some("classic") => false, Some("enhanced") => true, _ => return Err(invalid("ai")) },
        seed: options["seed"].as_u64().and_then(|n| u32::try_from(n).ok()).ok_or_else(|| invalid("seed"))?,
        heroes: options["heroes"].as_bool().ok_or_else(|| invalid("heroes"))?,
        unit: match &options["unit"] { Value::Null => None, Value::String(unit) => Some(unit.clone()), _ => return Err(invalid("unit")) },
    };
    let mut files = MemFiles::default();
    for asset in admission["assets"].as_array().ok_or_else(|| invalid("assets"))? {
        let url = asset["url"].as_str().ok_or_else(|| invalid("asset url"))?;
        match &asset["hex"] {
            Value::Null => {}
            Value::String(hex) => files.insert(url, bytes(hex).ok_or_else(|| invalid("asset hex"))?),
            _ => return Err(invalid("asset hex")),
        }
    }
    Ok((files, options))
}

fn bytes(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 { return None; }
    (0..hex.len()).step_by(2).map(|at| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok()).collect()
}
