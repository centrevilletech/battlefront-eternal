"""Bounds checks shared by readers of 2005 interface model segments."""
import struct

from ucfb import align4


class ConfigError(ValueError):
    """An invalid battle interface resource."""


class ArchiveBudget:
    def __init__(self):
        self.nodes = 100_000


def checked_chunks(data, start, end, budget):
    pos = start
    while pos < end:
        if end - pos < 8:
            if end - pos > 3 or any(data[pos:end]):
                raise ConfigError("invalid tail after chunks")
            return
        tag = data[pos:pos + 4]
        budget.nodes -= 1
        if budget.nodes < 0:
            raise ConfigError("too many archive nodes")
        if not all(32 <= byte < 127 for byte in tag):
            raise ConfigError(f"bad chunk tag at {pos}")
        size = struct.unpack_from("<I", data, pos + 4)[0]
        body = pos + 8
        stop = body + size
        if stop > end:
            raise ConfigError(f"chunk {tag!r} at {pos} runs past its parent")
        pos = min(align4(stop), end)
        if any(data[stop:pos]):
            raise ConfigError(f"nonzero alignment after {tag!r}")
        yield tag, body, stop
