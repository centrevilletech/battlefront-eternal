import * as THREE from "three";

// Quads gathered in draw order, one draw per run of the same texture: every
// interface element is its texture times a tint, alpha blended, optionally
// through a mask texture.

const QUADS = 2048;

const VERTEX = /* glsl */ `
  attribute vec2 maskUv;
  attribute vec4 tint;
  varying vec2 vUv;
  varying vec2 vMaskUv;
  varying vec4 vTint;
  void main() {
    vUv = uv;
    vMaskUv = maskUv;
    vTint = tint;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`;
const FRAGMENT = /* glsl */ `
  uniform sampler2D map;
  uniform sampler2D mask;
  uniform bool masked;
  varying vec2 vUv;
  varying vec2 vMaskUv;
  varying vec4 vTint;
  void main() {
    vec4 color = texture2D(map, vUv) * vTint;
    if (masked) color.a *= texture2D(mask, vMaskUv).a;
    gl_FragColor = color;
  }
`;

export class Batch {
  constructor() {
    this.position = new Float32Array(QUADS * 12);
    this.uv = new Float32Array(QUADS * 8);
    this.maskUv = new Float32Array(QUADS * 8);
    this.tint = new Float32Array(QUADS * 16);
    const index = new Uint32Array(QUADS * 6);
    for (let i = 0; i < QUADS; i++) index.set([i * 4, i * 4 + 1, i * 4 + 2, i * 4 + 2, i * 4 + 1, i * 4 + 3], i * 6);
    const geometry = new THREE.BufferGeometry();
    const attribute = (array, size) => new THREE.BufferAttribute(array, size).setUsage(THREE.DynamicDrawUsage);
    geometry.setAttribute("position", attribute(this.position, 3));
    geometry.setAttribute("uv", attribute(this.uv, 2));
    geometry.setAttribute("maskUv", attribute(this.maskUv, 2));
    geometry.setAttribute("tint", attribute(this.tint, 4));
    geometry.setIndex(new THREE.BufferAttribute(index, 1));
    this.geometry = geometry;
    this.mesh = new THREE.Mesh(geometry, []);
    this.mesh.frustumCulled = false;
    this.materials = new Map();
    this.point = new THREE.Vector3();
    this.count = 0;
    // Quads whose corners changed since the last upload. The arrays are
    // what the GPU holds, so a corner written with the value already there
    // (most of the HUD, most frames) needs no upload; end() uploads only
    // the runs of changed quads.
    this.dirty = new Uint8Array(QUADS);
    this.uploads = 0; // ranges uploaded by the last end(), for checks
  }

  // By texture, then mask (no per-quad key strings).
  material(texture, mask) {
    let byMask = this.materials.get(texture);
    if (!byMask) this.materials.set(texture, (byMask = new Map()));
    const key = mask || null;
    if (!byMask.has(key)) {
      byMask.set(
        key,
        new THREE.ShaderMaterial({
          uniforms: { map: { value: texture }, mask: { value: mask || texture }, masked: { value: Boolean(mask) } },
          vertexShader: VERTEX,
          fragmentShader: FRAGMENT,
          transparent: true,
          depthTest: false,
          depthWrite: false,
          side: THREE.DoubleSide,
        }),
      );
    }
    return byMask.get(key);
  }

  begin() {
    this.count = 0;
    this.geometry.clearGroups();
    this.mesh.material.length = 0;
    this.lastMaterial = null;
    this.lastGroup = null;
  }

  // The rectangle (x0, y0)-(x1, y1) at depth z under m, with uv (u0, v0) at
  // its first corner and (u1, v1) at its second. maskUv likewise for a mask.
  // corners: each corner's own [u, v] instead of uv (the turned minimap).
  quad(m, x0, y0, x1, y1, texture, uv, tint, z = 0, mask = null, maskUv = null, corners = null) {
    if (!texture || this.count >= QUADS) return;
    const i = this.count++;
    // Written straight into the buffers: this runs for every glyph and icon,
    // and the per-corner arrays and closures it used to build were most of
    // the HUD's garbage.
    const mu0 = maskUv ? maskUv[0] : 0, mv0 = maskUv ? maskUv[1] : 0, mu1 = maskUv ? maskUv[2] : 1, mv1 = maskUv ? maskUv[3] : 1;
    const c = corners;
    this.corner(i * 4, m, x0, y0, z, c ? c[0][0] : uv[0], c ? c[0][1] : uv[1], mu0, mv0, tint);
    this.corner(i * 4 + 1, m, x0, y1, z, c ? c[1][0] : uv[0], c ? c[1][1] : uv[3], mu0, mv1, tint);
    this.corner(i * 4 + 2, m, x1, y0, z, c ? c[2][0] : uv[2], c ? c[2][1] : uv[1], mu1, mv0, tint);
    this.corner(i * 4 + 3, m, x1, y1, z, c ? c[3][0] : uv[2], c ? c[3][1] : uv[3], mu1, mv1, tint);
    const material = this.material(texture, mask);
    if (this.lastMaterial === material) {
      this.lastGroup.count += 6;
    } else {
      const materials = this.mesh.material;
      let index = materials.indexOf(material);
      if (index < 0) index = materials.push(material) - 1;
      this.geometry.addGroup(i * 6, 6, index);
      this.lastMaterial = material;
      this.lastGroup = this.geometry.groups[this.geometry.groups.length - 1];
    }
  }

  // One corner: the point under m (as Vector3.applyMatrix4), its uv and
  // mask uv, and the tint.
  corner(v, m, x, y, z, u, w, mu, mv, tint) {
    const e = m.elements, d = 1 / (e[3] * x + e[7] * y + e[11] * z + e[15]);
    const p = this.position, t = this.tint, uv = this.uv, muv = this.maskUv;
    const px = (e[0] * x + e[4] * y + e[8] * z + e[12]) * d;
    const py = (e[1] * x + e[5] * y + e[9] * z + e[13]) * d;
    const pz = (e[2] * x + e[6] * y + e[10] * z + e[14]) * d;
    const p3 = v * 3, v2 = v * 2, v4 = v * 4;
    // Float32 stores round these; compare what the array would hold.
    let changed = p[p3] !== Math.fround(px) || p[p3 + 1] !== Math.fround(py) || p[p3 + 2] !== Math.fround(pz)
      || uv[v2] !== Math.fround(u) || uv[v2 + 1] !== Math.fround(w) || muv[v2] !== Math.fround(mu) || muv[v2 + 1] !== Math.fround(mv)
      || t[v4] !== Math.fround(tint[0]) || t[v4 + 1] !== Math.fround(tint[1]) || t[v4 + 2] !== Math.fround(tint[2]) || t[v4 + 3] !== Math.fround(tint[3]);
    if (!changed) return;
    p[p3] = px; p[p3 + 1] = py; p[p3 + 2] = pz;
    uv[v2] = u; uv[v2 + 1] = w;
    muv[v2] = mu; muv[v2 + 1] = mv;
    t[v4] = tint[0]; t[v4 + 1] = tint[1]; t[v4 + 2] = tint[2]; t[v4 + 3] = tint[3];
    this.dirty[v >> 2] = 1;
  }

  // A quad with its own UVs per corner (the turned minimap).
  quadCorners(m, x0, y0, x1, y1, texture, uvs, tint, mask) {
    this.quad(m, x0, y0, x1, y1, texture, null, tint, 0, mask, null, uvs);
  }

  // Upload the changed quads, as one run per attribute from the first to
  // the last (never more uploads than the whole buffer took, and none when
  // nothing changed: a static HUD, menus, the spawn screen).
  end() {
    const geometry = this.geometry;
    const ranges = dirtyRanges(this.dirty, this.count, GAP, MAX_RANGES);
    this.uploads = ranges.length / 2;
    for (const name of ["position", "uv", "maskUv", "tint"]) {
      const attribute = geometry.getAttribute(name);
      attribute.clearUpdateRanges();
      if (!ranges.length) continue;
      const size = 4 * attribute.itemSize;
      for (let i = 0; i < ranges.length; i += 2) attribute.addUpdateRange(ranges[i] * size, (ranges[i + 1] - ranges[i]) * size);
      attribute.needsUpdate = true;
    }
    this.dirty.fill(0, 0, this.count);
    geometry.setDrawRange(0, this.count * 6);
  }
}

const GAP = 8;
const MAX_RANGES = 1;

// The runs of set flags in dirty[0..count), as [start, end) pairs, with
// runs closer than gap merged, then the closest runs merged until at most
// max remain.
export function dirtyRanges(dirty, count, gap = GAP, max = Infinity) {
  const ranges = [];
  let start = -1, end = -1;
  for (let i = 0; i < count; i++) {
    if (!dirty[i]) continue;
    if (start >= 0 && i - end <= gap) { end = i + 1; continue; }
    if (start >= 0) ranges.push(start, end);
    start = i; end = i + 1;
  }
  if (start >= 0) ranges.push(start, end);
  while (ranges.length / 2 > max) {
    let at = 0, smallest = Infinity;
    for (let i = 2; i < ranges.length; i += 2) { const g = ranges[i] - ranges[i - 1]; if (g < smallest) { smallest = g; at = i; } }
    ranges.splice(at - 1, 2); // the end of one run and the start of the next: joined
  }
  return ranges;
}
