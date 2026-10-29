// The playable area from the `bnd_` config (docs/ai-nav.md).
//
// Each Boundary scope names paths by hash; each resolved path closes into a
// polygon, edges last→first then in order, each stored as its start (x, z)
// and the unnormalized normal (−dz, dx). A point is inside a polygon when it
// is on the inner side of, or on, every edge: dot(p − start, normal) <= 0.
// That is a half-plane intersection, so point order matters and concave
// shapes are not handled as such. Several polygons form a union. With no
// boundary at all nothing is outside. Being outside starts the Character's
// 10 s countdown in gameplay (src/battlefield.js); nothing here rejects
// routes.

export class Boundary {
  // polygons: arrays of points with x and z (or [x, y, z, ...] arrays).
  constructor(polygons = []) {
    this.polygons = polygons.map((points) => {
      const pts = points.map((p) => (Array.isArray(p) ? { x: p[0], z: p[2] } : { x: p.x, z: p.z }));
      const edges = [];
      const n = pts.length;
      if (n === 0) return edges;
      const push = (a, b) => edges.push({ x: a.x, z: a.z, nx: -(b.z - a.z), nz: b.x - a.x });
      push(pts[n - 1], pts[0]);
      for (let i = 0; i + 1 < n; i++) push(pts[i], pts[i + 1]);
      return edges;
    });
  }

  get empty() {
    return this.polygons.length === 0;
  }

  // Inside polygon i.
  contains(i, p) {
    for (const e of this.polygons[i]) {
      if ((p.x - e.x) * e.nx + (p.z - e.z) * e.nz > 0) return false;
    }
    return true;
  }

  // Outside every polygon of a nonempty boundary.
  outside(p) {
    if (this.polygons.length === 0) return false;
    for (let i = 0; i < this.polygons.length; i++) if (this.contains(i, p)) return false;
    return true;
  }
}
