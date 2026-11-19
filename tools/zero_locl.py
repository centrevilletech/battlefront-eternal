"""Localization (`Locl` in core.lvl): display text for keys like level.bespin2.CP1.

A Locl chunk holds NAME (the language) and BODY: entries of a u32 key hash, a
u16 entry size including this 6-byte header, then UTF-16 text. Keys are hashed
like property names (zero_hash.fnv1a). An archive holds one chunk per
language; tools/zero_language.py says which the tools read.
"""

import struct

from ucfb import child, chunks, cstr_at
from zero_hash import fnv1a


def tables(data):
    """Each language's text in an archive, in file order: (its name in small
    letters, its BODY's start and end)."""
    found = []

    def walk(start, end):
        for tag, b, e in chunks(data, start, end):
            if tag == b"lvl_":
                walk(b + 8, e)
            elif tag == b"Locl":
                name = child(data, b, e, b"NAME")
                body = child(data, b, e, b"BODY")
                if name and body:
                    found.append((cstr_at(data, *name).lower(), *body))
            elif tag == b"ucfb":
                walk(b, e)

    walk(0, len(data))
    return found


def languages(data):
    """The languages an archive has text in, in file order."""
    return list(dict.fromkeys(name for name, _, _ in tables(data)))


def localization(data, language="english"):
    """key hash -> text for one language."""
    table = {}
    for name, at, stop in tables(data):
        if name != language:
            continue
        while at + 6 <= stop:
            key, size = struct.unpack_from("<IH", data, at)
            if size < 6:
                break
            table[key] = data[at + 6:at + size].decode("utf-16-le", "replace").rstrip("\0")
            at += size
    return table


def localize(table, key):
    return table.get(fnv1a(key))
