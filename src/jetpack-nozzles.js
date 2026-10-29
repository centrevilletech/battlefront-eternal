import * as THREE from "three";

// The original emitter offsets sit below the visible outlet rims. Recover
// those rims from the rigidly skinned pack itself, then use the exact same
// bone/bind transforms as its vertices. No world-down or ribcage Euler guess.
export class JetpackNozzles {
  constructor(actor, definition) {
    this.actor = actor;
    this.frames = [];
    const geometry = actor.body.geometry;
    const positions = geometry.attributes.position;
    const skin = geometry.attributes.skinIndex;
    const weights = geometry.attributes.skinWeight;
    const vertices = [];
    let cursor = 0;
    for (const part of definition.parts) {
      if (/(jetpack|darktrooper_pack)/i.test(part.texture || "")) {
        for (let i = cursor; i < cursor + part.vertexCount; i++) {
          vertices.push({ index: i, point: new THREE.Vector3().fromBufferAttribute(positions, i) });
        }
      }
      cursor += part.vertexCount;
    }
    if (!vertices.length) return;
    const bounds = new THREE.Box3().setFromPoints(vertices.map(v => v.point));
    const middle = (bounds.min.x + bounds.max.x) / 2;
    for (const side of [-1, 1]) {
      const half = vertices.filter(v => (v.point.x - middle) * side > 0);
      if (!half.length) continue;
      const unique = new Map();
      for (const vertex of half) {
        const key = vertex.point.toArray().map(n => Math.round(n * 100000)).join(",");
        unique.set(key, vertex);
      }
      // Both retail packs use eight-sided mouths; the dark trooper
      // rims are tilted, so a horizontal minimum-Y slice misses them.
      const rim = [...unique.values()].sort((a, b) => a.point.y - b.point.y).slice(0, 8);
      if (rim.length < 6) continue;
      const boneIndex = skin.getX(rim[0].index);
      if (rim.some(v => skin.getX(v.index) !== boneIndex || weights.getX(v.index) < 0.999)) continue;
      const center = new THREE.Vector3();
      for (const v of rim) center.add(v.point);
      center.multiplyScalar(1 / rim.length);
      const normal = new THREE.Vector3();
      const a = new THREE.Vector3(), b = new THREE.Vector3();
      for (let i = 1; i < rim.length - 1; i++) {
        normal.crossVectors(a.subVectors(rim[i].point, rim[0].point), b.subVectors(rim[i + 1].point, rim[0].point));
        if (normal.lengthSq() > 1e-9) break;
      }
      if (normal.lengthSq() < 1e-9) continue;
      normal.normalize();
      if (rim.some(v => Math.abs(normal.dot(v.point.clone().sub(rim[0].point))) > .0005)) continue;
      if (normal.y > 0) normal.negate();
      const bindToBone = actor.skeleton.boneInverses[boneIndex].clone().multiply(actor.body.bindMatrix);
      const localCenter = center.clone().applyMatrix4(bindToBone);
      const localDirection = normal.clone().transformDirection(bindToBone);
      let radius = 0;
      for (const v of rim) radius += v.point.clone().applyMatrix4(bindToBone).distanceTo(localCenter);
      radius /= rim.length;
      // Reject a flat box edge or unrelated bottom geometry: an outlet is a
      // nearly circular ring, not the whole lower boundary of the backpack.
      if (radius < 0.012 || radius > 0.1 || rim.some(v => Math.abs(v.point.distanceTo(center) - radius) > radius * 0.12)) continue;
      this.frames.push({
        bone: actor.bones[boneIndex], localCenter, localDirection, localRadius: radius,
        rimIndices: rim.map(v => v.index), rimCenter: new THREE.Vector3(),
        position: new THREE.Vector3(), direction: new THREE.Vector3(), radius,
      });
    }
  }

  update() {
    if (!this.frames.length) return null;
    let updatedBone = null;
    for (const frame of this.frames) {
      if (frame.bone !== updatedBone) {
        frame.bone.updateWorldMatrix(true, false);
        updatedBone = frame.bone;
      }
      frame.rimCenter.copy(frame.localCenter).applyMatrix4(frame.bone.matrixWorld);
      frame.direction.copy(frame.localDirection).transformDirection(frame.bone.matrixWorld);
      // Start 2 mm inside the mouth so filtering cannot expose a detached gap.
      frame.position.copy(frame.rimCenter).addScaledVector(frame.direction, -0.002);
      frame.radius = frame.localRadius * frame.bone.matrixWorld.getMaxScaleOnAxis();
    }
    return this.frames;
  }
}
