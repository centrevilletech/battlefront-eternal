import * as THREE from "three";
import { corner } from "./barriers.js";

// Line geometry for a navigation overlay: hub circles, arcs, barrier
// footprints, boundary polygons and live paths. Positions come back as flat
// arrays of segment endpoints (x, y, z pairs); `lines()` wraps one in a
// THREE.LineSegments. heightAt(x, z) places footprints that have no height
// of their own (barriers, boundaries); without it they sit at their stored y.

export function graphSegments(graph, out = []) {
  if (!graph) return out;
  for (const hub of graph.hubs) circle(out, hub.x, hub.y, hub.z, hub.radius, 16);
  for (const arc of graph.arcs) out.push(arc.a.x, arc.a.y, arc.a.z, arc.b.x, arc.b.y, arc.b.z);
  return out;
}

export function barrierSegments(barriers, heightAt = null, radius = 0, out = []) {
  const c = [{}, {}, {}, {}].map(() => ({ x: 0, y: 0, z: 0 }));
  for (const b of barriers.list) {
    for (let i = 0; i < 4; i++) corner(b, i, radius, 0, c[i]);
    for (let i = 0; i < 4; i++) {
      const a = c[i];
      const d = c[(i + 1) & 3];
      out.push(a.x, heightAt ? heightAt(a.x, a.z) : a.y, a.z, d.x, heightAt ? heightAt(d.x, d.z) : d.y, d.z);
    }
  }
  return out;
}

export function boundarySegments(boundary, heightAt = null, out = []) {
  for (const edges of boundary.polygons) {
    for (let i = 0; i < edges.length; i++) {
      const e = edges[i];
      const x1 = e.x + e.nz;
      const z1 = e.z - e.nx;
      out.push(e.x, heightAt ? heightAt(e.x, e.z) : 0, e.z, x1, heightAt ? heightAt(x1, z1) : 0, z1);
    }
  }
  return out;
}

// The hint arrows: position to one metre along the authored forward.
export function hintSegments(hints, out = []) {
  for (const h of hints.list) {
    const p = h.position;
    const f = h.forward;
    out.push(p.x, p.y, p.z, p.x + f.x, p.y + f.y, p.z + f.z);
  }
  return out;
}

// A NavControl's path: the remaining nodes from the segment start, plus the
// steering target.
export function pathSegments(control, out = []) {
  const e = control.executor;
  if (e) {
    let prev = e.prev;
    for (let i = e.index; i < e.path.nodes.length; i++) {
      const n = e.path.nodes[i];
      out.push(prev.x, prev.y + 0.2, prev.z, n.x, n.y + 0.2, n.z);
      prev = n;
    }
  }
  const r = control.request;
  if (r.valid && control.unit) {
    const p = control.unit.position;
    out.push(p.x, p.y + 0.3, p.z, r.steeringTarget.x, r.steeringTarget.y + 0.3, r.steeringTarget.z);
  }
  return out;
}

export function lines(positions, color = 0x00ffff) {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.Float32BufferAttribute(positions, 3));
  return new THREE.LineSegments(geometry, new THREE.LineBasicMaterial({ color, depthTest: false, transparent: true }));
}

function circle(out, x, y, z, r, n) {
  for (let i = 0; i < n; i++) {
    const a0 = (i / n) * Math.PI * 2;
    const a1 = ((i + 1) / n) * Math.PI * 2;
    out.push(x + r * Math.cos(a0), y, z + r * Math.sin(a0), x + r * Math.cos(a1), y, z + r * Math.sin(a1));
  }
}
