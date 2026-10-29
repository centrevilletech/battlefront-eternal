import * as THREE from "three";
import { CameraRig, PIVOT } from "./rig.js";
import { HoloMap, TILT, drawn, dotTexture } from "./holomap.js";
import { GridRoom, StageFloor } from "./rooms.js";

// The page spawn screen's stage (view.js), a scene in metres on the HUD's
// canvas: the picked class's body standing on a hologram projector, his
// feet at the origin; the battlefield projected as a hologram beside him
// (holomap.js); all of it seen through a camera that sweeps in, swings on
// a change and drifts (rig.js). By look:
// - "grid": the 2004 screen's hologram room round it all (rooms.js), the
//   battle hidden;
// - "world": the battle behind (the mission's camera shots, director.js),
//   darkened to a vignette with a pool of light round him, a floor of
//   light under him.
// The look's colours come from the page (colors()).
//
// Under the HUD's quads: the room or the vignette, and the map's texture
// (before()). Over them: the scene (after()).

const RING_SIZE = 1.5; // m across: the projector's ring under him
const GLOW_SIZE = 2.8;
const COLUMN = { radius: 0.5, top: 0.3, height: 2.35 };
const MOTES = 40;
const TURN = 0.35; // rad/s: the ring turning
const PULSE_DECAY = 1.6; // per second: a change's flash fading
const BUILD = 0.65; // s: a body built up from its feet
const BUILD_TOP = 2.3; // m: past the top of any head
const SPIN_DRAG = 0.012; // rad a pixel dragged
const SPIN_FRICTION = 2.6; // per second, a flung body's turn slowing
const SPIN_FLING = 9; // rad/s at most
const SPIN_HOME = 2.2; // s after a drag, before he turns back to face the camera
const TILT_DRAG = 0.004; // rad a pixel dragged, up or down
const MAP_DEPTH = 0.55; // m: the disc stands this much nearer than his plane
const MAP_FILL = 0.8; // of its slot's width (the rim stands out past it)
const MAP_LIFT = 0.12; // m: between the projector's lamp and the disc

const VIGNETTE_VERTEX = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`;
// Dark at the edges, a pool of light where the soldier stands, the battle
// behind graded toward the look's colour; a dip to dark on a cut.
const VIGNETTE_FRAGMENT = /* glsl */ `
  uniform vec3 shade;
  uniform vec3 pool;
  uniform vec2 center;
  uniform float aspect;
  uniform float strength;
  uniform float dip;
  varying vec2 vUv;
  void main() {
    vec2 d = (vUv - center) * vec2(aspect, 1.0);
    float r = length(d * vec2(0.8, 1.0));
    float dark = smoothstep(0.15, 1.0, r);
    float a = mix(0.42, 0.9, dark) * strength;
    vec3 c = mix(pool, shade, dark);
    gl_FragColor = vec4(c, min(a + dip, 1.0));
  }
`;

const COLUMN_VERTEX = /* glsl */ `
  varying vec2 vUv;
  varying vec3 vNormal;
  varying vec3 vView;
  void main() {
    vUv = uv;
    vec4 view = modelViewMatrix * vec4(position, 1.0);
    vNormal = normalize(normalMatrix * normal);
    vView = normalize(-view.xyz);
    gl_Position = projectionMatrix * view;
  }
`;
// Bright at its foot, gone toward its top, brighter at its edges, scan
// lines rising through it.
const COLUMN_FRAGMENT = /* glsl */ `
  uniform vec3 color;
  uniform float time;
  uniform float strength;
  varying vec2 vUv;
  varying vec3 vNormal;
  varying vec3 vView;
  void main() {
    float edge = 1.0 - abs(dot(normalize(vNormal), vView));
    float fade = pow(1.0 - vUv.y, 1.6);
    float lines = 0.75 + 0.25 * sin((vUv.y * 46.0 - time * 3.0) * 6.2832);
    float a = fade * (0.08 + 0.5 * pow(edge, 2.2)) * lines * strength;
    gl_FragColor = vec4(color * a, a);
  }
`;

// The projector's ring: rings, ticks and arcs in white (tinted by its
// material).
function ringTexture() {
  return drawn(512, (ctx, s) => {
    const c = s / 2;
    ctx.strokeStyle = "#fff";
    const ring = (r, width, alpha, dash = []) => {
      ctx.globalAlpha = alpha;
      ctx.lineWidth = width;
      ctx.setLineDash(dash);
      ctx.beginPath();
      ctx.arc(c, c, r, 0, Math.PI * 2);
      ctx.stroke();
    };
    ring(s * 0.47, 6, 1);
    ring(s * 0.43, 2, 0.6, [18, 10]);
    ring(s * 0.33, 3, 0.8);
    ring(s * 0.29, 1.5, 0.5, [4, 8]);
    ring(s * 0.16, 2, 0.45);
    ctx.setLineDash([]);
    for (let i = 0; i < 60; i++) {
      const a = (i / 60) * Math.PI * 2;
      const long = i % 5 === 0;
      ctx.globalAlpha = long ? 0.9 : 0.45;
      ctx.lineWidth = long ? 3 : 1.5;
      const r0 = s * (long ? 0.37 : 0.385), r1 = s * 0.41;
      ctx.beginPath();
      ctx.moveTo(c + Math.cos(a) * r0, c + Math.sin(a) * r0);
      ctx.lineTo(c + Math.cos(a) * r1, c + Math.sin(a) * r1);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.lineWidth = 10;
    for (let i = 0; i < 3; i++) {
      const a = (i / 3) * Math.PI * 2;
      ctx.beginPath();
      ctx.arc(c, c, s * 0.47, a, a + 0.5);
      ctx.stroke();
    }
  });
}

const additive = (map, color = 0xffffff) => new THREE.MeshBasicMaterial({ map, color, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, side: THREE.DoubleSide });
const ease = (t) => 1 - Math.pow(1 - Math.min(Math.max(t, 0), 1), 3);

export class SpawnStage {
  // hud: the BattleHud (its textures, its spawn map's drawing).
  constructor({ hud }) {
    this.hud = hud;
    this.drawn = typeof document !== "undefined";
    this.time = 0;
    this.pulse = 0;
    this.strength = 0; // rising as the screen opens
    this.mode = "world";
    this.rig = new CameraRig();
    this.scene = new THREE.Scene();
    this.framed = false;
    this.mapPlaced = false;
    this.spin = 0;
    this.spinVelocity = 0;
    this.sinceDrag = Infinity;
    this.dragging = false;
    this.building = Infinity; // s into a body's build, Infinity once built
    this.bodyMaterials = [];
    this.cutGlow = new THREE.Color(1, 1, 1);
    this.light = null; // the body's: { key, rim }

    // The vignette, a quad over the whole canvas.
    this.vignette = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), new THREE.ShaderMaterial({
      uniforms: {
        shade: { value: new THREE.Color(0, 0, 0) },
        pool: { value: new THREE.Color(0, 0, 0) },
        center: { value: new THREE.Vector2(0.5, 0.42) },
        aspect: { value: 16 / 9 },
        strength: { value: 1 },
        dip: { value: 0 },
      },
      vertexShader: VIGNETTE_VERTEX,
      fragmentShader: VIGNETTE_FRAGMENT,
      transparent: true,
      depthTest: false,
      depthWrite: false,
    }));
    this.vignette.frustumCulled = false;
    this.vignetteScene = new THREE.Scene();
    this.vignetteScene.add(this.vignette);
    this.screenCamera = new THREE.Camera();
    this.dip = 0;

    this.room = new GridRoom(hud.texture?.("shell_cube") ?? null);
    this.floor = new StageFloor();
    this.scene.add(this.floor.mesh);

    // The projector under him.
    const projector = (this.projector = new THREE.Group());
    this.ringMaterial = additive(this.drawn ? ringTexture() : null);
    this.ringInner = additive(this.ringMaterial.map);
    this.glowMaterial = additive(this.drawn ? dotTexture() : null);
    const flat = (size, material, y) => {
      const mesh = new THREE.Mesh(new THREE.PlaneGeometry(size, size), material);
      mesh.rotation.x = -Math.PI / 2;
      mesh.position.y = y;
      mesh.renderOrder = -2;
      projector.add(mesh);
      return mesh;
    };
    this.glow = flat(GLOW_SIZE, this.glowMaterial, 0.004);
    this.ring = flat(RING_SIZE, this.ringMaterial, 0.008);
    this.inner = flat(RING_SIZE * 0.62, this.ringInner, 0.012);
    this.columnMaterial = new THREE.ShaderMaterial({
      uniforms: { color: { value: new THREE.Color(1, 1, 1) }, time: { value: 0 }, strength: { value: 0 } },
      vertexShader: COLUMN_VERTEX,
      fragmentShader: COLUMN_FRAGMENT,
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.DoubleSide,
    });
    const column = new THREE.Mesh(new THREE.CylinderGeometry(COLUMN.top, COLUMN.radius, COLUMN.height, 48, 1, true), this.columnMaterial);
    column.position.y = COLUMN.height / 2;
    column.renderOrder = 2;
    projector.add(column);
    this.motes = Array.from({ length: MOTES }, (_, i) => ({ a: Math.random() * Math.PI * 2, r: 0.15 + Math.random() * 0.4, y: (i / MOTES) * COLUMN.height, speed: 0.25 + Math.random() * 0.45 }));
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(new Float32Array(MOTES * 3), 3));
    this.moteMaterial = new THREE.PointsMaterial({ map: this.glowMaterial.map, size: 0.06, color: 0xffffff, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, sizeAttenuation: true });
    this.points = new THREE.Points(geometry, this.moteMaterial);
    this.points.frustumCulled = false;
    this.points.renderOrder = 3;
    projector.add(this.points);
    this.scene.add(projector);

    // Where the body goes: the view's preview root is added here.
    this.figure = new THREE.Group();
    this.scene.add(this.figure);

    this.map = new HoloMap({ hud });
    this.scene.add(this.map.root);
  }

  get camera() {
    return this.rig.camera;
  }

  // The look: mode "grid" or "world", projector "cone" or "emitter", and
  // its colours, each [r, g, b] 0..1 (alpha in [3] where it has one):
  // accent (the projector, the motes, the build's edge), shade and pool (the
  // vignette), room and pulse (the grid room's tint and running light),
  // floor, hologram, back (under the map), beam, key and rim (the body's
  // light).
  colors({ mode, projector, accent, shade, pool, room, pulse, floor, hologram, back, beam, key, rim }) {
    if (mode) this.mode = mode;
    if (projector) this.map.style(projector);
    if (accent) {
      for (const material of [this.ringMaterial, this.ringInner, this.glowMaterial, this.moteMaterial]) material.color.setRGB(accent[0], accent[1], accent[2]);
      this.columnMaterial.uniforms.color.value.setRGB(accent[0], accent[1], accent[2]);
      this.cutGlow.setRGB(accent[0] * 2, accent[1] * 2, accent[2] * 2);
    }
    const u = this.vignette.material.uniforms;
    if (shade) u.shade.value.setRGB(shade[0], shade[1], shade[2]);
    if (pool) u.pool.value.setRGB(pool[0], pool[1], pool[2]);
    this.room.colors({ tint: room?.slice(0, 3), pulse: pulse?.slice(0, 3) });
    this.floor.colors({ color: floor?.slice(0, 3) });
    this.map.colors({ tint: hologram?.slice(0, 3), back, beam: beam?.slice(0, 3) });
    this.light = { key, rim };
    this.lit = null;
    this.floor.mesh.visible = this.mode !== "grid";
  }

  // The rest framing: his body fills `figure` ({ x, top, bottom } in NDC)
  // on a canvas of `aspect`.
  frame(aspect, figure) {
    this.framed = this.rig.frame(aspect, figure);
    return this.framed;
  }

  // The map's disc in `slot` ({ x, y, w, h } in NDC, its centre and size),
  // seen from the camera at rest: across the slot's middle a little nearer
  // than he stands, floating over its projector, as big as fits under the
  // slot's top.
  placeMap(slot) {
    if (!this.framed || !(slot.w > 0 && slot.h > 0)) return false;
    const camera = this.rig.rest();
    const near = new THREE.Vector3(slot.x, slot.y, -1).unproject(camera);
    const far = new THREE.Vector3(slot.x, slot.y, 1).unproject(camera);
    const ray = new THREE.Ray(near, far.sub(near).normalize());
    const at = ray.intersectPlane(new THREE.Plane(new THREE.Vector3(0, 0, 1), -MAP_DEPTH), new THREE.Vector3());
    if (!at) return false;
    const t = Math.tan(THREE.MathUtils.degToRad(camera.fov / 2));
    let radius = Math.max((slot.w / 2) * at.distanceTo(camera.position) * t * camera.aspect * MAP_FILL, 0.05);
    const floor = this.map.lampHeight + MAP_LIFT;
    const top = slot.y + slot.h / 2;
    const center = new THREE.Vector3();
    const lean = [Math.cos(TILT), Math.sin(TILT)];
    for (let i = 0; i < 8; i++) {
      center.set(at.x, floor + radius * lean[0], MAP_DEPTH);
      const high = new THREE.Vector3(at.x, center.y + radius * lean[0], MAP_DEPTH - radius * lean[1]).project(camera);
      if (high.y <= top) break;
      const low = new THREE.Vector3(at.x, floor, MAP_DEPTH + radius * lean[1]).project(camera);
      radius *= Math.min(Math.max((top - low.y) / Math.max(high.y - low.y, 1e-3), 0.5), 0.98);
    }
    this.map.place(center, radius, camera.position);
    this.mapPlaced = true;
    this.rig.update(0);
    return true;
  }

  // His body (the preview's root): stood at the origin, a metre a metre.
  setBody(root, scale = 1) {
    if (root.parent !== this.figure) this.figure.add(root);
    root.position.set(0, 0, 0);
    root.scale.setScalar(scale);
  }

  // The body's materials (a new class's): lit in the look's light and built
  // up from the feet.
  dress(materials, build = true) {
    this.bodyMaterials = materials;
    this.lit = null;
    this.building = build ? 0 : Infinity;
    this.applyBuild();
  }

  // The screen opened: the camera sweeps in, the body builds, the projector
  // lights.
  open() {
    this.rig.sweepIn();
    this.rig.settle();
    this.spin = 0;
    this.spinVelocity = 0;
    this.pulse = 1;
    this.building = -0.35;
  }

  // A change: "class" (a new body building, a swing, a flash) or "team" (a
  // bigger swing); "spawn" pushes in; "cut" dips the battle behind to dark
  // across a cut.
  changed(kind) {
    if (kind === "cut") {
      this.dip = 1;
      return;
    }
    if (kind === "settle") {
      this.rig.settle();
      return;
    }
    this.rig.kick(kind);
    if (kind !== "spawn") this.pulse = 1;
    if (kind === "team") this.map.reproject();
  }

  // A drag on him, dx and dy pixels in `seconds`: across turns him (the
  // speed kept for a fling), up and down tilts the camera.
  drag(dx, dy, seconds = 1 / 60) {
    this.dragging = true;
    this.sinceDrag = 0;
    const turn = dx * SPIN_DRAG;
    this.spin += turn;
    const speed = turn / Math.max(seconds, 1 / 120);
    this.spinVelocity = Math.max(-SPIN_FLING, Math.min(SPIN_FLING, this.spinVelocity * 0.5 + speed * 0.5));
    this.rig.tilt(-dy * TILT_DRAG);
  }

  // Let go: flung at the drag's speed, unless it had stopped (still).
  release(still = false) {
    this.dragging = false;
    this.sinceDrag = 0;
    if (still) this.spinVelocity = 0;
  }

  // The pointer's place on the page, -1..1 (y up).
  look(x, y) {
    this.rig.look(x, y);
  }

  applyBuild() {
    const building = this.building < BUILD;
    const cut = building ? -0.05 + ease(this.building / BUILD) * BUILD_TOP : 1e9;
    for (const material of this.bodyMaterials) {
      const u = material.uniforms;
      if (!u?.cut) continue;
      u.cut.value = this.building < 0 ? -1 : cut;
      u.cutGlow.value.copy(this.cutGlow);
    }
  }

  // The body in the look's light: its key light's colour, a rim.
  lightBody() {
    const light = this.light;
    if (!light || this.lit === this.bodyMaterials) return;
    this.lit = this.bodyMaterials;
    for (const material of this.bodyMaterials) {
      const u = material.uniforms;
      if (!u?.rim) continue;
      if (light.key) u.keyTint.value.setRGB(light.key[0], light.key[1], light.key[2]);
      if (light.rim) u.rim.value.setRGB(light.rim[0] * light.rim[3], light.rim[1] * light.rim[3], light.rim[2] * light.rim[3]);
      u.rimPower.value = 3;
      u.ambient.value = 0.62;
    }
  }

  update(dt, open) {
    this.time += dt;
    this.pulse = Math.max(this.pulse - PULSE_DECAY * dt, 0);
    this.strength += ((open ? 1 : 0) - this.strength) * Math.min(dt * 3.5, 1);
    this.dip = Math.max(this.dip - dt * 3, 0);
    const s = this.strength;
    const flare = 1 + 1.4 * this.pulse;

    // His turn: dragged, flung, then home to face the camera.
    if (!this.dragging) {
      this.sinceDrag += dt;
      this.spin += this.spinVelocity * dt;
      this.spinVelocity *= Math.exp(-SPIN_FRICTION * dt);
      if (this.sinceDrag > SPIN_HOME) {
        // The nearest whole turn back to the front.
        const home = Math.round(this.spin / (Math.PI * 2)) * Math.PI * 2;
        this.spin += (home - this.spin) * Math.min(dt * 2.5, 1);
        this.spinVelocity *= Math.exp(-6 * dt);
      }
    }
    this.figure.rotation.y = this.spin;
    if (this.building < BUILD) {
      this.building += dt;
      this.applyBuild();
    }
    this.lightBody();

    this.ring.rotation.z = this.time * TURN;
    this.inner.rotation.z = -this.time * TURN * 1.7;
    this.ringMaterial.opacity = 0.9 * s * Math.min(flare, 1.6);
    this.ringInner.opacity = 0.55 * s;
    this.glowMaterial.opacity = 0.5 * s * flare;
    this.columnMaterial.uniforms.time.value = this.time;
    this.columnMaterial.uniforms.strength.value = (0.5 + 0.9 * this.pulse) * s;
    this.moteMaterial.opacity = 0.85 * s;
    const positions = this.points.geometry.attributes.position.array;
    this.motes.forEach((mote, i) => {
      mote.y += mote.speed * dt * (1 + 2 * this.pulse);
      if (mote.y > COLUMN.height * 0.9) {
        mote.y = 0;
        mote.a = Math.random() * Math.PI * 2;
      }
      mote.a += dt * 0.4;
      positions[i * 3] = Math.cos(mote.a) * mote.r;
      positions[i * 3 + 1] = mote.y;
      positions[i * 3 + 2] = Math.sin(mote.a) * mote.r;
    });
    this.points.geometry.attributes.position.needsUpdate = true;
    this.room.update(this.time, s);
    this.floor.update(this.time, s);
    this.map.update(dt, open);
    this.rig.update(dt);
  }

  // Under the HUD's quads: the map's texture, then the grid room (the
  // battle hidden) or the vignette over the battle. view: the map's
  // { team, rings }.
  before(renderer, view) {
    if (view) this.map.render(renderer, view);
    const canvas = renderer.domElement;
    const aspect = canvas.clientWidth / Math.max(canvas.clientHeight, 1);
    if (this.rig.aspect !== aspect) this.rig.aspect = aspect;
    if (this.mode === "grid") {
      this.room.render(renderer, this.rig.camera);
      return;
    }
    const u = this.vignette.material.uniforms;
    u.aspect.value = aspect;
    // The pool round him, where the camera has him.
    const at = new THREE.Vector3(0, PIVOT, 0).project(this.rig.camera);
    u.center.value.set((at.x + 1) / 2, (at.y + 1) / 2);
    u.strength.value = this.strength;
    u.dip.value = this.dip * this.strength;
    renderer.render(this.vignetteScene, this.screenCamera);
  }

  // Over the HUD's quads: the scene.
  after(renderer) {
    if (!this.framed) return;
    renderer.clearDepth();
    renderer.render(this.scene, this.rig.camera);
  }

  // A point of the canvas (NDC) onto the map: its pin's post ({ pin }) or
  // the point of the map ({ x, y, out }, the HUD's map units), or null.
  pick(x, y) {
    if (!this.mapPlaced) return null;
    const raycaster = new THREE.Raycaster();
    raycaster.setFromCamera(new THREE.Vector2(x, y), this.rig.camera);
    const pin = this.map.pickPin(raycaster);
    if (pin) return { pin };
    return this.map.pick(raycaster);
  }

  dispose() {
    for (const material of [this.ringMaterial, this.ringInner, this.glowMaterial, this.moteMaterial]) material.dispose();
    this.ringMaterial.map?.dispose();
    this.glowMaterial.map?.dispose();
    this.columnMaterial.dispose();
    this.vignette.material.dispose();
    this.vignette.geometry.dispose();
    this.points.geometry.dispose();
    this.room.dispose();
    this.floor.dispose();
    this.map.dispose();
  }
}
