import * as THREE from "three";
import { lighting } from "./materials.js";
import { particleDepth } from "./particle-depth.js";
import { sheathColor } from "./blaster-bolts.js";

// The Enhanced look of the arc caster's lightning (Video > Bolt Caster; the
// HUD names the weapon DN BOLT CASTER). Presentation only: the bolts are the
// ones the world throws (Lightning.spawn, from combat.js's arc() here or a
// worker's or a server's presentation events), with their own ends, charge
// and timing; what they strike and how hard is untouched. Classic keeps the
// 2004 textured ribbons (lightning.js).
//
// A bolt is drawn as real lightning, from the game's own LightningBolt
// settings (Width, SpreadFactor, the branches and children, the colours):
//
//   - its channel is a jagged line by midpoint displacement. The coarse kinks
//     writhe smoothly; the fine ones jump to a new shape with each strike,
//     20 to 35 times a second, and its brightness jumps with them.
//   - it leaves the gun as a leader that races to its target in a few
//     hundredths of a second, then flashes with the return stroke. A link of
//     the chain waits for the one before it to land, so the lightning is seen
//     jumping from one soldier to the next.
//   - thinner children twist about the main channel, and short branches fork
//     off it, both redrawn with each strike.
//   - it lights the ground and soldiers around it (the frame's point lights,
//     ordnance.js LightPool) from the gun, along the bolt and where it lands,
//     flickering with the strikes, and throws sparks where it lands.
//   - a soldier it strikes is shocked: arcs crawl over his body bone to bone
//     and to the ground, a crackling glow covers him (a skinned overlay of his
//     own body's buffers), and his upper body twitches on top of whatever he
//     is playing (the page's bones only: what is hit is decided elsewhere).
//   - a droideka with its shield up is not touched: the bolt stops on the
//     bubble and electricity spreads over the shield from where it struck
//     (the world's own hit ripples the bubble there).
//   - a gun held at a charge crackles at its muzzle, more as the charge
//     builds.
//
// Everything is drawn in one instanced draw of glowing capsules (one per
// segment), shaded per pixel from the closest distance between the view ray
// and the segment, as blaster-bolts.js draws plasma, so the lightning reads
// the same side on and end on (the shooter's own view down it). Where two
// segments of a line meet, each pixel belongs to only one of them (the plane
// halving the angle between them decides), so the glow is seamless instead of
// beaded at every joint.

const LINK_DELAY = 0.05; // s between a chain's links landing
const LEADER = 0.035; // s for a leader to cross, plus LEADER_PER_M a metre
const LEADER_PER_M = 0.0014;
const AFTERGLOW = 0.1; // s a bolt outlives FadeTime here, fading
const SHOCK_BASE = 0.7; // s a struck soldier is shocked, plus SHOCK_CHARGE x charge
const SHOCK_CHARGE = 0.7;
const SPARK_GRAVITY = 9.8;
const MAX_POINTS = 257; // a channel's points (depth 8)
const UP = new THREE.Vector3(0, 1, 0);
const SIDE = new THREE.Vector3(1, 0, 0);

// The upper body's bones a shock twitches (radians at full strength). The
// hands are left alone: whatever they hold stays steady enough to read.
const TWITCH = {
  bone_b_spine: 0.08, bone_ribcage: 0.1, bone_neck: 0.12, bone_head: 0.18,
  bone_l_clavicle: 0.1, bone_r_clavicle: 0.1, bone_l_upperarm: 0.26, bone_r_upperarm: 0.26,
  bone_l_forearm: 0.32, bone_r_forearm: 0.32,
};

// A seeded random number in [0, 1) from two integers, without state.
function hash(seed, n) {
  let t = (seed ^ Math.imul(n | 0, 0x9e3779b1)) >>> 0;
  t = Math.imul(t ^ (t >>> 16), 0x85ebca6b);
  t = Math.imul(t ^ (t >>> 13), 0xc2b2ae35);
  return ((t ^ (t >>> 16)) >>> 0) / 4294967296;
}
const signed = (seed, n) => hash(seed, n) * 2 - 1;
// Smooth value noise along one axis, in [-1, 1].
function wave(seed, x) {
  const i = Math.floor(x), f = x - i, u = f * f * (3 - 2 * f);
  return signed(seed, i) * (1 - u) + signed(seed, i + 1) * u;
}
const newSeed = () => (Math.random() * 0xffffffff) >>> 0;
// A set of arcs (a shock's, a muzzle's) emptied for its next one: what it
// held is kept in `spare` for reuse, so a crackle that changes shape 30
// times a second makes no garbage.
function restart(owner) {
  const spare = (owner.spare ||= []);
  for (const arc of owner.arcs) spare.push(arc);
  owner.arcs.length = 0;
  return spare;
}

const QUAD = new Float32Array([-1, -1, 1, -1, 1, 1, -1, 1]);
const QUAD_INDEX = [0, 1, 2, 0, 2, 3];
// One interleaved instance buffer (a single upload a frame): name, size,
// offset in floats.
const STRIDE = 20;
const LAYOUT = [["iA", 3, 0], ["iB", 3, 3], ["iNA", 3, 6], ["iNB", 3, 9], ["iColor", 4, 12], ["iSize", 4, 16]];

const vertexShader = /* glsl */ `
  attribute vec2 corner;
  attribute vec3 iA;
  attribute vec3 iB;
  attribute vec3 iNA;   // the joint's plane at A (zero: A is an end)
  attribute vec3 iNB;   // the joint's plane at B
  attribute vec4 iColor; // rgb sheath colour (max channel 1), a brightness
  attribute vec4 iSize;  // core radius, glow radius, white-hot share
  uniform float pixelAngle; // image-plane units per pixel at unit depth
  uniform float nearDepth;  // just past the camera's near plane
  uniform float closeRange; // m from the eye where the glow starts to narrow (0: never)
  varying vec2 vPlane;
  varying vec3 vA;
  varying vec3 vB;
  varying vec3 vNA;
  varying vec3 vNB;
  varying vec4 vColor;
  varying vec3 vSize;
  varying float vEnergy;
  varying float vCoreShare;
  void main() {
    vec3 a = (modelViewMatrix * vec4(iA, 1.0)).xyz;
    vec3 b = (modelViewMatrix * vec4(iB, 1.0)).xyz;
    const float NEAR = 0.05;
    if (a.z > -NEAR && b.z > -NEAR) { gl_Position = vec4(2.0, 2.0, 2.0, 1.0); return; }
    bool clipA = a.z > -NEAR, clipB = b.z > -NEAR;
    if (clipA) a = mix(b, a, (b.z + NEAR) / (b.z - a.z));
    if (clipB) b = mix(a, b, (a.z + NEAR) / (a.z - b.z));
    float nearest = min(-a.z, -b.z);
    // Close to the eye (a shooter's own bolt in first person) the glow
    // narrows, so the gun's end doesn't fill the view.
    float close = closeRange > 0.0 ? clamp(nearest / closeRange, 0.3, 1.0) : 1.0;
    // Held to a couple of pixels far away, its brightness thinned to match,
    // so a distant bolt stays visible without growing brighter.
    float glow = max(iSize.y * close, nearest * pixelAngle * 2.2);
    float core = max(iSize.x * close, nearest * pixelAngle * 0.65);
    vEnergy = clamp(iSize.y * close / glow, 0.12, 1.0);
    vCoreShare = clamp(iSize.x * close / core, 0.0, 1.0);
    vec2 ap = a.xy / -a.z, bp = b.xy / -b.z;
    float reach = glow * 2.4;
    float ra = reach / -a.z, rb = reach / -b.z, r = max(ra, rb);
    vec2 dir = bp - ap;
    float len = length(dir);
    dir = len > 1e-7 ? dir / len : vec2(1.0, 0.0);
    vec2 across = vec2(-dir.y, dir.x);
    float along = corner.x * 0.5 + 0.5;
    vec2 p = mix(ap - dir * ra, bp + dir * rb, along) + across * r * corner.y;
    vPlane = p;
    vA = a;
    vB = b;
    mat3 toView = mat3(modelViewMatrix);
    vNA = clipA ? vec3(0.0) : toView * iNA;
    vNB = clipB ? vec3(0.0) : toView * iNB;
    vColor = iColor;
    vSize = vec3(core, glow, iSize.z);
    // All four corners share one depth (past the near plane, so a segment
    // at the gun in first person still draws), so the image-plane position
    // interpolates linearly across the quad.
    float depth = max(nearest, nearDepth);
    gl_Position = projectionMatrix * vec4(p * depth, -depth, 1.0);
  }
`;

const fragmentShader = /* glsl */ `
  uniform mat4 projectionMatrix;
  uniform vec2 fade;
  uniform vec2 eyeFade;
  uniform sampler2D sceneDepth;
  uniform float softParticles;
  uniform vec2 depthResolution;
  uniform vec2 cameraRange;
  varying vec2 vPlane;
  varying vec3 vA;
  varying vec3 vB;
  varying vec3 vNA;
  varying vec3 vNB;
  varying vec4 vColor;
  varying vec3 vSize;
  varying float vEnergy;
  varying float vCoreShare;
  void main() {
    vec3 ray = normalize(vec3(vPlane, -1.0));
    // Where two segments meet, the side of the plane halving their angle
    // that a pixel's ray passes the joint on decides which one draws it.
    if (dot(vNA, vNA) > 0.0 && dot(ray * dot(ray, vA) - vA, vNA) < 0.0) discard;
    if (dot(vNB, vNB) > 0.0 && dot(ray * dot(ray, vB) - vB, vNB) >= 0.0) discard;
    vec3 span = vB - vA;
    float rv = dot(ray, span), ra = dot(ray, vA), vv = dot(span, span);
    float den = vv - rv * rv;
    float s = den > 1e-9 ? clamp((rv * ra - dot(span, vA)) / den, 0.0, 1.0) : 0.0;
    vec3 closest = vA + span * s;
    float along = dot(ray, closest);
    if (along <= 0.0) discard;
    float d2 = max(dot(closest, closest) - along * along, 0.0);
    float d = sqrt(d2);
    float core = vSize.x, glow = vSize.y;
    float hot = (1.0 - smoothstep(core * 0.35, core * 1.25, d)) * vCoreShare;
    float sheath = exp(-d2 / (glow * glow) * 3.2);
    float halo = exp(-d / glow * 1.7) * 0.25;
    // Light adds up as energy and saturates per channel: the core goes white
    // while the sheath around it keeps the bolt's hue instead of clipping.
    vec3 energy = mix(vColor.rgb, vec3(1.0), 0.75) * hot * 2.4 * (0.4 + vSize.z) + vColor.rgb * (sheath * 1.7 + halo);
    vec3 color = 1.0 - exp(-energy * vColor.a * vEnergy);
    vec3 point = ray * along;
    float depth = -point.z;
    float near = clamp((fade.y - depth) / max(fade.y - fade.x, 1.0), 0.0, 1.0);
    // Nothing flares in the eye: in the world, what passes within half a
    // metre of it fades (not over the first-person arms, which are that
    // close).
    color *= near * near * (eyeFade.y > 0.0 ? smoothstep(eyeFade.x, eyeFade.y, depth) : 1.0);
    // Most of a capsule's quad is past its glow: let go before the scene's
    // depth is read (the soft edge only dims it further).
    if (max(color.r, max(color.g, color.b)) < 0.003) discard;
    if (softParticles > 0.5) {
      float stored = texture2D(sceneDepth, gl_FragCoord.xy / depthResolution).x;
      float surface = cameraRange.x * cameraRange.y / (cameraRange.y - stored * (cameraRange.y - cameraRange.x));
      color *= smoothstep(0.0, max(glow * 0.6, 0.04), surface - depth);
    }
    if (max(color.r, max(color.g, color.b)) < 0.003) discard;
    // Only the projection's z and w rows make the depth.
    vec4 hp = vec4(point, 1.0);
    float clipZ = dot(vec4(projectionMatrix[0].z, projectionMatrix[1].z, projectionMatrix[2].z, projectionMatrix[3].z), hp);
    float clipW = dot(vec4(projectionMatrix[0].w, projectionMatrix[1].w, projectionMatrix[2].w, projectionMatrix[3].w), hp);
    gl_FragDepth = clamp(clipZ / clipW * 0.5 + 0.5, 0.0, 1.0);
    // Premultiplied: the light adds, and the sheath also covers part of what
    // is behind it, so the bolt keeps its hue in front of bright ground or sky.
    gl_FragColor = vec4(color, max(color.r, max(color.g, color.b)) * 0.45);
  }
`;

// Glowing capsules, one instance a segment; a sphere is a segment of no
// length.
export class ArcBatch {
  // soft: faded against the scene's depth (not for the first-person view's
  // own scene, which has a depth range of its own).
  constructor(capacity = 512, { soft = true } = {}) {
    this.soft = soft;
    this.count = 0;
    this.capacity = 0;
    this.data = null;
    this.range = { start: 0, count: 0 };
    this.geometry = new THREE.InstancedBufferGeometry();
    this.geometry.setAttribute("corner", new THREE.BufferAttribute(QUAD, 2));
    this.geometry.setIndex(QUAD_INDEX);
    this.material = new THREE.ShaderMaterial({
      name: "arc-lightning",
      uniforms: {
        fade: lighting.fade, eyeFade: { value: new THREE.Vector2(soft ? 0.12 : 0, soft ? 0.5 : 0) }, closeRange: { value: soft ? 2.5 : 0 },
        pixelAngle: { value: 0.002 }, nearDepth: { value: 0.51 },
        sceneDepth: particleDepth.map, softParticles: soft ? particleDepth.enabled : { value: 0 },
        depthResolution: particleDepth.resolution, cameraRange: particleDepth.cameraRange,
      },
      vertexShader,
      fragmentShader,
      transparent: true,
      depthWrite: false,
      blending: THREE.CustomBlending,
      blendEquation: THREE.AddEquation,
      blendSrc: THREE.OneFactor,
      blendDst: THREE.OneMinusSrcAlphaFactor,
      side: THREE.DoubleSide,
      forceSinglePass: true,
    });
    // Over the first-person arms, arcs run between their bones, inside the
    // models: drawn over them.
    if (!soft) this.material.depthTest = false;
    this.mesh = new THREE.Mesh(this.geometry, this.material);
    this.mesh.name = "arc-lightning";
    this.mesh.frustumCulled = false;
    this.mesh.renderOrder = 12;
    this.mesh.visible = false;
    const size = new THREE.Vector2();
    this.mesh.onBeforeRender = (renderer, scene, camera, ...rest) => {
      // Shown empty only to have its program built while a battle loads.
      if (!this.count) return;
      if (this.soft) particleDepth.capture(renderer, scene, camera, ...rest);
      if (camera.isPerspectiveCamera) {
        // The pixels being drawn: under an upscaler, the 3D frame's.
        const target = renderer.getRenderTarget();
        const height = target ? target.height : renderer.getDrawingBufferSize(size).y;
        this.material.uniforms.pixelAngle.value = (2 * Math.tan(THREE.MathUtils.degToRad(camera.fov) / 2)) / Math.max(height, 1) / (camera.zoom || 1);
        this.material.uniforms.nearDepth.value = camera.near * 1.02;
      }
    };
    this.grow(capacity);
  }

  grow(capacity) {
    const data = new Float32Array(capacity * STRIDE);
    if (this.data) data.set(this.data);
    this.data = data;
    this.buffer = new THREE.InstancedInterleavedBuffer(data, STRIDE).setUsage(THREE.DynamicDrawUsage);
    for (const [name, size, offset] of LAYOUT) this.geometry.setAttribute(name, new THREE.InterleavedBufferAttribute(this.buffer, size, offset));
    // three takes the instance limit from the first buffers it binds and
    // keeps it until the geometry is disposed: grown, the new instances
    // would not be drawn.
    delete this.geometry._maxInstanceCount;
    this.capacity = capacity;
  }

  // Room for `more` instances after those pushed so far.
  reserve(more) {
    if (this.count + more > this.capacity) this.grow(Math.max(this.capacity * 2, this.count + more));
  }

  begin() {
    this.count = 0;
  }

  // One segment from a to b. na / nb: the joint planes' normals at its ends
  // (null at a line's end); rgb the sheath colour with its largest channel at
  // 1; hot how white its core runs (0 to 1).
  push(a, b, na, nb, rgb, brightness, core, glow, hot = 0.6) {
    if (brightness <= 0.002) return;
    if (this.count === this.capacity) this.grow(this.capacity * 2);
    const d = this.data, o = this.count++ * STRIDE;
    d[o] = a.x; d[o + 1] = a.y; d[o + 2] = a.z;
    d[o + 3] = b.x; d[o + 4] = b.y; d[o + 5] = b.z;
    if (na) { d[o + 6] = na.x; d[o + 7] = na.y; d[o + 8] = na.z; } else d[o + 6] = d[o + 7] = d[o + 8] = 0;
    if (nb) { d[o + 9] = nb.x; d[o + 10] = nb.y; d[o + 11] = nb.z; } else d[o + 9] = d[o + 10] = d[o + 11] = 0;
    d[o + 12] = rgb[0]; d[o + 13] = rgb[1]; d[o + 14] = rgb[2]; d[o + 15] = brightness;
    d[o + 16] = core; d[o + 17] = glow; d[o + 18] = hot; d[o + 19] = 0;
  }

  // keep: shown even empty (its program is built by the first draw or the
  // loading's warm-up).
  end(keep = false) {
    let drawn = this.count;
    if (!drawn && keep) {
      // Kept empty, nothing would be drawn, and the driver builds its
      // pipeline for the program's first real draw (ANGLE on Vulkan and
      // Metal, SwiftShader), stalling the first bolt; so one dark capsule
      // is drawn: zero brightness, which the fragment shader lets go.
      this.data.fill(0, 0, STRIDE);
      drawn = 1;
    }
    this.geometry.instanceCount = drawn;
    this.mesh.visible = drawn > 0;
    if (!drawn) return;
    // Only what was drawn is sent, through one range kept for it (three
    // empties the list once it has uploaded).
    const ranges = this.buffer.updateRanges;
    ranges.length = 0;
    this.range.count = drawn * STRIDE;
    ranges.push(this.range);
    this.buffer.needsUpdate = true;
  }
}

// The crackling glow over a shocked soldier: his body's own buffers and
// skeleton drawn again, a little proud of the skin, in light. Veins of
// noise fixed to the skin (its bind pose) jump to a new pattern 24 times a
// second, brightest edge on.
const shockVertex = /* glsl */ `
  #include <common>
  #include <skinning_pars_vertex>
  uniform float inflate;
  varying vec3 vBind;
  varying vec3 vNormalView;
  varying vec3 vView;
  void main() {
    #include <beginnormal_vertex>
    #include <skinbase_vertex>
    #include <skinnormal_vertex>
    #include <begin_vertex>
    #include <skinning_vertex>
    vec3 n = normalize(objectNormal);
    transformed += n * inflate;
    vBind = position;
    vec4 view = modelViewMatrix * vec4(transformed, 1.0);
    vView = view.xyz;
    vNormalView = normalize(normalMatrix * n);
    gl_Position = projectionMatrix * view;
  }
`;

const shockFragment = /* glsl */ `
  uniform float time;
  uniform float intensity;
  uniform float seed;
  uniform vec3 color;
  varying vec3 vBind;
  varying vec3 vNormalView;
  varying vec3 vView;
  float cellHash(vec3 p) { return fract(sin(dot(p, vec3(127.1, 311.7, 74.7))) * 43758.5453); }
  float noise(vec3 p) {
    vec3 i = floor(p), f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    float a = mix(mix(cellHash(i), cellHash(i + vec3(1.0, 0.0, 0.0)), f.x), mix(cellHash(i + vec3(0.0, 1.0, 0.0)), cellHash(i + vec3(1.0, 1.0, 0.0)), f.x), f.y);
    float b = mix(mix(cellHash(i + vec3(0.0, 0.0, 1.0)), cellHash(i + vec3(1.0, 0.0, 1.0)), f.x), mix(cellHash(i + vec3(0.0, 1.0, 1.0)), cellHash(i + vec3(1.0, 1.0, 1.0)), f.x), f.y);
    return mix(a, b, f.z);
  }
  void main() {
    vec3 V = normalize(-vView);
    float facing = abs(dot(normalize(vNormalView), V));
    float rim = pow(1.0 - facing, 2.2);
    float step = floor(time * 24.0);
    vec3 q = vBind * 6.5 + vec3(seed, step * 1.37, step * 0.71);
    float n = noise(q) * 0.62 + noise(q * 2.15 + 3.1) * 0.38;
    float m = noise(q * 1.3 + vec3(5.2, -step * 0.9, 1.7));
    // Veins: the noise's crossings of a half, thin and bright.
    float vein = 1.0 - smoothstep(0.0, 0.045, abs(n - 0.5));
    float vein2 = 1.0 - smoothstep(0.0, 0.03, abs(m - 0.5));
    float web = max(vein, vein2 * 0.7);
    vec3 c = color * (rim * 0.85 + web * (0.9 + rim)) + vec3(0.85, 0.92, 1.0) * web * web * 0.9;
    gl_FragColor = vec4(c * intensity, 1.0);
  }
`;

function shockMaterial() {
  return new THREE.ShaderMaterial({
    name: "arc-shock",
    uniforms: { time: { value: 0 }, intensity: { value: 0 }, seed: { value: 0 }, color: { value: new THREE.Color() }, inflate: { value: 0.012 } },
    vertexShader: shockVertex,
    fragmentShader: shockFragment,
    transparent: true,
    depthWrite: false,
    blending: THREE.AdditiveBlending,
  });
}

// A path's points, reused frame to frame.
class Path {
  constructor(capacity = MAX_POINTS) {
    this.points = Array.from({ length: capacity }, () => new THREE.Vector3());
    this.count = 0;
  }
}

export class ArcLightning {
  // lights: the frame's short-lived point lights (ordnance.js LightPool);
  // units: the page's soldiers (registry.js Units), to find the shooter and
  // the soldiers struck; collision: where sparks land; effects: the
  // LightningBolt settings by name (a charging gun's colour); viewScene: the
  // first-person weapon's own scene (graphics.js), where what crackles over
  // the player's own arms and gun is drawn over them; bubbleOf(unit): a
  // droideka's shield bubble as this page draws it (droideka-shield.js).
  constructor(scene, { lights = null, units = null, collision = null, effects = null, viewScene = null, bubbleOf = (unit) => unit?.bubble ?? null } = {}) {
    this.scene = scene;
    this.bubbleOf = bubbleOf;
    this.effects = effects;
    this.lights = lights;
    this.units = units;
    this.collision = collision;
    // Sized for a busy fight (a dozen guns at once): growing mid-battle
    // makes new buffers.
    this.batch = new ArcBatch(4096);
    scene.add(this.batch.mesh);
    this.out = this.batch;
    this.viewBatch = null;
    if (viewScene) {
      this.viewBatch = new ArcBatch(64, { soft: false });
      this.viewBatch.mesh.name = "arc-lightning-view";
      viewScene.add(this.viewBatch.mesh);
    }
    // Both programs are built while the battle loads (graphics.warmup()
    // builds what is shown, and so does the first draw), not at the first
    // bolt: the batch is shown empty, and a stand-in for a shock's glow (one
    // bone, nothing drawn) is kept, for the first frames drawn.
    this.batch.mesh.visible = true;
    this.warming = 90;
    this.warm = this.warmShock();
    scene.add(this.warm);
    this.bolts = [];
    this.shocks = new Map(); // unit -> its shock
    this.shields = new Map(); // unit -> the shock on his shield
    this.charges = new Map(); // unit -> its muzzle crackle
    this.sparks = [];
    this.sparkPool = []; // spent sparks, reused
    this.overlays = new WeakMap(); // actor -> its shock glow
    this.joints = new WeakMap(); // actor -> the bones a shock twitches
    this.colors = new WeakMap(); // LightningBolt def -> its drawn colours
    this.chests = new WeakMap(); // actor -> the bone a bolt strikes him at
    this.chestAt = new WeakMap(); // actor -> his chest this frame
    this.placed = new WeakMap(); // object -> the frame its world matrix was placed
    this.frame = 0;
    this.time = 0;
    this.main = new Path();
    this.child = new Path();
    this.branch = new Path();
    this.twig = new Path();
    this.web = new Path(16);
    this.normals = Array.from({ length: MAX_POINTS }, () => new THREE.Vector3());
    this.hasNormal = new Uint8Array(MAX_POINTS);
    this.dir = new THREE.Vector3();
    this.u = new THREE.Vector3();
    this.w = new THREE.Vector3();
    this.a = new THREE.Vector3();
    this.b = new THREE.Vector3();
    this.c = new THREE.Vector3();
    this.d = new THREE.Vector3();
    this.e = new THREE.Vector3();
    this.hit = { distance: 0, point: new THREE.Vector3(), normal: new THREE.Vector3() };
    this.euler = new THREE.Euler();
    // displace()'s settings, filled in for each line drawn (no object a
    // line).
    this.jag = { rough: 0, spread: 0, seed: 0, jag: 0, time: 0, writhe: 0, coarse: 0 };
    this.active = false;
  }

  // The settings for the next displace().
  jagged(rough, spread, seed, jag, time = 0, writhe = 0, coarse = 0) {
    const o = this.jag;
    o.rough = rough; o.spread = spread; o.seed = seed; o.jag = jag; o.time = time; o.writhe = writhe; o.coarse = coarse;
    return o;
  }

  // A bolt as Lightning.spawn made it: { def, start, end, charge, follow }.
  add(bolt) {
    const def = bolt.def;
    const record = {
      bolt, def,
      charge: THREE.MathUtils.clamp(Number(bolt.charge) || 0, 0, 1),
      start: bolt.start.clone(), end: bolt.end.clone(),
      from: new THREE.Vector3().copy(bolt.start), to: new THREE.Vector3().copy(bolt.end),
      born: this.time, age: 0, delay: 0,
      life: Math.max(def.FadeTime || 0.6, 0.25) + AFTERGLOW,
      seed: newSeed(), jag: newSeed(), branchSeed: newSeed(), nextStrike: 0, strike: 1, flash: 0,
      arrived: false, air: false, shooter: null, shooterGeneration: 0, weapon: null,
      source: null, sourceGeneration: 0, target: null, targetGeneration: 0,
      surface: null, floor: null, children: [],
    };
    let colors = this.colors.get(def);
    if (!colors) {
      const sheath = sheathColor(def.Color || [130, 170, 255, 255]);
      // A touch deeper than the authored colour, so the core's white stands
      // out. (Shared by its bolts: read, never written.)
      colors = { rgb: sheath.map((c) => Math.pow(c, 1.6)), childRgb: sheathColor(def.ChildColor || def.Color || [140, 180, 255, 100]).map((c) => Math.pow(c, 1.4)) };
      this.colors.set(def, colors);
    }
    record.rgb = colors.rgb;
    record.childRgb = colors.childRgb;
    const children = Math.max(Math.floor((def.NumChildren ?? 1) * record.charge), 1);
    for (let i = 0; i < children; i++) record.children.push({ seed: newSeed(), next: 0, on: true });
    // A link of a chain starts where an earlier bolt of the same shot ended.
    let parent = null;
    for (const other of this.bolts) if (this.time - other.born < 0.25 && other.end.distanceToSquared(record.start) < 1e-4) parent = other;
    if (parent) {
      record.delay = parent.delay + LINK_DELAY;
      record.source = parent.target;
      record.sourceGeneration = parent.targetGeneration;
      // It struck something to link on from.
      parent.air = false;
    }
    if (bolt.follow) this.findShooter(record);
    const struck = this.unitAt(record.end, record.shooter);
    if (struck) {
      record.target = struck;
      record.targetGeneration = struck.generation;
    } else this.findSurface(record);
    // A lone bolt that met nothing ran the cone's whole length into the air
    // (else it ended on a mine or a droid): it fizzles out there.
    if (!struck && !record.surface && !parent) {
      const reach = record.weapon?.def?.ordnance?.ConeLength ?? 30;
      record.air = Math.abs(record.start.distanceTo(record.end) - reach) < 0.1;
    }
    this.bolts.push(record);
  }

  // Who threw a bolt held to a gun: the soldier whose gun origin as drawn
  // is where the bolt's own hold puts its start.
  findShooter(record) {
    const at = record.bolt.follow(this.a) || this.a.copy(record.start);
    let best = null, nearest = 0.5 * 0.5;
    for (const unit of this.units?.all?.() || []) {
      const combat = unit.combat;
      if (!combat || unit.removed || !unit.soldier || typeof combat.drawnGunOrigin !== "function") continue;
      const origin = combat.drawnGunOrigin(this.b);
      if (!origin) continue;
      const d = origin.distanceToSquared(at);
      if (d < nearest) { nearest = d; best = unit; }
    }
    if (!best) return;
    record.shooter = best;
    record.shooterGeneration = best.generation;
    record.weapon = best.combat.arsenal?.weapons?.find((w) => w.def?.ordnance?.LightningEffect) || null;
  }

  // The soldier a bolt's end is on (it ends 1 m above his feet), if any.
  unitAt(point, exclude) {
    let best = null, nearest = 1.6 * 1.6;
    for (const unit of this.units?.all?.() || []) {
      if (!unit.soldier || unit.removed || !unit.actor?.root || unit === exclude) continue;
      const p = unit.actor.root.position;
      const dx = p.x - point.x, dy = p.y + 1 - point.y, dz = p.z - point.z;
      const d = dx * dx + dy * dy + dz * dz;
      if (d < nearest) { nearest = d; best = unit; }
    }
    return best;
  }

  // A bolt that struck no one: the surface it stopped on, if it did, for
  // its sparks to fly off.
  findSurface(record) {
    if (!this.collision) return;
    const dir = this.dir.subVectors(record.end, record.start);
    const length = dir.length();
    if (length < 0.1) return;
    dir.divideScalar(length);
    const hit = this.collision.cast(record.start, dir, length + 0.25, this.hit);
    if (hit && Math.abs(hit.distance - length) < 0.35) record.surface = hit.normal.clone();
  }

  update(dt, camera) {
    this.time += dt;
    this.frame++;
    this.active = true;
    this.batch.begin();
    this.viewBatch?.begin();
    try {
      // Shocks first: a bolt ends on a body as its twitch leaves it.
      for (const shock of this.shocks.values()) this.drawShock(shock, dt, camera);
      for (const shock of this.shields.values()) this.drawShieldShock(shock, dt);
      const bolts = this.bolts;
      let kept = 0;
      for (let i = 0; i < bolts.length; i++) if (this.drawBolt(bolts[i], dt, camera)) bolts[kept++] = bolts[i];
      bolts.length = kept;
      this.drawSparks(dt);
      this.drawCharges(dt, camera);
    } catch (error) {
      // A look must never stop the battle: report it once and start clean.
      if (!this.failed) console.error("Bolt caster lightning:", error);
      this.failed = true;
      this.out = this.batch;
      this.clear();
      this.active = true;
      this.batch.begin();
      this.viewBatch?.begin();
    }
    this.batch.end(this.warming > 0);
    this.viewBatch?.end();
  }

  // A shock glow's program, on a body of one bone: one small triangle at
  // intensity 0 (adding nothing), a real draw so the driver's pipeline is
  // built now and not at the first struck soldier; gone once it has been
  // drawn for `warming` frames.
  warmShock() {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.Float32BufferAttribute(new Float32Array([0, 0, 0, 0.01, 0, 0, 0, 0.01, 0]), 3));
    geometry.setAttribute("normal", new THREE.Float32BufferAttribute(new Float32Array([0, 1, 0, 0, 1, 0, 0, 1, 0]), 3));
    geometry.setAttribute("skinIndex", new THREE.Uint16BufferAttribute(new Uint16Array(12), 4));
    geometry.setAttribute("skinWeight", new THREE.Float32BufferAttribute(new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]), 4));
    const mesh = new THREE.SkinnedMesh(geometry, shockMaterial());
    const bone = new THREE.Bone();
    mesh.add(bone);
    mesh.bind(new THREE.Skeleton([bone]));
    mesh.name = "arc-shock-warm";
    mesh.frustumCulled = false;
    mesh.onBeforeRender = () => {
      if (--this.warming > 0) return;
      mesh.visible = false;
      queueMicrotask(() => {
        mesh.removeFromParent();
        mesh.geometry.dispose();
        mesh.material.dispose();
        mesh.skeleton.dispose();
      });
    };
    return mesh;
  }

  // Classic is on: nothing of this look stays on show.
  clear() {
    if (!this.active) return;
    this.active = false;
    this.bolts.length = 0;
    this.sparks.length = 0;
    for (const shock of [...this.shocks.values()]) this.endShock(shock);
    this.shields.clear();
    this.charges.clear();
    this.batch.begin();
    this.batch.end(this.warming > 0);
    this.viewBatch?.begin();
    this.viewBatch?.end();
  }

  // The player's own first-person arms, when they are what is drawn of him.
  firstPersonView(unit) {
    const view = unit.combat?.firstPerson ? unit.combat.view : null;
    return this.viewBatch && view?.root?.visible && view.bones?.length ? view : null;
  }

  // A soldier's shield bubble while it is up, else null.
  shieldOf(unit) {
    const bubble = unit ? this.bubbleOf(unit) : null;
    return bubble?.on && bubble.mesh?.visible ? bubble : null;
  }

  // Whether a point is outside a bubble (an ellipsoid: its mesh's position
  // and scale).
  outside(bubble, point) {
    const c = bubble.mesh.position, r = bubble.mesh.scale;
    return ((point.x - c.x) / r.x) ** 2 + ((point.y - c.y) / r.y) ** 2 + ((point.z - c.z) / r.z) ** 2 > 1;
  }

  // The point on a bubble on the line from its centre toward `toward`
  // (where a line from there to the centre crosses it).
  onShield(bubble, toward, out) {
    const c = bubble.mesh.position, r = bubble.mesh.scale;
    const x = (toward.x - c.x) / r.x, y = (toward.y - c.y) / r.y, z = (toward.z - c.z) / r.z;
    const l = Math.hypot(x, y, z) || 1;
    return out.set(c.x + (x / l) * r.x, c.y + (y / l) * r.y, c.z + (z / l) * r.z);
  }

  // Whether a unit is still the body it was (not gone, nor given a new life).
  same(unit, generation) {
    return Boolean(unit && !unit.removed && unit.generation === generation && unit.actor);
  }

  // Where a bolt strikes a soldier: his ribcage as drawn (or 1 m above his
  // feet).
  chest(unit, out) {
    const actor = unit.actor;
    let bone = this.chests.get(actor);
    if (bone === undefined) {
      bone = actor.byName?.bone_ribcage || actor.bones?.find((b) => /ribcage|spine/i.test(b.name)) || null;
      this.chests.set(actor, bone);
    }
    if (bone) {
      // Asked for by every bolt ending on him, his shock and the links
      // leaving him: his bones are placed once a frame.
      let at = this.chestAt.get(actor);
      if (!at) this.chestAt.set(actor, at = { frame: -1, point: new THREE.Vector3() });
      if (at.frame !== this.frame) {
        at.point.setFromMatrixPosition(this.place(bone));
        at.frame = this.frame;
      }
      return out.copy(at.point);
    }
    return out.copy(actor.root.position).setY(actor.root.position.y + 1);
  }

  // A bone's (or any object's) world matrix brought up to date along its
  // chain, once a frame, as three's updateWorldMatrix(true, false) does it.
  place(object) {
    if (this.placed.get(object) === this.frame) return object.matrixWorld;
    const parent = object.parent;
    if (parent !== null) this.place(parent);
    if (object.matrixAutoUpdate) object.updateMatrix();
    if (parent === null) object.matrixWorld.copy(object.matrix);
    else object.matrixWorld.multiplyMatrices(parent.matrixWorld, object.matrix);
    this.placed.set(object, this.frame);
    return object.matrixWorld;
  }

  // The drawn start of a bolt this frame: its shooter's muzzle as drawn
  // (the gun in his hands, or in first person the arms' gun), else where
  // its hold put it, else where it was thrown from.
  boltFrom(record, out) {
    const shooter = record.shooter;
    if (shooter && this.same(shooter, record.shooterGeneration) && !shooter.combat?.dead && !shooter.combat?.mounted && record.weapon) {
      const at = shooter.combat.firePoint?.(record.weapon);
      if (at?.position) return out.copy(at.position);
    }
    if (record.source && this.same(record.source, record.sourceGeneration)) {
      // From a droideka's shield, it leaves the bubble toward its next target.
      const bubble = this.shieldOf(record.source);
      if (bubble) return this.onShield(bubble, record.to, out);
      return this.chest(record.source, out);
    }
    return out.copy(record.bolt.follow ? record.bolt.start : record.start);
  }

  // The drawn end of a bolt this frame: the chest of the soldier it struck
  // as drawn, or in first person the player's own hands (his eye is just
  // above his chest), else where it was thrown to.
  boltTo(record, out) {
    const target = record.target;
    if (target && this.same(target, record.targetGeneration)) {
      const own = this.firstPersonView(target);
      if (own?.hand) return own.hand.getWorldPosition(out);
      // A droideka's shield up: it stops on the bubble, on the side it came
      // from.
      const bubble = this.shieldOf(target);
      if (bubble && this.outside(bubble, record.from)) return this.onShield(bubble, record.from, out);
      return this.chest(target, out);
    }
    return out.copy(record.end);
  }

  // One bolt's frame; false once it is over.
  drawBolt(record, dt, camera) {
    record.age += dt;
    const t = record.age - record.delay;
    if (t < 0) return true;
    if (t >= record.life) return false;
    const def = record.def;
    const charge = record.charge;
    const from = this.boltFrom(record, record.from);
    const to = this.boltTo(record, record.to);
    const length = from.distanceTo(to);
    if (length < 0.05) return true;
    // The leader crosses, then the return stroke flashes.
    const leader = LEADER + LEADER_PER_M * length;
    const reach = Math.min(t / leader, 1);
    if (reach >= 1 && !record.arrived) {
      record.arrived = true;
      record.flash = 1;
      this.land(record);
    }
    record.flash = Math.max(record.flash - dt * 8, 0);
    if (this.time >= record.nextStrike) {
      record.jag = newSeed();
      record.branchSeed = newSeed();
      record.strike = 0.62 + 0.38 * Math.random();
      if (Math.random() < 0.12) record.strike *= 0.45;
      record.nextStrike = this.time + 0.028 + Math.random() * 0.045;
      if (record.arrived && !record.air && !record.own && Math.random() < 0.45) this.spray(record, to, 1 + Math.floor(Math.random() * 3), 0.6);
    }
    const remain = 1 - t / record.life;
    const fade = remain > 0.45 ? 1 : Math.pow(remain / 0.45, 1.4);
    const widthScale = (0.3 + 0.7 * charge) * (0.75 + 0.25 * fade);
    const brightness = (0.55 + 0.45 * charge) * record.strike * fade * (1 + 1.5 * record.flash);
    // As it dies the colour cools toward violet.
    const cool = 1 - fade;
    const rgb = this.rgb ||= [0, 0, 0];
    rgb[0] = THREE.MathUtils.lerp(record.rgb[0], 0.62, cool * 0.6);
    rgb[1] = THREE.MathUtils.lerp(record.rgb[1], 0.42, cool * 0.6);
    rgb[2] = record.rgb[2];

    // The channel.
    this.basis(from, to);
    const depth = THREE.MathUtils.clamp(Math.ceil(Math.log2(length / 0.32)), 2, 8);
    const spread = Math.max(def.SpreadFactor ?? 0.8, 0.1) * 0.75 * Math.min(1, 0.4 + length / 16);
    displace(this.main, from, to, depth, this.u, this.w, this.jagged(0.2, spread, record.seed, record.jag, this.time, 2.6, 2));
    const n = this.main.count - 1;
    const width = def.Width ?? 0.5;
    const core = width * 0.07 * widthScale;
    const glow = width * 0.3 * widthScale;
    const shown = reach * n;
    this.line(this.main, shown, rgb, brightness, core, glow, 0.9, record.air ? 0.25 : 0.7);
    if (reach < 1) {
      // The leader's head.
      const at = Math.min(Math.floor(shown), n - 1);
      const tip = this.d.lerpVectors(this.main.points[at], this.main.points[at + 1], shown - at);
      this.batch.push(tip, tip, null, null, rgb, 1.4 * brightness, core * 2.2, glow * 2.2, 1);
    }

    // Children twisting about the channel, and branches, once it has landed.
    if (record.arrived) {
      const childWidth = def.ChildWidth ?? width * 0.8;
      const childAlpha = (def.ChildColor?.[3] ?? 100) / 255;
      const amplitude = (def.ChildSpreadFactor ?? 1) * 0.2 * (0.6 + 0.4 * charge) * Math.min(1, 0.35 + length / 12);
      for (const child of record.children) {
        if (this.time >= child.next) {
          child.seed = newSeed();
          child.on = Math.random() < 0.78;
          child.next = this.time + 0.02 + Math.random() * 0.04;
        }
        if (!child.on) continue;
        this.twist(this.main, this.child, child.seed, amplitude);
        this.line(this.child, this.child.count - 1, record.childRgb, brightness * (0.5 + childAlpha), childWidth * 0.035 * widthScale, childWidth * 0.2 * widthScale, 0.5, 1);
      }
      this.branches(record, rgb, brightness, core, glow, charge);
    }

    // The flares at the gun and where it lands, and the light it throws.
    const flicker = brightness * (0.85 + 0.15 * Math.random());
    if (record.shooter || record.bolt.follow) this.batch.push(from, from, null, null, rgb, 0.9 * flicker, core * 1.5, glow * (from.distanceToSquared(camera.position) < 2.25 ? 0.6 : 1.1), 1);
    if (record.arrived && !record.air && !record.own) {
      this.batch.push(to, to, null, null, rgb, flicker, core * 2, glow * 1.8, 1);
      this.batch.push(to, to, null, null, rgb, 0.18 * flicker, 0, glow * 6, 0);
    } else if (record.arrived && record.air) this.fizzle(record, to, rgb, brightness, core, glow);
    if (this.lights) {
      const light = 1.1 * flicker;
      if (record.shooter || record.bolt.follow) this.lights.add(from, rgb, 4 + 2.5 * charge, light * 0.8);
      if (record.arrived && !record.air) this.lights.add(to, rgb, 5 + 4 * charge, light);
      if (length > 10 && reach >= 1) this.lights.add(this.c.lerpVectors(from, to, 0.5), rgb, Math.min(length * 0.5, 10), light * 0.6);
    }
    return true;
  }

  // An orthonormal pair across the line from a to b.
  basis(a, b) {
    const dir = this.dir.subVectors(b, a).normalize();
    this.u.crossVectors(dir, Math.abs(dir.y) > 0.95 ? SIDE : UP).normalize();
    this.w.crossVectors(dir, this.u).normalize();
  }

  // A child strand: the channel's points (every other one) pushed off it by
  // a smooth twist, nothing at the ends.
  twist(main, out, seed, amplitude) {
    const n = main.count - 1;
    const stride = n >= 16 ? 2 : 1;
    const m = Math.floor(n / stride);
    for (let k = 0; k <= m; k++) {
      const i = Math.min(k * stride, n);
      const s = i / n;
      const envelope = Math.pow(Math.sin(Math.PI * s), 0.7);
      const x = (wave(seed, k * 0.42) * amplitude + signed(seed, k * 3 + 7) * 0.04) * envelope;
      const y = (wave(seed + 101, k * 0.42) * amplitude + signed(seed, k * 3 + 8) * 0.04) * envelope;
      out.points[k].copy(main.points[i]).addScaledVector(this.u, x).addScaledVector(this.w, y);
    }
    out.count = m + 1;
  }

  // Branches forking off the channel for this strike, some forking again.
  branches(record, rgb, brightness, core, glow, charge) {
    const def = record.def;
    const main = this.main;
    const n = main.count - 1;
    const length = record.from.distanceTo(record.to);
    const most = Math.round((def.MaxBranches ?? 3) * (0.5 + 0.5 * charge)) + (length > 12 ? 1 : 0);
    const odds = 0.35 + (def.BranchFactor ?? 0.15);
    for (let k = 0; k < most; k++) {
      const seed = record.branchSeed + k * 977;
      if (hash(seed, 1) > odds) continue;
      const at = Math.floor(n * (0.12 + 0.68 * hash(seed, 2)));
      const start = main.points[at];
      const next = main.points[Math.min(at + 1, n)];
      const angle = hash(seed, 3) * Math.PI * 2;
      const spread = (def.BranchSpreadFactor ?? 0.8) * (0.7 + 0.8 * hash(seed, 4));
      const dir = this.d.subVectors(next, start).normalize()
        .addScaledVector(this.u, Math.cos(angle) * spread).addScaledVector(this.w, Math.sin(angle) * spread).normalize();
      const reach = (def.BranchLength ?? 4) * (0.22 + 0.45 * hash(seed, 5)) * (0.55 + 0.45 * charge);
      const end = this.e.copy(start).addScaledVector(dir, reach);
      this.forkBasis(dir);
      displace(this.branch, start, end, 3, this.bu, this.bw, this.jagged(0.3, 0.35, seed, seed ^ 0x5bd1e995));
      this.line(this.branch, this.branch.count - 1, rgb, brightness * 0.8, core * 0.75, glow * 0.7, 0.7, 0.1);
      if (hash(seed, 6) < 0.45) {
        const from = this.branch.points[3 + Math.floor(hash(seed, 7) * 2)];
        const angle2 = angle + (hash(seed, 8) - 0.5) * 2.4;
        const dir2 = this.d.copy(dir).addScaledVector(this.u, Math.cos(angle2) * 0.9).addScaledVector(this.w, Math.sin(angle2) * 0.9).normalize();
        const end2 = this.e.copy(from).addScaledVector(dir2, reach * 0.45);
        this.forkBasis(dir2);
        displace(this.twig, from, end2, 2, this.bu, this.bw, this.jagged(0.3, 0.2, seed + 5, seed ^ 0x27d4eb2f));
        this.line(this.twig, this.twig.count - 1, rgb, brightness * 0.6, core * 0.45, glow * 0.45, 0.5, 0.05);
      }
    }
  }

  // A bolt's end in the air: short forks spraying on from it, new with
  // each strike.
  fizzle(record, to, rgb, brightness, core, glow) {
    const dir = this.d.subVectors(record.to, record.from).normalize();
    for (let k = 0; k < 3; k++) {
      const seed = record.branchSeed + 31 * k + 7;
      const angle = hash(seed, 1) * Math.PI * 2;
      const way = this.e.copy(dir).addScaledVector(this.u, Math.cos(angle) * 0.9).addScaledVector(this.w, Math.sin(angle) * 0.9).normalize();
      const end = this.c.copy(to).addScaledVector(way, 0.3 + 0.6 * hash(seed, 2));
      this.forkBasis(way);
      displace(this.twig, to, end, 2, this.bu, this.bw, this.jagged(0.32, 0.12, seed, seed ^ 0x2545f491));
      this.line(this.twig, this.twig.count - 1, rgb, brightness * 0.5, core * 0.3, glow * 0.4, 0.4, 0.05);
    }
  }

  forkBasis(dir) {
    this.bu ||= new THREE.Vector3();
    this.bw ||= new THREE.Vector3();
    this.bu.crossVectors(dir, Math.abs(dir.y) > 0.95 ? SIDE : UP).normalize();
    this.bw.crossVectors(dir, this.bu).normalize();
  }

  // A path's first `shown` segments (the last in part) as capsules, its
  // widths falling from 1 at its start to `taper` at its end.
  line(path, shown, rgb, brightness, core, glow, hot, taper) {
    const points = path.points;
    const n = Math.min(Math.ceil(shown), path.count - 1);
    if (n < 1 || brightness <= 0.002) return;
    const normals = this.normals;
    const has = this.hasNormal;
    for (let i = 1; i < n; i++) {
      const prev = points[i - 1], at = points[i], next = points[i + 1];
      const ix = at.x - prev.x, iy = at.y - prev.y, iz = at.z - prev.z;
      const ox = next.x - at.x, oy = next.y - at.y, oz = next.z - at.z;
      const li = Math.sqrt(ix * ix + iy * iy + iz * iz), lo = Math.sqrt(ox * ox + oy * oy + oz * oz);
      if (li < 1e-6 || lo < 1e-6) { has[i] = 0; continue; }
      const ki = 1 / li, ko = 1 / lo;
      const nx = ix * ki, ny = iy * ki, nz = iz * ki;
      const m = normals[i];
      m.x = nx + ox * ko; m.y = ny + oy * ko; m.z = nz + oz * ko;
      const l = Math.sqrt(m.x * m.x + m.y * m.y + m.z * m.z);
      if (l < 0.1) { m.x = nx; m.y = ny; m.z = nz; }
      else { const k = 1 / l; m.x *= k; m.y *= k; m.z *= k; }
      has[i] = 1;
    }
    // Written straight into the batch (ArcBatch.push's layout): a line's
    // segments are most of what is drawn, and this is their hot loop.
    const out = this.out;
    out.reserve(n);
    const d = out.data;
    const r = rgb[0], g = rgb[1], bl = rgb[2];
    const last = Math.max(path.count - 1, 1);
    for (let i = 0; i < n; i++) {
      const a = points[i];
      let b = points[i + 1];
      if (i === n - 1 && shown < n) b = this.c.lerpVectors(a, b, shown - i);
      const scale = 1 + (taper - 1) * ((i + 0.5) / last);
      const o = out.count++ * STRIDE;
      d[o] = a.x; d[o + 1] = a.y; d[o + 2] = a.z;
      d[o + 3] = b.x; d[o + 4] = b.y; d[o + 5] = b.z;
      if (i > 0 && has[i]) { const na = normals[i]; d[o + 6] = na.x; d[o + 7] = na.y; d[o + 8] = na.z; } else d[o + 6] = d[o + 7] = d[o + 8] = 0;
      if (i + 1 < n && has[i + 1]) { const nb = normals[i + 1]; d[o + 9] = nb.x; d[o + 10] = nb.y; d[o + 11] = nb.z; } else d[o + 9] = d[o + 10] = d[o + 11] = 0;
      d[o + 12] = r; d[o + 13] = g; d[o + 14] = bl; d[o + 15] = brightness;
      d[o + 16] = core * scale; d[o + 17] = glow * scale; d[o + 18] = hot; d[o + 19] = 0;
    }
  }

  // The bolt has landed: sparks, a flash of light, and a shock for the
  // soldier it struck.
  land(record) {
    if (record.air) return;
    const to = this.boltTo(record, this.c);
    record.own = Boolean(record.target && this.firstPersonView(record.target));
    const unit = record.target && this.same(record.target, record.targetGeneration) ? record.target : null;
    const bubble = unit ? this.shieldOf(unit) : null;
    // Off a shield the sparks glance away from the bubble.
    if (bubble) record.surface = this.d.subVectors(to, bubble.mesh.position).normalize().clone();
    if (!record.own) this.spray(record, to, 10 + Math.round(16 * record.charge), 1);
    this.lights?.flash(to, record.rgb, 6 + 4 * record.charge, 0.16);
    if (!unit) return;
    if (bubble) {
      this.shieldStruck(record, unit, bubble, to);
      return;
    }
    const life = SHOCK_BASE + SHOCK_CHARGE * record.charge;
    const strength = 0.6 + 0.4 * record.charge;
    let shock = this.shocks.get(unit);
    if (shock && shock.generation !== unit.generation) {
      this.endShock(shock);
      shock = null;
    }
    if (shock) {
      shock.life = Math.max(shock.life - shock.age, life);
      shock.age = 0;
      shock.strength = Math.max(shock.strength, strength);
      shock.rgb = record.rgb;
      return;
    }
    this.shocks.set(unit, {
      unit, generation: unit.generation, age: 0, life, strength, rgb: record.rgb, seed: newSeed(),
      flicker: 1, nextFlicker: 0, arcs: [], nextArcs: 0, nextTwitch: 0, twitch: null,
    });
  }

  // Sparks off where a bolt lands: thrown back toward the shooter and up,
  // falling, skipping once off the floor.
  spray(record, at, count, speed) {
    let floor = record.floor;
    if (floor === null) {
      floor = at.y - 1.5;
      if (record.target?.actor?.root) floor = record.target.actor.root.position.y;
      else if (this.collision) {
        const hit = this.collision.cast(this.a.copy(at).setY(at.y + 0.2), this.b.set(0, -1, 0), 4, this.hit);
        if (hit) floor = at.y + 0.2 - hit.distance;
      }
      record.floor = floor;
    }
    const back = this.d.subVectors(record.from, record.to).normalize();
    const away = record.surface || back;
    for (let i = 0; i < count && this.sparks.length < 600; i++) {
      // Spent sparks are reused: a burst of them is a few dozen objects.
      const spark = this.sparkPool.pop() || { p: new THREE.Vector3(), v: new THREE.Vector3(), age: 0, life: 0, floor: 0, bounced: false, rgb: null };
      const v = spark.v.set(Math.random() * 2 - 1, Math.random() * 2 - 1, Math.random() * 2 - 1).normalize();
      v.addScaledVector(away, 0.9).addScaledVector(UP, 0.6).normalize().multiplyScalar((2 + Math.random() * 4.5) * speed);
      spark.p.copy(at);
      spark.age = 0;
      spark.life = 0.25 + Math.random() * 0.4;
      spark.floor = floor;
      spark.bounced = false;
      spark.rgb = record.rgb;
      this.sparks.push(spark);
    }
  }

  drawSparks(dt) {
    const sparks = this.sparks, batch = this.batch;
    let kept = 0;
    for (let i = 0; i < sparks.length; i++) {
      const spark = sparks[i];
      if (this.stepSpark(spark, dt)) sparks[kept++] = spark;
      else this.sparkPool.push(spark);
    }
    sparks.length = kept;
    if (!kept) return;
    // Each a short streak (ArcBatch.push's layout, written here).
    batch.reserve(kept);
    const d = batch.data;
    for (let i = 0; i < kept; i++) {
      const { p, v, rgb } = sparks[i];
      const k = 1 - sparks[i].age / sparks[i].life;
      const brightness = 1.4 * Math.pow(k, 0.7);
      if (brightness <= 0.002) continue;
      const o = batch.count++ * STRIDE;
      d[o] = p.x; d[o + 1] = p.y; d[o + 2] = p.z;
      d[o + 3] = p.x - v.x * 0.022; d[o + 4] = p.y - v.y * 0.022; d[o + 5] = p.z - v.z * 0.022;
      d[o + 6] = d[o + 7] = d[o + 8] = d[o + 9] = d[o + 10] = d[o + 11] = 0;
      d[o + 12] = rgb[0]; d[o + 13] = rgb[1]; d[o + 14] = rgb[2]; d[o + 15] = brightness;
      d[o + 16] = 0.006; d[o + 17] = 0.022; d[o + 18] = 1; d[o + 19] = 0;
    }
  }

  // A spark's flight this frame; false once it is spent.
  stepSpark(spark, dt) {
    spark.age += dt;
    if (spark.age >= spark.life) return false;
    spark.v.y -= SPARK_GRAVITY * dt;
    spark.p.addScaledVector(spark.v, dt);
    if (spark.p.y < spark.floor) {
      spark.p.y = spark.floor;
      if (spark.bounced) return false;
      spark.bounced = true;
      spark.v.y *= -0.35;
      spark.v.x *= 0.55;
      spark.v.z *= 0.55;
    }
    return true;
  }

  // A shocked soldier's frame: arcs over him, his glow, his light and his
  // twitch.
  drawShock(shock, dt, camera) {
    const unit = shock.unit;
    shock.age += dt;
    if (!this.same(unit, shock.generation) || shock.age >= shock.life) {
      this.endShock(shock);
      return;
    }
    if (this.time >= shock.nextFlicker) {
      shock.flicker = 0.5 + 0.5 * Math.random();
      shock.nextFlicker = this.time + 1 / 30;
    }
    const k = 1 - shock.age / shock.life;
    const attack = Math.min(shock.age / 0.04, 1);
    const intensity = attack * Math.pow(k, 1.2) * shock.flicker * shock.strength * (1 + 1.4 * Math.max(0, 1 - shock.age / 0.15));
    const actor = unit.actor;
    if (!unit.walker && !unit.seat && !unit.held?.frame) this.twitch(shock, intensity);
    // His glow, on whatever layer his body is (his own body in first person
    // is on a hidden one).
    const overlay = this.overlay(actor);
    if (overlay) {
      overlay.visible = intensity > 0.01 && actor.body.visible;
      overlay.layers.mask = actor.body.layers.mask;
      const u = overlay.material.uniforms;
      u.time.value = this.time;
      u.intensity.value = intensity * 0.7;
      u.seed.value = (shock.seed % 1000) * 0.37;
      u.color.value.fromArray(shock.rgb);
    }
    const shown = actor.root.visible && actor.body?.visible !== false && camera.layers.test(actor.body?.layers ?? actor.root.layers);
    const chest = this.chest(unit, this.e);
    const own = shown ? null : this.firstPersonView(unit);
    if (shown) {
      if (this.time >= shock.nextArcs) this.pickArcs(shock, intensity);
      for (const arc of shock.arcs) this.drawCrawl(shock, arc, intensity);
    } else if (own) {
      // His own arms and gun in first person, over them.
      own.root.updateMatrixWorld(true);
      if (this.time >= shock.nextArcs) this.pickArmArcs(shock, own);
      this.out = this.viewBatch;
      for (const arc of shock.arcs) this.drawArmArc(shock, own, arc, intensity, camera);
      this.out = this.batch;
    }
    this.lights?.add(chest, shock.rgb, 3 + 1.5 * shock.strength, 0.9 * intensity);
  }

  // A new set of arcs over the player's own first-person gun (his hands
  // are mostly below the view): along it between its grip and its muzzle,
  // a little off its line, and now and then down to his other hand.
  pickArmArcs(shock, view) {
    shock.nextArcs = this.time + 0.03 + Math.random() * 0.035;
    const spare = restart(shock);
    const arc = (t1, t2) => {
      const arc = spare.pop() || { gun: true, t1: 0, t2: 0, o1: [0, 0], o2: [0, 0], seed: 0 };
      arc.t1 = t1; arc.t2 = t2;
      shock.arcs.push(arc);
      return arc;
    };
    const off = (o) => { o[0] = (Math.random() * 2 - 1) * 0.035; o[1] = (Math.random() * 2 - 1) * 0.035; };
    const count = 2 + Math.floor(Math.random() * 2);
    for (let i = 0; i < count; i++) {
      const t1 = 0.25 + Math.random() * 0.75;
      const t2 = THREE.MathUtils.clamp(t1 + (Math.random() < 0.5 ? -1 : 1) * (0.15 + Math.random() * 0.35), 0.2, 1);
      const a = arc(t1, t2);
      off(a.o1); off(a.o2);
      a.seed = newSeed();
    }
    shock.wrist ||= view.bones.find((bone) => /l_wrist|l_hand/i.test(bone.name)) || null;
    if (shock.wrist && Math.random() < 0.5) {
      const a = arc(0.3 + Math.random() * 0.4, -1);
      off(a.o1);
      a.o2[0] = a.o2[1] = 0;
      a.seed = newSeed();
    }
  }

  // A point along the first-person gun (t from its grip, 0, to its muzzle,
  // 1; -1 for his other hand), pushed across the view by `o`.
  gunPoint(shock, view, t, o, camera, out) {
    if (t < 0) return shock.wrist ? out.setFromMatrixPosition(shock.wrist.matrixWorld) : null;
    const combat = shock.unit.combat;
    const at = combat?.firePoint?.(combat.arsenal?.current?.(0));
    if (!at?.position) return null;
    const grip = (view.hand || view.root).getWorldPosition(this.d);
    out.lerpVectors(grip, at.position, t);
    this.right ||= new THREE.Vector3();
    this.upward ||= new THREE.Vector3();
    this.right.set(1, 0, 0).applyQuaternion(camera.quaternion);
    this.upward.set(0, 1, 0).applyQuaternion(camera.quaternion);
    return out.addScaledVector(this.right, o[0]).addScaledVector(this.upward, o[1]);
  }

  drawArmArc(shock, view, arc, intensity, camera) {
    const pa = this.gunPoint(shock, view, arc.t1, arc.o1, camera, this.a);
    const pb = pa && this.gunPoint(shock, view, arc.t2, arc.o2, camera, this.b);
    if (!pb) return;
    this.basis(pa, pb);
    displace(this.twig, pa, pb, 3, this.u, this.w, this.jagged(0.3, 0.04, arc.seed, newSeed()));
    this.line(this.twig, this.twig.count - 1, shock.rgb, 1.3 * intensity, 0.0025, 0.012, 0.9, 1);
  }

  // A new set of arcs over a shocked body: bone to bone across it, and now
  // and then from a limb to the ground.
  pickArcs(shock, intensity) {
    const actor = shock.unit.actor;
    shock.nextArcs = this.time + 0.03 + Math.random() * 0.035;
    shock.bones ||= (actor.bones || []).filter((bone) => !/^hp_|dummyroot|^bone_root$/i.test(bone.name));
    shock.low ||= shock.bones.filter((bone) => /foot|toe|calf|hand/i.test(bone.name));
    const bones = shock.bones;
    const spare = restart(shock);
    const arc = (a, b, seed) => {
      const arc = spare.pop() || { a: null, b: null, ground: false, gx: 0, gz: 0, seed: 0 };
      arc.a = a; arc.b = b; arc.ground = false; arc.seed = seed;
      shock.arcs.push(arc);
      return arc;
    };
    if (!bones.length) return;
    const count = 2 + Math.round(2.5 * Math.min(intensity, 1.2));
    for (let i = 0; i < count; i++) {
      const a = bones[Math.floor(Math.random() * bones.length)];
      const pa = this.a.setFromMatrixPosition(this.place(a));
      let b = null;
      for (let tries = 0; tries < 6 && !b; tries++) {
        const candidate = bones[Math.floor(Math.random() * bones.length)];
        if (candidate === a) continue;
        const d = pa.distanceTo(this.b.setFromMatrixPosition(this.place(candidate)));
        if (d > 0.15 && d < 0.95) b = candidate;
      }
      if (b) arc(a, b, newSeed());
    }
    if (Math.random() < 0.4) {
      const low = shock.low;
      const a = (low.length ? low : bones)[Math.floor(Math.random() * (low.length || bones.length))];
      const angle = Math.random() * Math.PI * 2, out = 0.35 + Math.random() * 0.6;
      const ground = arc(a, null, 0);
      ground.ground = true;
      ground.gx = Math.sin(angle) * out;
      ground.gz = Math.cos(angle) * out;
      ground.seed = newSeed();
    }
  }

  // One arc over a shocked body, its ends just off the skin.
  drawCrawl(shock, arc, intensity) {
    const root = shock.unit.actor.root.position;
    const pa = this.a.setFromMatrixPosition(this.place(arc.a));
    this.offSkin(pa, root, 0.07);
    const pb = this.b;
    if (arc.b) {
      pb.setFromMatrixPosition(this.place(arc.b));
      this.offSkin(pb, root, 0.07);
    } else pb.set(root.x + arc.gx, root.y + 0.03, root.z + arc.gz);
    this.basis(pa, pb);
    displace(this.twig, pa, pb, 3, this.u, this.w, this.jagged(0.34, 0.1, arc.seed, newSeed()));
    this.line(this.twig, this.twig.count - 1, shock.rgb, 1.1 * intensity, 0.006, 0.03, 0.8, arc.b ? 1 : 0.4);
  }

  // A point on a body pushed out from his axis, onto his skin.
  offSkin(point, root, by) {
    const dx = point.x - root.x, dz = point.z - root.z;
    const l = Math.hypot(dx, dz);
    if (l > 1e-4) { point.x += (dx / l) * by; point.z += (dz / l) * by; }
  }

  // The upper body jerked off its pose. A bone the animation wrote this
  // frame takes that as its pose; one it left keeps the last one.
  twitch(shock, intensity) {
    const actor = shock.unit.actor;
    if (!shock.twitch) {
      // His bones' records are kept for his next shock.
      let joints = this.joints.get(actor);
      if (!joints) {
        joints = [];
        for (const [name, amount] of Object.entries(TWITCH)) {
          const bone = actor.byName?.[name];
          if (bone) joints.push({ bone, amount, base: new THREE.Quaternion(), applied: new THREE.Quaternion(), on: false, offset: new THREE.Quaternion() });
        }
        this.joints.set(actor, joints);
      }
      for (const joint of joints) {
        joint.on = false;
        joint.offset.identity();
      }
      shock.twitch = joints;
    }
    if (this.time >= shock.nextTwitch) {
      shock.nextTwitch = this.time + 0.04 + Math.random() * 0.035;
      const strength = Math.min(intensity, 1);
      for (const joint of shock.twitch) {
        const r = joint.amount * strength;
        this.euler.set((Math.random() * 2 - 1) * r, (Math.random() * 2 - 1) * r, (Math.random() * 2 - 1) * r);
        joint.offset.setFromEuler(this.euler);
      }
    }
    for (const joint of shock.twitch) {
      const q = joint.bone.quaternion;
      if (!joint.on || !q.equals(joint.applied)) joint.base.copy(q);
      joint.on = true;
      joint.applied.copy(joint.base).multiply(joint.offset);
      q.copy(joint.applied);
    }
  }

  // A bolt stopped by a droideka's shield: electricity spreads over it from
  // where it struck (the world's own hit, from where the arc started, ripples
  // the bubble there).
  shieldStruck(record, unit, bubble, at) {
    const c = bubble.mesh.position, r = bubble.mesh.scale;
    const impact = new THREE.Vector3((at.x - c.x) / r.x, (at.y - c.y) / r.y, (at.z - c.z) / r.z).normalize();
    const life = 0.55 + 0.45 * record.charge;
    const strength = 0.6 + 0.4 * record.charge;
    const shock = this.shields.get(unit);
    if (shock && shock.generation === unit.generation) {
      shock.life = Math.max(shock.life - shock.age, life);
      shock.age = 0;
      shock.impact.copy(impact);
      shock.strength = Math.max(shock.strength, strength);
      return;
    }
    this.shields.set(unit, { unit, generation: unit.generation, impact, age: 0, life, strength, rgb: record.rgb, flicker: 1, nextFlicker: 0, arcs: [], nextArcs: 0 });
  }

  // The shock on a shield: arcs racing over the bubble out from where it was
  // struck, a flare there, and its light.
  drawShieldShock(shock, dt) {
    const unit = shock.unit;
    shock.age += dt;
    const bubble = this.same(unit, shock.generation) ? this.shieldOf(unit) : null;
    if (!bubble || shock.age >= shock.life) {
      this.shields.delete(unit);
      return;
    }
    if (this.time >= shock.nextFlicker) {
      shock.flicker = 0.55 + 0.45 * Math.random();
      shock.nextFlicker = this.time + 1 / 30;
    }
    const k = 1 - shock.age / shock.life;
    const intensity = Math.min(shock.age / 0.03, 1) * Math.pow(k, 1.3) * shock.flicker * shock.strength * (1 + 1.2 * Math.max(0, 1 - shock.age / 0.12));
    if (this.time >= shock.nextArcs) {
      shock.nextArcs = this.time + 0.03 + Math.random() * 0.03;
      const spare = restart(shock);
      const count = 4 + Math.round(4 * Math.min(intensity, 1));
      for (let i = 0; i < count; i++) {
        const arc = spare.pop() || { a: new THREE.Vector3(), b: new THREE.Vector3() };
        this.turned(shock.impact, Math.random() * 0.3, arc.a);
        // A couple stay about the strike; the rest race far round the bubble.
        const reach = i < 2 ? 0.3 + Math.random() * 0.4 : 0.8 + Math.random() * (0.8 + 0.6 * k);
        this.turned(shock.impact, reach, arc.b);
        shock.arcs.push(arc);
      }
    }
    const c = bubble.mesh.position, r = bubble.mesh.scale;
    const points = this.web.points, m = points.length - 1;
    for (const arc of shock.arcs) {
      // Along the bubble from a to b, a jag across it, just off its skin.
      const jag = newSeed();
      for (let i = 0; i <= m; i++) {
        const t = i / m, sway = Math.sin(Math.PI * t) * 0.07;
        const d = this.e.copy(arc.a).lerp(arc.b, t).normalize();
        this.basis(this.a.set(0, 0, 0), d);
        d.addScaledVector(this.u, signed(jag, i * 2) * sway).addScaledVector(this.w, signed(jag, i * 2 + 1) * sway).normalize();
        points[i].set(c.x + d.x * r.x * 1.02, c.y + d.y * r.y * 1.02, c.z + d.z * r.z * 1.02);
      }
      this.web.count = m + 1;
      this.line(this.web, m, shock.rgb, 1.4 * intensity, 0.008, 0.045, 0.85, 0.3);
    }
    const at = this.onShield(bubble, this.e.copy(c).add(this.a.set(shock.impact.x * r.x, shock.impact.y * r.y, shock.impact.z * r.z)), this.c);
    this.batch.push(at, at, null, null, shock.rgb, 1.3 * intensity, 0.03, 0.22, 1);
    this.lights?.add(at, shock.rgb, 3.5 + 1.5 * shock.strength, 0.8 * intensity);
  }

  // A unit direction turned off `from` by `angle` (radians) toward a random
  // side.
  turned(from, angle, out) {
    this.basis(this.a.set(0, 0, 0), from);
    const spin = Math.random() * Math.PI * 2;
    out.copy(from).multiplyScalar(Math.cos(angle))
      .addScaledVector(this.u, Math.sin(angle) * Math.cos(spin)).addScaledVector(this.w, Math.sin(angle) * Math.sin(spin));
    return out.normalize();
  }

  endShock(shock) {
    this.shocks.delete(shock.unit);
    for (const joint of shock.twitch || []) {
      if (joint.on && joint.bone.quaternion.equals(joint.applied)) joint.bone.quaternion.copy(joint.base);
      joint.on = false;
    }
    const overlay = this.overlays.get(shock.unit.actor);
    if (overlay) overlay.visible = false;
  }

  // His shock glow, built the first time he is shocked.
  overlay(actor) {
    let overlay = this.overlays.get(actor);
    if (overlay !== undefined) return overlay;
    const body = actor.body;
    overlay = null;
    if (body?.isSkinnedMesh && body.skeleton) {
      const geometry = new THREE.BufferGeometry();
      geometry.index = body.geometry.index;
      for (const name of ["position", "normal", "skinIndex", "skinWeight"]) if (body.geometry.attributes[name]) geometry.setAttribute(name, body.geometry.attributes[name]);
      overlay = new THREE.SkinnedMesh(geometry, shockMaterial());
      overlay.name = "arc-shock";
      overlay.boundingSphere = body.boundingSphere;
      overlay.renderOrder = 6;
      overlay.visible = false;
      overlay.castShadow = false;
      overlay.receiveShadow = false;
      (body.parent || actor.root).add(overlay);
      overlay.position.copy(body.position);
      overlay.quaternion.copy(body.quaternion);
      overlay.scale.copy(body.scale);
      overlay.bind(body.skeleton, body.bindMatrix);
    }
    this.overlays.set(actor, overlay);
    return overlay;
  }

  // A gun held at a charge crackles at its muzzle, more as it builds: arcs
  // reaching out from it and wrapping back along the barrel, a pulsing glow,
  // and its light.
  drawCharges(dt, camera) {
    for (const unit of this.units?.all?.() || []) {
      const combat = unit.combat;
      if (!combat || unit.removed || unit.dead || combat.dead || combat.mounted || !unit.soldier) continue;
      const weapon = combat.arsenal?.current?.(0);
      const def = weapon?.def;
      if (!weapon?.charging || !def?.ordnance?.LightningEffect) { this.charges.delete(unit); continue; }
      const at = combat.firePoint?.(weapon);
      if (!at?.position) continue;
      const most = weapon.levels?.[0]?.MaxPressedTime || def.MaxPressedTime || 1;
      const charge = THREE.MathUtils.clamp((weapon.held || 0) / most, 0, 1);
      let crackle = this.charges.get(unit);
      if (!crackle) this.charges.set(unit, crackle = { arcs: [], next: 0, rgb: null });
      if (!crackle.rgb) {
        const bolt = this.effects?.lightning?.[def.ordnance.LightningEffect];
        crackle.rgb = sheathColor(bolt?.Color || [130, 170, 255, 255]).map((c) => Math.pow(c, 1.6));
      }
      const muzzle = this.c.copy(at.position);
      // Along the barrel, back from the muzzle: toward the eye in first
      // person, the gun's origin otherwise.
      const back = combat.firstPerson ? this.d.copy(camera.position) : combat.drawnGunOrigin?.(this.d);
      if (back) back.sub(muzzle);
      const barrel = back && back.lengthSq() > 1e-4 ? back.normalize() : null;
      const near = muzzle.distanceToSquared(camera.position) < 2.25;
      const size = near ? 0.55 : 1;
      if (this.time >= crackle.next) {
        crackle.next = this.time + 0.03 + Math.random() * 0.03;
        const spare = restart(crackle);
        const arc = () => {
          const arc = spare.pop() || { v: new THREE.Vector3(), reach: 0, along: 0, seed: 0 };
          crackle.arcs.push(arc);
          return arc;
        };
        const out = 1 + Math.round(3 * charge);
        for (let i = 0; i < out; i++) {
          const a = arc();
          const v = a.v.set(Math.random() * 2 - 1, Math.random() * 2 - 1, Math.random() * 2 - 1).normalize();
          if (barrel) v.addScaledVector(barrel, -0.6).normalize();
          a.reach = (0.08 + Math.random() * (0.1 + 0.4 * charge)) * size;
          a.along = 0;
          a.seed = newSeed();
        }
        // Well charged, now and then one reaches out much further.
        if (charge > 0.5 && Math.random() < charge * 0.6) {
          const a = arc();
          const v = a.v.set(Math.random() * 2 - 1, Math.random() * 1.6 - 1, Math.random() * 2 - 1).normalize();
          if (barrel) v.addScaledVector(barrel, -0.8).normalize();
          a.reach = (0.45 + Math.random() * 0.6 * charge) * size;
          a.along = 0;
          a.seed = newSeed();
        }
        const wrap = barrel ? Math.round(1 + 2 * charge * Math.random()) : 0;
        for (let i = 0; i < wrap; i++) {
          const a = arc();
          a.v.set(Math.random() * 2 - 1, Math.random() * 2 - 1, Math.random() * 2 - 1).normalize();
          a.reach = (0.015 + Math.random() * 0.04) * size;
          a.along = (0.08 + Math.random() * (0.12 + 0.2 * charge)) * size;
          a.seed = newSeed();
        }
      }
      const rgb = crackle.rgb;
      const bright = 0.45 + 1.1 * charge;
      // The player's own gun in first person: over it, in its own scene.
      if (this.firstPersonView(unit)) this.out = this.viewBatch;
      for (const arc of crackle.arcs) {
        const end = this.b.copy(muzzle);
        if (arc.along && barrel) end.addScaledVector(barrel, arc.along);
        end.addScaledVector(arc.v, arc.reach);
        this.basis(muzzle, end);
        const long = arc.reach > 0.4 * size;
        displace(this.twig, muzzle, end, long ? 3 : 2, this.u, this.w, this.jagged(0.36, long ? 0.14 : 0.06, arc.seed, newSeed()));
        this.line(this.twig, this.twig.count - 1, rgb, bright, 0.005 * size, 0.024 * size, 0.8, 0.3);
      }
      const pulse = 0.85 + 0.15 * Math.sin(this.time * 38) + (charge >= 0.98 ? 0.25 * Math.random() : 0);
      this.out.push(muzzle, muzzle, null, null, rgb, bright * pulse, (0.015 + 0.02 * charge) * size, (0.05 + 0.16 * charge) * size * pulse, 1);
      this.out = this.batch;
      this.lights?.add(muzzle, rgb, 1.5 + 2.5 * charge, (0.3 + 0.7 * charge) * pulse);
    }
  }
}

// A jagged line from a to b in 2^depth segments, by midpoint displacement
// across the line (u, w): the coarse levels (below `coarse`) writhe smoothly
// with `time`, the finer ones take the shape of the strike's `jag`. Each
// level's push is a share (`rough`) of its segment's length, at most
// `spread` (less at each finer level).
const FINER = Array.from({ length: 9 }, (_, level) => Math.pow(0.64, level));
export function displace(path, a, b, depth, u, w, { rough, spread, seed, jag, time, writhe, coarse }) {
  const n = 1 << depth;
  const p = path.points;
  p[0].copy(a);
  p[n].copy(b);
  const length = a.distanceTo(b);
  const ux = u.x, uy = u.y, uz = u.z, wx = w.x, wy = w.y, wz = w.z;
  for (let level = 0, step = n; level < depth; level++, step >>= 1) {
    const half = step >> 1;
    const reach = Math.min((length / (1 << level)) * rough, spread * FINER[level]);
    const fine = level >= coarse;
    const sx = (seed + level * 13 + 1) >>> 0, sy = (seed + level * 13 + 2) >>> 0, phase = time * writhe;
    for (let i = half; i < n; i += step) {
      let x, y;
      if (fine) {
        x = signed(jag, i * 2);
        y = signed(jag, i * 2 + 1);
      } else {
        x = wave(sx, phase + i * 0.61);
        y = wave(sy, phase + i * 0.61 + 17.3);
      }
      const q = p[i], l = p[i - half], r = p[i + half];
      const kx = x * reach, ky = y * reach;
      q.x = l.x + (r.x - l.x) * 0.5; q.y = l.y + (r.y - l.y) * 0.5; q.z = l.z + (r.z - l.z) * 0.5;
      q.x += ux * kx; q.y += uy * kx; q.z += uz * kx;
      q.x += wx * ky; q.y += wy * ky; q.z += wz * ky;
    }
  }
  path.count = n + 1;
  return path;
}
