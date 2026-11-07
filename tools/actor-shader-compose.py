#!/usr/bin/env python3
"""Derive the actor variants no battle has been seen to need yet.

The observed actor programs (shaders/actors/manifest.json rows without "derived")
are three.js's own expansion of the zero material for one set of defines, one
skin mode, one cascade count and one lamp mode. A battle can meet a material
the census never did (a rigid unlit part without scroll under High shadows
stopped one). The material space is small and closed: kind (lit or unlit), cutout
or blended or neither, scroll, heat, skinned or rigid, and 0/2/3/4 cascades.
This tool fills every cell of it that is missing:

  --write   derive them and rewrite the manifest rows and shared chunks
  --check   fail unless the checked-in derived rows are exactly what --write
            derives from the observed rows (CI: tests/actor-compose.test.js)

A derived program is an observed one with the edit between two observed
neighbours applied to it: the edit that adds a feature (heat, scroll, cutout,
blending) or a skin mode or a cascade count, learned from any observed pair that
differs only in that, applied hunk by hunk to a base that has the rest. The edits
touch separate regions (a feature mostly adds a define; a cascade count unrolls
the shadow loops), and each is only applied where the base's own lines match;
anything else stops the derivation. Define lines are then put in the order three
emits them. Validation: removing any one observed variant and deriving it back
from the others gives the same text, for every variant that has a derivation.
Derived rows carry "derived": true so loading skips linking them ahead of time
(the first use of one costs one link).
"""
import copy, difflib, hashlib, importlib.util, json, pathlib, sys

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('chunks', HERE / 'actor-shader-chunks.py')
chunks = importlib.util.module_from_spec(spec); spec.loader.exec_module(chunks)
ROOT = chunks.ROOT

FEATURES = ('SCROLL', 'ALPHA_TEST', 'BLENDED', 'HEATED')
# The order three emits a material's defines in (the material's own, then what it derives from
# them, then the shadow defines and OPAQUE), which a derived source has to follow.
RANK = ['ZERO_LIGHTS', 'DYNAMIC', 'UNLIT', 'SCROLL', 'ALPHA_TEST', 'BLENDED', 'HEATED', 'ALPHA_TO_COVERAGE',
        'USE_ALPHATEST', 'USE_SHADOWMAP', 'SHADOWMAP_TYPE_PCF_SOFT', 'OPAQUE']
STAGES = ('vertex', 'fragment')


def define_name(line):
    parts = line.split()
    return parts[1] if line.startswith('#define ') and len(parts) > 1 else None


def reorder(source):
    """Reassign the slots the ranked define lines occupy so they read in three's order."""
    rank = {name: i for i, name in enumerate(RANK)}
    out, i = source.splitlines(keepends=True), 0
    while i < len(out):
        if not out[i].startswith('#define '):
            i += 1; continue
        j = i
        while j < len(out) and out[j].startswith('#define '):
            j += 1
        slots = [k for k in range(i, j) if define_name(out[k]) in rank]
        for k, line in zip(slots, sorted((out[k] for k in slots), key=lambda l: rank[define_name(l)])):
            out[k] = line
        i = j
    return ''.join(out)


def after_zero_lights(lines):
    for k, line in enumerate(lines):
        if line.startswith('#define ZERO_LIGHTS'):
            return k + 1
    raise ValueError('no ZERO_LIGHTS define')


def transfer(a, b, c):
    """The edit a->b applied to c (a and c alike): a hunk applies where its old lines are c's too."""
    A, B, C = (x.splitlines(keepends=True) for x in (a, b, c))
    where = [None] * (len(A) + 1)
    for block in difflib.SequenceMatcher(None, A, C, autojunk=False).get_matching_blocks():
        for k in range(block.size):
            where[block.a + k] = block.b + k
    out = C[:]
    ops = [o for o in difflib.SequenceMatcher(None, A, B, autojunk=False).get_opcodes() if o[0] != 'equal']
    for _, i1, i2, j1, j2 in reversed(ops):
        defines_only = all(l.startswith('#define ') for l in A[i1:i2] + B[j1:j2])
        if i2 > i1:
            at = [where[i] for i in range(i1, i2)]
            if None in at or at != list(range(at[0], at[0] + len(at))):
                # A define the target lacks or has elsewhere: such an edit applies by name (the
                # defines are ranked afterwards); anything else is a real conflict.
                if not defines_only:
                    raise ValueError(f'conflict at {i1}:{i2}')
                gone = {define_name(l) for l in A[i1:i2]}
                out = [l for l in out if define_name(l) not in gone]
                k = after_zero_lights(out); out[k:k] = B[j1:j2]
                continue
            out[at[0]:at[-1] + 1] = B[j1:j2]
        elif i1 == 0 or where[i1 - 1] is None:
            if not defines_only:
                raise ValueError(f'insert anchor lost at {i1}')
            k = after_zero_lights(out); out[k:k] = B[j1:j2]
        else:
            k = where[i1 - 1] + 1; out[k:k] = B[j1:j2]
    return ''.join(out)


class Variant:
    """One cell: its text, uniforms and attributes, as observed or derived."""
    def __init__(self, text, uniforms, attributes, row=None):
        self.text, self.uniforms, self.attributes, self.row = text, uniforms, attributes, row


def cell_of(row):
    s = row.get('selector')
    if not s:
        return None
    names = {d[0] for d in s['defines']}
    if 'RIGID_SKIN' in names or s['lamps']:
        return None
    return ('DYNAMIC' if 'DYNAMIC' in names else 'UNLIT', frozenset(names & set(FEATURES)), s['mode'], s['cascades'])


def apply_edit(cur, before, after):
    """cur with the edit before->after applied, in text, uniforms and attributes."""
    text = {st: transfer(before.text[st], after.text[st], cur.text[st]) for st in STAGES}
    uniforms = {k: v for k, v in cur.uniforms.items() if k not in before.uniforms or k in after.uniforms}
    uniforms.update({k: v for k, v in after.uniforms.items() if k not in before.uniforms})
    attributes = {k: v for k, v in cur.attributes.items() if k not in before.attributes or k in after.attributes}
    attributes.update({k: v for k, v in after.attributes.items() if k not in before.attributes})
    return Variant(text, uniforms, attributes)


def derive(pool, cell):
    """The variant for `cell` from the others in the pool, or None when no neighbours suffice."""
    kind, features, mode, cascades = cell
    others = {k: v for k, v in pool.items() if k != cell}

    # The largest observed subset of the features in this mode, else in the other mode (then the mode edit).
    bases = [(len(k[1]), k) for k in others if k[0] == kind and k[1] <= features and k[3] == cascades]
    same = sorted((b for b in bases if b[1][2] == mode), reverse=True)
    if same:
        cur = copy.deepcopy(others[same[0][1]]); have = same[0][1][1]
    else:
        other = sorted((b for b in bases if b[1][2] != mode), reverse=True)
        if not other:
            return None
        cur = copy.deepcopy(others[other[0][1]]); have = other[0][1][1]
        for k in sorted(others, key=lambda k: (k[3] != cascades, k[0] != kind, len(k[1]))):
            twin = (k[0], k[1], mode, k[3])
            if k[2] != mode and twin in others:
                try:
                    cur = apply_edit(cur, others[k], others[twin]); break
                except ValueError:
                    continue
        else:
            return None
    for feature in sorted(features - have):
        for k in sorted(others, key=lambda k: ((k[2] != mode) + (k[3] != cascades), k[0] != kind, len(k[1]))):
            twin = (k[0], k[1] | {feature}, k[2], k[3])
            if feature in k[1] or twin not in others:
                continue
            try:
                cur = apply_edit(cur, others[k], others[twin]); break
            except ValueError:
                continue
        else:
            return None
    cur.text = {st: reorder(t) for st, t in cur.text.items()}
    return cur


def all_cells():
    for kind in ('DYNAMIC', 'UNLIT'):
        # Cutout and blending exclude each other (describe() in materials.js: a hard-edged material is opaque).
        for cut_or_blend in ((), ('ALPHA_TEST',), ('BLENDED',)):
            for scroll in ((), ('SCROLL',)):
                for heat in ((), ('HEATED',)):
                    for mode in ('rigid', 'skinned'):
                        for cascades in (0, 2, 3, 4):
                            yield (kind, frozenset(cut_or_blend + scroll + heat), mode, cascades)


def observed(manifest):
    pool = {}
    for row in manifest['programs']:
        cell = cell_of(row) if not row.get('derived') else None
        if cell:
            pool[cell] = Variant({st: ''.join((ROOT / p).read_text() for p in chunks.parts(row, st)) for st in STAGES},
                                 row['uniforms'], row['attributes'], row)
    return pool


def derived_rows(pool, template):
    """Every missing cell, derived to a fixed point (a derived cell may seed another)."""
    made, todo, progress = {}, [c for c in all_cells() if c not in pool], True
    while progress:
        progress = False
        for cell in list(todo):
            v = derive({**pool, **made}, cell)
            if v:
                made[cell] = v; todo.remove(cell); progress = True
    if todo:
        raise SystemExit('cannot derive: ' + ', '.join(f'{c[0]} {sorted(c[1])} {c[2]} {c[3]}' for c in todo[:8]))
    rows, sources = [], {}
    for cell in sorted(made, key=lambda c: (c[2], c[3], c[0], sorted(c[1]))):
        v = made[cell]; kind, features, mode, cascades = cell
        key = f'actors/{mode}-' + hashlib.sha256((v.text['vertex'] + v.text['fragment']).encode()).hexdigest()[:12]
        names = sorted({kind, 'ZERO_LIGHTS', *features})
        for lamps in ((False, True) if kind == 'DYNAMIC' else (False,)):
            row = copy.deepcopy(template[mode])
            row.update({'key': key + ('-lamps' if lamps else ''), 'lamps': lamps, 'derived': True,
                        'uniforms': v.uniforms, 'attributes': v.attributes, 'vertex': 'x', 'fragment': 'x',
                        'selector': {'defines': [[n, '16' if n == 'ZERO_LIGHTS' else ''] for n in names], 'skin': mode == 'skinned',
                                     'cascades': cascades, 'mode': mode, 'lamps': lamps}})
            rows.append(row); sources[row['key']] = v.text
    return rows, sources


def build():
    manifest = chunks.load()
    base = [r for r in manifest['programs'] if not r.get('derived')]
    manifest['programs'] = base
    pool = observed(manifest)
    template = {}
    for mode in ('rigid', 'skinned'):
        row = next(r for r in base if r.get('selector') and r['selector']['mode'] == mode)
        template[mode] = {k: row[k] for k in row if k not in ('selector',)}
    rows, derived = derived_rows(pool, template)
    sources = {r['key']: {st: ''.join((ROOT / p).read_text() for p in chunks.parts(r, st)) for st in STAGES} for r in base}
    sources.update(derived)
    return manifest, rows, sources


def main(argv):
    manifest, rows, sources = build()
    if argv[1:] == ['--write']:
        files, lists = chunks.factor(sources)
        manifest['programs'] = manifest['programs'] + rows
        for old in (ROOT / 'shared').glob('*.glsl'):
            old.unlink()
        for name, text in files.items():
            (ROOT / name).write_text(text)
        for row in manifest['programs']:
            for st in STAGES:
                row[st] = lists[row['key']][st]
        (ROOT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        print('derived', len(rows), 'rows;', len(files), 'chunks')
    elif argv[1:] == ['--check']:
        on_disk = {r['key']: r for r in chunks.load()['programs'] if r.get('derived')}
        problems = [f'missing derived row {r["key"]}' for r in rows if r['key'] not in on_disk]
        problems += [f'stale derived row {k}' for k in on_disk if k not in sources]
        for r in rows:
            d = on_disk.get(r['key'])
            if d and (d['selector'] != r['selector'] or d['uniforms'] != r['uniforms'] or d['attributes'] != r['attributes']
                      or {st: ''.join((ROOT / p).read_text() for p in chunks.parts(d, st)) for st in STAGES} != sources[r['key']]):
                problems.append(f'derived row differs: {r["key"]}')
        if problems:
            print('\n'.join(problems[:20])); return 1
        print('derived actor variants match:', len(rows))
    else:
        print(__doc__); return 2
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
