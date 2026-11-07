import * as THREE from "three";

// Drawn resource facts for the native camera phase. Unit identity and
// retained rig readers remain with the page until retained scene ownership.
const killerScratch = new THREE.Vector3();
export function drawnKiller(death, refs, follow, out) {
  if (!death || !refs) return null;
  if (death.killerHull >= 0) {
    // A hull credited by itself: while it is the same hull's life.
    const hull = refs.vehicles?.[death.killerHull];
    if (!hull?.alive || (hull.aiTarget?.generation ?? 0) !== death.killerHullLife) return null;
    hull.seatEye(0, out.point, true);
    out.size = 2 * (hull.radius || 3);
    out.living = true;
    out.friendly = (refs.relation?.(hull.team) ?? -1) > 0;
    return out;
  }
  if (death.killerUnit < 0) return null;
  let unit = refs.units?.byId(death.killerUnit) ?? null;
  if (unit && unit.generation !== death.killerLife) unit = null;
  let living = Boolean(unit?.alive && !unit.removed);
  if (!living && follow && death.killerCharacter >= 0) {
    const character = refs.characters?.[death.killerCharacter];
    const next = character?.unit;
    if (next && next.alive && !next.removed && (next.character ?? character) === character) {
      unit = next;
      living = true;
    }
  }
  if (!unit || unit.removed) return null;
  const seat = living ? unit.seat : null;
  if (seat?.vehicle) {
    // His seat's eye, as the hull is drawn.
    seat.vehicle.seatEye(seat.index ?? 0, out.point, true);
    out.size = 2 * (seat.vehicle.radius || 3);
  } else if (seat?.turret) {
    seat.origin(out.point);
    out.size = 3;
  } else {
    // His body as drawn: the eyes standing, crouched or prone, the middle
    // of a corpse.
    const feet = unit.actor?.root?.position ?? unit.presentation?.position ?? unit.position;
    const posture = unit.soldier?.posture;
    const eyes = !living ? 0.5 : unit.walker ? 1 : posture === "prone" ? 0.6 : posture === "crouch" ? 1.2 : 1.7;
    out.point.copy(feet ?? killerScratch.set(0, 0, 0)).y += eyes;
    out.size = unit.walker ? 1.6 : 1.9;
  }
  out.living = living;
  out.friendly = (refs.relation?.(unit.team) ?? -1) > 0;
  return out;
}
