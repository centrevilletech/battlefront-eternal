import * as THREE from "three";

// The 2004 interface's space, which the HUD, the spawn screen, the
// scoreboard and the menus all draw in:
//
// - 1024 units across the screen and H = 768 + (h / w - 0.75) x 768 units
//   down it, y down;
// - one fixed perspective camera, 60 degrees across and 46.8 up whatever the
//   window's shape, onto a plane 886.81 units away, so a group turned about
//   its own axes shows in perspective;
// - a safe area 90% of the screen, from 5% in: a group at the safe area's
//   (a, b) sits at the screen's (0.05 + 0.9 a, 0.05 + 0.9 b).

export const IW = 1024;
export const TAN_X = Math.tan(Math.PI / 6);
export const TAN_Y = 0.75 * TAN_X;
export const PLANE = IW / (2 * TAN_X); // 886.81
export const SAFE = 0.05;
export const SAFE_SCALE = 0.9;

// The interface height for a window w x h pixels.
export function interfaceHeight(w, h) {
  return Math.floor(768 + (h / Math.max(w, 1) - 0.75) * 768);
}

// The camera every group is drawn through.
export function interfaceCamera() {
  return new THREE.PerspectiveCamera(THREE.MathUtils.radToDeg(2 * Math.atan(TAN_Y)), TAN_X / TAN_Y, 1, 10000);
}

// The matrix from an anchor's space (x right, y down, in units, from the
// screen point ax, ay: 0..1 across and down the screen, or the safe area's
// when `safe`) to the camera's, for an interface H units tall.
export function anchorMatrix(ax, ay, safe, H, out = new THREE.Matrix4()) {
  if (safe) {
    ax = SAFE + SAFE_SCALE * ax;
    ay = SAFE + SAFE_SCALE * ay;
  }
  return out.set(1, 0, 0, (ax - 0.5) * IW, 0, -768 / H, 0, -(ay - 0.5) * 768, 0, 0, 1, -PLANE, 0, 0, 0, 1);
}

// Where a point of a group (x, y, z under m) shows, in interface units.
export function planePoint(m, x, y, z, H) {
  const p = new THREE.Vector3(x, y, z).applyMatrix4(m);
  const k = PLANE / Math.max(-p.z, 1e-3);
  return { x: p.x * k + IW / 2, y: (384 - p.y * k) * (H / 768) };
}
