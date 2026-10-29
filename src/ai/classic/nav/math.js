// Small numeric helpers the navigation code shares.
//
// The original game keeps navigation state in 32-bit floats and compares against
// float32 constants. Where a comparison can tie (search scores, squared
// distances against literal thresholds) the port rounds through f32 so the
// ties land where the original's do. Elsewhere plain doubles are close
// enough: a waypoint a few ulps off changes nothing downstream.

export const f32 = Math.fround;

// The literal constants, as the float32 values the original game stores.
export const TINY = f32(0.0001); // 9.999999747e-5: "zero length" for points and segments
export const TINY_DIR = f32(9.999999e-9); // 9.99999905e-9: direction / goal-equality threshold
export const NEAR_WAYPOINT = f32(0.040000003); // 0.2 m squared: progress skip, Droideka stop
export const FLT_MAX = 3.4028234663852886e38;

// Horizontal squared length of (ax - bx, az - bz).
export function h2(ax, az, bx, bz) {
  const dx = ax - bx;
  const dz = az - bz;
  return dx * dx + dz * dz;
}

// Full 3D squared distance between two {x, y, z}.
export function d2(a, b) {
  const dx = a.x - b.x;
  const dy = a.y - b.y;
  const dz = a.z - b.z;
  return dx * dx + dy * dy + dz * dz;
}

// Componentwise exact equality, as the original game's vector compare.
export function same(a, b) {
  return a.x === b.x && a.y === b.y && a.z === b.z;
}

// Normalize {x, y, z} in place; a zero vector stays zero (D3DXVec3Normalize).
export function normalize(v) {
  const l = Math.sqrt(v.x * v.x + v.y * v.y + v.z * v.z);
  if (l > 0) {
    v.x /= l;
    v.y /= l;
    v.z /= l;
  }
  return v;
}

// The horizontal unit direction of v into out; a zero horizontal vector
// becomes (1, 0, 0).
export function flatDir(out, x, z) {
  const l = Math.sqrt(x * x + z * z);
  if (l > 0) return set(out, x / l, 0, z / l);
  return set(out, 1, 0, 0);
}

export function set(out, x, y, z) {
  out.x = x;
  out.y = y;
  out.z = z;
  return out;
}

export function copy(out, v) {
  out.x = v.x;
  out.y = v.y;
  out.z = v.z;
  return out;
}

export function vec(x = 0, y = 0, z = 0) {
  return { x, y, z };
}

// Lowest set bit's index; 0 for 0.
export function lowestBit(mask) {
  mask >>>= 0;
  if (mask === 0) return 0;
  return 31 - Math.clz32(mask & -mask);
}
