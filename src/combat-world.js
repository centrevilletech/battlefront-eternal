// Collision reads for the page's crosshair and first-person presentation.
// World targeting and combat use the native registry and prepared services.
export class CombatWorld {
  constructor(combat) {
    this.combat = combat;
  }

  // The collision world's cast (src/collision.js), into `out`.
  cast(origin, direction, far, out, ignoreBody, staticOnly) {
    return this.combat.collision.cast(origin, direction, far, out, ignoreBody, staticOnly);
  }

  // The first unit along the crosshair's ray (Units.raycast; the page's).
  raycastUnits(origin, direction, far, out, filter) {
    return this.combat.ordnance?.units?.raycast?.(origin, direction, far, null, out, filter, true) ?? null;
  }

  presentation(event) {
    this.combat.onPresentation?.(event);
  }
}
