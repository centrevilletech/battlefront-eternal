"""Power-ups the world places (base powerupitem), as against those a
dispenser throws or a soldier drops: Yavin 4: Arena's one
com_item_powerup_health100. The original game creates every world instance
through its class, so a placed power-up is an ordinary item from the
battle's start: it falls under its Gravity, anyone may take it, and it
lasts its class's LifeSpan like any other (the item update compares its
age with the class's LifeSpan, which no placement changes).
docs/live-objects.md "Placed power-ups"."""

from zero_weapons import item_class

ITEM_BASES = ("powerupitem",)


def placed_items(records, classes, archives):
    """Each placed power-up: its class, where it stands (the instance's
    4x4, column-major, as props.json's instances give theirs), its team
    and its class's item settings (tools/zero_weapons.py item_class)."""
    out = []
    for record in records:
        cls = classes.get(record["class"])
        if not cls or (cls["base"] or "").lower() not in ITEM_BASES:
            continue
        item = item_class(archives, record["class"])
        if not item:
            continue
        props = dict(record["props"])
        out.append({"class": record["class"], "name": record["name"], "team": int(float(props.get("Team", "0"))),
                    "xfrm": record["xfrm"], "item": item})
    return out
