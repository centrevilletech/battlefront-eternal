// The game's bitmap fonts (core.lvl's gamefont_large, _medium, _small and
// _tiny, in common.json): per glyph its page, advance, box from the pen and
// the line's top, and its place on the page.
export class Fonts {
  constructor(fonts, texture) {
    this.fonts = {};
    for (const [name, font] of Object.entries(fonts || {})) {
      const glyphs = new Map(font.glyphs.map(([code, page, advance, x0, x1, y0, y1, u0, v0, u1, v1]) => [code, { page, advance, x0, x1, y0, y1, uv: [u0, v0, u1, v1] }]));
      this.fonts[name] = { lineHeight: font.lineHeight, pages: font.pages.map(texture), glyphs };
    }
  }

  lineHeight(name) {
    return this.fonts[name]?.lineHeight ?? 18;
  }

  // How far below a line's top its glyphs reach at most (past the line's
  // own height: the box the original's text is measured by).
  glyphHeight(name) {
    const font = this.fonts[name];
    if (!font) return this.lineHeight(name);
    if (font.glyphHeight === undefined) {
      let bottom = font.lineHeight;
      for (const glyph of font.glyphs.values()) if (glyph.x0 < glyph.x1) bottom = Math.max(bottom, glyph.y1);
      font.glyphHeight = bottom;
    }
    return font.glyphHeight;
  }

  // How far below a line's top its capitals' middle is (the "A"'s box),
  // for centring a line of capitals on a point.
  capsMiddle(name) {
    const font = this.fonts[name];
    const glyph = font?.glyphs.get(65);
    return glyph ? (glyph.y0 + glyph.y1) / 2 : this.lineHeight(name) / 2;
  }

  width(name, text, scale = 1, spacing = 1) {
    const font = this.fonts[name];
    if (!font) return 0;
    let width = 0;
    for (const ch of text) {
      const glyph = font.glyphs.get(ch.charCodeAt(0));
      if (glyph) width += Math.floor(glyph.advance * scale + 0.5) + spacing;
    }
    return width;
  }

  // align: 0 left, 1 right, 2 centred on x.
  draw(batch, m, x, y, text, name, tint, align = 0, scale = 1, spacing = 1, z = 0) {
    const font = this.fonts[name];
    if (!font || !text) return;
    const width = this.width(name, text, scale, spacing);
    let pen = align === 0 ? x : align === 1 ? x - width : x - width * 0.5;
    const round = (v) => Math.floor(v * scale + 0.5);
    for (const ch of text) {
      const glyph = font.glyphs.get(ch.charCodeAt(0));
      if (!glyph) continue;
      if (glyph.x0 < glyph.x1) {
        batch.quad(m, pen + round(glyph.x0), y + round(glyph.y0), pen + round(glyph.x1), y + round(glyph.y1), font.pages[glyph.page], glyph.uv, tint, z);
      }
      pen += round(glyph.advance) + spacing;
    }
  }
}
