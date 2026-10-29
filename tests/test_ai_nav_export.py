"""The navigation extractor (tools/zero_ai.py) on synthetic archives built
here, chunk by chunk (no retail bytes), plus counts checked against the
installed retail maps when they are present."""
import contextlib
import hashlib
import io
import json
import math
import struct
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
import zero_ai
from zero_ai import candidates, decode_archive, export_map
from zero_hash import fnv1a

GAME = ROOT / "data" / "game" / "install" / "GameData" / "Data" / "_LVL_PC"


# ---------------------------------------------------------------- builders


def chunk(tag, *parts):
    body = b"".join(parts)
    out = tag.encode("latin-1") + struct.pack("<I", len(body)) + body
    return out + b"\0" * (-len(out) % 4)


def archive(*sections):
    return chunk("ucfb", *sections)


def cstr(text):
    return text.encode("latin-1") + b"\0"


def floats(*values):
    return struct.pack(f"<{len(values)}f", *values)


def fixed(name):
    return name.encode("latin-1").ljust(16, b"\0")


def xfrm(x=(1, 0, 0), y=(0, 1, 0), z=(0, 0, 1), t=(0, 0, 0)):
    return (*x, *y, *z, *t)


HUBS = [("Hub0", (0, 0, 0), 2.0, [0, 2]),
        ("ABCDEFGHIJKLMNOP", (10, 0, 0), 3.0, [0, 1]),  # sixteen characters, no NUL
        ("Hub2", (0, 0, 10), 4.0, [1, 2])]
ARCS = [("Connection0", 0, 1, 0x1F), ("Connection1", 1, 2, 0x1F), ("SixteenCharsLong", 0, 2, 0x1F)]
W31 = 0x8178  # slot 0 weight 15, slot 1 weight 16
W30 = 0x7978  # slot 0 weight 15, slot 1 weight 15
ONLY_SLOT1 = 0xF9  # slot 1 weight 31


def base_words(n=3):
    words = {}
    for h in range(n):
        words[(h, 0, (h + 1) % n)] = W31
        words[(h, 0, (h + 2) % n)] = W30
        words[(h, 2, (h + 1) % n)] = ONLY_SLOT1
    return words


def hub_tables(h, n, words):
    return b"".join(struct.pack(f"<{n}I", *[words.get((h, t, d), 0) for d in range(n)]) for t in range(5))


def node_body(hubs, words):
    out = b""
    for h, (name, position, radius, slots) in enumerate(hubs):
        out += fixed(name) + floats(*position, radius) + bytes(list(slots) + [255] * (8 - len(slots)))
        out += hub_tables(h, len(hubs), words)
    return out


def arcs_body(arcs):
    return b"".join(fixed(name) + struct.pack("<BBI", a, b, mask) for name, a, b, mask in arcs)


def plan(hubs=HUBS, arcs=ARCS, words=None, info=None, node=None, arc_bytes=None):
    words = base_words(len(hubs)) if words is None else words
    info = struct.pack("<hhH", len(hubs), len(arcs), 5) if info is None else info
    node = node_body(hubs, words) if node is None else node
    arc_bytes = arcs_body(arcs) if arc_bytes is None else arc_bytes
    return chunk("plan", chunk("INFO", info), chunk("NODE", node), chunk("ARCS", arc_bytes))


def prop(key, value):
    return chunk("PROP", struct.pack("<I", key) + cstr(value))


def hint(code, name, matrix=xfrm(), props=(), props_first=False, xfrm_bytes=None):
    info = chunk("INFO", chunk("TYPE", cstr(code)), chunk("NAME", cstr(name)),
                 chunk("XFRM", floats(*matrix) if xfrm_bytes is None else xfrm_bytes))
    extra = b"".join(prop(key, value) for key, value in props)
    return chunk("Hint", extra + info if props_first else info + extra)


def barrier(name, matrix=xfrm(), size=(1, 0, 1), flags=0x3F, props=()):
    info = chunk("INFO", chunk("NAME", cstr(name)), chunk("XFRM", floats(*matrix)),
                 chunk("SIZE", floats(*size)), chunk("FLAG", struct.pack("<I", flags)))
    return chunk("BARR", info, *[prop(key, value) for key, value in props])


def region(kind, name, matrix=xfrm(), size=(1, 1, 1), props=()):
    info = chunk("INFO", chunk("TYPE", cstr(kind)), chunk("NAME", cstr(name)),
                 chunk("XFRM", floats(*matrix)), chunk("SIZE", floats(*size)))
    return chunk("regn", info, *[prop(key, value) for key, value in props])


def world(name, *children):
    return chunk("wrld", chunk("NAME", cstr(name)), *children)


def path(name, points, count=None, words=(0, 3), raw_prop=b"\x01\x02synthetic\0"):
    count = len(points) if count is None else count
    parts = [chunk("NAME", cstr(name)), chunk("INFO", struct.pack("<hhH", count, *words))]
    if raw_prop is not None:
        parts.append(chunk("PROP", raw_prop))
    parts.append(chunk("PNTS", b"".join(floats(*point) for point in points)))
    return chunk("path", *parts)


def paths(*items):
    return chunk("PATH", *items)


def data_entry(key, args=(), strings=b""):
    return chunk("DATA", struct.pack("<IB", key, len(args)) + struct.pack(f"<{len(args)}I", *args)
                 + struct.pack("<I", len(strings)) + strings)


def boundary(*names):
    scope = chunk("SCOP", *[data_entry(fnv1a("Path"), [fnv1a(name)]) for name in names])
    return chunk("bnd_", chunk("NAME", struct.pack("<I", fnv1a("TestConfig"))), data_entry(fnv1a("Boundary")), scope)


def square(y=0.0):
    return [(0, y, 0, 1, 0, 0, 0), (8, y, 0, 1, 0, 0, 0), (8, y, 8, 1, 0, 0, 0), (0, y, 8, 1, 0, 0, 0)]


def kinds_of(doc):
    return [warning["kind"] for warning in doc["warnings"]]


# ---------------------------------------------------------------- graph


class Graph(unittest.TestCase):
    def test_valid_graph_round_trips(self):
        doc, tables = decode_archive(archive(plan()), "syn/syn1.lvl")
        graph = doc["graph"]
        self.assertEqual(doc["map"], "syn1")
        self.assertEqual(graph["info"], [3, 3, 5])
        self.assertEqual([hub["name"] for hub in graph["hubs"]], ["Hub0", "ABCDEFGHIJKLMNOP", "Hub2"])
        self.assertEqual(graph["hubs"][1], {"name": "ABCDEFGHIJKLMNOP", "position": [10, 0, 0], "radius": 3.0,
                                            "arcs": [0, 1, 255, 255, 255, 255, 255, 255]})
        self.assertEqual(graph["arcs"], [{"name": "Connection0", "hubs": [0, 1], "mask": 0x1F},
                                         {"name": "Connection1", "hubs": [1, 2], "mask": 0x1F},
                                         {"name": "SixteenCharsLong", "hubs": [0, 2], "mask": 0x1F}])
        self.assertEqual(graph["routes"], {"file": "ai-nav.bin", "byteOffset": 0, "count": 45, "tables": 5,
                                           "order": "hub,table,destination"})
        expected = b"".join(hub_tables(h, 3, base_words()) for h in range(3))
        self.assertEqual(tables, expected)
        words = struct.unpack("<45I", tables)
        self.assertEqual(words[(0 * 5 + 0) * 3 + 1], W31)  # word index = (hub * 5 + table) * N + destination
        self.assertEqual(words[(2 * 5 + 0) * 3 + 1], W30)
        self.assertEqual(words[(1 * 5 + 2) * 3 + 2], ONLY_SLOT1)
        self.assertEqual(doc["bin"], {"file": "ai-nav.bin", "bytes": 180, "sha256": hashlib.sha256(expected).hexdigest()})
        counts = doc["counts"]
        self.assertEqual((counts["hubs"], counts["arcs"], counts["routeWords"], counts["routeNonzero"]), (3, 3, 45, 9))
        self.assertEqual(counts["weightSums"], {"31": 6, "30": 3})
        self.assertEqual(doc["warnings"], [])
        self.assertEqual(json.loads(json.dumps(doc, allow_nan=False)), doc)

    def test_lvl_wrapper_header_is_skipped(self):
        inner = plan() + world("w")
        wrapped = archive(chunk("lvl_", struct.pack("<II", fnv1a("section"), len(inner)), inner))
        plain_doc, plain_tables = decode_archive(archive(plan() + world("w")), "x.lvl")
        doc, tables = decode_archive(wrapped, "x.lvl")
        self.assertEqual(tables, plain_tables)
        self.assertEqual(doc["graph"], plain_doc["graph"])
        self.assertEqual(doc["world"], "w")

    def test_archive_without_navigation(self):
        doc, tables = decode_archive(archive(chunk("tex_", b"abcd")), "x.lvl", "none")
        self.assertIsNone(tables)
        self.assertIsNone(doc["graph"])
        self.assertIsNone(doc["bin"])
        self.assertIsNone(doc["world"])
        self.assertEqual(doc["boundary"], {"configs": [], "references": []})
        self.assertEqual(doc["counts"]["routeWords"], 0)
        self.assertEqual(doc["counts"]["weightSums"], {})

    def test_candidates_include_a_zero_low_byte(self):
        self.assertEqual(candidates(0), [])
        self.assertEqual(candidates(W31), [(0, 15), (1, 16)])
        self.assertEqual(candidates(0xF900), [(0, 0), (1, 31)])
        self.assertEqual(candidates(0x01000000), [(0, 0), (0, 0), (0, 0), (1, 0)])


class GraphFatal(unittest.TestCase):
    def fatal(self, data, pattern):
        with self.assertRaisesRegex(ValueError, pattern):
            decode_archive(data, "x.lvl")

    def test_info_must_be_six_bytes(self):
        self.fatal(archive(plan(info=struct.pack("<hh", 3, 3))), r"plan/INFO is 4 bytes, not 6")

    def test_node_size(self):
        self.fatal(archive(plan(node=node_body(HUBS, base_words())[:-4])), r"plan/NODE is 296 bytes, not 3 \* \(40 \+ 20 \* 3\) = 300")

    def test_arcs_size(self):
        self.fatal(archive(plan(arc_bytes=arcs_body(ARCS) + b"\0\0")), r"ARCS is 68 bytes, not 22 \* 3 = 66")

    def test_arc_endpoint_out_of_range(self):
        arcs = ARCS[:2] + [("Bad", 0, 3, 0x1F)]
        self.fatal(archive(plan(arcs=arcs)), r"arc 2 joins hubs 0 and 3; there are 3 hubs")

    def test_slot_arc_out_of_range(self):
        hubs = [("Hub0", (0, 0, 0), 2.0, [0, 7])] + HUBS[1:]
        self.fatal(archive(plan(hubs=hubs)), r"hub 0 slot 1 names arc 7; there are 3 arcs")

    def test_slot_arc_must_touch_its_hub(self):
        hubs = [("Hub0", (0, 0, 0), 2.0, [0, 1])] + HUBS[1:]
        self.fatal(archive(plan(hubs=hubs)), r"hub 0 slot 1 names arc 1, which joins hubs 1 and 2")

    def test_candidate_on_unused_slot(self):
        words = base_words()
        words[(0, 1, 1)] = 0xFD  # slot 5, weight 31; hub 0 has two arcs
        self.fatal(archive(plan(words=words)), r"hub 0 table 1 to hub 1 \(word 0xfd\) routes through unused slot 5")

    def test_zero_low_byte_is_a_candidate_on_slot_zero(self):
        hubs = [("Hub0", (0, 0, 0), 2.0, [255, 0, 2])] + HUBS[1:]
        words = {key: value for key, value in base_words().items() if key[0] != 0}
        words[(0, 0, 1)] = 0xF900  # byte 0 is zero: slot 0 (unused), weight 0
        self.fatal(archive(plan(hubs=hubs, words=words)), r"routes through unused slot 0")

    def test_non_finite_position(self):
        hubs = [("Hub0", (math.nan, 0, 0), 2.0, [0, 2])] + HUBS[1:]
        self.fatal(archive(plan(hubs=hubs)), r"hub 0 position holds a non-finite float")

    def test_negative_counts(self):
        self.fatal(archive(plan(hubs=[], arcs=[], info=struct.pack("<hhH", -1, 0, 5))), r"negative count: -1 hubs")

    def test_more_than_one_plan(self):
        self.fatal(archive(plan(), plan()), r"archive has 2 plan chunks")


# ---------------------------------------------------------------- graph warnings


class GraphWarnings(unittest.TestCase):
    def test_route_capability_is_kept(self):
        arcs = [("Connection0", 0, 1, 0x01)] + ARCS[1:]
        words = base_words()
        words[(0, 1, 1)] = 0xF8  # table 1 (bit 2) through slot 0, arc 0, whose mask is 1
        doc, tables = decode_archive(archive(plan(arcs=arcs, words=words)), "x.lvl")
        self.assertEqual(kinds_of(doc), ["routeCapability"])
        warning = doc["warnings"][0]
        self.assertEqual({k: warning[k] for k in ("hub", "table", "destination", "slot", "arc", "arcMask", "word")},
                         {"hub": 0, "table": 1, "destination": 1, "slot": 0, "arc": 0, "arcMask": 1, "word": 0xF8})
        self.assertEqual(struct.unpack_from("<I", tables, 4 * ((0 * 5 + 1) * 3 + 1))[0], 0xF8)
        self.assertEqual(doc["counts"]["routeNonzero"], 10)

    def test_route_weight_sum(self):
        words = base_words()
        words[(0, 3, 1)] = 0x5150  # slot 0 weight 10, slot 1 weight 10
        doc, _ = decode_archive(archive(plan(words=words)), "x.lvl")
        self.assertEqual(kinds_of(doc), ["routeWeightSum"])
        self.assertEqual(doc["warnings"][0]["sum"], 20)
        self.assertEqual(doc["counts"]["weightSums"], {"31": 6, "30": 3, "20": 1})

    def test_route_hang_forms(self):
        words = base_words()
        words[(0, 1, 1)] = 0x0100  # two candidates, both weight 0
        words[(0, 1, 2)] = 0xF0  # one candidate, slot 0 weight 30
        words[(0, 3, 1)] = 0x8078  # slot 0 weight 15, slot 0 again weight 16
        words[(0, 3, 2)] = 0xF8  # slot 0 weight 31: the fallback handles it
        doc, _ = decode_archive(archive(plan(words=words)), "x.lvl")
        self.assertEqual(kinds_of(doc), ["routeWeightSum", "routeHang", "routeHang", "routeHang"])
        hangs = [(w["table"], w["destination"], w["word"], w["reason"]) for w in doc["warnings"] if w["kind"] == "routeHang"]
        self.assertEqual(hangs, [(1, 1, 0x0100, "zeroWeights"), (1, 2, 0xF0, "singleSlot"), (3, 1, 0x8078, "singleSlot")])

    def test_hub_slot_gap(self):
        hubs = [("Hub0", (0, 0, 0), 2.0, [0, 255, 2])] + HUBS[1:]
        words = {key: value for key, value in base_words().items() if key[0] != 0}
        words[(0, 0, 1)] = 0xF8  # slot 0
        words[(0, 0, 2)] = 0xFA  # slot 2
        doc, _ = decode_archive(archive(plan(hubs=hubs, words=words)), "x.lvl")
        self.assertEqual(kinds_of(doc), ["hubSlotGap"])
        self.assertEqual(doc["warnings"][0]["hub"], 0)
        self.assertEqual(doc["warnings"][0]["arcs"], [0, 255, 2, 255, 255, 255, 255, 255])

    def test_hub_mask_differs(self):
        arcs = ARCS + [("Extra", 0, 1, 0x20)]  # touches hub 0, listed only by hub 1
        hubs = [HUBS[0], ("Hub1", (10, 0, 0), 3.0, [0, 1, 3]), HUBS[2]]
        doc, _ = decode_archive(archive(plan(hubs=hubs, arcs=arcs)), "x.lvl")
        self.assertEqual(kinds_of(doc), ["hubMaskDiffers"])
        warning = doc["warnings"][0]
        self.assertEqual((warning["hub"], warning["slotMask"], warning["touchingMask"]), (0, 0x1F, 0x3F))

    def test_zero_length_arc(self):
        hubs = HUBS[:2] + [("Hub2", (10, 0, 0), 4.0, [1, 2])]  # hub 2 sits on hub 1
        doc, _ = decode_archive(archive(plan(hubs=hubs)), "x.lvl")
        self.assertEqual(kinds_of(doc), ["zeroLengthArc"])
        self.assertEqual((doc["warnings"][0]["arc"], doc["warnings"][0]["hubs"]), (1, [1, 2]))


# ---------------------------------------------------------------- world, paths, boundary


MODE, PRIMARY = fnv1a("Mode"), fnv1a("PrimaryStance")


class World(unittest.TestCase):
    def test_hint_kinds_properties_and_type_warning(self):
        props = [(MODE, "0"), (PRIMARY, "3"), (MODE, "3"), (0x12345678, "x")]
        hints = [hint(code, f"hint{code}", xfrm(t=(1, 2, int(code))), props=props if code == "2" else ())
                 for code in "0123456"]
        doc, _ = decode_archive(archive(world("synthetic", *hints)), "x.lvl")
        self.assertEqual(doc["world"], "synthetic")
        self.assertNotIn("worlds", doc)
        self.assertEqual([(h["type"], h["kind"], h["bit"]) for h in doc["hints"]],
                         [("0", "snipe", 1), ("1", "patrol", 2), ("2", "cover", 4), ("3", None, 0),
                          ("4", "jetjump", 8), ("5", "minefield", 16), ("6", "land", 32)])
        self.assertEqual(doc["hints"][2]["properties"], [["ec6ee012", "Mode", "0"], ["bf0f1cd1", "PrimaryStance", "3"],
                                                         ["ec6ee012", "Mode", "3"], ["12345678", None, "x"]])
        self.assertEqual(doc["hints"][3]["xfrm"], [1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 2, 3])
        self.assertEqual(doc["counts"]["hints"], {"snipe": 1, "patrol": 1, "cover": 1, "jetjump": 1, "minefield": 1,
                                                  "land": 1, "unsupported": 1})
        self.assertEqual(kinds_of(doc), ["hintType"])
        self.assertEqual((doc["warnings"][0]["hint"], doc["warnings"][0]["type"]), (3, "3"))

    def test_jet_forward(self):
        doc, _ = decode_archive(archive(world("w", hint("4", "jet", xfrm(z=(0, 0, 2))), hint("4", "ok"))), "x.lvl")
        self.assertEqual(kinds_of(doc), ["jetForward"])
        self.assertEqual((doc["warnings"][0]["hint"], doc["warnings"][0]["length"]), (0, 2.0))
        self.assertEqual(doc["hints"][0]["xfrm"][6:9], [0, 0, 2])

    def test_barrier_is_raw_and_basis_is_checked(self):
        layer = fnv1a("Layer")
        good = barrier("barrier0", xfrm(t=(1, 2, 5)), size=(1.5, 0, 2.5), flags=0x3F, props=[(layer, "1"), (layer, "1")])
        skewed = barrier("barrier1", xfrm(x=(2, 0, 0)))
        doc, _ = decode_archive(archive(world("w", good, skewed)), "x.lvl")
        self.assertEqual(doc["barriers"][0], {"name": "barrier0", "xfrm": [1, 0, 0, 0, 1, 0, 0, 0, 1, 1, 2, 5],
                                              "size": [1.5, 0, 2.5], "flags": 0x3F,
                                              "properties": [["07a640f6", "Layer", "1"], ["07a640f6", "Layer", "1"]]})
        self.assertEqual(kinds_of(doc), ["barrierBasis"])
        self.assertEqual((doc["warnings"][0]["barrier"], doc["warnings"][0]["xLength"]), (1, 2.0))
        self.assertEqual(doc["counts"]["barriers"], 2)

    def test_regions_keep_duplicates_and_properties(self):
        layer = fnv1a("Layer")
        doc, _ = decode_archive(archive(world(
            "w", region("box", "templecapture", props=[(layer, "2")]),
            region("cylinder", "templecapture", xfrm(t=(4, 5, 6)), size=(3, 2, 3)))), "x.lvl")
        self.assertEqual([(r["type"], r["name"]) for r in doc["regions"]], [("box", "templecapture"), ("cylinder", "templecapture")])
        self.assertEqual(doc["regions"][0]["properties"], [["07a640f6", "Layer", "2"]])
        self.assertEqual(doc["regions"][1], {"type": "cylinder", "name": "templecapture",
                                             "xfrm": [1, 0, 0, 0, 1, 0, 0, 0, 1, 4, 5, 6], "size": [3, 2, 3], "properties": []})

    def test_worlds_concatenate_in_archive_order(self):
        doc, _ = decode_archive(archive(world("first", hint("2", "a")), world("second", hint("0", "b"), hint("3", "c"))), "x.lvl")
        self.assertEqual(doc["world"], "first")
        self.assertEqual(doc["worlds"], ["first", "second"])
        self.assertEqual([h["name"] for h in doc["hints"]], ["a", "b", "c"])
        self.assertEqual(doc["warnings"][0]["hint"], 2)

    def test_paths_keep_points_and_quaternion_order(self):
        doc, _ = decode_archive(archive(paths(path("CP1Spawn", [(1, 2, 3, 0.5, 0.25, 0.125, 0.0625)], words=(-2, 3)),
                                              path("empty", [], raw_prop=None))), "x.lvl")
        self.assertEqual(doc["paths"][0], {"name": "CP1Spawn", "info": [1, -2, 3],
                                           "points": [[1, 2, 3, 0.5, 0.25, 0.125, 0.0625]],
                                           "prop": b"\x01\x02synthetic\0".hex()})
        self.assertEqual(doc["paths"][1], {"name": "empty", "info": [0, 0, 3], "points": [], "prop": None})
        self.assertEqual((doc["counts"]["paths"], doc["counts"]["pathPoints"]), (2, 1))

    def test_boundary_resolves_its_path(self):
        doc, _ = decode_archive(archive(paths(path("jungle1", square()), path("Boundary", square()), path("CP1Spawn", square())),
                                        boundary("Boundary")), "x.lvl")
        config = doc["boundary"]["configs"][0]
        self.assertEqual(config["name"], f"{fnv1a('TestConfig'):08x}")
        self.assertEqual(config["entries"], [{
            "key": "54eacca7", "keyName": "Boundary", "args": [], "strings": None,
            "children": [{"key": "84874d36", "keyName": "Path", "args": [f"{fnv1a('Boundary'):08x}"],
                          "strings": None, "children": []}]}])
        self.assertEqual(doc["boundary"]["references"], [{"hash": f"{fnv1a('Boundary'):08x}", "path": 1}])
        self.assertEqual(doc["warnings"], [])

    def test_boundary_entry_strings_and_raw_arguments(self):
        entry = data_entry(fnv1a("Other"), [0x3F800000], b"ab\0\0")  # 1.0f stays raw hex
        doc, _ = decode_archive(archive(chunk("bnd_", entry)), "x.lvl")
        self.assertEqual(doc["boundary"]["configs"], [{"name": None, "entries": [
            {"key": f"{fnv1a('Other'):08x}", "keyName": None, "args": ["3f800000"], "strings": "61620000", "children": []}]}])
        self.assertEqual(doc["boundary"]["references"], [])

    def test_boundary_path_missing(self):
        doc, _ = decode_archive(archive(paths(path("boundary1", square())), boundary("Nowhere")), "x.lvl")
        self.assertEqual(doc["boundary"]["references"], [{"hash": f"{fnv1a('Nowhere'):08x}", "path": None}])
        self.assertEqual(kinds_of(doc), ["boundaryPath"])
        self.assertEqual(doc["warnings"][0]["hash"], f"{fnv1a('Nowhere'):08x}")

    def test_duplicate_path_hash_resolves_to_the_first(self):
        # The hash folds case, so these two names collide.
        doc, _ = decode_archive(archive(paths(path("CP1Spawn", square()), path("cp1spawn", square(1))),
                                        boundary("CP1SPAWN")), "x.lvl")
        self.assertEqual(doc["boundary"]["references"][0]["path"], 0)
        self.assertEqual(kinds_of(doc), ["duplicatePath"])
        self.assertEqual(doc["warnings"][0]["paths"], [0, 1])


class WorldFatal(unittest.TestCase):
    def fatal(self, data, pattern):
        with self.assertRaisesRegex(ValueError, pattern):
            decode_archive(data, "x.lvl")

    def test_chunk_overrunning_its_container(self):
        broken = chunk("Hint", b"INFO" + struct.pack("<I", 200) + bytes(16))
        self.fatal(archive(world("w", broken)), r"ucfb/wrld/Hint: INFO at \+0 \(200 bytes\) overruns its container by 184 bytes")
        whole = archive(plan())
        self.fatal(whole[:-8], r"archive: ucfb at \+0 .* overruns its container by 8 bytes")

    def test_every_byte_is_accounted_for(self):
        good = hint("2", "a")
        stray = good[:4] + struct.pack("<I", len(good) - 8 + 4) + good[8:] + bytes(4)
        self.fatal(archive(world("w", stray)), r"ucfb/wrld/Hint: 4 stray bytes")
        name = chunk("NAME", b"ab")  # 2 bytes, then 2 bytes of padding
        self.fatal(archive(chunk("wrld", name[:10] + b"\0\x01")), r"nonzero padding after NAME")

    def test_hint_prop_before_info(self):
        self.fatal(archive(world("w", hint("2", "a", props=[(MODE, "1")], props_first=True))),
                   r"ucfb/wrld/Hint/PROP comes before its INFO")

    def test_xfrm_must_be_48_bytes(self):
        self.fatal(archive(world("w", hint("2", "a", xfrm_bytes=floats(*xfrm()[:11])))), r"Hint/INFO/XFRM is 44 bytes, not 48")

    def test_prop_must_be_hash_and_terminated_string(self):
        bad = chunk("Hint", chunk("INFO", chunk("TYPE", cstr("2")), chunk("NAME", cstr("a")), chunk("XFRM", floats(*xfrm()))),
                    chunk("PROP", struct.pack("<I", MODE) + b"12"))
        self.fatal(archive(world("w", bad)), r"PROP is not a name hash and a NUL-terminated string")

    def test_pnts_count_mismatch(self):
        self.fatal(archive(paths(path("p", square()[:2], count=3))), r"PATH/path/PNTS holds 2 points; its INFO says 3")
        self.fatal(archive(paths(path("p", [], count=-1))), r"negative point count")

    def test_bnd_data_extent(self):
        good = data_entry(fnv1a("Path"), [1])
        body = good[8:8 + 13] + bytes(4)  # four bytes past the empty string table
        self.fatal(archive(chunk("bnd_", chunk("DATA", body))),
                   r"bnd_/DATA: 1 arguments and a 0-byte string table fill 13 bytes, not its 17")
        self.fatal(archive(chunk("bnd_", chunk("SCOP", data_entry(PRIMARY)))), r"bnd_/SCOP does not follow a DATA")


# ---------------------------------------------------------------- writing


class Export(unittest.TestCase):
    def test_export_writes_both_files_and_nothing_on_error(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            source = tmp / "syn1.lvl"
            source.write_bytes(archive(plan(), world("w", hint("2", "a")), paths(path("p", square()))))
            doc = export_map("syn1", archive=source, out=tmp / "maps")
            folder = tmp / "maps" / "syn1"
            self.assertEqual(json.loads((folder / "ai-nav.json").read_text()), doc)
            self.assertEqual((folder / "ai-nav.bin").read_bytes(), b"".join(hub_tables(h, 3, base_words()) for h in range(3)))
            self.assertEqual(doc["source"]["bytes"], source.stat().st_size)
            self.assertEqual(doc["source"]["sha256"], hashlib.sha256(source.read_bytes()).hexdigest())
            self.assertNotIn(" ", (folder / "ai-nav.json").read_text()[:40])

            before = (folder / "ai-nav.json").read_bytes()
            source.write_bytes(archive(plan(info=b"\0\0")))
            with self.assertRaises(ValueError):
                export_map("syn1", archive=source, out=tmp / "maps")
            self.assertEqual((folder / "ai-nav.json").read_bytes(), before)

            source.write_bytes(archive(world("w")))  # no plan: no bin, and a stale one goes
            doc = export_map("syn1", archive=source, out=tmp / "maps")
            self.assertIsNone(doc["bin"])
            self.assertFalse((folder / "ai-nav.bin").exists())

    def test_cli_summary_and_exit_status(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            good, bad = tmp / "good.lvl", tmp / "bad.lvl"
            good.write_bytes(archive(plan(), boundary("Boundary"), paths(path("Boundary", square()))))
            bad.write_bytes(archive(plan(arc_bytes=b"")))
            out, err = io.StringIO(), io.StringIO()
            with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
                self.assertEqual(zero_ai.main(["syn2", "--archive", str(good), "--out", str(tmp / "maps")]), 0)
                self.assertEqual(zero_ai.main(["syn3", "--archive", str(bad), "--out", str(tmp / "maps")]), 1)
            self.assertIn("syn2: 3 hubs, 3 arcs, 9/45 routes (31: 6, 30: 3)", out.getvalue())
            self.assertIn("boundary -> Boundary; warnings: none", out.getvalue())
            self.assertIn("syn3: ucfb/plan/ARCS is 0 bytes, not 22 * 3 = 66", err.getvalue())
            self.assertFalse((tmp / "maps" / "syn3").exists())


# ---------------------------------------------------------------- retail


class Retail(unittest.TestCase):
    def decode(self, name):
        source = GAME / name[:3].upper() / f"{name}.lvl"
        if not source.exists():
            self.skipTest(f"{source} is not installed")
        doc, tables = decode_archive(source.read_bytes(), f"{name[:3].upper()}/{name}.lvl", name)
        self.assertEqual(len(tables), 4 * doc["counts"]["routeWords"])
        self.assertNotIn("routeCapability", kinds_of(doc))
        for hub in doc["graph"]["hubs"]:
            self.assertTrue(all(math.isfinite(v) for v in hub["position"] + [hub["radius"]]))
        return doc

    def boundary_names(self, doc):
        return [doc["paths"][ref["path"]]["name"] for ref in doc["boundary"]["references"]]

    def test_nab2(self):
        doc = self.decode("nab2")
        counts = doc["counts"]
        self.assertEqual(doc["graph"]["info"], [86, 137, 5])
        self.assertEqual((counts["hubs"], counts["arcs"]), (86, 137))
        self.assertEqual({k: v for k, v in counts["hints"].items() if v}, {"cover": 87, "jetjump": 5})
        self.assertEqual(len(doc["hints"]), 92)
        self.assertEqual((counts["barriers"], counts["paths"], counts["regions"]), (374, 8, 16))
        self.assertEqual(counts["weightSums"], {"31": 10259, "30": 23})
        self.assertEqual(self.boundary_names(doc), ["Boundary"])

    def test_bes2(self):
        doc = self.decode("bes2")
        counts = doc["counts"]
        self.assertEqual(doc["graph"]["info"], [119, 138, 5])
        self.assertEqual({k: v for k, v in counts["hints"].items() if v}, {"snipe": 19, "cover": 11, "minefield": 1})
        self.assertEqual((counts["barriers"], counts["paths"], counts["regions"]), (514, 7, 33))
        self.assertEqual(counts["weightSums"], {"31": 70206, "30": 4})
        self.assertEqual(self.boundary_names(doc), ["boundary1"])


if __name__ == "__main__":
    unittest.main()
