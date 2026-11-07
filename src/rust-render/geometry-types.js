// WebGL vertex/index component types, shared by retained mesh and line uploads.
export const GL_TYPES = { Float32Array: 0x1406, Uint16Array: 0x1403, Uint8Array: 0x1401, Int16Array: 0x1402, Int8Array: 0x1400, Uint32Array: 0x1405 };

// Cold scene facts used by the native static descriptor compiler. No material
// inspection or compiler reflection is involved, and no frame calls this scan.
export function staticFeatures(core, far = false) {
  const scene = far ? core.graphics.farScene : core.graphics.scene;
  const camera = far ? core.graphics.farCamera : core.graphics.camera;
  let points = 0, shadows = 0;
  scene.traverseVisible(o => {
    if (!o.layers.test(camera.layers)) return;
    if (o.isPointLight) points++;
    if (o.isDirectionalLight && o.castShadow && core.renderer.shadowMap.enabled) shadows++;
  });
  return Uint32Array.of(shadows, points, core.graphics.csm ? 1 : 0, core.graphics.settings.get("lampLighting") ? 1 : 0);
}
