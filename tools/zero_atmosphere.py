"""The world's haze: the fx config's `FogCloud` effect and the `dusteffect`
objects placed in the world.

FogCloud (Geonosis, Endor, the Citadel, both Yavins) is a band of large soft
billboards around the camera. The engine lays a fixed pattern of
`ParticleDensity` points over every 256 m world cell (one random draw, the
same in every cell), scrolls it with `Velocity` (x, z in m/s), and stands
each point `Height` above the terrain under it. It draws the points that lie
in the view's slab between `Range` near and far: camera-facing squares
`ParticleSize` across, each turning at `Rotation` half-turns a second (either
way), tinted `Color` with its alpha scaled by 1 - t^2, t running -1..1 as the
squared distance runs from near^2 to far^2.

A dusteffect class (Geonosis, Hoth, Kamino, the Citadel, Tatooine Dune Sea,
both Yavins) is a cloud of `NumParticles` camera-facing squares that every
placed copy of the class shares. A particle starts at a random point between
`MinPos` and `MaxPos` and moves at a random velocity between `MinVel` and
`MaxVel` for a random life between `MinLifetime` and `MaxLifetime`,
`MinSize`..`MaxSize` across. Its alpha rises at 1 a second to `Alpha` and
over its last second falls as `Alpha` x life left x that. With
`RadiusFadeMin` set, it fades out between that and `RadiusFadeMax` from the
object's vertical axis, and dies past it. A copy keeps only its position:
the world's axes, the vertical one scaled by `HeightScale` (default 0.2), so
particles and squares alike lie flattened into low sheets. It draws while the
view depth of its position plus the class's radius lies between
`MinDistance` and `MaxDistance`, its alpha scaled by (MaxDistance - that
depth) / 200, up to 1.
"""

import math

from zero_config import value

# The engine's defaults (its FogCloudEffect constructor).
FOG_CLOUD = {
    "Texture": ["fluffy"],
    "Range": [40.0, 90.0],
    "Color": [192, 192, 192, 160],
    "Velocity": [0.0, 0.0],
    "Rotation": [0.1],
    "Height": [16.0],
    "ParticleSize": [28.0],
    "ParticleDensity": [30],
}
# The pattern repeats every CELL metres (a constant of the engine).
CELL = 256.0

# A dusteffect class's keys with the engine's defaults (its constructor).
DUST = {
    "NumParticles": 50,
    "MinPos": [-20.0, -1.0, -20.0],
    "MaxPos": [20.0, 1.0, 20.0],
    "MinVel": [-3.0, -1.0, -3.0],
    "MaxVel": [3.0, 1.0, 3.0],
    "MinSize": 10.0,
    "MaxSize": 15.0,
    "MinLifetime": 2.0,
    "MaxLifetime": 3.0,
    "Alpha": 0.5,
    "MinDistance": 0.0,
    "MaxDistance": 0.0,
    "RadiusFadeMin": -1.0,
    "RadiusFadeMax": -1.0,
    "HeightScale": 0.2,
    "Texture": "whitesmoke",
}


def fog_cloud(fx_configs):
    """The enabled FogCloud effect, or None."""
    for entries in fx_configs:
        for name, values, children in entries:
            if name != "Effect" or not values or values[0] != "FogCloud":
                continue
            if not value(children, "Enable", [0])[0]:
                return None
            got = {key: value(children, key, default) for key, default in FOG_CLOUD.items()}
            color = [int(c) for c in got["Color"][:4]]
            near, far = (got["Range"] + got["Range"][-1:])[:2]
            velocity = (list(got["Velocity"]) + [0.0])[:2]
            return {
                "texture": got["Texture"][0],
                "range": [near, far],
                # Three values leave the alpha opaque, as the engine reads them.
                "color": color + [255] * (4 - len(color)),
                "velocity": velocity,
                "rotation": got["Rotation"][0],
                "height": got["Height"][0],
                "size": got["ParticleSize"][0],
                "density": int(got["ParticleDensity"][0]),
                "cell": CELL,
            }
    return None


def _last(props, key):
    """A class property's value: the last one set wins, as the engine reads
    the class's inheritance chain in order."""
    found = [v for k, v in props if k == key]
    return found[-1] if found else None


def _numbers(text):
    numbers = []
    for word in str(text).split():
        try:
            numbers.append(float(word))
        except ValueError:
            break
    return numbers


def dust_class(cls):
    """A dusteffect class's settings, defaults filled in."""
    props = cls["props"]
    out = {}
    for key, default in DUST.items():
        raw = _last(props, key)
        if raw is None:
            out[key] = default
        elif isinstance(default, str):
            out[key] = raw.strip()
        elif isinstance(default, list):
            numbers = _numbers(raw)
            out[key] = (numbers + default[len(numbers):])[:3]
        else:
            numbers = _numbers(raw)
            out[key] = int(numbers[0]) if numbers and isinstance(default, int) else (numbers[0] if numbers else default)
    return {
        "texture": out["Texture"],
        "count": max(0, int(out["NumParticles"])),
        "minPos": out["MinPos"], "maxPos": out["MaxPos"],
        "minVel": out["MinVel"], "maxVel": out["MaxVel"],
        "minSize": out["MinSize"], "maxSize": out["MaxSize"],
        "minLifetime": out["MinLifetime"], "maxLifetime": out["MaxLifetime"],
        "alpha": out["Alpha"],
        "minDistance": out["MinDistance"], "maxDistance": out["MaxDistance"],
        "radiusFade": [out["RadiusFadeMin"], out["RadiusFadeMax"]],
        "heightScale": out["HeightScale"],
        "radius": dust_radius(out),
    }


def dust_radius(settings):
    """How far the class's particles reach from the object: half the
    largest size, plus the radius fade's end when it has one, else the
    farthest start plus the fastest speed over the longest life."""
    reach = settings["MaxSize"] * 0.5
    if settings["RadiusFadeMin"] > 0:
        return reach + settings["RadiusFadeMax"]
    length = lambda v: math.sqrt(sum(c * c for c in v))
    speed = max(length(settings["MinVel"]), length(settings["MaxVel"]))
    start = max(length(settings["MinPos"]), length(settings["MaxPos"]))
    return reach + speed * settings["MaxLifetime"] + start


def dust_effects(records, classes):
    """The dusteffect objects placed in the world: their classes, and each
    copy's class and position (the XFRM's translation; the engine drops
    its rotation)."""
    kinds = {}
    instances = []
    for record in records:
        cls = classes.get(record["class"])
        if not cls or (cls.get("base") or "").lower() != "dusteffect":
            continue
        if record["class"] not in kinds:
            kinds[record["class"]] = dust_class(cls)
        if kinds[record["class"]]["count"]:
            instances.append({"class": record["class"], "position": list(record["xfrm"][9:12])})
    used = {i["class"] for i in instances}
    kinds = {name: kind for name, kind in kinds.items() if name in used}
    return {"classes": kinds, "instances": instances} if instances else None


def parse_atmosphere(fx_configs, records, classes):
    """The map's haze, or None where it has none."""
    found = {"fogCloud": fog_cloud(fx_configs), "dust": dust_effects(records, classes)}
    return found if any(found.values()) else None


def atmosphere_textures(atmosphere):
    if not atmosphere:
        return set()
    names = {kind["texture"] for kind in ((atmosphere.get("dust") or {}).get("classes") or {}).values()}
    if atmosphere.get("fogCloud"):
        names.add(atmosphere["fogCloud"]["texture"])
    return {name for name in names if name}
