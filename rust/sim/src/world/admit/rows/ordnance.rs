//! OrdnanceSim's tagged definition fields (ordnance-sim.js).
//!
//! These fields are a tagged stream, not a generated fixed record. Keep the
//! field names beside the port of writeFields; the native reader declares
//! the matching list in ordnance/defs.rs. In particular, absent, null and
//! false cannot be merged into a single numeric default during admission.
use super::combat::{Definitions, ExportCall};
use crate::store::{DefId, Strings};
use crate::world::admit::{AdmitError, Stage};
use bfe_content::value::Value;

// Reuse the native reader's field order rather than copying its indices.
pub const ORDNANCE_FIELDS: &[&str] = &crate::ordnance::defs::OrdnanceDef::FIELDS;
pub const EXPLOSION_FIELDS: &[&str] = &["Damage", "DamageRadiusInner", "DamageRadiusOuter", "PersonScale", "AnimalScale", "DroidScale", "VehicleScale", "BuildingScale"];

/// The nativeServices observer runs immediately after the corresponding F1
/// call. Unit admission supplies it once hit and blast services are bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Prepared { Ordnance, Explosion, Item }

/// F1's ordnance and explosion tables. They are distinct from combat's
/// explosion table even when both refer to the same content allocation.
#[derive(Default)]
pub struct OrdnanceRows {
    pub definitions: Definitions,
    pub explosions: Definitions,
    pub calls: Vec<ExportCall>,
    sent: usize,
}
impl OrdnanceRows {
    pub fn from_definitions(definitions:Definitions,explosions:Definitions,sent:usize)->Result<Self,AdmitError> {
        if sent>definitions.values.len() {return Err(AdmitError::Content {stage:Stage::Store,message:"sent ordnance count exceeds its table".into()});}
        Ok(Self {definitions,explosions,calls:Vec::new(),sent})
    }
    pub fn sent_count(&self)->usize {self.sent}
    /// OrdnanceSim.defId drains every previously reserved definition before
    /// returning the requested handle. This is observable call order.
    pub fn definition(&mut self, def: &Value, strings: &mut Strings) -> Result<DefId, AdmitError> {
        self.definition_with(def, strings, &mut |_,_,_,_,_| Ok(()))
    }
    pub fn definition_with(&mut self, def: &Value, strings: &mut Strings, prepared: &mut impl FnMut(Prepared, DefId, &Value, &mut Strings, &mut Vec<ExportCall>) -> Result<(),AdmitError>) -> Result<DefId, AdmitError> {
        let id = self.definitions.id(def, Stage::Store)?;
        while self.sent < self.definitions.values.len() {
            let next = self.sent;
            self.sent += 1;
            let value = self.definitions.values[next].clone();
            let row = self.fields_with(&value, ORDNANCE_FIELDS, strings, prepared)?;
            self.calls.push(ExportCall::new("ord_def", vec![next as f64], row).with_strings(strings));
            prepared(Prepared::Ordnance, DefId(next as u32), &value, strings, &mut self.calls)?;
        }
        Ok(id)
    }

    /// OrdnanceSim.explosionId sends a newly seen explosion immediately,
    /// including nested definition fields before their enclosing record.
    pub fn explosion(&mut self, def: &Value, strings: &mut Strings) -> Result<DefId, AdmitError> {
        self.explosion_with(def, strings, &mut |_,_,_,_,_| Ok(()))
    }
    pub fn explosion_with(&mut self, def: &Value, strings: &mut Strings, prepared: &mut impl FnMut(Prepared, DefId, &Value, &mut Strings, &mut Vec<ExportCall>) -> Result<(),AdmitError>) -> Result<DefId, AdmitError> {
        if let Some(id) = self.explosions.get(def) { return Ok(id); }
        let id = self.explosions.id(def, Stage::Store)?;
        let row = self.fields_with(def, EXPLOSION_FIELDS, strings, prepared)?;
        self.calls.push(ExportCall::new("ord_explosion", vec![id.0 as f64], row).with_strings(strings));
        // A combat blast binding can intern fields before encoding resumes on
        // the parent ordnance definition. Deferring this changes string ids.
        prepared(Prepared::Explosion, id, def, strings, &mut self.calls)?;
        Ok(id)
    }

    /// OrdnanceSim.writeFields registers object fields before writing the
    /// parent's block. Number words are copied, preserving -0 and NaN bits.
    pub fn fields(&mut self, def: &Value, names: &[&str], strings: &mut Strings) -> Result<Vec<f64>, AdmitError> {
        self.fields_with(def, names, strings, &mut |_,_,_,_,_| Ok(()))
    }
    pub fn fields_with(&mut self, def: &Value, names: &[&str], strings: &mut Strings, prepared: &mut impl FnMut(Prepared, DefId, &Value, &mut Strings, &mut Vec<ExportCall>) -> Result<(),AdmitError>) -> Result<Vec<f64>, AdmitError> {
        let mut row = Vec::with_capacity(names.len() * 2);
        for name in names { row.extend(self.field_with(def.get(name), strings, prepared)?); }
        Ok(row)
    }

    /// OrdnanceSim.fieldWords. Content Values have no function/symbol tag;
    /// every container, including an array, is an explosion reference.
    pub fn field(&mut self, value: Option<&Value>, strings: &mut Strings) -> Result<[f64; 2], AdmitError> {
        self.field_with(value, strings, &mut |_,_,_,_,_| Ok(()))
    }
    pub fn field_with(&mut self, value: Option<&Value>, strings: &mut Strings, prepared: &mut impl FnMut(Prepared, DefId, &Value, &mut Strings, &mut Vec<ExportCall>) -> Result<(),AdmitError>) -> Result<[f64; 2], AdmitError> {
        Ok(match value {
            None => [0.0, 0.0],
            Some(Value::Null) => [1.0, 0.0],
            Some(Value::Bool(false)) => [2.0, 0.0],
            Some(Value::Bool(true)) => [3.0, 0.0],
            Some(Value::Number(number)) => [4.0, *number],
            Some(Value::String(text)) => [5.0, strings.intern_str(text).0 as f64],
            Some(value) => [6.0, self.explosion_with(value, strings, prepared)?.0 as f64],
        })
    }
}



/// attachOrdnanceSim initially sends no definitions. They are registered
/// lazily by fleet and unit construction through the shared Cx tables.
pub fn build(_cx:&mut super::Cx)->Result<super::Calls,AdmitError> { Ok(Vec::new()) }
