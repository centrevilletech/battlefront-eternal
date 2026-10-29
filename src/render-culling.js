import * as THREE from 'three';

// The static world's opaque material buckets span much more than one view.
// Each view draws one range of the bucket's own, unchanged index buffer: from
// its first triangle in a visible spatial cell to its last. Triangles keep
// their original order (coplanar surfaces and alpha-to-coverage depend on it),
// and nothing is copied or uploaded per frame: the earlier per-view index
// compaction re-uploaded whole visible ranges whenever the view crossed a
// cell edge, a burst of buffer traffic while moving or turning. Triangles
// between two visible runs are drawn too; the GPU clips them cheaply.
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
  const views = [viewSlot(parts.length), viewSlot(parts.length)];
  let next = 0;
  const matrix = new THREE.Matrix4(), frustum = new THREE.Frustum();
  const previous = mesh.onBeforeRender;
  mesh.onBeforeRender = function(renderer, scene, camera, drawGeometry, material, group) {
    previous.call(this, renderer, scene, camera, drawGeometry, material, group);
    matrix.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse).multiply(mesh.matrixWorld);
    let view = views[0].camera === camera ? views[0] : views[1].camera === camera ? views[1] : null;
    if (!view) { view = views[next++ % views.length]; view.camera = camera; view.valid = false; }
    if (!view.valid || !view.matrix.equals(matrix)) {
      frustum.setFromProjectionMatrix(matrix);
      for (let i = 0; i < parts.length; i++) view.visible[i] = frustum.intersectsBox(parts[i].bounds) ? 1 : 0;
      let start = -1, end = 0;
      for (let i = 0; i < runs.length; i += 3) {
        if (!view.visible[runs[i]]) continue;
        if (start < 0) start = runs[i + 1];
        end = runs[i + 2];
      }
      view.start = Math.max(start, 0); view.count = start < 0 ? 0 : end - start;
      view.matrix.copy(matrix); view.valid = true;
    }
    geometry.setDrawRange(view.start, view.count);
  };
}

function viewSlot(parts) {
  return { camera: null, valid: false, matrix: new THREE.Matrix4(), visible: new Uint8Array(parts), start: 0, count: 0 };
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
