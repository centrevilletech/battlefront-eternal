import * as THREE from "three";

// The interface's 3D models (the spawn screen's room, the team logos, a
// class's body) and how the 2004 interface draws them. The spawn screen and
// the scoreboard draw theirs with it.
//
// An interface model hangs at its screen point, on the ray through it: the
// 2004 game multiplies the point's place on the interface plane (886.81
// units out) by a factor, then scales the model. On the plane that is the
// model scaled by scale / factor.
// - The room (shell_cube): factor 0.009, scale 1.6, at the screen's centre,
//   its own origin on the axis (the corner sits left of centre). That puts
//   the eye 5 room units in front of the origin; the grid lines of all
//   three retail screenshots fit it.
// - The logos: size parameter (0.25 to 0.5) x 221 units on the plane, which
//   gives the screenshots' 78 and 40 pixels across.
// - Bodies: 0.936 / (0.006 / (W/800)) = 200 units per metre, the clone
//   trooper's 385 pixels.
//
// Models are lit as the interface lights them, whatever their material
// says: ambient (144, 144, 144) and a white light from the front left, above
// (the screenshots' lit side), clamped at full. The room isn't lit; its
// vertices' own tints darken it (roomTints).
//
// A stage of the port's own (the page spawn screen, src/spawn-screen/page)
// may light them more: keyTint colours the light, rim adds light round the
// model's edges (its strength falling off by rimPower), recolor redraws a
// model in one colour by its brightness ([r, g, b, amount]), and cut hides
// what is above a height in the world (metres there), its edge lit in
// cutGlow, to build a body up from its feet. Unset, they leave the
// original's light as it is.

export const PLANE = 1024 / (2 * Math.tan(Math.PI / 6)); // 886.81
export const ROOM_SCALE = 1.6 / 0.009;
export const LOGO_UNITS = 221.3;
export const BODY_UNITS = 0.936 / (0.006 / (1024 / 800));
export const DROIDEKA_UNITS = 0.9 * BODY_UNITS;
const AMBIENT = 144 / 255;
const TO_LIGHT = new THREE.Vector3(-1, 1, 1).normalize();

const VERTEX = /* glsl */ `
  #include <common>
  #include <skinning_pars_vertex>
  uniform vec2 scroll;
  uniform float time;
  varying vec2 vUv;
  varying vec3 vNormal;
  varying vec3 vTint;
  varying float vLift;
  void main() {
    #ifdef USE_COLOR
      vTint = color;
    #else
      vTint = vec3(1.0);
    #endif
    #include <beginnormal_vertex>
    #include <skinbase_vertex>
    #include <skinnormal_vertex>
    #include <defaultnormal_vertex>
    #include <begin_vertex>
    #include <skinning_vertex>
    #include <project_vertex>
    vNormal = normalize(transformedNormal);
    vLift = (modelMatrix * vec4(transformed, 1.0)).y;
    // Scrolling materials move at -speed * t, in repeats per second.
    vUv = uv - scroll * time;
  }
`;
const FRAGMENT = /* glsl */ `
  uniform sampler2D map;
  uniform bool hasMap;
  uniform vec3 color;
  uniform float opacity;
  uniform float lit;
  uniform vec3 toLight;
  uniform float ambient;
  uniform float cutoff;
  uniform float solid;
  uniform vec3 keyTint;
  uniform vec3 rim;
  uniform float rimPower;
  uniform vec4 recolor;
  uniform float cut;
  uniform vec3 cutGlow;
  varying vec2 vUv;
  varying vec3 vNormal;
  varying vec3 vTint;
  varying float vLift;
  void main() {
    if (vLift > cut) discard;
    vec4 texel = hasMap ? texture2D(map, vUv) : vec4(1.0);
    vec3 n = normalize(vNormal) * (gl_FrontFacing ? 1.0 : -1.0);
    vec3 light = vec3(min(ambient + max(dot(n, toLight), 0.0), 1.0)) * keyTint;
    vec4 c = vec4(texel.rgb * color * vTint * mix(vec3(1.0), light, lit), texel.a * opacity);
    c.rgb += rim * pow(1.0 - max(n.z, 0.0), rimPower) * lit;
    if (recolor.a > 0.0) c.rgb = mix(c.rgb, recolor.rgb * dot(c.rgb, vec3(0.299, 0.587, 0.114)) * 2.0, recolor.a);
    c.rgb += cutGlow * (1.0 - smoothstep(0.0, 0.08, cut - vLift));
    if (c.a <= cutoff) discard;
    // An opaque part's texture alpha holds something else (a gloss mask);
    // what it writes is solid.
    gl_FragColor = vec4(c.rgb, mix(c.a, 1.0, solid));
  }
`;

// map: the texture; lit: the interface light or flat; scroll: [u, v] per
// second; blend: draws alpha blended (else opaque); cutoff: texels at or
// under this alpha aren't drawn (-1: all are, as an opaque part's alpha
// may hold something else); tints: multiplied by the vertices' colours.
export function interfaceMaterial({ map = null, color = [1, 1, 1], opacity = 1, lit = true, scroll = null, blend = true, cutoff = 0, depth = true, tints = false } = {}) {
  return new THREE.ShaderMaterial({
    vertexColors: tints,
    uniforms: {
      map: { value: map },
      hasMap: { value: Boolean(map) },
      color: { value: new THREE.Color().fromArray(color) },
      opacity: { value: opacity },
      lit: { value: lit ? 1 : 0 },
      toLight: { value: TO_LIGHT },
      ambient: { value: AMBIENT },
      cutoff: { value: cutoff },
      solid: { value: blend ? 0 : 1 },
      scroll: { value: new THREE.Vector2(...(scroll || [0, 0])) },
      time: { value: 0 },
      keyTint: { value: new THREE.Color(1, 1, 1) },
      rim: { value: new THREE.Color(0, 0, 0) },
      rimPower: { value: 2.5 },
      recolor: { value: new THREE.Vector4(0, 0, 0, 0) },
      cut: { value: 1e9 },
      cutGlow: { value: new THREE.Color(0, 0, 0) },
    },
    vertexShader: VERTEX,
    fragmentShader: FRAGMENT,
    transparent: blend,
    depthTest: depth,
    depthWrite: depth,
    side: THREE.DoubleSide,
  });
}

// A texture the material scrolls: repeating, set before its first upload.
export function repeating(texture) {
  if (texture && texture.wrapS !== THREE.RepeatWrapping) {
    texture.wrapS = texture.wrapT = THREE.RepeatWrapping;
    if (texture.image) texture.needsUpdate = true;
  }
  return texture;
}

// The room's vertex tints, which the common model pack doesn't carry: grey
// 192, and 68 toward the open end of the right-hand wall (its last two
// columns of vertices, x > 0.4 and z > 1 in the model's units). The dark
// end is the screenshots' darker right edge.
export function roomTints(geometry) {
  if (geometry.getAttribute("color")) return;
  const position = geometry.getAttribute("position");
  const colors = new Float32Array(position.count * 3);
  for (let i = 0; i < position.count; i++) {
    const grey = position.getX(i) > 0.4 && position.getZ(i) > 1 ? 68 / 255 : 192 / 255;
    colors.set([grey, grey, grey], i * 3);
  }
  geometry.setAttribute("color", new THREE.BufferAttribute(colors, 3));
}

// The meshes of a model, in order (a hologram-pack model is one per part).
export function meshesOf(object) {
  const meshes = [];
  object?.traverse((child) => {
    if (child.isMesh) meshes.push(child);
  });
  return meshes;
}

// Every mesh of an object gets the interface's material. parts: the model's
// records (common.json holograms) for each mesh's texture and scroll (a
// mesh's own userData.scroll wins); texture(name): the HUD's textures (the
// 4x atlases apply). Returns the materials made, for tinting and disposal.
export function dressModel(object, parts, texture, options = {}) {
  const materials = [];
  meshesOf(object).forEach((mesh, i) => {
    const part = parts?.[i] || parts?.[0] || {};
    const map = (part.texture && texture(part.texture)) || (mesh.material?.map ?? mesh.material?.uniforms?.map?.value ?? null);
    const scroll = mesh.userData.scroll ?? part.scroll ?? null;
    if (scroll) repeating(map);
    const material = interfaceMaterial({ map, scroll, ...options });
    mesh.material = material;
    mesh.frustumCulled = false;
    materials.push(material);
  });
  return materials;
}

// A body's own materials swapped for interface ones on the same textures
// (skinning kept); cut-out parts stay cut out.
export function dressBody(object) {
  const made = [];
  for (const mesh of meshesOf(object)) {
    // The shadow proxy draws only in the shadow pass.
    if (!mesh.layers.isEnabled(0)) continue;
    const swap = (material) => {
      const map = material?.map ?? material?.uniforms?.map?.value ?? null;
      const blend = Boolean(material?.transparent);
      const dressed = interfaceMaterial({ map, blend, cutoff: blend ? 0 : material?.alphaTest || -1 });
      dressed.blending = material?.blending ?? THREE.NormalBlending;
      made.push(dressed);
      return dressed;
    };
    mesh.material = Array.isArray(mesh.material) ? mesh.material.map(swap) : swap(mesh.material);
    mesh.frustumCulled = false;
  }
  return made;
}

// The camera the models are drawn with: the HUD's 46.8 degrees up, at the
// window's own shape, so a model keeps its proportions on any screen.
export class InterfaceCamera {
  constructor() {
    this.camera = new THREE.PerspectiveCamera(2 * THREE.MathUtils.radToDeg(Math.atan(0.75 * Math.tan(Math.PI / 6))), 4 / 3, 1, 20000);
  }

  // aspect: the canvas's width / height; H: the interface height.
  setView(aspect, H) {
    this.aspect = aspect;
    this.H = H;
    if (this.camera.aspect !== aspect) {
      this.camera.aspect = aspect;
      this.camera.updateProjectionMatrix();
    }
  }

  // An interface point (x, y) on the plane, in the camera's space: x keeps
  // its place across the screen and y its place down it.
  place(x, y, out = new THREE.Vector3()) {
    return out.set((x - 512) * 0.75 * this.aspect, (this.H / 2 - y) * (768 / this.H), -PLANE);
  }
}
