//! Measure tips in the same logical viewport and font used to paint them.
use super::{tip_inset, FOOTER_BOTTOM, FRAME, LEFT, PANEL_ORIGIN, TIP_WIDTH};
use bfe_render::interface::Font;
use std::ops::Range;

#[derive(Clone, Copy, Debug)]
pub struct TipPanel {
    pub unit: f64,
    pub origin: [f64; 2],
    pub width: f64,
    pub inset: [f64; 2],
    pub max_text_height: f64,
}

impl TipPanel {
    pub fn new(viewport: [f64; 2], border: [f64; 2], led_height: f64, wide: bool) -> Self {
        let unit = (viewport[0].max(1.) / FRAME[0]).min(viewport[1].max(1.) / FRAME[1]);
        let compact = unit < 0.9;
        let origin = if unit < 0.65 {
            [16., 64.]
        } else if compact || wide {
            [LEFT, 70.]
        } else {
            PANEL_ORIGIN
        };
        let width = if compact || wide {
            FRAME[0] - 2. * origin[0]
        } else {
            TIP_WIDTH
        };
        let inset = border.map(tip_inset);
        let max_text_height = FOOTER_BOTTOM - led_height - 20. - origin[1] - 2. * inset[1];
        Self {
            unit,
            origin,
            width,
            inset,
            max_text_height,
        }
    }

    pub fn text_width(self) -> f64 {
        self.width - 2. * self.inset[0]
    }

    pub fn controls_height(self) -> f64 {
        18. / self.unit
    }

    pub fn bitmap_scale(self, font: &Font) -> f64 {
        (11. / (self.unit * font.height.max(1.))).max(1.)
    }
}

/// Called when the selected text, font or panel width changes. The draw pass
/// borrows these rows. A blank paragraph consumes a line, as in the text table.
pub fn wrap_bitmap_tip(text: &str, font: &Font, width: f64) -> Vec<Vec<u16>> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut row = Vec::new();
        let mut used = 0.;
        let space = font.width(&[32], 1., 0.);
        for word in paragraph.split_whitespace() {
            let word: Vec<_> = word.encode_utf16().collect();
            if !row.is_empty() {
                if used + space + font.width(&word, 1., 0.) > width {
                    lines.push(std::mem::take(&mut row));
                    used = 0.;
                } else {
                    row.push(32);
                    used += space;
                }
            }
            for ch in word {
                let advance = font.width(std::slice::from_ref(&ch), 1., 0.);
                if !row.is_empty() && used + advance > width {
                    lines.push(std::mem::take(&mut row));
                    used = 0.;
                }
                row.push(ch);
                used += advance;
            }
        }
        lines.push(row);
    }
    lines
}

/// Partition oversized notes at measured word boundaries. A single word
/// longer than a page can break at a character boundary.
pub fn tip_pages(text: &str, mut fits: impl FnMut(&str) -> bool) -> Vec<Range<usize>> {
    let mut pages = Vec::new();
    let ends: Vec<_> = text
        .char_indices()
        .map(|(i, ch)| i + ch.len_utf8())
        .collect();
    let mut start = 0;
    while start < ends.len() {
        let from = if start == 0 { 0 } else { ends[start - 1] };
        if fits(&text[from..]) {
            pages.push(from..text.len());
            break;
        }
        let count = ends[start..]
            .partition_point(|&end| fits(text[from..end].trim_end()))
            .max(1);
        let limit = ends[start + count - 1];
        let chunk = &text[from..limit];
        let cut = if text[limit..].starts_with(char::is_whitespace) {
            limit
        } else {
            chunk
                .rmatch_indices(char::is_whitespace)
                .find_map(|(i, ch)| (!chunk[..i].trim().is_empty()).then_some(from + i + ch.len()))
                .unwrap_or(limit)
        };
        pages.push(from..cut);
        start = ends.partition_point(|&end| end <= cut);
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_words_and_paragraphs_are_reachable_as_complete_pages() {
        let text = "Bend here.\n\n六六六六六六六六六六六六六六\nFold again.";
        let pages = tip_pages(text, |s| s.trim().chars().count() <= 8);
        assert!(pages.len() > 3);
        assert_eq!(
            pages.iter().map(|r| &text[r.clone()]).collect::<String>(),
            text
        );
        assert!(pages
            .iter()
            .all(|r| text[r.clone()].trim().chars().count() <= 8));
        let font = super::super::tests::fonts_with_glyphs();
        let rows = wrap_bitmap_tip("abcd efg\n\nabcdefg", &font.small, 40.);
        let rows: Vec<_> = rows
            .iter()
            .map(|s| String::from_utf16(s).unwrap())
            .collect();
        assert_eq!(rows, ["abcd", "efg", "", "abcd", "efg"]);
    }
}
