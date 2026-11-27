"""The battle art named by classes in a prepared 2005 map."""
import hashlib
import json
import mmap
from pathlib import Path
import tempfile

from ucfb import chunks, iter_all_archives, node_name
from zero_hud_chunks2 import ArchiveBudget, checked_chunks
from zero_hud_icon import extent, parts, raster
from zero_outputs2 import check_existing, named_image_matches
from zero_texture import SWBF2_DECODABLE, decode_texture, write_png
from zero_paths import write_text_lf  # noqa: E402


BITMAP_FIELDS = {"maptexture", "icontexture", "healthtexture"}
MODEL_FIELDS = {"hudtag", "hudmodel"}


def references(classes, fields):
    # The admitted class tables include ancestors, weapons and ordnance.
    # Walk ordered pairs so a repeated field in another seat also gets its art.
    return {value.lower() for bucket in classes.values() for cls in bucket.values()
            for field, value in cls["props"] if value and field.lower() in fields}


def named_nodes(data, sections, tag, names):
    for start, end in sections:
        for kind, body, stop in chunks(data, start, end):
            if kind == tag:
                name = (node_name(data, body, stop) or "").lower()
                if name in names:
                    yield name, body, stop


def installed_nodes(game, tag, names):
    """Fetch only explicitly referenced resources outside the admitted packages."""
    if not names:
        return
    for path in sorted(game.rglob("*")):
        if path.suffix.lower() != ".lvl" or not path.is_file() or path.stat().st_size < 8:
            continue
        with path.open("rb") as stream, mmap.mmap(stream.fileno(), 0, access=mmap.ACCESS_READ) as data:
            for name, body, stop in named_nodes(data, iter_all_archives(data), tag, names):
                yield name, data[body:stop]


def pixels(image):
    width, height = image.width, image.height
    if not (0 < width <= 4096 and 0 < height <= 4096 and width * height <= 4_194_304):
        raise ValueError(f"Invalid class artwork dimensions {width} by {height}")
    return width, height, bytes(image.rgba)


def bitmap_images(names, textures, game):
    """Admitted art wins. Copies elsewhere must agree before they can supply a missing name."""
    images = {}
    for name, image in textures.items():
        key = name.lower()
        if key in names and key not in images:
            images[key] = pixels(image)
    missing = names - images.keys()
    ambiguous = set()
    for name, data in installed_nodes(game, b"tex_", missing):
        parsed = decode_texture(data, 0, len(data), lazy=True, decodable=SWBF2_DECODABLE)
        if not parsed or parsed[1] is None:
            raise ValueError(f"Unsupported installed class bitmap {name!r}")
        image = pixels(parsed[1])
        if images.setdefault(name, image) != image:
            ambiguous.add(name)
    for name in ambiguous:
        del images[name]
    return images, ambiguous


def named_models(names, sources, game):
    """Read the actual icon segments, including models without a HUD name prefix."""
    models = {}
    budget = ArchiveBudget()
    for data, sections in sources:
        for name, body, stop in named_nodes(data, sections, b"modl", names - models.keys()):
            if name not in models:
                fields = list(checked_chunks(data, body, stop, budget))
                models[name] = [parts(data, fields, budget)]
    # Keep the alternatives until they have been projected with their diffuse art.
    for name, data in installed_nodes(game, b"modl", names - models.keys()):
        fields = list(checked_chunks(data, 0, len(data), budget))
        models.setdefault(name, []).append(parts(data, fields, budget))
    return models


def check_art_directory(directory):
    # The map and its HUD folder belong to preparation. A library above them can be linked.
    check_existing(directory.parent, directory=True)
    check_existing(directory, directory=True)


def publish(directory, images, catalog):
    """An identifier is a catalog key. Only digest names ever become bitmap paths."""
    check_art_directory(directory)
    directory.mkdir(parents=True, exist_ok=True)
    record = directory / "catalog.json"
    check_existing(record, directory=False)
    paths = {}
    with tempfile.TemporaryDirectory(dir=directory) as temp:
        work = Path(temp)
        candidate = work / "image"
        for name, (width, height, rgba) in sorted(images.items()):
            write_png(candidate, width, height, rgba)
            file = hashlib.sha256(candidate.read_bytes()).hexdigest() + ".png"
            paths[name] = file
            candidate.replace(work / file)
        for file in work.iterdir():
            target = directory / file.name
            check_existing(target, directory=False)
            file.replace(target)
        catalog["textures"] = paths
        candidate = work / record.name
        write_text_lf(candidate, json.dumps(catalog, sort_keys=True))
        candidate.replace(record)


def catalog_current(directory):
    """Validate the PNGs a map needs, even if a damaged receipt also omits one."""
    try:
        check_art_directory(directory)
    except ValueError:
        return False
    record = directory / "catalog.json"
    check_existing(record, directory=False)
    catalog = json.loads(record.read_bytes())
    textures = catalog["textures"]
    if not isinstance(textures, dict) or not all(key in textures for key in catalog["meshes"].values()):
        return False
    # Only open entries enumerated locally. Catalog values never become paths.
    files = {p.name: p for p in directory.iterdir() if p.suffix == ".png"}
    return all(name in files and named_image_matches(files[name]) for name in set(textures.values()))


def loose_textures(textures, model_sets, taken=()):
    """The textures of a map's archives that no model draws and the world does not take: the
    art a battle's scripts and HUD name at run time (Space Assault's ship and system pictures)."""
    drawn = {name.lower() for name in taken}
    for models in model_sets:
        for parts in models.values():
            for part in parts:
                for key in ("texture", "bump"):
                    if part.get(key):
                        drawn.add(part[key].lower())
    return {name.lower() for name in textures if name.lower() not in drawn}


def write_map_art(directory, classes, textures, game, sources=(), loose=()):
    names = references(classes, BITMAP_FIELDS) | {name.lower() for name in loose}
    tags = references(classes, MODEL_FIELDS)
    models = named_models(tags, sources, game)
    diffuse = {part["texture"].lower() for choices in models.values() for model in choices
               for part in model if part["texture"]}
    bitmaps, ambiguous = bitmap_images(names | diffuse | (tags - models.keys()), textures, game)
    images = {name: bitmaps[name] for name in names if name in bitmaps}
    aliases = {}
    conflicting_models = ambiguous & (tags - models.keys())
    missing_diffuse = {}
    extents = {}
    for name in sorted(tags):
        if name not in models:
            if name in bitmaps:
                aliases[name] = name
                images[name] = bitmaps[name]
            continue
        choices = (raster(model, bitmaps.get) for model in models[name])
        projection = next(choices)
        if any(other != projection for other in choices):
            conflicting_models.add(name)
            continue
        if projection is None:
            continue
        key = "mesh." + name
        while key in names or key in tags or key in images:
            key = "mesh." + key
        aliases[name] = key
        images[key] = projection
        extents[key] = extent(models[name][0])
        absent = {part["texture"].lower() for model in models[name] for part in model
                  if part["texture"] and part["texture"].lower() not in bitmaps}
        if absent:
            missing_diffuse[name] = sorted(absent)
    publish(directory, images, {
        "meshes": aliases, "missingDiffuse": missing_diffuse,
        "bounds": {key: box for key, box in sorted(extents.items()) if box},
        "missing": {"textures": sorted(names - bitmaps.keys() - ambiguous),
                    "meshes": sorted(tags - aliases.keys() - conflicting_models)},
        "ambiguous": {"textures": sorted(ambiguous), "meshes": sorted(conflicting_models)},
    })
