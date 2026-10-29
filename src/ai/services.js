import { Vector3 } from "three";

// Repair jobs include destroyed buildings, which the living-target registry
// deliberately omits. Keep stable references to their underlying gameplay
// objects so the same fusion cutter lock completes the job.
export function createServiceCandidates(units, live, vehicles) {
  const buildings = new Map();
  return (center, radius) => {
    const candidates = [...units.all()].filter((u) => u.soldier && u.alive && !u.seat);
    for (const vehicle of vehicles?.vehicles || []) if (vehicle.aiTarget) candidates.push(vehicle.aiTarget);
    for (const building of live?.buildings || []) {
      // A destructible command post (docs/objectives.md) is no bot's repair
      // job: four pilots' fusion cutters put the shield generator back to
      // full faster than the Empire's infantry could hurt it, so no
      // objective ever fell. The port's choice; players still repair it.
      if (building.desc?.base === "commandpost") continue;
      let target = buildings.get(building);
      if (!target) {
        const bounds = { center: new Vector3(), radius: 0 };
        target = {
          building, kind: building.weapon ? 8 : 9, category: 4,
          classDef: building.desc.settings, className: building.desc.class,
          position: building.position, velocity: new Vector3(),
          get generation() { return building.aiTarget?.generation ?? 1; },
          get team() { return building.team; },
          get alive() { return building.alive; },
          get health() { return building.health; },
          get maxHealth() { return building.maxHealth; },
          get healthType() { return { building: 4, rebuild: 5, build: 6 }[building.healthType] ?? 4; },
          get sphere() { bounds.radius = building.sphere(bounds.center); return bounds; },
          aimPoint(out) { building.sphere(out); return out; },
          origin(out) { return this.aimPoint(out); },
          forward(out) { return out.set(0, 0, 1).transformDirection(building.matrix); },
        };
        buildings.set(building, target);
      }
      candidates.push(target);
    }
    return candidates.filter((u) => u.position.distanceToSquared(center) <= (radius + (u.sphere?.radius || 0)) ** 2);
  };
}
