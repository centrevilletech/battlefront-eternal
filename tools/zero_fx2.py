"""Prepare the visual effect library selected by a Battlefront II mission."""

from collections import Counter, defaultdict
import json
import math
from pathlib import Path
import re
import struct
import sys
import tempfile
from contextlib import contextmanager

from ucfb import align4, child, chunks, iter_all_archives, node_name, u32
from xform import IDENTITY, mat4, mul
from zero_config import parse
from zero_environment import attachments
from zero_family import SWBF2, profile
from zero_fx import NAMES, emitters, textures as emitter_assets
from zero_hash import fnv1a
from zero_mission2 import mission_assets
from zero_model import lod_name, parse_model, parse_skeleton
from zero_paths import child as game_path
from zero_scripts2 import write_if_changed
from zero_swbf2 import class_catalog, package_sections, property_name, resolve_class
from zero_texture import collect_textures, write_png, SWBF2_DECODABLE
from zero_vehicle import FX_KEYS
from zero_weapons import effective
from zero_world import instance_records

# Additional format vocabulary also documented by swbf-unmunge (MIT).
EXTRA = """LightBeam Length InitialWidth FinalWidth FlareIntensity FadeFactor FadeLength
BeamIntensity DrawDistance EntityLight ConeLength FlickerType OmniRadius ShadowCaster ProjectedTexture
BlurValue BlurRes Looping ViewAngleFactor ScrollRate""".split()
CONFIG_NAMES = {**NAMES, **{fnv1a(key): key for key in EXTRA}}
DUST_FIELDS = """SpawnDistance MaxRandomSideOffset CenterDeadZoneRadius MinParticleScale MaxParticleScale
SpawnDelay ReferenceSpeed DustParticleSpeed SpeedParticleMinLength SpeedParticleMaxLength
ParticleLengthMinSpeed ParticleLengthMaxSpeed""".split()
# A world's bloom (its Effect("HDR")) and the values the game gives a field
# the world leaves out.
HDR_FIELDS = {"DownSizeFactor": 0.25, "NumBloomPasses": 5, "MaxTotalWeight": 1.2, "GlowThreshold": 0.5, "GlowFactor": 1.0}
CONFIG_NAMES.update({fnv1a(key): key for key in ["Effect", "Enable", *DUST_FIELDS, *HDR_FIELDS]})
PACK_VERSION = 10
# Every bolt and beam draws its glow with this shared texture, which the game keeps beside its other
# effect textures and no class names.
GLOW_TEXTURE = "lightglow"
GLOW_ROUND_BASES = ("bolt", "laser", "bullet", "fatray", "beam")
MAX_EFFECT_DEPTH = 32
PARTICLE_LIMIT = 4096
FLASH_FIELDS = ("MuzzleFlash", "FlashRadius", "FlashLength", "FlashColor",
                "FlashLightColor", "FlashLightRadius", "FlashLightDuration")
ODF_NAMES = {f"{fnv1a(key):08x}": key
             for key in (*FX_KEYS, *FLASH_FIELDS, "Discharge", "AddParticleEffect", "ExplosionCritical")}
# MuzzleFlash selects a shared mesh preset (handled by FLASH_FIELDS) and
# Discharge is an unused legacy field; keeping both in ODF_NAMES stops them
# from falling through to the raw-hash fallback in bindings().
# A guided rocket names a second explosion under ExplosionCritical (e04f08c1);
# with its name the walk follows it to that explosion's effects.
EFFECT_FIELDS = set("""
MuzzleFlashEffect TrailEffect OrdnanceEffect ImpactEffect ImpactEffectTerrain
ImpactEffectSoft ImpactEffectRigid ImpactEffectStatic ImpactEffectWater ImpactEffectShield Effect WaterEffect
HitEffect AddHitEffect DeflectEffect ChargeUpEffect LightningEffect PlayEffectOnOwner PlayEffectOnOwnerAimer
SmolderEffect ThrustEffect ContrailEffect JetEffect DamageEffect DeathEffect DeathDustEffect ChunkSmokeEffect
ChunkTrailEffect ChunkTerrainEffect AttachEffect AddParticleEffect ShieldEffect StompEffect WakeEffect
WaterSplashEffect WakeWaterSplashEffect DestructChargeEffect
""".split())


def skip_resource(skipped, name, reason):
    skipped.append({"resource": name, "reason": reason})
    print(f"Skipping effect resource {name}: {reason}", file=sys.stderr, flush=True)


@contextmanager
def resource(skipped, name):
    """An invalid cosmetic resource leaves the rest of the mission available."""
    try:
        yield
    except (ValueError, TypeError, IndexError, KeyError, AttributeError, ArithmeticError, RecursionError, struct.error) as exc:
        skip_resource(skipped, name, f"{type(exc).__name__}: {exc}")


def finite(value):
    n = float(value)
    if not math.isfinite(n):
        raise ValueError(f"non-finite number {value}")
    return n


def read_config(data, start, end):
    """Check the bounds of each scope before the shared config reader enters it."""
    pending = [(start, end, 0)]
    while pending:
        pos, limit, depth = pending.pop()
        if depth > MAX_EFFECT_DEPTH:
            raise ValueError(f"effect scope nesting exceeds {MAX_EFFECT_DEPTH}")
        while pos < limit:
            tag = data[pos:pos + 4]
            if tag == b"\0\0\0\0" and pos + 4 <= limit:
                pos += 4
                continue
            body = pos + 8
            if body > limit or not all(32 <= c < 127 for c in tag):
                raise ValueError("truncated or invalid chunk header inside effect scope")
            stop = body + u32(data, pos + 4)
            if stop > limit:
                raise ValueError(f"truncated {tag.decode()} chunk inside effect scope")
            if tag == b"SCOP":
                pending.append((body, stop, depth + 1))
            elif tag == b"DATA":
                if stop - body < 5:
                    raise ValueError("truncated DATA header")
                count = data[body + 4]
                if body + 5 + 4 * count > stop:
                    raise ValueError(f"DATA value count {count} exceeds its chunk")
            pos = align4(stop)
    return parse(data, start, end, CONFIG_NAMES)


def check_record_depth(record):
    """Record roots sit two levels below the pack root in the native JSON reader."""
    pending = [(record, 0)]
    while pending:
        value, depth = pending.pop()
        if isinstance(value, (dict, list, tuple)):
            if depth >= MAX_EFFECT_DEPTH:
                raise ValueError(f"prepared effect nesting exceeds {MAX_EFFECT_DEPTH}")
            children = value.values() if isinstance(value, dict) else value
            pending.extend((child, depth + 1) for child in children)


def validate_config(tree):
    numeric = {"BurstCount", "BurstDelay", "StartDelay", "MaxParticles", "Lifetime",
               "BlurValue", "BlurRes", "TimePerFrame", "FrameSize", "TotalFrames",
               "Length", "InitialWidth", "FinalWidth", "BeamIntensity", "FlareIntensity"}
    pending = [tree]
    while pending:
        for key, values, children in pending.pop():
            if key in numeric and (not values or any(not isinstance(n, (float, int)) for n in values)):
                raise ValueError(f"{key} requires numeric values")
            if any(isinstance(n, float) and not math.isfinite(n) for n in values):
                raise ValueError(f"{key} contains a non-finite number")
            if children:
                pending.append(children)


def checked_fields(fields, strings, vectors=()):
    for key, value in fields.items():
        if key in strings:
            if not isinstance(value, str) or not value:
                raise ValueError(f"{key} needs a name")
        elif key in vectors:
            if not isinstance(value, list) or not value:
                raise ValueError(f"{key} needs numeric components")
            for n in value:
                finite(n)
        else:
            finite(value)
    return fields


def identifier(name):
    if not isinstance(name, str) or not re.fullmatch(r"[a-zA-Z0-9_-]+", name):
        raise ValueError(f"Invalid effect package identifier {name!r}")
    return name


def sources(game, map_id, assets):
    family = profile(SWBF2)
    paths = [(family.map_relpath(map_id), assets["layers"])]
    paths.extend((family.side_relpath(identifier(side)), names) for side, names in assets["sides"].items())
    if assets["sky"]:
        paths.append((f"{family.planet_folder(map_id)}/spa_sky.lvl", [assets["sky"]]))
    result = []
    for rel, names in paths:
        path = game_path(game, *Path(rel).parts)
        if path is None:
            raise FileNotFoundError(rel)
        data = path.read_bytes()
        # A map without a requested layer uses its root alone. A side without
        # individual package names is loaded whole.
        sections = package_sections(data, names)
        if rel == paths[0][0] and not names:
            sections = sections[:1]
        result.append((data, sections))
    for path in family.shared_archive_paths(game):
        data = path.read_bytes()
        result.append((data, list(iter_all_archives(data))))
    return result


def config_extras(tree):
    """Keep named non-particle blocks and fields the particle player omits."""
    extra = []
    for key, values, children in tree:
        if key in ("LightBeam", "EntityLight"):
            extra.append([key, values, children])
        elif key not in NAMES.values() or key in ("MinLodDist", "MaxLodDist", "BoundingRadius", "SoundName"):
            extra.append([key, values, children])
        else:
            nested = config_extras(children)
            if nested:
                extra.append([key, values, nested])
    return extra


def resources(archives):
    effects, models, rigs, textures = {}, {}, {}, {}
    for data, sections in archives:
        for start, end in sections:
            nodes = chunks(data, start, end)
            skeletons = {}
            for tag, b, e in nodes:
                if tag == b"skel":
                    rig = parse_skeleton(data, b, e)
                    if rig:
                        name = rig["name"].lower()
                        rigs.setdefault(name, rig)
                        skeletons[name] = rig
            for tag, b, e in nodes:
                if tag == b"fx__":
                    name = child(data, b, e, b"NAME")
                    if name:
                        effects.setdefault(u32(data, name[0]), (data, b, e))
                elif tag == b"modl":
                    name, full = lod_name(node_name(data, b, e) or "")
                    if full:
                        models.setdefault(name.lower(), (data, b, e, skeletons))
        collect_textures(data, textures, sections, decodable=SWBF2_DECODABLE)
    return effects, models, rigs, textures


def class_target(key):
    if key.startswith("WeaponName") or key in ("Weapon", "OffhandWeapon"):
        return ("wpnc",)
    if key.startswith("Explosion") or key == "FinalExplosion":
        return ("expc",)
    if key.endswith("OrdnanceName"):
        return ("ordc", "entc")
    if key.startswith("Class") or key in ("AttachOdf", "DropItemClass", "SpawnClass"):
        return ("entc",)
    return ()


ROUND_FIELDS = set("""
Velocity LifeSpan Range Gravity LaserTexture LaserWidth LaserLength LaserGlowColor GlowLength BlurLength
FadeOutTime LightColor LightRadius ImpactEffect ImpactEffectTerrain ImpactEffectStatic ImpactEffectSoft
ImpactEffectRigid ImpactEffectWater ImpactEffectShield
""".split())


def glow_textures(rounds):
    """The shared glow texture, when a round that draws a glow strip is in the pack."""
    return {GLOW_TEXTURE} if any(r.get("base") in GLOW_ROUND_BASES for r in rounds.values()) else set()


def bindings(catalog, seeds, definitions, links=None, rounds=None, flashes=None, skipped=None):
    """Walk owned slots, ordnance, explosions, attachments and spawned entities."""
    skipped = [] if skipped is None else skipped
    pending = list(seeds)
    visited = set()
    result, classes, laser_textures = {}, {}, set()
    while pending:
        tag, name = pending.pop()
        name = name.lower()
        if (tag, name) in visited:
            continue
        visited.add((tag, name))
        cls = resolve_class(tag, name, catalog)
        if cls is None:
            continue
        if problem := cls.get("parentProblem"):
            skip_resource(skipped, f"{tag}/{name}", f"class parent {problem['parent']!r}: {problem['reason']}")
            continue
        props = [(ODF_NAMES.get(k, k), v) for k, v in cls["props"]]
        if tag == "entc":
            classes[name] = props
        if rounds is not None and tag == "ordc" and cls["engineBase"] in ("bolt", "laser", "bullet", "fatray", "beam"):
            with resource(skipped, f"ordc/{name}"):
                fields = effective(cls, ROUND_FIELDS)
                string_keys = {"LaserTexture", *(key for key in fields if key.startswith("ImpactEffect"))}
                checked_fields(fields, string_keys, ("LaserGlowColor", "LightColor"))
                rounds[f"ordc/{name}"] = {"base": cls["engineBase"], **fields}
        if flashes is not None and tag == "wpnc":
            settings = effective({"props": props}, FLASH_FIELDS)
            if settings:
                flashes[f"wpnc/{name}"] = {"disabled": True}
                with resource(skipped, f"wpnc/{name} flash"):
                    flashes[f"wpnc/{name}"] = checked_fields(settings, ("MuzzleFlash",), ("FlashColor", "FlashLightColor"))
        refs = defaultdict(list)
        for key, value in props:
            words = value.split()
            if not words:
                continue
            target = words[0].lower()
            for next_tag in class_target(key):
                if target in catalog[next_tag]:
                    if links is not None:
                        owners = links.setdefault(f"{tag}/{name}", {})
                        owners.setdefault(key, []).append(f"{next_tag}/{target}")
                    pending.append((next_tag, target))
                    break
            if key == "LaserTexture":
                laser_textures.add(value)
            # Preserve even an unidentified property when its value resolves
            # to an effect. Conversely keep a known field with a missing group.
            if key in EFFECT_FIELDS or (re.fullmatch(r"[0-9a-f]{8}", key) and fnv1a(target) in definitions):
                refs[key].append(words[0])
        if refs:
            result[f"{tag}/{name}"] = dict(refs)
    return result, classes, laser_textures


def placements(instances, classes, rigs):
    placed, missing = [], set()
    for instance in instances:
        name = instance["class"].lower()
        props = [*classes.get(name, ()), *instance["props"]]
        model = dict(props).get("GeometryName", name).lower()
        rig = rigs.get(model)
        joints = {n.lower(): m for n, m in zip(rig["names"], rig["model"])} if rig else {}
        for effect, node in attachments(props):
            local = joints.get(node.lower())
            if local is None:
                missing.add((name, effect, node))
            else:
                placed.append({"effect": effect.lower(), "owner": instance["name"], "hardpoint": node,
                               "matrix": mul(mat4(instance["xfrm"]), local)})
    return placed, sorted(missing)


def mounting_points(classes, rigs, skipped=None):
    """Resolve each exhaust, contrail and damage socket independently."""
    skipped = [] if skipped is None else skipped
    result, missing = {}, set()
    for name, props in classes.items():
        settings = dict(props)
        rig = rigs.get(settings.get("GeometryName", name).lower())
        joints = {n.lower(): m for n, m in zip(rig["names"], rig["model"])} if rig else {}
        entries, current = [], {}
        for key, value in props:
            for kind, first in (("Damage", "StartPercent"), ("Contrail", "Effect"), ("Thrust", "Effect")):
                if key.startswith(kind):
                    field = key[len(kind):]
                    if field == first:
                        current[kind] = {}
                        entries.append((kind, current[kind]))
                    if kind in current:
                        current[kind][field] = value
                    break
        mounts = []
        for kind, fields in entries:
            if "Effect" not in fields:
                continue
            effect = fields["Effect"].lower()
            with resource(skipped, f"{name}/{kind}/{effect}"):
                entry = {"kind": kind.lower(), "effect": effect}
                node = fields.get("AttachPoint", "").lower()
                if node:
                    entry["joint"] = node
                local = joints.get(node) if node else IDENTITY
                if local is None:
                    missing.add((name, effect, node))
                    continue
                shift = IDENTITY.copy()
                if "AttachOffset" in fields:
                    offset = [finite(v) for v in fields["AttachOffset"].split()]
                    if len(offset) != 3:
                        raise ValueError("attachment offset needs three coordinates")
                    shift[12:15] = offset
                for key in ("StartPercent", "StopPercent", "EffectScale", "InheritVelocity",
                            "EffectMinScale", "EffectMaxScale", "EffectScaleStart", "EffectMinSpeed"):
                    if key in fields:
                        entry[key] = finite(fields[key])
                entry["matrix"] = [finite(n) for n in mul(local, shift)]
                mounts.append(entry)
        if mounts:
            with resource(skipped, f"{name}/mount limits"):
                result[name] = {"mounts": mounts, "maxHealth": finite(settings.get("MaxHealth", 0)),
                                "minSpeed": finite(settings.get("MinSpeed", 0)), "maxSpeed": finite(settings.get("MaxSpeed", 0))}
    return result, missing


def light_beams(tree):
    result = []
    for key, _, children in tree:
        if key != "LightBeam":
            continue
        fields = {k: v for k, v, _ in children}
        def widths(key):
            v = fields.get(key, [0.])
            return [finite(v[0]), finite(v[-1])]
        record = {"length": fields.get("Length", [0.])[0], "start": widths("InitialWidth"),
                       "end": widths("FinalWidth"), "color": (fields.get("Color", []) + [255.] * 4)[:4],
                       "beam": fields.get("BeamIntensity", [0.])[0], "flare": fields.get("FlareIntensity", [0.])[0],
                       "fade": fields.get("FadeFactor", [1.])[0], "fadeLength": fields.get("FadeLength", [1.])[0],
                       "distance": fields.get("DrawDistance", [0.])[0]}
        result.append(checked_fields(record, (), ("start", "end", "color")))
    return result


def particle_materials(tree):
    """Geometry blocks in the same order as the shared particle reader."""
    for key, _, children in tree:
        if key == "ParticleEmitter":
            yield next((body for tag, _, body in children if tag == "Geometry"), ())
            yield from particle_materials(children)
        elif key in ("LightningBolt", "Shield"):
            yield ()


def blur_settings(tree, particles):
    for body, particle in zip(particle_materials(tree), particles, strict=True):
        geometry = particle.get("geometry", {})
        if geometry.get("blend") == "blur":
            fields = {key: values for key, values, _ in body}
            geometry["blurValue"] = fields.get("BlurValue", [0.])[0]
            geometry["blurRes"] = fields.get("BlurRes", [0.])[0]
        if geometry.get("emitters"):
            blur_settings(body, geometry["emitters"])


def space_dust(tree):
    for key, kind, entries in tree:
        if key != "Effect" or [str(name).lower() for name in kind] != ["spacedust"]:
            continue
        fields = {key: values[0] for key, values, _ in entries if values}
        if fields.get("Enable") and fields.get("Texture"):
            return {"Texture": fields["Texture"].lower(), **{k: finite(fields[k]) for k in DUST_FIELDS if k in fields}}
    return None


def hdr_bloom(tree):
    """The world's bloom block, whatever its Enable says: the game makes the
    bloom for a world that declares one and the player's light bloom option
    turns it on or off. None for a world without one."""
    for key, kind, entries in tree:
        if key != "Effect" or [str(name).lower() for name in kind] != ["hdr"]:
            continue
        fields = {key: values[0] for key, values, _ in entries if values}
        return {k: finite(fields.get(k, default)) for k, default in HDR_FIELDS.items()}
    return None


def beam_image(path):
    # A radial alpha mask for the flare. The tapered beam samples its white center.
    size = 33
    rgba = bytearray()
    for y in range(size):
        for x in range(size):
            r2 = (2 * x / (size - 1) - 1) ** 2 + (2 * y / (size - 1) - 1) ** 2
            rgba.extend((255, 255, 255, round(255 * max(0., 1. - r2) ** 2)))
    write_png(path, size, size, bytes(rgba))


def scope_models(group):
    for emitter in group:
        geometry = emitter.get("geometry", {})
        if model := geometry.get("model"):
            geometry["model"] = f"effects/{model.lower()}"
        scope_models(geometry.get("emitters", ()))


def population(group):
    count = 0
    for e in group:
        if "geometry" not in e:
            continue
        # The player limits each burst to 128 births and intervals to at least 1 ms.
        interval = max(0.001, min(map(finite, e["burstDelay"])))
        lifetime = finite(e["transformer"]["lifetime"])
        bursts = 1 + math.ceil(min(float(PARTICLE_LIMIT), max(0., lifetime) / interval))
        live = math.ceil(min(128, max(map(finite, e["burstCount"])))) * bursts
        limits = [finite(n) for n in e["maxParticles"]]
        if min(limits) >= 0:
            live = min(live, math.ceil(min(PARTICLE_LIMIT, max(limits))))
        count += max(0, live) * (1 + population(e["geometry"].get("emitters", ())))
    return min(PARTICLE_LIMIT, count)


def particle_budget(effects, bindings, placed):
    """Reserve for one simultaneous activation per source, up to the shared display limit."""
    sources = Counter(n.lower() for fields in bindings.values() for names in fields.values() for n in names)
    sources.update(item["effect"] for item in placed)


    total = sum(sources[name] * population(group) for name, group in effects.items())
    return 1 << (min(PARTICLE_LIMIT, max(1, total)) - 1).bit_length()


def write_pack(game, root, map_id, manifest):
    from extract import write_meshes, write_textures

    mission = identifier(manifest["script"])
    assets = mission_assets(manifest, identifier(map_id))
    archives = sources(game, map_id, assets)
    definitions, model_nodes, rigs, texture_nodes = resources(archives)
    catalog = class_catalog(archives)
    world, spans = archives[0]
    instances = [obj for start, end in spans for tag, b, e in chunks(world, start, end) if tag == b"wrld"
                 for obj in instance_records(world, b, e, property_name)]
    seeds = {("entc", name) for packages in assets["sides"].values() for name in packages}
    seeds.update(("entc", obj["class"]) for obj in instances)
    # Spawn-pad overrides can refer to classes not mentioned in their ODF.
    seeds.update((tag, value) for obj in instances for key, value in obj["props"] for tag in class_target(key))
    links, rounds, flashes = {}, {}, {}
    skipped = []
    bound, classes, textures = bindings(catalog, sorted(seeds), definitions, links, rounds, flashes, skipped)
    textures |= glow_textures(rounds)
    world_effects = definitions.get(fnv1a(map_id))
    dust = hdr = None
    if world_effects:
        with resource(skipped, f"{map_id}/space dust"):
            dust = space_dust(read_config(*world_effects))
        with resource(skipped, f"{map_id}/bloom"):
            hdr = hdr_bloom(read_config(*world_effects))
    if dust:
        textures.add(dust["Texture"])
    mounts, missing_mounts = mounting_points(classes, rigs, skipped)
    placed, missing_nodes = [], set()
    for obj in instances:
        with resource(skipped, f"{obj['name']}/attachments"):
            attached, absent = placements([obj], classes, rigs)
            for entry in attached:
                entry["matrix"] = [finite(n) for n in entry["matrix"]]
            placed.extend(attached)
            missing_nodes.update(absent)
    wanted = {name.lower() for fields in bound.values() for values in fields.values() for name in values}
    wanted.update(item["effect"] for item in placed)
    effects, extensions, beams, models, missing = {}, {}, {}, set(), []
    for name in sorted(wanted):
        node = definitions.get(fnv1a(name))
        if node is None:
            missing.append(name)
            continue
        with resource(skipped, name):
            tree = read_config(*node)
            # Check even fields beyond the subset used for drawing.
            validate_config(tree)
            particles = emitters(tree)
            blur_settings(tree, particles)
            population(particles)
            beam = light_beams(tree)
            extra = config_extras(tree)
            check_record_depth(particles)
            check_record_depth(extra)
            used, meshes = emitter_assets(particles)
            scope_models(particles)
            effects[name] = particles
            if beam:
                beams[name] = beam
            if extra:
                extensions[name] = extra
            textures.update(used)
            models.update(m.lower() for m in meshes)
        if name not in effects:
            missing.append(name)
    # Guns with flash settings share a mesh, tinted and timed by the weapon.
    muzzle_model = "com_sfx_muzzleflash" if any("MuzzleFlash" in fields for fields in flashes.values()) else None
    if muzzle_model:
        models.add(muzzle_model)
    meshes = {}
    for name in sorted(models):
        if name in model_nodes and (model := parse_model(*model_nodes[name], swbf2=True)):
            meshes[name] = model[1]
            textures.update(part[k] for part in model[1] for k in ("texture", "bump") if part.get(k))

    out = root / "maps" / map_id / "effects" / mission
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=out) as tmp:
        stage = Path(tmp)
        packed = write_meshes(meshes, sorted(meshes), stage, "models", swbf2=True)
        paths = write_textures(texture_nodes, textures, stage / "textures")
        if beams:
            beam_image(stage / "beam.png")
        data = {"version": PACK_VERSION, "rounds": rounds, "flashes": flashes, "beams": beams, "effects": effects,
                "spaceDust": dust, "hdr": hdr, "particleBudget": particle_budget(effects, bound, placed),
                "models": packed, "textures": paths, "bindings": bound,
                "muzzleModel": f"effects/{muzzle_model}" if muzzle_model in packed else None,
                "instances": placed, "mounts": mounts, "links": links, "extensions": extensions,
                "missing": {"effects": missing, "invalid": skipped, "models": sorted(models - meshes.keys()),
                            "textures": sorted(textures - paths.keys()), "hardpoints": sorted(set(missing_nodes) | missing_mounts)}}
        # Publish complete resources before their description. Replacing files
        # also leaves any hard-linked copy of a previous preparation intact.
        for file in sorted(stage.rglob("*")):
            if file.is_file():
                write_if_changed(out / file.relative_to(stage), file.read_bytes())
        write_if_changed(out / "effects.json", json.dumps(data, sort_keys=True, allow_nan=False).encode())
    print(f"{map_id}: {len(effects)}/{len(wanted)} named effects, {len(placed)} placed emitters, "
          f"{len(packed)}/{len(models)} models, {len(paths)}/{len(textures)} textures, "
          f"{len(skipped)} invalid effect resources skipped", flush=True)


def pack_current(out):
    try:
        pack = json.loads((out / "effects.json").read_bytes())
        # A pack from before the world's bloom was read is prepared again.
        if pack.get("version") != PACK_VERSION or "hdr" not in pack:
            return False
        if any(not isinstance(pack.get(k), dict) for k in
               ("effects", "models", "textures", "bindings", "mounts", "links", "rounds", "beams")):
            return False
        # A pack from before the glow texture was exported is prepared again, unless the game has none.
        if glow_textures(pack["rounds"]) and GLOW_TEXTURE not in pack["textures"] \
                and GLOW_TEXTURE not in pack.get("missing", {}).get("textures", ()):
            return False
        files = list(pack["textures"].values())
        if pack["models"]:
            files.extend(("models.bin", "models_index.bin"))
        if pack["beams"]:
            files.append("beam.png")
        return all(not Path(name).is_absolute() and ".." not in Path(name).parts and (out / name).is_file()
                   for name in files)
    except (AttributeError, KeyError, OSError, RecursionError, TypeError, ValueError):
        return False


def add_step(steps, state, root, map_id, mission, action):
    from prepare import step

    key = f"effects:{map_id}"
    out = root / "maps" / identifier(map_id) / "effects" / identifier(mission)
    if state.done(key) and not pack_current(out):
        state.data["done"].remove(key)
    steps.append(step(key, "core", f"{map_id} effects", 1, True, action))
    return (out / "effects.json").relative_to(root).as_posix()


if __name__ == "__main__":
    from zero_paths import GAME, PUBLIC
    write_pack(GAME, PUBLIC, sys.argv[1], json.loads(Path(sys.argv[2]).read_bytes()))
