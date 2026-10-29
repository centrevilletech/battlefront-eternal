"""Name hashes used inside munged Zero engine data.

Animation banks key bones and clips by a CRC-32 (MSB first, poly 0x04C11DB7,
lowercased input). Entity class and instance properties, and localization
keys, are keyed by a 32-bit FNV-1a over each byte OR 0x20. That folds letters
to lower case, but also turns "_" into 0x7f, so it is not the hash of the
lowercased name for names with "_" (or "@[\\]^").
"""


def _crc_table():
    table = []
    for i in range(256):
        c = i << 24
        for _ in range(8):
            c = ((c << 1) ^ 0x04C11DB7) if c & 0x80000000 else (c << 1)
            c &= 0xFFFFFFFF
        table.append(c)
    return table


_TABLE = _crc_table()


def crc(name):
    r = 0xFFFFFFFF
    for ch in name.lower().encode("latin1"):
        r = ((r << 8) & 0xFFFFFFFF) ^ _TABLE[((r >> 24) ^ ch) & 0xFF]
    return r ^ 0xFFFFFFFF


def fnv1a(name):
    h = 2166136261
    for ch in name.encode("latin1"):
        h ^= ch | 0x20
        h = (h * 16777619) & 0xFFFFFFFF
    return h
