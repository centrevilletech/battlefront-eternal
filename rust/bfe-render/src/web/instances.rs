// The instance pass's bindings (instances.rs): where its records start in
// `input`, and their size (f64 slots).
use super::BfeRenderer;
use crate::instances::{IN_RECORDS, REC_STRIDE};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
impl BfeRenderer {
    pub fn records_at(&self) -> u32 { IN_RECORDS as u32 }
    pub fn record_stride(&self) -> u32 { REC_STRIDE as u32 }
}
