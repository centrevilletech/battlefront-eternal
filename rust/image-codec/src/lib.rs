// Image decoding for the loading worker. GPU upload and CPU colour reads use
// the same PNG decoder, with the Canvas conversion requested only by CPU reads.
// No DOM image decoder or graphics context is needed to produce these bytes.
use wasm_bindgen::prelude::*;
#[path = "../../bfe-render/src/texture_image.rs"]
mod texture_image;
use texture_image::Image;

#[wasm_bindgen]
pub struct DecodedPng { image: Image }

#[wasm_bindgen]
impl DecodedPng {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 { self.image.width }
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 { self.image.height }

    // Canvas composition consumes a borrowed view before freeing this image.
    // Keeping it inside the decoder worker avoids a second full RGBA transfer.
    #[wasm_bindgen(getter)]
    pub fn pixels_ptr(&self) -> usize { self.image.rgba.as_ptr() as usize }
    #[wasm_bindgen(getter)]
    pub fn pixels_len(&self) -> usize { self.image.rgba.len() }

    // Consuming the result frees its Rust allocation after bindgen copies the
    // bytes into a transferable JS buffer. The worker does not retain an image.
    pub fn take_pixels(self) -> Vec<u8> { self.image.rgba }
}

// TextureLibrary and texturePixels share one worker and one module instance.
// Width limits are validated before the output allocation inside decode_png.
#[wasm_bindgen]
pub fn decode_texture_png(bytes: &[u8], max_size: u32, cpu: bool) -> Result<DecodedPng, JsValue> {
    let mut image = texture_image::decode_image(bytes, max_size).map_err(|e| JsValue::from_str(&e))?;
    if cpu { image.canvas_pixels(); }
    Ok(DecodedPng { image })
}

