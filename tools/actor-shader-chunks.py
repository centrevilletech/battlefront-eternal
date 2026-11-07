#!/usr/bin/env python3
"""Join and factor the actor family's shared GLSL chunks.

The 108 actor programs are complete GLSL ES 3.00 sources. Stored whole they
would repeat the same few thousand lines per variant, so the manifest lists each
stage as an ordered array of chunk files under shaders/actors/shared, which Rust
concatenates before compiling (programs.rs sources()). The chunks are derived:

  --join DIR     write every program's complete vertex and fragment source to
                 DIR/<key with / as ->.vert|.frag, the file a person reads or edits
  --factor DIR   read those files back and rewrite shared/ and the manifest's
                 chunk lists from them (edit a variant in DIR, then factor)
  --check        join the checked-in chunks, factor the result in memory and fail
                 unless it gives back exactly the files and lists on disk (CI),
                 and no chunk file is left unreferenced

Factoring is deterministic: the longest source of each stage is the base; every
other source is diffed against it with difflib and cut at the matching blocks of
three or more lines; each cut piece is a shared chunk, each difference a
variant chunk named by its content hash.
"""
import difflib, hashlib, json, pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent / 'rust/bfe-render/shaders/actors'
STAGES = (('vertex', 'vert'), ('fragment', 'frag'))


def load():
    return json.loads((ROOT / 'manifest.json').read_text())


def parts(row, stage):
    value = row[stage]
    return value if isinstance(value, list) else [value]


def joined(manifest):
    """key -> {stage: complete source text}."""
    return {row['key']: {stage: ''.join((ROOT / p).read_text() for p in parts(row, stage)) for stage, _ in STAGES}
            for row in manifest['programs']}


def factor(sources):
    """Return ({chunk path: text}, {key: {stage: [chunk paths]}}) for complete sources."""
    files, lists = {}, {key: {} for key in sources}
    for stage, ext in STAGES:
        by_key = {key: text[stage].splitlines(keepends=True) for key, text in sources.items()}
        base = max(by_key.values(), key=lambda lines: sum(map(len, lines)))
        blocks = {key: [m for m in difflib.SequenceMatcher(None, base, lines, autojunk=False).get_matching_blocks() if m.size >= 3]
                  for key, lines in by_key.items()}
        cuts = {0, len(base)}
        for found in blocks.values():
            for m in found:
                cuts.update([m.a, m.a + m.size])
        cuts = sorted(cuts)
        shared = {}
        for i, (a, b) in enumerate(zip(cuts, cuts[1:])):
            name = f'shared/{ext}-{i:03}.glsl'
            files[name] = ''.join(base[a:b])
            shared[(a, b)] = name

        def delta(text):
            if not text:
                return []
            name = 'shared/variant-' + hashlib.sha256(text.encode()).hexdigest()[:12] + '.glsl'
            files[name] = text
            return [name]

        for key, lines in by_key.items():
            at, out = 0, []
            for m in blocks[key]:
                out += delta(''.join(lines[at:m.b]))
                out += [name for (a, b), name in shared.items() if a >= m.a and b <= m.a + m.size]
                at = m.b + m.size
            out += delta(''.join(lines[at:]))
            assert ''.join(files[q] for q in out) == sources[key][stage]
            lists[key][stage] = out
    return files, lists


def on_disk():
    return {f'shared/{p.name}': p.read_text() for p in (ROOT / 'shared').glob('*.glsl')}


def main(argv):
    manifest = load()
    if len(argv) == 3 and argv[1] == '--join':
        out = pathlib.Path(argv[2]); out.mkdir(parents=True, exist_ok=True)
        for key, text in joined(manifest).items():
            for stage, ext in STAGES:
                (out / (key.replace('/', '-') + '.' + ext)).write_text(text[stage])
        print('joined', len(manifest['programs']), 'programs')
    elif len(argv) == 3 and argv[1] == '--factor':
        src = pathlib.Path(argv[2])
        sources = {row['key']: {stage: (src / (row['key'].replace('/', '-') + '.' + ext)).read_text() for stage, ext in STAGES}
                   for row in manifest['programs']}
        files, lists = factor(sources)
        for old in (ROOT / 'shared').glob('*.glsl'):
            old.unlink()
        for name, text in files.items():
            (ROOT / name).write_text(text)
        for row in manifest['programs']:
            for stage, _ in STAGES:
                row[stage] = lists[row['key']][stage]
        (ROOT / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        print('factored', len(sources), 'programs into', len(files), 'chunks')
    elif argv[1:] == ['--check']:
        files, lists = factor(joined(manifest))
        disk = on_disk()
        used = {p for row in manifest['programs'] for stage, _ in STAGES for p in parts(row, stage)}
        problems = [f'unreferenced chunk {p}' for p in sorted(set(disk) - used)]
        problems += [f'chunk differs or is missing: {p}' for p in sorted(files) if disk.get(p) != files[p]]
        problems += [f'{row["key"]}: chunk list is not the factored one' for row in manifest['programs']
                     if any(parts(row, stage) != lists[row['key']][stage] for stage, _ in STAGES)]
        if problems:
            print('\n'.join(problems[:20])); return 1
        print('actor shader chunks are regenerable:', len(files), 'chunks')
    else:
        print(__doc__); return 2
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
