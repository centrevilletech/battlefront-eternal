// PNG inputs for TextureLibrary.load and texturePixels in the browser host.
// Decode straight RGBA bytes, without applying the PNG gamma/profile chunks.
// The battle lights and blends authored byte values in gamma space; applying
// an image viewer's colour conversion here would change both lighting and HUD.
use std::io::Cursor;

#[derive(Debug)]
pub struct Image {
    // Dimensions describe logical texels, never a compressed block footprint.
    // Rows remain in file order so a shared image has one orientation.
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Image {
    // Port of texturePixels' Canvas draw/read conversion, for CPU consumers
    // only. GPU uploads use the straight decoded bytes above. Canvas first
    // quantizes premultiplied channels, then reads normalized f32 channels and
    // unpremultiplies them with ties-to-even byte conversion. Integer division
    // alone differs at half-byte boundaries and can change a detected hue.
    pub fn canvas_pixels(&mut self) {
        const BYTE_SCALE: f32 = 1.0 / 255.0;
        for pixel in self.rgba.as_chunks_mut::<4>().0 {
            let alpha = pixel[3] as u32;
            let a = alpha as f32 * BYTE_SCALE;
            let inverse = if alpha == 0 { 0.0 } else { 1.0 / a };
            for c in &mut pixel[..3] {
                let premultiplied = (*c as u32 * alpha + 127) / 255;
                let normalized = premultiplied as f32 * BYTE_SCALE;
                *c = ((normalized * inverse) * 255.0).round_ties_even().min(255.0) as u8;
            }
        }
    }
}

// Port of TextureLibrary's PNG decode. Device limits are checked before the
// output allocation, including for images with very small encoded payloads.
// An RGBA result also gives CPU lamp discovery the same row and channel order
// as GPU upload. Flipping and premultiplication belong to the sampler upload,
// never this shared source: clones may request different sampler settings.
// Raster HUD atlases are lossless WebP, while imported source images are PNG.
// Decode both in Rust so atlas upgrades cannot secretly reintroduce a browser
// codec. Container identification uses bytes rather than a URL suffix.
pub fn decode_image(bytes: &[u8], max_size: u32) -> Result<Image,String> {
    if bytes.starts_with(b"RIFF") && bytes.get(8..12)==Some(b"WEBP") {
        let mut decoder=image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).map_err(|e|format!("WebP header: {e}"))?;
        let (width,height)=decoder.dimensions();
        if width==0 || height==0 || width>max_size || height>max_size || decoder.is_animated() {
            return Err("WebP dimensions exceed texture limits or image is animated".into());
        }
        // The atlas generator writes only lossless data. Reject another codec
        // rather than accepting a different lossy chroma reconstruction here.
        if decoder.is_lossy() { return Err("Texture WebP must be lossless".into()); }
        let count=(width as usize).checked_mul(height as usize).and_then(|n|n.checked_mul(4)).ok_or("WebP output size overflow")?;
        if count>128<<20{return Err("WebP output exceeds 128 MiB".into());}
        decoder.set_memory_limit(count.saturating_mul(4));
        let alpha=decoder.has_alpha();
        let mut rgba=vec![0;count];
        let length=if alpha { count } else { count/4*3 };
        decoder.read_image(&mut rgba[..length]).map_err(|e|format!("WebP pixels: {e}"))?;
        if !alpha { for i in (0..count/4).rev() { let rgb=[rgba[3*i],rgba[3*i+1],rgba[3*i+2]];rgba[4*i..4*i+3].copy_from_slice(&rgb);rgba[4*i+3]=255; } }
        Ok(Image {width,height,rgba})
    } else { decode_png(bytes,max_size) }
}

pub fn decode_png(encoded: &[u8], max_size: u32) -> Result<Image, String> {
    let mut decoder = png::Decoder::new(Cursor::new(encoded));
    // Expand palette and transparency chunks before selecting the output size.
    // STRIP_16 keeps the high byte; it does not round through a float colour.
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| format!("PNG header: {e}"))?;
    let info = reader.info();
    if info.width == 0 || info.height == 0 || info.width > max_size || info.height > max_size {
        return Err(format!("PNG {}x{} exceeds texture limit {max_size}", info.width, info.height));
    }
    let pixels = (info.width as usize).checked_mul(info.height as usize)
        .and_then(|n| n.checked_mul(4)).ok_or("PNG dimensions overflow")?;
    // The decoder's own row/filter limits remain active for malformed inputs.
    // Reading metadata cannot force allocation of an unchecked image buffer.
    let bytes = reader.output_buffer_size().ok_or("PNG output size overflows")?;
    if pixels>128<<20{return Err("PNG output exceeds 128 MiB".into());}
    let mut rgba = vec![0; pixels];
    // Decode into the final allocation. Expanding RGB/grey backwards below
    // avoids another full-size image at peak loading memory on older PCs.
    let frame = reader.next_frame(&mut rgba[..bytes]).map_err(|e| format!("PNG pixels: {e}"))?;
    let channels = frame.color_type.samples();
    if channels != 4 {
        for pixel in (0..pixels / 4).rev() {
            let src = pixel * channels;
            let (r, g, b, a) = match frame.color_type {
                png::ColorType::Rgb => (rgba[src], rgba[src + 1], rgba[src + 2], 255),
                png::ColorType::Grayscale => (rgba[src], rgba[src], rgba[src], 255),
                png::ColorType::GrayscaleAlpha => (rgba[src], rgba[src], rgba[src], rgba[src + 1]),
                _ => return Err("PNG decoder returned unexpanded palette".into()),
            };
            rgba[pixel * 4..pixel * 4 + 4].copy_from_slice(&[r, g, b, a]);
        }
    }
    Ok(Image { width: frame.width, height: frame.height, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    // Every colour/alpha byte pair was read through the original software
    // Canvas path. Keep its digest independent of the implementation's math.
    // This covers zero alpha, opaque pixels and all rounding boundaries.
    #[test]
    fn cpu_pixels_match_all_canvas_colour_alpha_pairs() {
        let mut rgba = Vec::with_capacity(256 * 256 * 4);
        for a in 0..=255 { for c in 0..=255 { rgba.extend_from_slice(&[c, c, c, a]); } }
        let mut image = Image { width: 256, height: 256, rgba };
        image.canvas_pixels();
        assert_eq!(format!("{:x}", Sha256::digest(&image.rgba)), "906f0efb4d3343ead2f1b23f94866dae85025fd21344c717e6491e90ffdbe09f");
    }

    // Encoder fixtures carry independent, authored expected pixel values.
    // No browser image decoder or retail asset is needed for codec coverage.
    fn fixture(colour: png::ColorType, depth: png::BitDepth, data: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut encoded, 2, 1);
            encoder.set_color(colour);
            encoder.set_depth(depth);
            encoder.set_source_gamma(png::ScaledFloat::new(0.45455));
            encoder.write_header().unwrap().write_image_data(data).unwrap();
        }
        encoded
    }

    #[test]
    fn straight_alpha_and_gamma_bytes_are_preserved() {
        let expected = [19, 127, 241, 3, 255, 2, 81, 128];
        let image = decode_png(&fixture(png::ColorType::Rgba, png::BitDepth::Eight, &expected), 2).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.rgba, expected);
    }

    #[test]
    fn rgb_expansion_does_not_overwrite_the_next_pixel() {
        let image = decode_png(&fixture(png::ColorType::Rgb, png::BitDepth::Eight, &[11, 12, 13, 21, 22, 23]), 2).unwrap();
        assert_eq!(image.rgba, [11, 12, 13, 255, 21, 22, 23, 255]);
    }

    // Sixteen-bit inputs retain the high byte, matching STRIP_16 rather than
    // rounding a normalized float. Grey alpha expands to all three channels.
    #[test]
    fn grey_alpha_sixteen_bit_and_device_limit() {
        let bytes = fixture(png::ColorType::GrayscaleAlpha, png::BitDepth::Sixteen, &[17, 255, 129, 255, 31, 0, 222, 0]);
        assert_eq!(decode_png(&bytes, 2).unwrap().rgba, [17, 17, 17, 129, 31, 31, 31, 222]);
        assert!(decode_png(&bytes, 1).unwrap_err().contains("texture limit"));
        assert!(decode_png(&bytes[..20], 2).is_err());
    }
    // HUD upgrades use lossless WebP. Nonzero RGB behind zero alpha also
    // checks that decoding never performs an implicit premultiply conversion.
    #[test]
    fn lossless_atlas_rows_and_limits_are_preserved() {
        let expected=[19,127,241,3,255,2,81,128,71,18,39,0,11,12,13,255];
        let mut bytes=Vec::new();
        image_webp::WebPEncoder::new(&mut bytes).encode(&expected,2,2,image_webp::ColorType::Rgba8).unwrap();
        let image=decode_image(&bytes,2).unwrap();
        assert_eq!((image.width,image.height),(2,2));assert_eq!(image.rgba,expected);
        assert!(decode_image(&bytes,1).is_err());
        assert!(decode_image(&bytes[..20],2).is_err());
    }

}
