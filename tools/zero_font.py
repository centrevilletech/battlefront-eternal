"""Bitmap fonts (`font` chunks in core.lvl): the HUD's text.

A font chunk holds NAME; HEAD, the glyph count (u16), page count (u8), line
height (u8) and baseline (u8); FTEX, the glyph pages as textures; and FBOD,
24 bytes per glyph: the character code (u16), its page (u8), advance (u8),
its box x0, x1, y0, y1 in pixels from the pen and the line's top (u8 each),
then u0, u1, v0, v1 (f32) on the page.
"""

import struct

from ucfb import child, chunks, cstr_at, iter_archives
from zero_texture import decode_texture

GLYPH = 24


def collect_fonts(data):
    """Fonts by name, each {lineHeight, baseline, pages, glyphs} with glyphs as
    [code, page, advance, x0, x1, y0, y1, u0, v0, u1, v1]; and the page
    textures, name -> (width, height, rgba)."""
    fonts = {}
    pages = {}
    for start, end in iter_archives(data):
        for tag, body, stop in chunks(data, start, end):
            if tag != b"font":
                continue
            name = child(data, body, stop, b"NAME")
            head = child(data, body, stop, b"HEAD")
            ftex = child(data, body, stop, b"FTEX")
            fbod = child(data, body, stop, b"FBOD")
            if not (name and head and ftex and fbod):
                continue
            count, _, line_height, baseline = struct.unpack_from("<HBBB", data, head[0])
            names = []
            for t, b, e in chunks(data, *ftex):
                if t != b"tex_":
                    continue
                parsed = decode_texture(data, b, e)
                if parsed and parsed[1]:
                    names.append(parsed[0])
                    pages[parsed[0]] = parsed[1]
            glyphs = []
            for i in range(min(count, (fbod[1] - fbod[0]) // GLYPH)):
                at = fbod[0] + i * GLYPH
                code, page, advance, x0, x1, y0, y1 = struct.unpack_from("<HBBBBBB", data, at)
                u0, u1, v0, v1 = struct.unpack_from("<4f", data, at + 8)
                glyphs.append([code, page, advance, x0, x1, y0, y1, u0, v0, u1, v1])
            fonts[cstr_at(data, *name)] = {"lineHeight": line_height, "baseline": baseline, "pages": names, "glyphs": glyphs}
    return fonts, pages
