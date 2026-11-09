//! Rasterize the page theme's licensed Saira at content admission, never per frame.
use bfe_render::interface::{Font, Glyph};
use std::collections::HashMap;

pub fn atlas(bytes: &[u8]) -> Result<(Vec<u8>, Font), String> {
    let source = fontdue::Font::from_bytes(bytes, fontdue::FontSettings::default())
        .map_err(str::to_owned)?;
    let mut pixels = vec![0; 1024 * 1024 * 4];
    for pixel in pixels.chunks_exact_mut(4) {
        pixel[..3].fill(255);
    }
    let mut glyphs = HashMap::new();
    // The Latin subset shipped by electron/ui/fonts covers the page's labels
    // and Western player names. Keep layout in baseline coordinates.
    let (mut x, mut y) = (4, 4);
    for code in (32..=255u16).chain([
        0x2013, 0x2014, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2026,
    ]) {
        let ch = char::from_u32(code as u32).unwrap();
        let (m, bitmap) = source.rasterize(ch, 64.);
        if x + m.width + 4 >= 1024 {
            x = 4;
            y += 92;
        }
        if y + m.height >= 1024 {
            return Err("page font atlas overflow".into());
        }
        for row in 0..m.height {
            for col in 0..m.width {
                let at = ((y + row) * 1024 + x + col) * 4;
                pixels[at..at + 4].copy_from_slice(&[255, 255, 255, bitmap[row * m.width + col]]);
            }
        }
        glyphs.insert(
            code,
            Glyph {
                page: 0,
                advance: m.advance_width as f64,
                bounds: [
                    m.xmin as f64,
                    -(m.ymin as f64) - m.height as f64,
                    (m.xmin + m.width as i32) as f64,
                    -(m.ymin as f64),
                ],
                uv: [
                    x as f64 / 1024.,
                    y as f64 / 1024.,
                    (x + m.width) as f64 / 1024.,
                    (y + m.height) as f64 / 1024.,
                ],
            },
        );
        x += m.width + 8;
    }
    Ok((
        pixels,
        Font {
            height: 64.,
            glyphs,
        },
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn saira_rasterizes_from_the_shipped_font_without_system_fonts() {
        let (pixels, font) =
            super::atlas(include_bytes!("../../assets/fonts/saira-700.ttf")).unwrap();
        assert!(font.glyphs[&('W' as u16)].advance > font.glyphs[&('I' as u16)].advance);
        assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
        assert!(font.glyphs[&0x2026].advance > 0.);
    }
}
