"""Localization (`Locl` in core.lvl): display text for keys like level.bespin2.CP1.

A Locl chunk holds NAME (the language) and BODY: entries of a u32 key hash, a
u16 entry size including this 6-byte header, then UTF-16 text. Keys are hashed
like property names (zero_hash.fnv1a).
"""

import struct

from ucfb import child, chunks, cstr_at
from zero_hash import fnv1a


def localization(data, language="english"):
    """key hash -> text for one language."""
    table = {}

    def walk(start, end):
        for tag, b, e in chunks(data, start, end):
            if tag == b"lvl_":
                walk(b + 8, e)
            elif tag == b"Locl":
                name = child(data, b, e, b"NAME")
                body = child(data, b, e, b"BODY")
                if not name or not body or cstr_at(data, *name).lower() != language:
                    continue
                at, stop = body
                while at + 6 <= stop:
                    key, size = struct.unpack_from("<IH", data, at)
                    if size < 6:
                        break
                    table[key] = data[at + 6:at + size].decode("utf-16-le", "replace").rstrip("\0")
                    at += size
            elif tag == b"ucfb":
                walk(b, e)

    walk(0, len(data))
    return table


def localize(table, key):
    return table.get(fnv1a(key))
