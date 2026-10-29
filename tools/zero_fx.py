"""Particle effects (`fx__`).

An fx__ chunk is NAME (the FNV hash of the effect's name) and the effect's
.fx source as a config tree (zero_config): ParticleEmitter blocks, each with
emission settings, a Spawner, a Transformer, and a Geometry. In the munged
tree the next emitter of an effect sits inside the previous one's block, after
its Geometry; an EMITTER geometry holds the emitter it spawns per particle.

Ranges are (min, max). Inside a Spawner they carry a leading selector,
(0, min, max). Colours are 0..255, as RGB (Red, Green, Blue) or HSV (Hue,
Saturation, Value). Transformer keys run for their Lifetime, then hand over to
Next.

A LightningBolt block (only the arc caster's arccaster_lightning has one) is a
bolt drawn between two points rather than particles: it comes out as one
entry with "lightning" set to its settings.
"""

from ucfb import chunks, child, iter_archives, u32
from zero_config import parse
from zero_hash import fnv1a

KEYWORDS = """
ParticleEmitter MaxParticles StartDelay BurstDelay BurstCount MaxLodDist MinLodDist BoundingRadius SoundName
Size Hue Saturation Value Red Green Blue Alpha Spawner Spread Circle Offset PositionX PositionY PositionZ
PositionScale VelocityScale InheritVelocityFactor StartRotation RotationVelocity FadeInTime Transformer
Lifetime Position Rotation Color Accelerate Reach Scale Move Next Geometry BlendMode Type Texture Model
SparkLength TimePerFrame FrameSize TotalFrames TextureSize Width FadeTime NoRegisterStep WorldAligned
LightningBolt SpreadFactor InterpolationSpeed BranchFactor BranchSpreadFactor BranchLength MaxBranches
BreakDistance ChildBreakDistance ChildTextureSize ChildWidth ChildSpreadFactor NumChildren ChildColor
Shield SpecularColor Radius Height
""".split()
NAMES = {fnv1a(name): name for name in KEYWORDS}


def _get(entries, name):
    return next((entry for entry in entries if entry[0] == name), None)


def _range(entries, name, default=(0.0, 0.0)):
    """A (min, max) pair; Spawner values drop their leading selector."""
    entry = _get(entries, name)
    if not entry or not entry[1]:
        return list(default)
    values = [v for v in entry[1] if isinstance(v, float)]
    return values[-2:] if len(values) >= 2 else values * 2


def _scalar(entries, name, default=0.0):
    entry = _get(entries, name)
    values = [v for v in entry[1] if isinstance(v, float)] if entry else []
    return values[0] if values else default


def _string(entries, name):
    entry = _get(entries, name)
    values = [v for v in entry[1] if isinstance(v, str)] if entry else []
    return values[0] if values else None


def _color(entries):
    """Ranges for four channels, and whether they are HSV."""
    hsv = _get(entries, "Hue") is not None
    names = ("Hue", "Saturation", "Value", "Alpha") if hsv else ("Red", "Green", "Blue", "Alpha")
    return {"hsv": hsv, "channels": [_range(entries, n, (255.0, 255.0)) for n in names]}


def _box(entries):
    return [_range(entries, axis) for axis in ("PositionX", "PositionY", "PositionZ")]


def _keys(entries):
    """A transformer track: Lifetime plus one operation per key, along Next."""
    out = []
    while entries:
        key = {"lifetime": _scalar(entries, "Lifetime", 0.0)}
        for op in ("Accelerate", "Reach", "Scale", "Move"):
            entry = _get(entries, op)
            if entry:
                values = [v for v in entry[1] if isinstance(v, float)]
                key[op.lower()] = values if len(values) > 1 else (values[0] if values else 0.0)
        out.append(key)
        nxt = _get(entries, "Next")
        entries = nxt[2] if nxt else None
    return out


def _emitter(values, entries):
    spawner = (_get(entries, "Spawner") or [None, [], []])[2]
    transformer = (_get(entries, "Transformer") or [None, [], []])[2]
    geometry = (_get(entries, "Geometry") or [None, [], []])[2]
    circle = _get(spawner, "Circle")
    shape = circle or _get(spawner, "Spread")
    offset = _get(spawner, "Offset")
    tracks = {}
    for track in ("Position", "Size", "Color", "Rotation"):
        entry = _get(transformer, track)
        if entry:
            tracks[track.lower()] = _keys(entry[2])
    kind = (_string(geometry, "Type") or "PARTICLE").lower()
    out = {
        "name": next((v for v in values if isinstance(v, str)), ""),
        "maxParticles": _range(entries, "MaxParticles", (-1.0, -1.0)),
        "startDelay": _range(entries, "StartDelay"),
        "burstDelay": _range(entries, "BurstDelay", (1.0, 1.0)),
        "burstCount": _range(entries, "BurstCount", (1.0, 1.0)),
        "registerStep": _get(entries, "NoRegisterStep") is None,
        "size": _range(entries, "Size", (1.0, 1.0)),
        "color": _color(entries),
        "spawner": {
            "shape": "circle" if circle else "spread",
            "box": _box(shape[2]) if shape else [[0.0, 0.0]] * 3,
            "offset": _box(offset[2]) if offset else [[0.0, 0.0]] * 3,
            "positionScale": _range(spawner, "PositionScale"),
            "velocityScale": _range(spawner, "VelocityScale"),
            "inheritVelocity": _range(spawner, "InheritVelocityFactor"),
            "size": _range(spawner, "Size", (1.0, 1.0)),
            "color": _color(spawner),
            "startRotation": _range(spawner, "StartRotation"),
            "rotationVelocity": _range(spawner, "RotationVelocity"),
            "fadeInTime": _scalar(spawner, "FadeInTime"),
        },
        "transformer": {"lifetime": _scalar(transformer, "Lifetime", 7.0), **tracks},
        "geometry": {
            "type": kind,
            "blend": (_string(geometry, "BlendMode") or "NORMAL").lower(),
            "texture": _string(geometry, "Texture"),
            "model": _string(geometry, "Model"),
            "sparkLength": _scalar(geometry, "SparkLength", 0.0),
            "timePerFrame": _scalar(geometry, "TimePerFrame", 0.0),
            "frameSize": _scalar(geometry, "FrameSize", 1.0),
            "totalFrames": _scalar(geometry, "TotalFrames", 1.0),
            "width": _scalar(geometry, "Width", 0.0),
            "worldAligned": _get(geometry, "WorldAligned") is not None,
        },
    }
    if kind == "emitter":
        out["geometry"]["emitters"] = emitters(geometry)
    return out


# A LightningBolt's keys, with the engine's defaults.
LIGHTNING = {
    "Texture": "lightning", "Width": 2.0, "FadeTime": 1.0, "SpreadFactor": 1.0, "InterpolationSpeed": 10.0,
    "BranchFactor": 0.3, "BranchSpreadFactor": 0.8, "BranchLength": 4.0, "MaxBranches": 5.0, "BreakDistance": 2.0,
    "TextureSize": 5.0, "ChildBreakDistance": 1.5, "ChildTextureSize": 3.0, "ChildWidth": 0.3,
    "ChildSpreadFactor": 1.0, "NumChildren": 3.0, "Color": [255.0, 255.0, 255.0, 255.0],
    "ChildColor": [255.0, 255.0, 255.0, 255.0],
}


def _lightning(entries):
    out = {}
    for key, default in LIGHTNING.items():
        if key == "Texture":
            out[key] = _string(entries, key) or default
        elif isinstance(default, list):
            entry = _get(entries, key)
            values = [v for v in entry[1] if isinstance(v, float)] if entry else []
            out[key] = values if len(values) >= 3 else default
        else:
            out[key] = _scalar(entries, key, default)
    return out


def emitters(entries):
    """The emitters in a block, with each one's nested next emitter flattened
    into the list."""
    out = []
    for name, values, children in entries:
        if name == "ParticleEmitter":
            out.append(_emitter(values, children))
            out.extend(emitters(children))
        elif name == "LightningBolt":
            out.append({"lightning": _lightning(children)})
        elif name == "Shield":
            out.append({"shield": _shield(children)})
    return out


def _shield(entries):
    """A Shield block (the droideka's bubble): its texture, colours, and size.
    08aab3aa and c8a9e3c6 are its fade in and out times."""
    def floats(key, default):
        entry = _get(entries, key)
        values = [v for v in entry[1] if isinstance(v, float)] if entry else []
        return values or default
    fades = [_get(entries, "%08x" % h) for h in (0x08AAB3AA, 0xC8A9E3C6)]
    return {
        "Texture": _string(entries, "Texture"),
        "Color": floats("Color", [255.0, 255.0, 255.0, 255.0]),
        "SpecularColor": floats("SpecularColor", [255.0, 255.0, 255.0]),
        "Radius": _scalar(entries, "Radius", 1.0),
        "Height": _scalar(entries, "Height", 1.0),
        "FadeIn": next((v for v in (fades[0] or [None, []])[1] if isinstance(v, float)), 0.15),
        "FadeOut": next((v for v in (fades[1] or [None, []])[1] if isinstance(v, float)), 0.15),
    }


def effects(data):
    """name hash -> emitters for every fx__ chunk in an archive."""
    found = {}
    for start, end in iter_archives(data):
        for tag, b, e in chunks(data, start, end):
            if tag != b"fx__":
                continue
            name = child(data, b, e, b"NAME")
            if name:
                found.setdefault(u32(data, name[0]), emitters(parse(data, b, e, NAMES)))
    return found


def effect(archives, name):
    """An effect's emitters by name from the first archive holding it."""
    key = fnv1a(name)
    for found in archives:
        if key in found:
            return found[key]
    return None


def textures(emitter_list):
    """Every texture and model an effect's emitters draw."""
    tex, models = set(), set()
    for em in emitter_list:
        if em.get("lightning"):
            tex.add(em["lightning"]["Texture"])
            continue
        if em.get("shield"):
            if em["shield"].get("Texture"):
                tex.add(em["shield"]["Texture"])
            continue
        g = em["geometry"]
        if g.get("texture"):
            tex.add(g["texture"])
        if g.get("model"):
            models.add(g["model"])
        if g.get("emitters"):
            t, m = textures(g["emitters"])
            tex |= t
            models |= m
    return tex, models
