import * as THREE from "three";
import { Batch } from "../../interface/batch.js";

// The battlefield projected as a hologram beside the soldier (the page
// spawn screen, stage.js): the HUD's own spawn map (its posts, their names
// and blinking, the vehicles and units, the picked post's ring) drawn into
// a texture each frame, shown on a disc tilted back over a projector, with
// scan lines, a rolling band and a rim; each post a pin of light standing
// out of it, taller where it is picked or under the pointer; a cone of
// light from the projector up to it.
//
// The disc is placed in the scene's metres by the page (place()); a press
// on it reads back as a point of the map (pick()), in the HUD's map units.

// The map's span in HUD units: its icons keep the HUD's size, so this sets
// how big they are on it (the HUD's own spawn map is 420 across).
export const MAP_UNITS = 360;
export const TILT = THREE.MathUtils.degToRad(44); // the disc's top leaning away
const TEXTURE = 1024; // px
const PIN = { rest: 0.07, own: 0.12, hover: 0.26, picked: 0.42, width: 0.04 }; // of the disc's radius
const PIN_RATE = 9; // per second, toward a pin's height
const WAVE = 0.7; // s: a picked post's ripple

const FACE_VERTEX = /* glsl */ `
  varying vec2 vUv;
  void main() {
    vUv = uv;
    gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
  }
`;
// The map's picture in the hologram's colour, the posts' and the ring's
// kept; a split of red and blue at the sides, scan lines, a band of light
// rolling up, a glow round the rim.
const FACE_FRAGMENT = /* glsl */ `
  uniform sampler2D map;
  uniform vec3 tint;
  uniform float time;
  uniform float strength;
  varying vec2 vUv;
  void main() {
    vec2 p = vUv * 2.0 - 1.0;
    float r = length(p);
    if (r > 1.0) discard;
    float split = 0.0022 * (0.6 + r);
    vec4 m = texture2D(map, vUv);
    vec3 col = vec3(texture2D(map, vUv + vec2(split, 0.0)).r, m.g, texture2D(map, vUv - vec2(split, 0.0)).b);
    float lum = dot(col, vec3(0.299, 0.587, 0.114));
    // The map's own picture (blue) takes the hologram's colour; the posts,
    // units and ring (green, red, yellow, grey) keep theirs.
    float picture = smoothstep(0.015, 0.1, col.b - max(col.r, col.g));
    vec3 holo = mix(col * 1.7, tint * lum * 2.7, picture);
    float scan = 0.8 + 0.2 * sin(vUv.y * 520.0 - time * 7.0);
    float band = exp(-pow((fract(vUv.y * 0.55 - time * 0.11) - 0.5) * 8.0, 2.0));
    float flicker = 0.95 + 0.05 * sin(time * 41.0) * sin(time * 13.0);
    float rim = smoothstep(0.84, 1.0, r);
    vec3 c = holo * scan * (1.0 + 0.7 * band) + tint * (0.05 + 0.05 * band + 0.55 * rim * rim);
    c *= (1.0 - smoothstep(0.97, 1.0, r)) * strength * flicker;
    gl_FragColor = vec4(c, 1.0);
  }
`;

const BEAM_VERTEX = /* glsl */ `
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
// Bright at the projector, gone toward the disc, brighter at its edges,
// streaks running up it.
const BEAM_FRAGMENT = /* glsl */ `
  uniform vec3 color;
  uniform float time;
  uniform float strength;
  varying vec2 vUv;
  varying vec3 vNormal;
  varying vec3 vView;
  void main() {
    float edge = 1.0 - abs(dot(normalize(vNormal), vView));
    float fade = pow(1.0 - vUv.y, 1.3) * smoothstep(0.0, 0.08, vUv.y) + 0.12 * vUv.y;
    float streaks = 0.7 + 0.3 * sin(vUv.x * 6.2832 * 9.0 + time * 1.7) * sin(vUv.x * 6.2832 * 4.0 - time * 1.1);
    float lines = 0.8 + 0.2 * sin((vUv.y * 30.0 - time * 2.4) * 6.2832);
    float a = fade * (0.05 + 0.32 * pow(edge, 2.0)) * streaks * lines * strength;
    gl_FragColor = vec4(color * a, a);
  }
`;

const METAL_VERTEX = /* glsl */ `
  varying vec3 vView;
  void main() {
    vec4 view = modelViewMatrix * vec4(position, 1.0);
    vView = view.xyz;
    gl_Position = projectionMatrix * view;
  }
`;
// Faceted (each face's own normal), lit from above and the front, its
// edges in the hologram's light.
const METAL_FRAGMENT = /* glsl */ `
  uniform vec3 color;
  uniform vec3 rim;
  varying vec3 vView;
  void main() {
    vec3 n = normalize(cross(dFdx(vView), dFdy(vView)));
    vec3 toEye = normalize(-vView);
    float key = max(dot(n, normalize(vec3(-0.4, 0.9, 0.5))), 0.0);
    float edge = pow(1.0 - max(dot(n, toEye), 0.0), 2.0);
    vec3 c = color * (0.45 + 0.9 * key) + rim * edge * 0.55;
    gl_FragColor = vec4(c, 1.0);
  }
`;

// A texture drawn once on a square canvas: f(ctx, size).
export function drawn(size, f) {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  f(canvas.getContext("2d"), size);
  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.NoColorSpace;
  return texture;
}

// The disc's rim: a ring, ticks, arcs and a dashed inner ring, in white.
function rimTexture() {
  return drawn(1024, (ctx, s) => {
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
    const R = s * 0.43; // the disc's own rim
    ring(R, 4, 0.9);
    ring(R * 1.045, 1.5, 0.5, [3, 9]);
    ring(R * 1.1, 2.5, 0.65);
    ctx.setLineDash([]);
    for (let i = 0; i < 120; i++) {
      const a = (i / 120) * Math.PI * 2;
      const long = i % 10 === 0;
      ctx.globalAlpha = long ? 0.95 : 0.4;
      ctx.lineWidth = long ? 4 : 1.5;
      const r0 = R * (long ? 1.055 : 1.065), r1 = R * 1.09;
      ctx.beginPath();
      ctx.moveTo(c + Math.cos(a) * r0, c + Math.sin(a) * r0);
      ctx.lineTo(c + Math.cos(a) * r1, c + Math.sin(a) * r1);
      ctx.stroke();
    }
    ctx.globalAlpha = 1;
    ctx.lineWidth = 9;
    for (let i = 0; i < 4; i++) {
      const a = (i / 4) * Math.PI * 2 + 0.3;
      ctx.beginPath();
      ctx.arc(c, c, R * 1.1, a, a + 0.42);
      ctx.stroke();
    }
  });
}

// A pin's light: bright at its foot, fading up, soft at its sides.
function pinTexture() {
  return drawn(64, (ctx, s) => {
    const across = ctx.createLinearGradient(0, 0, s, 0);
    across.addColorStop(0, "rgba(255,255,255,0)");
    across.addColorStop(0.5, "rgba(255,255,255,1)");
    across.addColorStop(1, "rgba(255,255,255,0)");
    ctx.fillStyle = across;
    ctx.fillRect(0, 0, s, s);
    ctx.globalCompositeOperation = "destination-in";
    const up = ctx.createLinearGradient(0, s, 0, 0);
    up.addColorStop(0, "rgba(255,255,255,1)");
    up.addColorStop(0.7, "rgba(255,255,255,0.35)");
    up.addColorStop(1, "rgba(255,255,255,0)");
    ctx.fillStyle = up;
    ctx.fillRect(0, 0, s, s);
  });
}

// A soft dot.
export function dotTexture() {
  return drawn(64, (ctx, s) => {
    const g = ctx.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2);
    g.addColorStop(0, "rgba(255,255,255,1)");
    g.addColorStop(0.35, "rgba(255,255,255,0.45)");
    g.addColorStop(1, "rgba(255,255,255,0)");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, s, s);
  });
}

const additive = (map, color = 0xffffff) => new THREE.MeshBasicMaterial({ map, color, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, side: THREE.DoubleSide });

export class HoloMap {
  // hud: the BattleHud (its spawn map's drawing, its posts and colours).
  constructor({ hud }) {
    this.hud = hud;
    this.drawn = typeof document !== "undefined";
    this.radius = 1;
    this.time = 0;
    this.strength = 0;
    this.pins = new Map(); // post name -> { root, beam, dot, hit, height, materials }
    this.root = new THREE.Group(); // the projector on the floor, the beam, the disc
    this.disc = new THREE.Group(); // at the disc's centre, tilted, the map in its xy plane
    this.root.add(this.disc);

    // The map, drawn by the HUD into a texture.
    this.batch = new Batch();
    this.batchScene = new THREE.Scene();
    this.batchScene.add(this.batch.mesh);
    this.ortho = new THREE.OrthographicCamera(-MAP_UNITS / 2, MAP_UNITS / 2, MAP_UNITS / 2, -MAP_UNITS / 2, 0.1, 10);
    // The HUD's map units (x right, y down) onto the camera's view.
    this.matrix = new THREE.Matrix4().set(1, 0, 0, 0, 0, -1, 0, 0, 0, 0, 1, -1, 0, 0, 0, 1);
    this.target = new THREE.WebGLRenderTarget(TEXTURE, TEXTURE, { depthBuffer: false });
    this.target.texture.colorSpace = THREE.NoColorSpace;
    this.target.texture.generateMipmaps = false;

    const circle = new THREE.CircleGeometry(1, 96);
    this.back = new THREE.Mesh(circle, new THREE.MeshBasicMaterial({ color: 0x000000, transparent: true, opacity: 0.55, depthWrite: false, side: THREE.DoubleSide }));
    this.back.renderOrder = 4;
    this.faceMaterial = new THREE.ShaderMaterial({
      uniforms: { map: { value: this.target.texture }, tint: { value: new THREE.Color(1, 1, 1) }, time: { value: 0 }, strength: { value: 0 } },
      vertexShader: FACE_VERTEX,
      fragmentShader: FACE_FRAGMENT,
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.DoubleSide,
    });
    this.face = new THREE.Mesh(circle, this.faceMaterial);
    this.face.position.z = 0.002;
    this.face.renderOrder = 5;
    this.rimMaterial = this.drawn ? additive(rimTexture()) : new THREE.MeshBasicMaterial();
    this.rim = new THREE.Mesh(new THREE.PlaneGeometry(2 / 0.86, 2 / 0.86), this.rimMaterial);
    this.rim.position.z = 0.004;
    this.rim.renderOrder = 6;
    this.disc.add(this.back, this.face, this.rim);
    this.pinTexture = this.drawn ? pinTexture() : null;
    this.dotTexture = this.drawn ? dotTexture() : null;

    // The cone of light from the projector to the disc.
    this.beamMaterial = new THREE.ShaderMaterial({
      uniforms: { color: { value: new THREE.Color(1, 1, 1) }, time: { value: 0 }, strength: { value: 0 } },
      vertexShader: BEAM_VERTEX,
      fragmentShader: BEAM_FRAGMENT,
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.DoubleSide,
    });
    const beam = new THREE.CylinderGeometry(1, 0.05, 1, 48, 1, true);
    beam.translate(0, 0.5, 0); // its foot at the origin, up +y
    this.beam = new THREE.Mesh(beam, this.beamMaterial);
    this.beam.renderOrder = 3;
    this.root.add(this.beam);

    // The projector: a cone (style "cone", the 2005 game's) or a low
    // emitter; faceted metal lit from above, its edges catching the
    // hologram's light; a ring and a lamp glowing, a pool on the floor.
    this.projector = new THREE.Group();
    this.metal = new THREE.ShaderMaterial({
      uniforms: { color: { value: new THREE.Color(0.2, 0.23, 0.27) }, rim: { value: new THREE.Color(1, 1, 1) } },
      vertexShader: METAL_VERTEX,
      fragmentShader: METAL_FRAGMENT,
    });
    this.cone = new THREE.Mesh(new THREE.ConeGeometry(0.26, 0.34, 6, 1), this.metal);
    this.cone.position.y = 0.07 + 0.17;
    this.base = new THREE.Mesh(new THREE.CylinderGeometry(0.3, 0.36, 0.07, 6), this.metal);
    this.base.position.y = 0.035;
    this.glowRingMaterial = this.drawn ? additive(this.dotTexture) : new THREE.MeshBasicMaterial();
    this.pool = new THREE.Mesh(new THREE.PlaneGeometry(1.6, 1.6).rotateX(-Math.PI / 2), this.glowRingMaterial);
    this.pool.position.y = 0.003;
    this.pool.renderOrder = 2;
    this.bandMaterial = additive(null);
    this.band = new THREE.Mesh(new THREE.RingGeometry(0.2, 0.26, 6, 1).rotateX(-Math.PI / 2).rotateY(Math.PI / 6), this.bandMaterial);
    this.band.position.y = 0.072;
    this.band.renderOrder = 7;
    this.lampMaterial = this.drawn ? additive(this.dotTexture) : new THREE.MeshBasicMaterial();
    this.lamp = new THREE.Mesh(new THREE.PlaneGeometry(0.42, 0.42), this.lampMaterial);
    this.lamp.renderOrder = 7;
    this.projector.add(this.pool, this.base, this.band, this.cone, this.lamp);
    this.root.add(this.projector);
    this.style("cone");
  }

  // The projector's shape: "cone" or "emitter".
  style(kind) {
    this.kind = kind;
    const cone = kind === "cone";
    this.cone.visible = cone;
    this.lampHeight = cone ? 0.42 : 0.09;
    this.lamp.position.y = this.lampHeight;
  }

  // The look's colours: tint (the hologram), back (the dark under it, with
  // its alpha in [3]), beam; each [r, g, b] 0..1.
  colors({ tint, back, beam }) {
    if (tint) {
      this.faceMaterial.uniforms.tint.value.setRGB(...tint);
      for (const material of [this.rimMaterial, this.lampMaterial, this.glowRingMaterial, this.bandMaterial]) material.color?.setRGB(...tint);
      this.metal.uniforms.rim.value.setRGB(...tint);
    }
    if (back) {
      this.back.material.color.setRGB(back[0], back[1], back[2]);
      this.back.material.opacity = back[3] ?? 0.55;
    }
    if (beam) this.beamMaterial.uniforms.color.value.setRGB(...beam);
  }

  // Where it stands: the disc's centre (a Vector3) and radius (m), facing
  // `toward` (the camera at rest), tilted back; the projector on the floor
  // (y 0) under it, its cone of light up to the disc.
  place(center, radius, toward) {
    this.radius = radius;
    this.root.position.set(0, 0, 0);
    this.disc.position.copy(center);
    const flat = new THREE.Vector3(toward.x, center.y, toward.z);
    this.disc.lookAt(flat);
    this.disc.rotateX(-TILT);
    this.disc.scale.setScalar(radius);
    this.projector.position.set(center.x, 0, center.z + radius * 0.25);
    const foot = new THREE.Vector3(center.x, this.lampHeight, center.z + radius * 0.25);
    const up = new THREE.Vector3().subVectors(center, foot);
    const length = up.length();
    this.beam.position.copy(foot);
    this.beam.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), up.normalize());
    this.beam.scale.set(radius * 0.9, length, radius * 0.9);
    this.lamp.lookAt(toward);
    this.root.updateMatrixWorld(true);
  }

  // The HUD's spawn map into the texture (under the HUD's own pass, after
  // its quads are gathered): as `team` sees it, with its rings.
  render(renderer, { team, rings }) {
    const hud = this.hud;
    if (!hud.drawBattlefield) return;
    const own = hud.batch;
    hud.batch = this.batch;
    this.batch.begin();
    try {
      hud.drawBattlefield(this.matrix, MAP_UNITS, { team, mask: "map_mask", labels: true, rings, spawn: true });
    } finally {
      hud.batch = own;
    }
    this.batch.end();
    const previous = renderer.getRenderTarget();
    const clear = renderer.getClearColor(new THREE.Color());
    const alpha = renderer.getClearAlpha();
    renderer.setRenderTarget(this.target);
    renderer.setClearColor(0x000000, 0);
    renderer.clear(true, false, false);
    renderer.render(this.batchScene, this.ortho);
    renderer.setRenderTarget(previous);
    renderer.setClearColor(clear, alpha);
  }

  // A post's place on the disc (its xy plane, radius 1), from the HUD's map.
  local(post) {
    const point = this.hud.mapPoint?.(post.position.x ?? post.position[0], post.position.z ?? post.position[2], MAP_UNITS);
    if (!point) return null;
    return [(point[0] / MAP_UNITS) * 2, (-point[1] / MAP_UNITS) * 2];
  }

  // The posts' pins: posts the HUD's ({ name, position, team, ... }), team
  // whose view, picked and hovered post names, own(name) for one the team
  // may spawn at.
  pinPosts(posts, { team, picked, hovered, own = () => false }, dt) {
    const seen = new Set();
    for (const post of posts) {
      if (post.mapped === false || post.active === false) continue;
      const at = this.local(post);
      if (!at || Math.hypot(at[0], at[1]) > 1) continue;
      seen.add(post.name);
      let pin = this.pins.get(post.name);
      if (!pin) pin = this.addPin(post.name);
      pin.root.position.set(at[0], at[1], 0.006);
      const height = post.name === picked ? PIN.picked : post.name === hovered ? PIN.hover : own(post.name) ? PIN.own : PIN.rest;
      pin.height += (height - pin.height) * Math.min(dt * PIN_RATE, 1);
      const pulse = post.name === picked ? 1 + 0.18 * Math.sin(this.time * 5) : 1;
      // The planes span x (across) and z (out of the disc).
      pin.beam.scale.set(PIN.width * (post.name === picked ? 1.5 : 1), 1, Math.max(pin.height * pulse, 1e-3));
      pin.cross.scale.copy(pin.beam.scale);
      pin.dot.scale.setScalar(post.name === picked ? 0.16 : 0.09);
      const color = this.hud.postColor?.(post, team, true) ?? [1, 1, 1, 1];
      for (const material of pin.materials) material.color.setRGB(color[0], color[1], color[2]);
      pin.ring.visible = post.name === picked;
      pin.ring.rotation.z = this.time * 1.6;
      pin.waveAge += dt;
      pin.wave.visible = pin.waveAge < WAVE;
      if (pin.wave.visible) {
        const t = pin.waveAge / WAVE;
        pin.wave.scale.setScalar(1 + 6 * t);
        pin.wave.material.opacity = (1 - t) * (1 - t);
      }
    }
    for (const [name, pin] of this.pins) {
      if (seen.has(name)) continue;
      this.disc.remove(pin.root);
      for (const material of pin.materials) material.dispose();
      this.pins.delete(name);
    }
  }

  // A post picked: a ripple runs out from its pin, the beam flares.
  ping(name) {
    const pin = this.pins.get(name);
    if (pin) pin.waveAge = 0;
    this.flare = 1;
  }

  // Another team's map: it projects again, from nothing.
  reproject() {
    this.strength = Math.min(this.strength, 0.1);
  }

  addPin(name) {
    const root = new THREE.Group();
    const beamMaterial = additive(this.pinTexture);
    const dotMaterial = additive(this.dotTexture);
    const ringMaterial = additive(null);
    ringMaterial.opacity = 0.9;
    // Two planes across each other, standing out of the disc (its +z).
    const plane = new THREE.PlaneGeometry(1, 1).translate(0, 0.5, 0).rotateX(Math.PI / 2);
    const beam = new THREE.Mesh(plane, beamMaterial);
    const cross = new THREE.Mesh(plane, beamMaterial);
    cross.rotation.z = Math.PI / 2;
    const dot = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), dotMaterial);
    const ring = new THREE.Mesh(new THREE.RingGeometry(0.075, 0.09, 40, 1, 0, Math.PI * 1.6), ringMaterial);
    // A ripple running out from it as it is picked (ping()).
    const waveMaterial = additive(null);
    const wave = new THREE.Mesh(new THREE.RingGeometry(0.08, 0.095, 48), waveMaterial);
    wave.visible = false;
    for (const mesh of [beam, cross, dot, ring, wave]) mesh.renderOrder = 8;
    root.add(beam, cross, dot, ring, wave);
    this.disc.add(root);
    const pin = { root, beam, cross, dot, ring, wave, waveAge: Infinity, height: 0, materials: [beamMaterial, dotMaterial, ringMaterial, waveMaterial] };
    this.pins.set(name, pin);
    return pin;
  }

  update(dt, open) {
    this.time += dt;
    this.strength += ((open ? 1 : 0) - this.strength) * Math.min(dt * 3, 1);
    this.flare = Math.max((this.flare ?? 0) - dt * 2.5, 0);
    const s = this.strength;
    this.faceMaterial.uniforms.time.value = this.time;
    this.faceMaterial.uniforms.strength.value = s * (1 + 0.35 * this.flare);
    this.beamMaterial.uniforms.time.value = this.time;
    this.beamMaterial.uniforms.strength.value = s * (1 + 1.5 * this.flare);
    this.rim.rotation.z = -this.time * 0.08;
    this.rimMaterial.opacity = 0.75 * s;
    this.lampMaterial.opacity = (0.8 + 0.2 * Math.sin(this.time * 3.1)) * s;
    this.glowRingMaterial.opacity = 0.35 * s;
    this.bandMaterial.opacity = (0.6 + 0.3 * Math.sin(this.time * 2.3)) * s;
    // Rising out of the floor as it lights.
    this.projector.scale.set(1, 0.05 + 0.95 * s, 1);
    // Unfolding as it lights.
    this.disc.scale.setScalar(this.radius * (0.6 + 0.4 * s));
  }

  // A ray (a THREE.Raycaster) onto the disc: the point of the map it meets,
  // in the HUD's map units from the map's centre (x right, y down), and its
  // distance out (1 at the rim); or null.
  pick(raycaster) {
    this.root.updateMatrixWorld(true);
    const plane = new THREE.Plane().setFromNormalAndCoplanarPoint(
      new THREE.Vector3(0, 0, 1).transformDirection(this.disc.matrixWorld),
      new THREE.Vector3().setFromMatrixPosition(this.disc.matrixWorld),
    );
    const point = raycaster.ray.intersectPlane(plane, new THREE.Vector3());
    if (!point) return null;
    const local = this.disc.worldToLocal(point);
    const out = Math.hypot(local.x, local.y);
    if (out > 1.04) return null;
    return { x: (local.x / 2) * MAP_UNITS, y: (-local.y / 2) * MAP_UNITS, out };
  }

  // A pin's light under a ray: its post's name, or null.
  pickPin(raycaster) {
    let best = null;
    let distance = Infinity;
    for (const [name, pin] of this.pins) {
      const hits = raycaster.intersectObjects([pin.beam, pin.cross], false);
      if (hits[0] && hits[0].distance < distance) {
        distance = hits[0].distance;
        best = name;
      }
    }
    return best;
  }

  dispose() {
    this.target.dispose();
    for (const map of this.batch.materials.values()) for (const material of map.values()) material.dispose();
    this.batch.geometry.dispose();
    for (const material of [this.back.material, this.faceMaterial, this.rimMaterial, this.beamMaterial, this.metal, this.lampMaterial, this.glowRingMaterial, this.bandMaterial]) material.dispose();
    this.rimMaterial.map?.dispose();
    this.pinTexture?.dispose();
    this.dotTexture?.dispose();
    for (const pin of this.pins.values()) for (const material of pin.materials) material.dispose();
  }
}
