import * as THREE from 'three';
import { setLightMask } from './materials.js';
import { lightMask, packed } from './point-lights.js';

// The static world's opaque material buckets span much more than one view.
// Each view draws the runs of the bucket's own, unchanged index buffer that
// lie in visible spatial cells, neighbouring runs merged, in one multi-draw
// call (WEBGL_multi_draw) made with the state three has just bound for the
// mesh. Triangles keep their original order (coplanar surfaces and
// alpha-to-coverage depend on it), and nothing is copied or uploaded per
// frame: the earlier per-view index compaction re-uploaded whole visible
// ranges whenever the view crossed a cell edge, a burst of buffer traffic
// while moving or turning. Triangles between two visible runs used to be
// drawn as well (one range from the first to the last): every vertex of them
// was shaded before the GPU clipped them away, which an integrated GPU short
// of vertex throughput paid for in full. Without the extension, that single
// range is still drawn.
export function cullStaticColor(mesh, parts, triangleCells) {
  const geometry = mesh.geometry, index = geometry.index;
  if (geometry.groups.length || geometry.drawRange.start !== 0 || geometry.drawRange.count < index.count
    || Object.keys(geometry.morphAttributes).length || Array.isArray(mesh.material) || mesh.material.transparent) return;
  // Consecutive triangles in one cell form a run: [cell, first index, end].
  const spans = [];
  for (let first = 0; first < triangleCells.length;) {
    const cell = triangleCells[first];
    let end = first + 1;
    while (end < triangleCells.length && triangleCells[end] === cell) end++;
    spans.push(cell, first * 3, end * 3);
    first = end;
  }
  const runs = new Uint32Array(spans);
  const views = [viewSlot(parts.length, runs.length / 3), viewSlot(parts.length, runs.length / 3)];
  const bytes = index.array.BYTES_PER_ELEMENT;
  let next = 0, pending = null, multiDraw;
  const matrix = new THREE.Matrix4(), frustum = new THREE.Frustum();
  const previous = mesh.onBeforeRender, previousAfter = mesh.onAfterRender;
  // The map's materials read only the point lights that reach a visible
  // cell (materials.js selectLights): tighter than the bucket's own bounds,
  // which span the map.
  const boxes = parts.map((part) => part.bounds);
  mesh.userData.cellLights = true;
  mesh.onBeforeRender = function(renderer, scene, camera, drawGeometry, material, group) {
    previous.call(this, renderer, scene, camera, drawGeometry, material, group);
    matrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse).multiply(mesh.matrixWorld);
    let view = views[0].camera === camera ? views[0] : views[1].camera === camera ? views[1] : null;
    if (!view) { view = views[next++ % views.length]; view.camera = camera; view.valid = false; }
    if (!view.valid || !view.matrix.equals(matrix)) {
      frustum.setFromProjectionMatrix(matrix);
      for (let i = 0; i < parts.length; i++) view.visible[i] = frustum.intersectsBox(parts[i].bounds) ? 1 : 0;
      // The visible runs, a run that starts where the last ended joining it.
      let ranges = 0, start = -1, end = 0;
      for (let i = 0; i < runs.length; i += 3) {
        if (!view.visible[runs[i]]) continue;
        if (start >= 0 && runs[i + 1] === end) { end = runs[i + 2]; continue; }
        if (start >= 0) { view.offsets[ranges] = start * bytes; view.counts[ranges++] = end - start; }
        start = runs[i + 1]; end = runs[i + 2];
      }
      if (start >= 0) { view.offsets[ranges] = start * bytes; view.counts[ranges++] = end - start; }
      view.ranges = ranges;
      view.first = ranges ? view.offsets[0] / bytes : 0;
      view.span = ranges ? end - view.first : 0;
      view.drawn = 0;
      for (let k = 0; k < ranges; k++) view.drawn += view.counts[k];
      view.matrix.copy(matrix); view.valid = true;
    }
    if (multiDraw === undefined && renderer) multiDraw = renderer.extensions.has("WEBGL_multi_draw") ? renderer.extensions.get("WEBGL_multi_draw") : null;
    if (material?.userData.pointLit) { const pack = packed(scene, camera); setLightMask(material, pack, lightMask(pack, boxes, view.visible)); }
    const multi = view.ranges > 1 ? multiDraw : null;
    if (multi) {
      // three binds the program, uniforms and buffers, and draws nothing;
      // onAfterRender draws the runs with them.
      geometry.setDrawRange(0, 0);
      pending = { view, multi, gl: renderer.getContext(), info: renderer.info };
    } else geometry.setDrawRange(view.first, view.span);
  };
  mesh.onAfterRender = function(...args) {
    previousAfter.apply(this, args);
    if (!pending) return;
    const { view, multi, gl, info } = pending;
    pending = null;
    multi.multiDrawElementsWEBGL(gl.TRIANGLES, view.counts, 0, bytes === 4 ? gl.UNSIGNED_INT : gl.UNSIGNED_SHORT, view.offsets, 0, view.ranges);
    info.render.triangles += view.drawn / 3;
  };
}

function viewSlot(parts, runs) {
  return { camera: null, valid: false, matrix: new THREE.Matrix4(), visible: new Uint8Array(parts),
    offsets: new Int32Array(runs), counts: new Int32Array(runs), ranges: 0, first: 0, span: 0, drawn: 0 };
}

// Cells in Morton (Z-curve) order over x/z: a view's visible cells then tend
// to sit close together in the static index buffer, keeping its range short.
export function mortonOrder(parts) {
  const spread = (v) => {
    v = (v + 32768) & 0xffff;
    v = (v | (v << 8)) & 0x00ff00ff; v = (v | (v << 4)) & 0x0f0f0f0f;
    v = (v | (v << 2)) & 0x33333333; v = (v | (v << 1)) & 0x55555555;
    return v >>> 0;
  };
  return parts.map((part, i) => [i, spread(part.x) * 2 + spread(part.z)]).sort((a, b) => a[1] - b[1]).map(([i]) => i);
}
