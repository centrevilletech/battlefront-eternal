import * as THREE from "three";

// World regions (wrld regn: TYPE, XFRM, SIZE) as the 2004 game builds and
// tests them. Capture, control, kill, death and light regions all share it.
//
// The world loader keeps SIZE and its length. Building the runtime region
// then, by TYPE:
// - sphere: SIZE as it is; its radius is that length, about its centre
//   (type 1). Hand-sized spheres stretch one axis and leave
//   the others at 0.1: Tatooine: Dune Sea's HOMESTEAD is 0.1 x 8.15 x 0.1,
//   an 8.15 m sphere.
// - box: SIZE scaled by the lengths of the XFRM's axes, half extents along
//   its unit axes (types 2 and 5).
// - cylinder: SIZE scaled the same way, then x and z both set to
//   sqrt(x^2 + z^2): the radius about its unit y axis, y the half height
//   (types 3, 6, 7, 8). Tatooine's sarlacc pit (Sarlac01, 2.14 x 6.06 x
//   8.16) is 8.43 m across its middle.
// Other readers take the built size too: the AI's objective offset spans
// its x and z.

// { type, half: the built size [x, y, z], radius, center, inverse: world
// to the region's unit axes }.
export function buildRegion(region) {
  const matrix = new THREE.Matrix4();
  if (region.matrix?.length >= 16) matrix.fromArray(region.matrix);
  else if (region.center) matrix.setPosition(...region.center);
  const axes = [new THREE.Vector3(), new THREE.Vector3(), new THREE.Vector3()];
  matrix.extractBasis(...axes);
  const scale = axes.map((axis) => {
    const length = axis.length();
    if (length > 0) axis.divideScalar(length);
    return length || 1;
  });
  const size = region.size;
  let half = [0, 1, 2].map((i) => size[i] * scale[i]);
  let radius = 0;
  if (region.type === "sphere") {
    half = [...size];
    radius = Math.hypot(size[0], size[1], size[2]);
  } else if (region.type === "cylinder") {
    radius = Math.hypot(half[0], half[2]);
    half = [radius, half[1], radius];
  }
  const center = new THREE.Vector3().setFromMatrixPosition(matrix);
  const unit = new THREE.Matrix4().makeBasis(...axes).setPosition(center);
  return { type: region.type, half, radius, center, inverse: unit.invert() };
}

// Whether a built region holds a world point.
export function inRegion(built, point, scratch = new THREE.Vector3()) {
  if (built.type === "sphere") return scratch.copy(point).sub(built.center).length() < built.radius;
  scratch.copy(point).applyMatrix4(built.inverse);
  const [a, b, c] = built.half;
  if (built.type === "cylinder") return Math.hypot(scratch.x, scratch.z) < built.radius && Math.abs(scratch.y) < b;
  if (built.type === "box") return Math.abs(scratch.x) < a && Math.abs(scratch.y) < b && Math.abs(scratch.z) < c;
  return false;
}
