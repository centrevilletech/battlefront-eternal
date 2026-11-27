"""Hilt bindings and visible blade edges from ordered 2005 weapon properties."""

from struct import iter_unpack

from zero_hash import crc


def attachment(name, actor, animation):
    key = f"{crc(name):08x}"
    for i, bone in enumerate((actor or {}).get("names", ())):
        if bone.lower() == name.lower():
            parent = actor["parents"][i]
            return {"name": name, "boneCrc": key, "parentCrc": f"{crc(parent):08x}" if parent and parent != bone else None,
                    "local": actor["local"][i]}
    for joint in (animation or {}).get("joints", ()):
        if joint["boneCrc"] == key:
            return {"name": name, "skeleton": animation["name"], **joint}
    raise ValueError(f"missing actor attachment {name}")


def texture_color(image):
    """A representative RGBA8 texel for a swatch of the blade image."""
    _, _, rgba = image
    pixels = iter_unpack("4B", rgba)
    def visibility(pixel):
        r, g, b, a = pixel
        return a * (max(r, g, b) - min(r, g, b)), a, r + g + b
    return max(pixels, key=visibility)


def saber_hilts(weapon, actor, animation, models, skeletons, textures):
    props = weapon["properties"]
    if not any(k == "LightSaberTexture" for k, _ in props):
        return []
    hilts = {}
    model, hand, socket = "", "hp_weapons", "hp_fire"
    blade = {}

    def finish():
        if "texture" not in blade:
            return
        if model not in models or model not in skeletons:
            raise ValueError(f"missing hilt model or skeleton {model!r}")
        rig = skeletons[model]
        transform = next((m for n, m in zip(rig["names"], rig["model"]) if n.lower() == socket), None)
        if transform is None:
            raise ValueError(f"missing hilt socket {model}/{socket}")
        tex = blade["texture"].lower()
        if tex not in textures:
            raise ValueError(f"missing blade texture {blade['texture']}")
        if (model, hand) not in hilts:
            hilts[model, hand] = {"model": model, "attachment": attachment(hand, actor, animation), "blades": []}
        hilts[model, hand]["blades"].append({"socket": socket, "transform": transform, "color": texture_color(textures[tex]),
                                             **blade})

    try:
        for key, value in props:
            if key in ("GeometryName", "OffhandGeometryName"):
                finish()
                blade = {}
                model = value.lower()
                hand = "hp_weapons" if key == "GeometryName" else "bone_l_hand"
                socket = "hp_fire"
            elif key in ("FirePointName", "OffhandFirePointName", "AttachedFirePoint"):
                finish()
                blade = {}
                if key != "AttachedFirePoint":
                    names = value.lower().split()
                    if len(names) not in (1, 2):
                        raise ValueError(f"invalid {key} {value!r}")
                    socket = names[0]
                    if len(names) == 2:
                        hand = names[1]
                else:
                    socket = ""
            elif key in ("LightSaberLength", "LightSaberWidth"):
                blade["length" if key == "LightSaberLength" else "width"] = float(value)
            elif key == "LightSaberTexture":
                blade["texture"] = value
            elif key == "LightSaberTrailColor":
                blade["trailColor"] = [int(c) for c in value.split()]
        finish()
    except ValueError as error:
        raise ValueError(f"{weapon['class']}: {error}") from error
    return list(hilts.values())
