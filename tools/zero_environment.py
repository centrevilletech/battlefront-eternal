"""Export the effects a map's placed objects carry without rebuilding any
map/model/audio assets: Theed's fountains, steam vents (Cloud City, Bespin
Platforms, Hoth, Jabba's Palace), torches and campfires, smoke, dry-ice mist,
waterspouts, fireflies and the like.

An ODF's `AttachEffect` names the effect that every `AttachToHardPoint` after
it carries, until the next `AttachEffect` or `AttachOdf` (the original game keeps
the named effect pending and adds an attachment for each hardpoint, at most
30 a class; a hardpoint the model lacks attaches nothing). The retail
`576b09cd` property names an effect and its hardpoint together.

Usage: python3 tools/zero_environment.py [map ...] (default nab2 bes2)
Requires the locally installed retail archives used by tools/extract.py.
Maps whose objects carry no effects write nothing.
"""
import json
from pathlib import Path
import sys
from ucfb import nodes
from zero_odf import entity_classes, property_name
from zero_world import instance_records
from zero_model import collect_models
from zero_texture import collect_textures
from zero_fx import effects, effect, textures as effect_textures
from xform import mat4, mul
from extract import write_meshes, write_textures
import zero_addon

ROOT = Path(__file__).resolve().parents[1]
from zero_paths import GAME, PUBLIC  # noqa: E402
FOUNTAIN = 'nab2_prop_fountain'
MAX_ATTACHED = 30


def attachments(props):
    """(effect, hardpoint) pairs in ODF order, as the original game attaches them."""
    pending = None
    found = 0
    for key, value in props:
        if key == '576b09cd':  # Retail combined effect-name / hardpoint property.
            words = value.split()
            if len(words) == 2 and found < MAX_ATTACHED:
                found += 1
                yield tuple(words)
        elif key in ('6a6c7e0d', 'AttachEffect'):
            pending = value
        elif key == 'AttachOdf':
            pending = None
        elif key == 'AttachToHardPoint' and pending and found < MAX_ATTACHED:
            found += 1
            yield pending, value.split()[0] if value.split() else value


def export(name):
    data = zero_addon.map_archive(name, GAME).read_bytes()
    common = (GAME / 'Common/ENG/common.lvl').read_bytes()
    models, skeletons = {}, {}
    for archive in (data, common):
        collect_models(archive, models, skeletons)
    classes = {}
    for archive in (data, common):
        for cname, cls in entity_classes(archive).items():
            classes.setdefault(cname, cls)
    canonical = {cname.lower(): cname for cname in classes}
    archives = [effects(data), effects(common)]
    instances, basins, skipped = [], [], set()
    for b, e in nodes(data, b'wrld'):
        for record in instance_records(data, b, e, property_name):
            kind = canonical.get(record['class'].lower(), record['class'])
            cls = classes.get(kind)
            pairs = list(attachments(cls['props'])) if cls else []
            if not pairs:
                continue
            props = dict(cls['props'])
            model = props.get('GeometryName', kind)
            skeleton = skeletons.get(model) or skeletons.get(model.lower())
            world = mat4(record['xfrm'])
            for fx, hp in pairs:
                if effect(archives, fx) is None:
                    skipped.add(f'effect {fx}')
                    continue
                names = [n.lower() for n in (skeleton or {}).get('names', [])]
                if hp.lower() not in names:
                    skipped.add(f'{model}:{hp}')
                    continue
                transform = mul(world, skeleton['model'][names.index(hp.lower())])
                instances.append(dict(effect=fx, hardpoint=hp, owner=record['name'], matrix=transform))
            if kind == FOUNTAIN:
                water = [p for p in models[model] if p['texture'] == 'nab2_prop_fountain_water']
                points = [p for part in water for p in part['positions']]
                basins.append(dict(center=[world[12], world[13]+max(p[1] for p in points), world[14]],
                                   radius=max(abs(p[0]) for p in points), texture='nab2_prop_fountain_water'))
    if skipped:
        print(f'{name}: not attached: {", ".join(sorted(skipped))}')
    if not instances:
        print(f'{name}: no attached effects')
        return None
    library = {n: effect(archives, n) for n in sorted({i['effect'] for i in instances})}
    wanted, geometry = set(), set()
    for defs in library.values():
        t, m = effect_textures(defs)
        wanted |= t
        geometry |= m
    for model in geometry:
        if model not in models:
            raise ValueError(f'Missing FX geometry {model}')
        wanted.update(p['texture'] for p in models[model])
    decoded = {}
    for archive in (data, common):
        collect_textures(archive, decoded)
    out = PUBLIC / 'maps' / name / 'environment'
    out.mkdir(parents=True, exist_ok=True)
    paths = write_textures(decoded, wanted, out / 'textures')
    if wanted - paths.keys():
        print(f'{name}: missing FX textures: {sorted(wanted - paths.keys())}')
    packed = write_meshes(models, sorted(geometry), out, 'models')
    result = dict(version=2, instances=instances, basins=basins, effects=library, textures=paths, models=packed)
    (out / 'effects.json').write_text(json.dumps(result, separators=(',', ':')))
    print(f'{name}: {len(instances)} attachments of {len(library)} effects, {len(basins)} basins, {len(paths)} textures, {len(packed)} FX meshes')
    return out


if __name__ == '__main__':
    names = sys.argv[1:] or ['nab2', 'bes2']
    written = [out for out in (export(name) for name in names) if out]
    from ktx2 import encode_optional  # KTX2 copies of the textures just written; optional
    encode_optional(written)
