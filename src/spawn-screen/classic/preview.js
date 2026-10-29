import * as THREE from "three";
import { BODY_UNITS, DROIDEKA_UNITS, dressBody } from "../../interface/models.js";

// The picked class standing in the middle of the screen, as the 2004
// SpawnDisplay shows it: its feet at (W/2, H/1.2)
// whatever the safe area, facing the viewer with no turntable (the stance
// gives the three-quarter look), 200 interface units a metre (a droideka at
// 0.9 of that), lit by the interface's light. The body itself, posed and
// fidgeting, comes from the world port's previewBody(className).

export class ClassPreview {
  constructor(port) {
    this.port = port;
    this.root = new THREE.Group();
    this.className = null;
    this.body = null;
    this.materials = [];
    this.walker = false;
  }

  // Show a class (null: none). The body is made once per change.
  show(className) {
    if (className === this.className) return;
    this.clear();
    this.className = className;
    const body = className ? this.port.previewBody?.(className) : null;
    if (!body?.object) return;
    this.body = body;
    this.walker = Boolean(body.walker ?? body.droideka ?? /droideka/i.test(className));
    this.materials = dressBody(body.object);
    this.root.add(body.object);
  }

  update(dt) {
    this.body?.update?.(dt);
  }

  // Stand it at the interface point `feet` (camera: an InterfaceCamera).
  place(camera, feet) {
    camera.place(feet.x, feet.y, this.root.position);
    this.root.scale.setScalar(this.walker ? DROIDEKA_UNITS : BODY_UNITS);
  }

  clear() {
    if (this.body) {
      this.root.remove(this.body.object);
      this.body.dispose?.();
    }
    for (const material of this.materials) material.dispose();
    this.materials = [];
    this.body = null;
    this.className = null;
  }
}
