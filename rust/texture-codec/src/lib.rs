// Basis decoding lives in the loading worker, separate from the drawer.
// PNG-only battles must not reserve its tables in every worker/renderer heap.
// Restoration uses retained transcoded blocks and never needs this module.
use wasm_bindgen::prelude::*;
// Format priority and block sizing are shared source, not a second policy.
#[path = "../../bfe-render/src/texture_format.rs"]
pub mod texture_format;
use texture_format::{Capabilities, Format};

// The C++ transcoder is a long-lived library inside this module. Referencing
// its constructor entry prevents wasm-ld from wrapping every Rust export as
// a short-lived command, with constructors/destructors around each call.
// Initialize once before any decoder object can be constructed.
#[cfg(target_arch = "wasm32")]
fn initialize_resource_runtime() {
    extern "C" { fn __wasm_call_ctors(); }
    static INIT: std::sync::Once=std::sync::Once::new();
    INIT.call_once(||unsafe {__wasm_call_ctors();});
}

#[wasm_bindgen]
pub struct DecodedBasis { image: basis_transcode::Transcoded, format: Format }

#[wasm_bindgen]
impl DecodedBasis {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 { self.image.info.width }
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 { self.image.info.height }
    #[wasm_bindgen(getter)]
    pub fn format(&self) -> u32 { self.format.descriptor() }
    #[wasm_bindgen(getter)]
    pub fn premultiplied(&self) -> bool { self.image.info.premultiplied }
    // One payload backs every named mip range, layer and face.
    pub fn mip_metadata(&self) -> String {
        serde_json::to_string(&self.image.mips.iter().map(|m| serde_json::json!({"width":m.width,"height":m.height,"level":m.level,"layer":m.layer,"face":m.face,"offset":m.bytes.start,"length":m.bytes.len()})).collect::<Vec<_>>()).unwrap()
    }
    pub fn take_pixels(self) -> Vec<u8> { self.image.data }
}

// KTX2Loader's format selection and complete mip transcode in one Rust call.
// Encoded input dies with the decoder; the smaller GPU block chain survives.
#[wasm_bindgen]
pub fn decode_texture_basis(bytes: Vec<u8>, max_size: u32, bits: u32) -> Result<DecodedBasis, JsValue> {
    #[cfg(target_arch = "wasm32")]
    initialize_resource_runtime();
    let mut decoder = basis_transcode::Decoder::new(bytes, max_size).map_err(|e| JsValue::from_str(&e))?;
    let info = decoder.info();
    // TextureLibrary consumes ordinary 2D assets. Reject unhandled container
    // layouts explicitly rather than silently concatenating faces as mips.
    if info.faces != 1 || info.layers > 1 { return Err(JsValue::from_str("KTX2 texture arrays and cube assets are not supported")); }
    let format = texture_format::choose(Capabilities::from_bits(bits), info.uastc, info.alpha, info.width, info.height, bits & 64 != 0);
    let image = decoder.transcode(format.transcode()).map_err(|e| JsValue::from_str(&e))?;
    Ok(DecodedBasis { image, format })
}
