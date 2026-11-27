#!/usr/bin/env python3
"""Audit 2005 (Star Wars: Battlefront II) .lvl archives: chunk types, counts,
sizes, header/version fields, and reader compatibility status compared to the
2004 preparation tools.

    BFE_FAMILY=swbf2 BFE_GAME_DIR=<install> python3 tools/audit-swbf2.py
    python3 tools/audit-swbf2.py path/to/tat2.lvl [--json out.json]
"""

import argparse
import json
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from ucfb import child, children_of, chunks, cstr_at, u32  # noqa: E402
from zero_hash import fnv1a  # noqa: E402
import zero_config  # noqa: E402
import zero_paths  # noqa: E402
import zero_texture  # noqa: E402

DEFAULT_ARCHIVES = (
    "tat/tat2.lvl",
    "spa/spa6.lvl",
    "side/all.lvl",
    "side/imp.lvl",
    "side/rep.lvl",
    "side/cis.lvl",
    "side/jed.lvl",
    "side/tur.lvl",
    "ingame.lvl",
    "common.lvl",
    "core.lvl",
    "load/tat2.lvl",
)

SOUND_BANK_LIST_TAG = struct.pack("<I", fnv1a("SoundBankList"))

# Compatibility status of each chunk type relative to the 2004 tools:
#   ("unchanged" | "differences" | "no_reader", short explanation)
CHUNK_STATUS = {
    "Locl": ("unchanged", "zero_locl.py reads the UTF-16 localization table unchanged"),
    "font": ("unchanged", "zero_font.py reads HEAD/FTEX/FBOD glyph tables and pages unchanged"),
    "skel": ("unchanged", "zero_model.parse_skeleton reads INFO/NAME/PRNT/XFRM unchanged"),
    "fx__": ("unchanged", "zero_fx.py / zero_config.py read NAME hash + DATA/SCOP tree unchanged"),
    "sky_": ("unchanged", "zero_config.py reads sky/dome/sun/water sections unchanged"),
    "bnd_": ("differences", "config tree; Path holds a 32-bit FNV id"),
    "snd_": ("unchanged", "zero_sound.py reads DATA/SCOP sound config tree unchanged"),
    "zaa_": ("unchanged", "zero_anim.parse_bank reads SMNA/MINA/TNJA/TADA banks unchanged (new clip names and per-side/map locations)"),
    "entc": ("differences", "same TYPE/BASE/PROP layout; new 2005 ODF property keys and per-class lvl_ packaging"),
    "wpnc": ("differences", "same TYPE/BASE/PROP layout; new 2005 weapon ODF property keys"),
    "ordc": ("differences", "same TYPE/BASE/PROP layout; new 2005 ordnance ODF property keys"),
    "expc": ("differences", "same TYPE/BASE/PROP layout; new 2005 explosion ODF property keys"),
    "modl": ("differences", "INFO is 72 bytes (4 leading ints vs 3 in 2004); segm MTRL is 24+ bytes with different flag bits and attached-light string"),
    "tex_": ("differences", "same FMT_/FACE/LVL_ container; adds D3DFMT_L8 (50) and D3DFMT_A8L8 (51) surfaces alongside DXT1/DXT3/RGB formats"),
    "tern": ("differences", "same INFO/PCHS/PTCH layout on ground maps; space maps carry a 16-byte void tern (NAME only, no INFO/PCHS)"),
    "coll": ("differences", "same NAME/NODE/INFO/POSI/TREE layout; adds optional 1-byte MASK collision filter chunk"),
    "prim": ("differences", "same INFO/NAME/PRNT/XFRM/DATA layout; adds optional 1-byte MASK collision filter chunk per shape"),
    "wrld": ("differences", "base world plus named mode/layer wrld chunks (root and inside lvl_ sub-archives); base wrld can carry anim/anmg/anmh world animations"),
    "lvl_": ("differences", "mixed at archive root alongside top-level chunks in maps (mode layers) and per-class sub-archives in sides; ucfb.iter_archives only descended when lvl_ was first"),
    "plan": ("differences", "2005 NODE header is 32 bytes + 8 bytes + 5 weight counts + variable weight bytes (vs 40 bytes + fixed branch table in 2004); ARCS adds 4-byte type flags (26 bytes vs 22)"),
    "path": ("differences", "lowercase config-tree chunk (NAME hash + DATA/SCOP Version/PathCount/Path/Nodes) replacing 2004 uppercase binary PATH chunk"),
    "lght": ("differences", "config-tree chunk (NAME hash + DATA/SCOP) defining Light entries (type, color, transform, shadows) and GlobalLights"),
    "port": ("differences", "config-tree chunk (NAME hash + DATA/SCOP) defining PVS Sector volumes (Base, Height, Point, Object) and Portal entries"),
    "CLTH": ("differences", "cloth mesh chunk (INFO model name, NAME, PRNT, XFRM, DATA texture/verts/uvs/fixed/indices/constraints, COLL primitives)"),
    "comb": ("differences", "config-tree chunk (NAME hash + DATA/SCOP) defining melee/lightsaber combo Animation/OffhandAnimation/ThrowAnimation/State machine"),
    "hud_": ("no_reader", "config-tree chunk (NAME hash + DATA/SCOP) defining 2005 HUD layout elements"),
    "sanm": ("differences", "config-tree chunk (DATA/SCOP) defining shared animation/state definitions in ingame.lvl"),
    "plnp": ("differences", "byte counts, 32-bit arc hashes and size-prefixed zlib routing blocks"),
    "zaf_": ("differences", "animation skeleton (NAME + BIN_ FFAZ/SREV/LEKS with 44-byte TNOJ joints) paired with zaa_ banks"),
    "scr_": ("differences", "same NAME/INFO/BODY wrapper, but BODY is Lua 5.0 bytecode with 4-byte float lua_Number (read by bfe-script-tool, not zero_lua.py)"),
    "gmod": ("no_reader", "LOD group index (LOD0/LOWD mapping; unused by 2004 extractor, which groups by name suffix)"),
    "SHDR": ("no_reader", "compiled Direct3D 9 shader bytecode in core.lvl (engine uses its own shaders)"),
    "SoundBankList": ("unchanged", "hashed sound bank chunk (0x5e262256) in core.lvl; zero_sound.py reads SoundBankList/SampleBank"),
}


def canonical_tag(raw_tag):
    if raw_tag == SOUND_BANK_LIST_TAG:
        return "SoundBankList"
    return raw_tag.decode("ascii") if all(32 <= c < 127 for c in raw_tag) else f"0x{int.from_bytes(raw_tag, 'little'):08x}"


def iter_chunks_recursive(data, start, end, depth=0):
    """Yield (raw_tag, body, end, depth) across top-level chunks and nested lvl_ sub-archives."""
    at = start
    while at + 8 <= end:
        tag = data[at:at + 4]
        if tag == bytes(4):
            at += 4
            continue
        b = at + 8
        e = b + u32(data, at + 4)
        if e > end:
            raise ValueError(f"Invalid archive chunk at {at}")
        at = (e + 3) & ~3
        yield tag, b, e, depth
        if tag == b"lvl_" and e - b >= 8:
            yield from iter_chunks_recursive(data, b + 8, e, depth + 1)


def inspect_header(raw_tag, data, b, e, detail):
    """Record version / header fields for one chunk into detail dict."""
    if raw_tag == b"modl":
        info = child(data, b, e, b"INFO")
        if info:
            detail.setdefault("infoSizes", set()).add(info[1] - info[0])
        for sb, se in children_of(data, b, e, b"segm"):
            mc = child(data, sb, se, b"MTRL")
            if mc:
                detail.setdefault("mtrlSizes", set()).add(mc[1] - mc[0])
            for vb, ve in children_of(data, sb, se, b"VBUF"):
                if ve - vb >= 12:
                    _, _, fl = struct.unpack_from("<III", data, vb)
                    detail.setdefault("vbufFlags", set()).add(f"{fl:#x}")
            sc = child(data, sb, se, b"SKIN")
            if sc and sc[1] - sc[0] >= 8:
                _, sk = struct.unpack_from("<II", data, sc[0])
                detail.setdefault("skinTypes", set()).add(sk)
    elif raw_tag == b"tex_":
        fmts = []
        for fb, fe in children_of(data, b, e, b"FMT_"):
            fi = child(data, fb, fe, b"INFO")
            if fi and fi[1] - fi[0] >= 16:
                fmt = u32(data, fi[0])
                fmts.append(fmt)
                label = f"{fmt:#x}" if fmt > 1000 else str(fmt)
                detail.setdefault("formats", set()).add(label)
        res = zero_texture.decode_texture(data, b, e, lazy=True, decodable=zero_texture.SWBF2_DECODABLE)
        if res and res[1] is not None:
            detail["decodable"] = detail.get("decodable", 0) + 1
        else:
            detail["undecodable"] = detail.get("undecodable", 0) + 1
            detail.setdefault("undecodableFormats", set()).add(tuple(fmts))
    elif raw_tag == b"tern":
        info = child(data, b, e, b"INFO")
        if info and info[1] - info[0] >= 28:
            unit = struct.unpack_from("<f", data, info[0])[0]
            grid = struct.unpack_from("<H", data, info[0] + 16)[0]
            detail.setdefault("headers", set()).add(f"INFO={info[1]-info[0]}B grid={grid} unit={unit:g}")
        else:
            detail.setdefault("headers", set()).add(f"void({e-b}B)")
    elif raw_tag in (b"coll", b"prim"):
        if child(data, b, e, b"MASK") is not None:
            detail["withMask"] = detail.get("withMask", 0) + 1
        else:
            detail["withoutMask"] = detail.get("withoutMask", 0) + 1
    elif raw_tag == b"wrld":
        subs = {ct.decode("latin1") for ct, _, _ in chunks(data, b, e)}
        if "TNAM" in subs or "SNAM" in subs:
            detail["baseWorlds"] = detail.get("baseWorlds", 0) + 1
        else:
            detail["layerWorlds"] = detail.get("layerWorlds", 0) + 1
        if subs & {"anim", "anmg", "anmh"}:
            detail["withWorldAnims"] = detail.get("withWorldAnims", 0) + 1
    elif raw_tag == b"plan":
        info = child(data, b, e, b"INFO")
        if info and info[1] - info[0] >= 6:
            hubs, arcs, extra = struct.unpack_from("<HHH", data, info[0])
            detail.setdefault("headers", set()).add(f"hubs={hubs} arcs={arcs} w2={extra}")
    elif raw_tag == b"plnp":
        if e > b:
            detail.setdefault("headers", set()).add(f"size={e-b} groups={data[b]}")
    elif raw_tag == b"path":
        parsed = zero_config.parse(data, b, e, {fnv1a("Version"): "Version", fnv1a("PathCount"): "PathCount"})
        ver = next((int(v[0]) for k, v, _ in parsed if k == "Version" and v), None)
        if ver is not None:
            detail.setdefault("versions", set()).add(ver)
    elif raw_tag == b"CLTH":
        dc = child(data, b, e, b"DATA")
        if dc:
            raw = data[dc[0]:dc[1]]
            z = raw.find(b"\x00")
            if z >= 0 and z + 5 <= len(raw):
                vcount = struct.unpack_from("<I", raw, z + 1)[0]
                detail["totalVerts"] = detail.get("totalVerts", 0) + vcount
    elif raw_tag == b"zaa_":
        bc = child(data, b, e, b"BIN_")
        if bc and bc[1] - bc[0] >= 24:
            detail.setdefault("innerTags", set()).add(data[bc[0]:bc[0] + 4].decode("latin1"))
            detail.setdefault("versions", set()).add(u32(data, bc[0] + 8))
            detail["totalClips"] = detail.get("totalClips", 0) + struct.unpack_from("<I", data, bc[0] + 20)[0]
    elif raw_tag == b"zaf_":
        bc = child(data, b, e, b"BIN_")
        if bc and bc[1] - bc[0] >= 4:
            detail.setdefault("innerTags", set()).add(data[bc[0]:bc[0] + 4].decode("latin1"))
            outer = child(data, *bc, b"FFAZ")
            version = child(data, *outer, b"SREV") if outer else None
            if version and version[1] - version[0] >= 4:
                detail.setdefault("versions", set()).add(u32(data, version[0]))
    elif raw_tag == b"scr_":
        body = child(data, b, e, b"BODY")
        if body and body[1] - body[0] >= 14 and data[body[0]:body[0] + 4] == b"\x1bLua":
            ver = data[body[0] + 4]
            num_sz = data[body[0] + 13]
            detail.setdefault("luaHeaders", set()).add(f"ver=0x{ver:02x} num={num_sz}B")
    elif raw_tag == b"font":
        hc = child(data, b, e, b"HEAD")
        if hc and hc[1] - hc[0] >= 5:
            glyphs, pages, lh, bl = struct.unpack_from("<HBBB", data, hc[0])
            detail.setdefault("headers", set()).add(f"glyphs={glyphs} pages={pages} h={lh}")
    elif raw_tag == b"Locl":
        nc = child(data, b, e, b"NAME")
        if nc:
            detail.setdefault("languages", set()).add(cstr_at(data, *nc))


def _freeze(detail):
    out = {}
    for k, v in sorted(detail.items()):
        if isinstance(v, set):
            out[k] = sorted(v, key=lambda x: str(x))
        else:
            out[k] = v
    return out


def audit_archive(path):
    """Audit a single .lvl archive; returns a JSON-serializable summary dict."""
    path = Path(path)
    data = path.read_bytes()
    top = chunks(data, 0, len(data))
    if not top or top[0][0] != b"ucfb":
        raise ValueError(f"{path}: not a ucfb archive")
    root_b, root_e = top[0][1], top[0][2]
    tags = {}
    sub_archives = 0
    for raw_tag, b, e, depth in iter_chunks_recursive(data, root_b, root_e):
        if raw_tag == b"lvl_":
            sub_archives += 1
        name = canonical_tag(raw_tag)
        entry = tags.setdefault(name, {"count": 0, "bytes": 0, "detail": {}})
        entry["count"] += 1
        entry["bytes"] += e - b
        inspect_header(raw_tag, data, b, e, entry["detail"])
    result_tags = {}
    for name, entry in sorted(tags.items()):
        status, note = CHUNK_STATUS.get(name, ("no_reader", "unknown chunk tag"))
        result_tags[name] = {
            "count": entry["count"],
            "bytes": entry["bytes"],
            "status": status,
            "note": note,
            "headers": _freeze(entry["detail"]),
        }
    return {
        "file": path.name,
        "bytes": len(data),
        "subArchives": sub_archives,
        "chunks": result_tags,
    }


def merge_audits(reports):
    """Aggregate chunk statistics across multiple archive reports."""
    merged = {}
    for rep in reports:
        for name, info in rep["chunks"].items():
            agg = merged.setdefault(name, {
                "count": 0, "bytes": 0, "status": info["status"], "note": info["note"],
                "archives": [], "headers": {},
            })
            agg["count"] += info["count"]
            agg["bytes"] += info["bytes"]
            agg["archives"].append(rep["archive"])
            for hk, hv in info["headers"].items():
                if isinstance(hv, list):
                    cur = set(agg["headers"].get(hk, []))
                    for item in hv:
                        cur.add(tuple(item) if isinstance(item, list) else item)
                    agg["headers"][hk] = sorted(cur, key=lambda x: str(x))
                elif isinstance(hv, int):
                    agg["headers"][hk] = agg["headers"].get(hk, 0) + hv
    return merged


def format_table(merged):
    lines = [
        "| Chunk | Count | Size | Status | Header / Version fields | Notes |",
        "| --- | ---: | ---: | --- | --- | --- |",
    ]
    for name, info in sorted(merged.items()):
        sz = f"{info['bytes']:,}"
        hdr = ", ".join(f"{k}={v}" for k, v in sorted(info["headers"].items()) if k != "vbufFlags")
        if "vbufFlags" in info["headers"]:
            fl = info["headers"]["vbufFlags"]
            hdr = (hdr + ", " if hdr else "") + f"vbufFlags({len(fl)} kinds)"
        lines.append(f"| `{name}` | {info['count']} | {sz} | {info['status']} | {hdr or '-'} | {info['note']} |")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("archives", nargs="*", help="paths to .lvl archives (default: the 12 opening-target archives under GAME)")
    parser.add_argument("--json", type=Path, help="write full audit JSON here")
    parser.add_argument("--markdown", action="store_true", help="print summary as a Markdown table")
    args = parser.parse_args()

    if args.archives:
        targets = [(a, Path(a)) for a in args.archives]
    else:
        game = zero_paths.GAME
        targets = [(rel, game / rel) for rel in DEFAULT_ARCHIVES]

    reports = []
    for label, path in targets:
        if not path.exists():
            print(f"missing archive: {path}", file=sys.stderr)
            return 2
        rep = audit_archive(path)
        rep["archive"] = label
        reports.append(rep)

    merged = merge_audits(reports)
    if args.json:
        args.json.write_text(json.dumps({"archives": reports, "summary": merged}, indent=2) + "\n")
    if args.markdown:
        print(format_table(merged))
    else:
        for rep in reports:
            print(f"{rep['archive']} ({rep['bytes']:,} B, {rep['subArchives']} lvl_ sub-archives): "
                  + ", ".join(f"{k}:{v['count']}" for k, v in sorted(rep["chunks"].items())))
        print("\nSUMMARY:")
        print(format_table(merged))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
