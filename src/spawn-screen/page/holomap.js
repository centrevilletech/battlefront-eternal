import {sceneComponent,sceneProperty,sceneTransform} from "../../rust-render/scene-changes.js";
import {interfaceNode,pickablePlane,planeHitDistance} from '../../interface/resources.js';
import {canvasImage,releaseImage} from '../../interface/images.js';
import {LOOK,paintArray} from '../../rust-render/interface-material.js';
import { interfaceBasic, surfaceMaterial as interfaceMaterial } from "../../rust-render/interface-material.js";
import { renderInterface } from "../../rust-render/draw-guard.js";
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



// A texture drawn once on a square canvas: f(ctx, size).
export function drawn(size, f) {
  const canvas = document.createElement("canvas");
  canvas.width = canvas.height = size;
  f(canvas.getContext("2d"), size);
  return canvasImage(canvas);
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

const additive = (map, color = 0xffffff) => interfaceBasic({ map, color, transparent: true, depthWrite: false, blending: THREE.AdditiveBlending, side: THREE.DoubleSide });

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
    this.target=hud.renderer.resources.interfaceTarget(TEXTURE);
    const circle={kind:"circle",radius:1,segments:96};
    this.back=interfaceNode(circle,interfaceBasic({color:0x000000,transparent:true,opacity:0.55,depthWrite:false,side:THREE.DoubleSide}));
    sceneProperty(this.back,"renderOrder",4);
    this.faceMaterial = interfaceMaterial("face", {
      uniforms: { map: { value: this.target.texture }, tint: { value: new THREE.Color(1, 1, 1) }, time: { value: 0 }, strength: { value: 0 } },
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.DoubleSide,
    });
    this.face = interfaceNode(circle,this.faceMaterial);
    sceneComponent(this.face.position,"z","=",0.002);
    sceneProperty(this.face,"renderOrder",5);
    this.rimMaterial = this.drawn ? additive(rimTexture()) : interfaceBasic();
    this.rim = interfaceNode({kind:"plane",width:2/.86,height:2/.86},this.rimMaterial);
    sceneComponent(this.rim.position,"z","=",0.004);
    sceneProperty(this.rim,"renderOrder",6);
    this.disc.add(this.back, this.face, this.rim);
    this.pinTexture = this.drawn ? pinTexture() : null;
    this.dotTexture = this.drawn ? dotTexture() : null;

    // The cone of light from the projector to the disc.
    this.beamMaterial = interfaceMaterial("beam", {
      uniforms: { color: { value: new THREE.Color(1, 1, 1) }, time: { value: 0 }, strength: { value: 0 } },
      transparent: true,
      depthWrite: false,
      blending: THREE.AdditiveBlending,
      side: THREE.DoubleSide,
    });
    this.beam=interfaceNode({kind:"uiCylinder",top:1,bottom:.05,height:1,segments:48,open:true,transforms:[["translateY",.5]]},this.beamMaterial);
    sceneProperty(this.beam,"renderOrder",3);
    this.root.add(this.beam);

    // The projector: a cone (style "cone", the 2005 game's) or a low
    // emitter; faceted metal lit from above, its edges catching the
    // hologram's light; a ring and a lamp glowing, a pool on the floor.
    this.projector = new THREE.Group();
    this.metal = interfaceMaterial("metal", {
      uniforms: { color: { value: new THREE.Color(0.2, 0.23, 0.27) }, rim: { value: new THREE.Color(1, 1, 1) } },
    });
    this.cone = interfaceNode({kind:"uiCylinder",top:0,bottom:.26,height:.34,segments:6},this.metal);
    sceneComponent(this.cone.position,"y","=",0.07 + 0.17);
    this.base = interfaceNode({kind:"uiCylinder",top:.3,bottom:.36,height:.07,segments:6},this.metal);
    sceneComponent(this.base.position,"y","=",0.035);
    this.glowRingMaterial = this.drawn ? additive(this.dotTexture) : interfaceBasic();
    this.pool = interfaceNode({kind:"plane",width:1.6,height:1.6,transforms:[["rotateX",-Math.PI/2]]},this.glowRingMaterial);
    sceneComponent(this.pool.position,"y","=",0.003);
    sceneProperty(this.pool,"renderOrder",2);
    this.bandMaterial = additive(null);
    this.band = interfaceNode({kind:"ring",inner:.2,outer:.26,segments:6,transforms:[["rotateX",-Math.PI/2],["rotateY",Math.PI/6]]},this.bandMaterial);
    sceneComponent(this.band.position,"y","=",0.072);
    sceneProperty(this.band,"renderOrder",7);
    this.lampMaterial = this.drawn ? additive(this.dotTexture) : interfaceBasic();
    this.lamp = interfaceNode({kind:"plane",width:.42,height:.42},this.lampMaterial);
    sceneProperty(this.lamp,"renderOrder",7);
    this.projector.add(this.pool, this.base, this.band, this.cone, this.lamp);
    this.root.add(this.projector);
    this.style("cone");
  }

  // The projector's shape: "cone" or "emitter".
  style(kind) {
    this.kind = kind;
    const cone = kind === "cone";
    sceneProperty(this.cone,"visible",cone);
    this.lampHeight = cone ? 0.42 : 0.09;
    sceneComponent(this.lamp.position,"y","=",this.lampHeight);
  }

  // The look's colours: tint (the hologram), back (the dark under it, with
  // its alpha in [3]), beam; each [r, g, b] 0..1.
  colors({ tint, back, beam }) {
    if (tint) {
      paintArray(this.faceMaterial,LOOK.TINT,tint);
      for (const material of [this.rimMaterial, this.lampMaterial, this.glowRingMaterial, this.bandMaterial]) paintArray(material,LOOK.COLOR,tint);
      paintArray(this.metal,LOOK.RIM,tint);
    }
    if (back) {
      paintArray(this.back.material,LOOK.COLOR,back);
      this.back.material.values[LOOK.OPACITY] = back[3] ?? 0.55;
    }
    if (beam) paintArray(this.beamMaterial,LOOK.COLOR,beam);
  }

  // Where it stands: the disc's centre (a Vector3) and radius (m), facing
  // `toward` (the camera at rest), tilted back; the projector on the floor
  // (y 0) under it, its cone of light up to the disc.
  place(center, radius, toward) {
    this.radius = radius;
    sceneTransform(this.root.position.set(0, 0, 0));
    sceneTransform(this.disc.position.copy(center));
    const flat = new THREE.Vector3(toward.x, center.y, toward.z);
    sceneTransform(this.disc.lookAt(flat));
    sceneTransform(this.disc.rotateX(-TILT));
    sceneTransform(this.disc.scale.setScalar(radius));
    sceneTransform(this.projector.position.set(center.x, 0, center.z + radius * 0.25));
    const foot = new THREE.Vector3(center.x, this.lampHeight, center.z + radius * 0.25);
    const up = new THREE.Vector3().subVectors(center, foot);
    const length = up.length();
    sceneTransform(this.beam.position.copy(foot));
    sceneTransform(this.beam.quaternion.setFromUnitVectors(new THREE.Vector3(0, 1, 0), up.normalize()));
    sceneTransform(this.beam.scale.set(radius * 0.9, length, radius * 0.9));
    sceneTransform(this.lamp.lookAt(toward));
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
    renderInterface(renderer, this.batchScene, this.ortho, "spawn-holomap");
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
      sceneTransform(pin.root.position.set(at[0], at[1], 0.006));
      const height = post.name === picked ? PIN.picked : post.name === hovered ? PIN.hover : own(post.name) ? PIN.own : PIN.rest;
      pin.height += (height - pin.height) * Math.min(dt * PIN_RATE, 1);
      const pulse = post.name === picked ? 1 + 0.18 * Math.sin(this.time * 5) : 1;
      // The planes span x (across) and z (out of the disc).
      sceneTransform(pin.beam.scale.set(PIN.width * (post.name === picked ? 1.5 : 1), 1, Math.max(pin.height * pulse, 1e-3)));
      sceneTransform(pin.cross.scale.copy(pin.beam.scale));
      sceneTransform(pin.dot.scale.setScalar(post.name === picked ? 0.16 : 0.09));
      const color = this.hud.postColor?.(post, team, true) ?? [1, 1, 1, 1];
      for (const material of pin.materials) paintArray(material,LOOK.COLOR,color);
      sceneProperty(pin.ring,"visible",post.name === picked);
      sceneComponent(pin.ring.rotation,"z","=",this.time * 1.6);
      pin.waveAge += dt;
      sceneProperty(pin.wave,"visible",pin.waveAge < WAVE);
      if (pin.wave.visible) {
        const t = pin.waveAge / WAVE;
        sceneTransform(pin.wave.scale.setScalar(1 + 6 * t));
        pin.wave.material.values[LOOK.OPACITY] = (1 - t) * (1 - t);
      }
    }
    for (const [name, pin] of this.pins) {
      if (seen.has(name)) continue;
      this.disc.remove(pin.root);
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
    ringMaterial.values[LOOK.OPACITY] = 0.9;
    // Two planes across each other, standing out of the disc (its +z).
    const plane={kind:"plane",width:1,height:1,transforms:[["translateY",.5],["rotateX",Math.PI/2]]};
    const beam = pickablePlane(plane,beamMaterial);
    const cross = pickablePlane(plane,beamMaterial);
    sceneComponent(cross.rotation,"z","=",Math.PI / 2);
    const dot = interfaceNode({kind:"plane",width:1,height:1},dotMaterial);
    const ring = interfaceNode({kind:"ring",inner:.075,outer:.09,segments:40,thetaLength:Math.PI*1.6},ringMaterial);
    // A ripple running out from it as it is picked (ping()).
    const waveMaterial = additive(null);
    const wave = interfaceNode({kind:"ring",inner:.08,outer:.095,segments:48},waveMaterial);
    sceneProperty(wave,"visible",false);
    for (const mesh of [beam, cross, dot, ring, wave]) sceneProperty(mesh,"renderOrder",8);
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
    this.faceMaterial.values[LOOK.TIME] = this.time;
    this.faceMaterial.values[LOOK.STRENGTH] = s * (1 + 0.35 * this.flare);
    this.beamMaterial.values[LOOK.TIME] = this.time;
    this.beamMaterial.values[LOOK.STRENGTH] = s * (1 + 1.5 * this.flare);
    sceneComponent(this.rim.rotation,"z","=",-this.time * 0.08);
    this.rimMaterial.values[LOOK.OPACITY] = 0.75 * s;
    this.lampMaterial.values[LOOK.OPACITY] = (0.8 + 0.2 * Math.sin(this.time * 3.1)) * s;
    this.glowRingMaterial.values[LOOK.OPACITY] = 0.35 * s;
    this.bandMaterial.values[LOOK.OPACITY] = (0.6 + 0.3 * Math.sin(this.time * 2.3)) * s;
    // Rising out of the floor as it lights.
    sceneTransform(this.projector.scale.set(1, 0.05 + 0.95 * s, 1));
    // Unfolding as it lights.
    sceneTransform(this.disc.scale.setScalar(this.radius * (0.6 + 0.4 * s)));
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
      const hit=Math.min(planeHitDistance(pin.beam,raycaster),planeHitDistance(pin.cross,raycaster));
      if(hit<distance){
        distance=hit;
        best = name;
      }
    }
    return best;
  }

  dispose() {
    this.hud.renderer.resources.disposeEffectTarget(this.target);
    releaseImage(this.rimMaterial.map);
    releaseImage(this.pinTexture);
    releaseImage(this.dotTexture);
  }
}
