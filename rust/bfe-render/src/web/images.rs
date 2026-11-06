// Drawing-context capabilities for the independent image worker.
use wasm_bindgen::prelude::*;
use crate::texture_format;

// Read the drawing context, not a worker context; bit 6 marks BC7 emulation.
#[wasm_bindgen]
pub fn texture_capabilities(gl: &web_sys::WebGl2RenderingContext) -> u32 {
    let mut bits = 0;
    for (bit, name) in [(1,"WEBGL_compressed_texture_astc"),(2,"EXT_texture_compression_bptc"),(4,"WEBGL_compressed_texture_s3tc"),(8,"WEBGL_compressed_texture_etc"),(16,"WEBGL_compressed_texture_etc1"),(32,"WEBGL_compressed_texture_pvrtc"),(32,"WEBKIT_WEBGL_compressed_texture_pvrtc")] {
        if gl.get_extension(name).ok().flatten().is_some() { bits |= bit; }
    }
    // Unmasked parameters are invalid when the privacy extension is absent.
    // Query only an enabled extension, leaving the ordinary capability bits.
    let name = if gl.get_extension("WEBGL_debug_renderer_info").ok().flatten().is_some() {
        gl.get_parameter(0x9246).ok().and_then(|s| s.as_string()).unwrap_or_default()
    } else { String::new() };
    if texture_format::unpacks_bc7(&name) { bits |= 64; }
    bits
}
