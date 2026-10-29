"""Traps (ODF base "trap", the original game's EntityTrap): Endor's Ewok log
trap, end_weap_treesmash. docs/live-objects.md "Ewok log traps" has the
original game's reading of each key.

What travels in props.json's instance (entry["trap"]):

- trigger, build: the clips it swings in (TriggerAnimation) and is built
  back up in (BuildAnimation), from the bank AnimationName names.
- triggerTeam, resetTeam: the side ("all", "imp", "rep", "cis") whose
  vehicles set it off, and the side it belongs to (whose repair weapons
  build it; the original game gives a trap placed with Team 0 that side's
  team).
- radius: the half size of the box round the trigger point that a vehicle's
  collision sphere must meet (TriggerRadius, 3 m by default); node, offset:
  where that point is (TriggerNode, TriggerOffset; the stock trap names
  neither: its origin, where the logs meet).
- minRadius: only a thing whose collision sphere is larger sets it off or is
  crushed (MinEnemyRadius; 4 m stops soldiers and speeder bikes, not AT-STs).
- deathTime: seconds into the swing it crushes (DeathTime; < 0 never).
- deathEffect, deathEffectOffset: the effect played, in the trap's frame,
  when the crush killed something (DeathEffect, DeathEffectOffset).
- sound: TriggerSound (none in the stock game).
- pad: the build pad (BuildModelOdf's GeometryName) at BuildPoint's place in
  the trap's frame, and the hologram's place on it (its hp_hologram).
"""

TEAMS = ("all", "imp", "rep", "cis")


def _floats(text, count):
    try:
        values = [float(v) for v in str(text).split()]
    except ValueError:
        return None
    return values[:count] if len(values) >= count else None


def trap_settings(cls, class_prop, classes, skeletons):
    """A trap class's settings for props.json (see the module note), or None
    when it names no TriggerAnimation."""
    trigger = class_prop(cls, "TriggerAnimation")
    if not isinstance(trigger, str) or not trigger.strip():
        return None
    model = class_prop(cls, "GeometryName")
    skeleton = skeletons.get(model) or {}
    names = [n.lower() for n in skeleton.get("names", [])]

    def node_at(name):
        if isinstance(name, str) and name.lower() in names:
            return [round(v, 4) for v in skeleton["model"][names.index(name.lower())][12:15]]
        return None

    def team(key):
        side = str(class_prop(cls, key, "") or "").strip().lower()
        return side if side in TEAMS else None

    build = class_prop(cls, "BuildAnimation")
    out = {
        "trigger": trigger.strip().lower(),
        "build": build.strip().lower() if isinstance(build, str) and build.strip() else None,
        "triggerTeam": team("TriggerTeam"),
        "resetTeam": team("ResetTeam"),
        "radius": float(class_prop(cls, "TriggerRadius", 3.0)),
        "minRadius": float(class_prop(cls, "MinEnemyRadius", 0.0)),
        "deathTime": float(class_prop(cls, "DeathTime", -1.0)),
        "node": class_prop(cls, "TriggerNode"),
        "offset": _floats(class_prop(cls, "TriggerOffset", "0 0 0"), 3) or [0.0, 0.0, 0.0],
        "deathEffect": class_prop(cls, "DeathEffect"),
        "deathEffectOffset": _floats(class_prop(cls, "DeathEffectOffset", "0 0 0"), 3) or [0.0, 0.0, 0.0],
        "sound": class_prop(cls, "TriggerSound"),
    }
    if out["node"] is not None:
        out["node"] = str(out["node"]).lower()
    pad_class = classes.get(class_prop(cls, "BuildModelOdf") or "")
    if pad_class:
        pad_model = class_prop(pad_class, "GeometryName")
        pad_skeleton = skeletons.get(pad_model) or {}
        pad_names = [n.lower() for n in pad_skeleton.get("names", [])]
        mount = [0.0, 0.0, 0.0]
        if "hp_hologram" in pad_names:
            mount = [round(v, 4) for v in pad_skeleton["model"][pad_names.index("hp_hologram")][12:15]]
        out["pad"] = {"model": pad_model, "at": node_at(class_prop(cls, "BuildPoint")) or [0.0, 0.0, 0.0], "hologram": mount}
    return out
