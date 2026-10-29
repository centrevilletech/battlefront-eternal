import * as THREE from "three";

// The droideka rigs contain two physical tubes per arm, but no individual
// muzzle markers. Locate the end rings once, in the arm's own coordinates.
export function barrelTips(points, forward = "z", sign = 1, across = "x") {
  if (!points.length) return [];
  let front = -Infinity, back = Infinity;
  for (const p of points) { const depth = p[forward] * sign; front = Math.max(front, depth); back = Math.min(back, depth); }
  const tolerance = Math.max(0.003, (front - back) * 0.005);
  const ends = points.filter(p => p[forward] * sign >= front - tolerance);
  const bounds = new THREE.Box3().setFromPoints(ends);
  const middle = (bounds.min[across] + bounds.max[across]) / 2;
  const halves = [new THREE.Box3(), new THREE.Box3()];
  for (const p of ends) halves[p[across] > middle ? 1 : 0].expandByPoint(p);
  return halves.filter(box => !box.isEmpty()).map(box => {
    const tip = box.getCenter(new THREE.Vector3());
    tip[forward] = sign * (front + 0.002);
    return tip;
  });
}

// Held guns face +Z in model space. Imported anchors are authoritative;
// recover a missing or zeroed legacy marker from the visible mouth.
// The SBD's tiny invisible wrist marker is deliberately exempt: its barrel belongs to the actor.
export function weaponMuzzle(parts, authored, def) {
  if (!def?.ordnance || def.base === "grenade") return authored;
  const points = [];
  for (const part of parts || []) {
    const a = part.geometry.attributes.position;
    for (let i = 0; i < a.count; i++) points.push(new THREE.Vector3().fromBufferAttribute(a, i));
  }
  if (!points.length) return authored;
  const bounds = new THREE.Box3().setFromPoints(points);
  if (bounds.max.z - bounds.min.z < .05) return authored;
  // The importer resolves the authored axis against the mesh once. Do not
  // raycast an already resolved point again: a hollow barrel's back wall
  // can otherwise pull its muzzle progressively inside on each load.
  if (authored && authored[2] > .05) return authored;
  const front = points.filter(p => p.z >= bounds.max.z - .003);
  const tip = new THREE.Box3().setFromPoints(front).getCenter(new THREE.Vector3());
  tip.z = bounds.max.z;
  return tip.toArray();
}

// Resolve a pivot to its own descendant fire markers, stopping at another
// weapon's aimers. This keeps the AT-AT chin guns out of its side turrets.
export function descendantMuzzles(node, byName, excluded = new Set()) {
  const names = new Map(Object.entries(byName).map(([name, bone]) => [bone, name]));
  const found = [];
  const visit = bone => {
    if (bone !== node && excluded.has(bone)) return;
    if (/^hp_(fire|gun(?!ner)|cannon|laser|turret)/i.test(names.get(bone) || '')) { found.push(bone); return; }
    for (const child of bone.children) visit(child);
  };
  if (node) visit(node);
  return found;
}

// Vehicle packs contain both rigid bone-local parts and skinned parts whose
// vertices are in model space. Convert the latter through the inverse bind
// before measuring a barrel; otherwise its muzzle moves twice with the rig.
export function vehicleMuzzleTips(rig, parts) {
  const world = [], inverse = [], points = new Map();
  const resolve = i => world[i] ||= (rig.bones[i].parent >= 0 ? resolve(rig.bones[i].parent).clone() : new THREE.Matrix4())
    .multiply(new THREE.Matrix4().fromArray(rig.bones[i].local));
  rig.bones.forEach((b, i) => { inverse[i] = resolve(i).clone().invert(); points.set(b.name.toLowerCase(), []); });
  for (const part of parts) {
    const a = part.geometry.attributes.position;
    for (let i = 0; i < a.count; i++) {
      const joint = part.joints?.[i];
      const name = joint !== undefined ? rig.bones[joint]?.name.toLowerCase() : part.bone?.toLowerCase();
      if (!points.has(name)) continue;
      const p = new THREE.Vector3().fromBufferAttribute(a, i);
      if (joint !== undefined) p.applyMatrix4(inverse[joint]);
      points.get(name).push(p);
    }
  }
  const tips = new Map();
  for (const [name, vertices] of points) {
    if (!vertices.length) continue;
    const bounds = new THREE.Box3().setFromPoints(vertices);
    if (bounds.max.z - bounds.min.z < .05) continue;
    const mouth = vertices.filter(p => p.z >= bounds.max.z - .003);
    const center = new THREE.Box3().setFromPoints(mouth).getCenter(new THREE.Vector3());
    center.z = bounds.max.z; tips.set(name, center.toArray());
  }
  return tips;
}

// Only identities cross the worker boundary. Resolve the selected barrel
// after presentation has posed the local rig, without advancing its cycle.
export function bindMountedRound(round, mount, seat = -1, weapon = -1, barrel = -1) {
  if (!round || typeof round !== 'object') return;
  round.mount = mount; round.mountSeat = seat; round.mountWeapon = weapon; round.mountBarrel = barrel;
}
const mountedPoint = new THREE.Vector3();
export function mountedMuzzle(round) {
  const mount = round.mount;
  if (!mount || mount.alive === false) return null;
  const node = round.mountSeat >= 0
    ? mount.seats?.[round.mountSeat]?.weapons?.[round.mountWeapon]?.muzzles?.[round.mountBarrel]?.fireNode
    : mount.firePoints?.[round.mountBarrel];
  return node?.getWorldPosition(mountedPoint) || null;
}
