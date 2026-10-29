// The game's bitmap fonts on a 2D canvas, for what the front end draws
// outside WebGL (the click gate, the loading screen). The glyph tables are
// common.json's (per glyph its page, advance, box from the pen and the
// line's top, and its place on the page), measured in interface units, as
// src/hud.js draws them; the pages are the 4x upscales where there are some.
// The pages are white, so a colour is the page tinted.

export class CanvasFonts {
  // fonts: common.json's fonts; pages: page name -> its file.
  constructor({ fonts = {}, pages = {} } = {}) {
    this.fonts = {};
    for (const [name, font] of Object.entries(fonts || {})) {
      const glyphs = new Map((font.glyphs || []).map(([code, page, advance, x0, x1, y0, y1, u0, v0, u1, v1]) =>
        [code, { page, advance, x0, x1, y0, y1, uv: [u0, v0, u1, v1] }]));
      this.fonts[name] = { lineHeight: font.lineHeight, pages: font.pages || [], glyphs };
    }
    this.urls = pages || {};
    this.images = new Map(); // page name -> Promise of its image (null if it won't load)
    this.loaded = new Map(); // page name -> its image, once loaded
    this.tinted = new Map(); // "page|colour" -> a tinted copy
  }

  // Start loading the pages of these fonts; resolves when they're in (or
  // have failed).
  load(names = Object.keys(this.fonts)) {
    const pages = names.flatMap((name) => this.fonts[name]?.pages || []);
    return Promise.all(pages.map((page) => this.page(page)));
  }

  page(name) {
    if (!this.images.has(name)) {
      const url = this.urls[name];
      const promise = url ? fetch(url)
        .then((r) => (r.ok ? r.blob() : Promise.reject(new Error(r.statusText))))
        .then((blob) => createImageBitmap(blob))
        .then((image) => {
          this.loaded.set(name, image);
          return image;
        })
        .catch(() => null) : Promise.resolve(null);
      this.images.set(name, promise);
    }
    return this.images.get(name);
  }

  has(name) {
    const font = this.fonts[name];
    return Boolean(font && font.pages.length && font.pages.every((page) => this.loaded.has(page)));
  }

  lineHeight(name) {
    return this.fonts[name]?.lineHeight ?? 18;
  }

  // Width in interface units, as the HUD measures it: each advance rounded,
  // plus the spacing.
  width(name, text, scale = 1, spacing = 1) {
    const font = this.fonts[name];
    if (!font) return 0;
    let width = 0;
    for (const ch of String(text)) {
      const glyph = font.glyphs.get(ch.charCodeAt(0));
      if (glyph) width += Math.floor(glyph.advance * scale + 0.5) + spacing;
    }
    return width;
  }

  // Draw text with its line's top at y. align: 0 left, 1 right, 2 centred on
  // x. color: a CSS colour, or null for the pages' own white.
  draw(ctx, x, y, text, name, { color = null, align = 0, scale = 1, spacing = 1 } = {}) {
    const font = this.fonts[name];
    if (!font || !text) return false;
    const width = this.width(name, text, scale, spacing);
    let pen = align === 0 ? x : align === 1 ? x - width : x - width * 0.5;
    const round = (v) => Math.floor(v * scale + 0.5);
    let drawn = false;
    for (const ch of String(text)) {
      const glyph = font.glyphs.get(ch.charCodeAt(0));
      if (!glyph) continue;
      const image = this.image(font.pages[glyph.page], color);
      if (image && glyph.x0 < glyph.x1) {
        const [u0, v0, u1, v1] = glyph.uv;
        const w = image.width;
        const h = image.height;
        ctx.drawImage(image, u0 * w, v0 * h, (u1 - u0) * w, (v1 - v0) * h,
          pen + round(glyph.x0), y + round(glyph.y0), round(glyph.x1) - round(glyph.x0), round(glyph.y1) - round(glyph.y0));
        drawn = true;
      }
      pen += round(glyph.advance) + spacing;
    }
    return drawn;
  }

  // A page, tinted when a colour is asked for.
  image(page, color) {
    const image = this.loaded.get(page);
    if (!image || !color) return image || null;
    const key = `${page}|${color}`;
    let tinted = this.tinted.get(key);
    if (!tinted) {
      // A worker has no document, but has OffscreenCanvas.
      tinted = typeof OffscreenCanvas === "function" ? new OffscreenCanvas(image.width, image.height) : document.createElement("canvas");
      tinted.width = image.width;
      tinted.height = image.height;
      const c = tinted.getContext("2d");
      c.drawImage(image, 0, 0);
      c.globalCompositeOperation = "source-in";
      c.fillStyle = color;
      c.fillRect(0, 0, tinted.width, tinted.height);
      this.tinted.set(key, tinted);
    }
    return tinted;
  }
}
