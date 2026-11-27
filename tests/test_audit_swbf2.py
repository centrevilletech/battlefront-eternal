"""tools/audit-swbf2.py: chunk inventory and header/version inspection over
synthetic ucfb archives (including mixed top-level + lvl_ sub-archives).
Nothing retail."""

import importlib.util
import struct
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "tools"))
sys.path.insert(0, str(HERE))
from test_addon_import import chunk, ucfb, write  # noqa: E402
from zero_hash import fnv1a  # noqa: E402


def _load_audit():
    spec = importlib.util.spec_from_file_location("audit_swbf2", HERE.parent / "tools" / "audit-swbf2.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


audit_swbf2 = _load_audit()


def lvl(name, *inner):
    body = b"".join(inner)
    return chunk(b"lvl_", struct.pack("<II", fnv1a(name), len(body)) + body)


class AuditSwbf2Tests(unittest.TestCase):
    def test_mixed_root_and_nested_lvl_chunks_and_headers(self):
        # Synthetic map with root base wrld, void tern, 72-byte modl INFO, 25-byte MTRL,
        # coll with MASK, Lua 5.0 float scr_, and nested lvl_ layer wrld.
        modl_chunk = chunk(
            b"modl",
            chunk(b"NAME", b"test_mesh\0")
            + chunk(b"INFO", bytes(72))
            + chunk(b"segm", chunk(b"MTRL", bytes(25))),
        )
        coll_chunk = chunk(b"coll", chunk(b"NAME", b"test_mesh\0") + chunk(b"MASK", b"\x01"))
        tern_void = chunk(b"tern", chunk(b"NAME", b"spa\0"))
        base_wrld = chunk(b"wrld", chunk(b"NAME", b"spa\0") + chunk(b"TNAM", b"spa\0"))
        layer_wrld = chunk(b"wrld", chunk(b"NAME", b"spa_obj\0"))
        lua50_body = b"\x1bLua\x50\x01\x04\x04\x04\x06\x08\x09\x09\x04" + bytes(4)
        scr_chunk = chunk(b"scr_", chunk(b"NAME", b"test\0") + chunk(b"BODY", lua50_body))
        archive = ucfb(modl_chunk, coll_chunk, tern_void, base_wrld, scr_chunk, lvl("spa_obj", layer_wrld))

        with tempfile.TemporaryDirectory() as tmp:
            path = write(Path(tmp) / "spa.lvl", archive)
            rep = audit_swbf2.audit_archive(path)
            rep["archive"] = "spa.lvl"

        self.assertEqual(rep["subArchives"], 1)
        chunks_map = rep["chunks"]
        self.assertEqual(chunks_map["wrld"]["count"], 2)
        self.assertEqual(chunks_map["wrld"]["headers"]["baseWorlds"], 1)
        self.assertEqual(chunks_map["wrld"]["headers"]["layerWorlds"], 1)
        self.assertEqual(chunks_map["modl"]["headers"]["infoSizes"], [72])
        self.assertEqual(chunks_map["modl"]["headers"]["mtrlSizes"], [25])
        self.assertEqual(chunks_map["coll"]["headers"]["withMask"], 1)
        self.assertEqual(chunks_map["tern"]["headers"]["headers"], ["void(12B)"])
        self.assertEqual(chunks_map["scr_"]["headers"]["luaHeaders"], ["ver=0x50 num=4B"])

        merged = audit_swbf2.merge_audits([rep])
        table = audit_swbf2.format_table(merged)
        self.assertIn("`modl`", table)
        self.assertIn("differences", table)


if __name__ == "__main__":
    unittest.main()
