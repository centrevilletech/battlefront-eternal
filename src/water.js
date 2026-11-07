import {sceneComponent,sceneLayer,sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import { EFFECT, EFFECT_TARGET } from './rust-render/effect-layout.js';
import { ownedEffectMaterial } from './rust-render/effect-material.js';
import { COLOR_BUFFER_BIT, DEPTH_BUFFER_BIT } from "./render-resources.js";
import { WaterState } from "./runtime/water-state.js";
import * as THREE from "three";
import { lighting } from "./materials.js";
import { WATER_QUALITIES } from "./settings.js";
import { OceanSurface } from "./ocean-surface.js";
import { SurfaceHost } from "./environment/surfaces.js";
import { Surface as LIMITS, SurfaceConfig as CONFIG, SurfaceFrame as FRAME, SurfaceResult as RESULT } from "./environment/layout.js";

// Terrain water, as the 2004 game draws it on PC: a flat surface over the
// map's water cells (32 m squares on Theed) at the layer's height, bobbing
// 0.06 m at 0.3 Hz.
//
// Two passes over the same surface, after the opaque scene:
//   1. what lies under the water is multiplied by RefractionColor;
//   2. a reflection, at ReflectionColor's alpha, plus white sun glints
//      (N.H)^8, blended by a Fresnel term from FresnelMin looking straight
//      down to FresnelMax at grazing angles.
// The surface normal comes from the NormalMapTextures frames, cross-faded at
// their frame rate and tiled Tile times per cell; the reflection is jittered
// by the BumpMapTextures frames (0.03 of the screen, turned with the view).
//
// Low uses MainTexture and scrolling specular masks. High reflects nearby
// geometry in screen space; Ultra uses a full mirrored scene. Enhanced water resolves opaque color/depth once for
// depth-aware refraction/absorption, with animated world-space normal detail.
//
// Rings spread where things meet the water (radius growing 1.5 m/s, fading
// in over the first 15% of their life and out over the rest), and under the
// surface the screen takes UnderwaterColor.

const RIPPLE_MAX = LIMITS.RIPPLES;
const RIPPLE_SEGMENTS = LIMITS.SEGMENTS;
const DISTORT = 0.03;
const REFLECTION_SIZE = [512, 256];
const rgba = (c, fallback = [255, 255, 255, 255]) => {
  const [r, g, b, a = 255] = c || fallback;
  return [r / 255, g / 255, b / 255, a / 255];
};



// Whether the water is a sea rather than a pool or canal: its cells reach
// all four edges of the water grid (the map ends in water on every side).
export function isSea(data) {
  const n = data.grid;
  if (!n || !data.cells?.length) return false;
  let west = false, east = false, north = false, south = false;
  for (const [i, j] of data.cells) {
    if (i === 0) west = true;
    if (i === n - 1) east = true;
    if (j === 0) north = true;
    if (j === n - 1) south = true;
  }
  return west && east && north && south;
}

export class Water extends WaterState {
  // data: the map's water (tools/zero_water.py); texture(name) gives a map
  // texture; graphics draws the scene; weather is the map's (an Enhanced
  // ocean storms only where it rains, ocean-surface.js).
  constructor({ data, texture, graphics, weather = null, surfaceOwner }) {
    super(data);
    this.data = data;
    this.graphics = graphics;
    const look = data.look;
    this.host = new SurfaceHost(surfaceOwner);
    this.host.config[CONFIG.HEIGHT] = data.height;
    this.host.config[CONFIG.BOBBING] = (look.OscillationEnable?.[0] ?? 1) > 0 ? 1 : 0;
    // The original draws an ocean where the look asks (OceanEnable). A sea
    // the look leaves flat (Kashyyyk's: water out to every edge of its
    // grid) takes the Enhanced ocean too, while Enhanced ocean effects are
    // on; Classic keeps the flat layer the 2004 game drew there.
    const oceanLook = (look.OceanEnable?.[0] || 0) > 0;
    const sea = !oceanLook && isSea(data);
    const ocean = oceanLook || sea;
    this.height = data.height;
    this.cell = data.cell;
    this.offset = data.offset;
    this.grid = data.grid;
    this.cells = new Set(data.cells.map(([i, j]) => j * data.grid + i));
    this.bobbing = (look.OscillationEnable?.[0] ?? 1) > 0;
    this.time = 0;
    this.wave = 0;
    this.frames = (key) => {
      const set = look[key];
      if (!set || set.length < 3) return null;
      const textures = Array.from({ length: Math.round(set[1]) }, (_, k) => texture(`${set[0]}${k}`)).filter(Boolean);
      return textures.length ? { textures, fps: set[2] } : null;
    };
    this.normals = this.frames("NormalMapTextures");
    this.bumps = this.frames("BumpMapTextures");
    this.masks = this.frames("SpecularMaskTextures");
    const config = this.host.config;
    config[CONFIG.NORMAL_COUNT] = this.normals?.textures.length || 0; config[CONFIG.NORMAL_FPS] = this.normals?.fps || 0;
    config[CONFIG.BUMP_COUNT] = this.bumps?.textures.length || 0; config[CONFIG.BUMP_FPS] = this.bumps?.fps || 0;
    config[CONFIG.MASK_COUNT] = this.masks?.textures.length || 0; config[CONFIG.MASK_FPS] = this.masks?.fps || 0;

    // Water.surface's authored mask and patches compile in water_resources.rs.
    // Scene anchors carry numeric inputs; no near material or geometry survives.
    const c0=rgba(look.RefractionColor);
    this.target=graphics.renderer.resources.effectTarget(EFFECT_TARGET.WATER_MIRROR);
    this.mirror=new THREE.PerspectiveCamera();this.mirror.userData.mirror=true;
    this.plainProjection=new THREE.Matrix4();this.obliqueProjection=new THREE.Matrix4();this.reflectionMatrix=new THREE.Matrix4();
    this.surfaceUniforms={tint:{value:new THREE.Vector3(c0[0],c0[1],c0[2])},time:{value:0},
      sceneColor:{value:null},sceneDepth:{value:null},resolution:{value:new THREE.Vector2(1,1)},cameraRange:{value:new THREE.Vector2(.5,200)}};
    this.nativeSources=[];this.resourceCore=null;this.sourceId=-1;this.normalIds=null;this.bumpIds=null;this.maskIds=null;this.distort=0;
    this.group=new THREE.Group();
    this.tintPass=this.surfaceAnchor(EFFECT.WATER_TINT,-20);
    this.mirrorPass=this.surfaceAnchor(EFFECT.WATER_REFLECTION,-19);
    this.mainMap=look.MainTexture?.[0]?texture(look.MainTexture[0]):null;
    this.lowPass=this.mainMap?this.surfaceAnchor(EFFECT.WATER_TEXTURED,-20):null;
    this.group.add(this.tintPass,this.mirrorPass);if(this.lowPass)this.group.add(this.lowPass);
    graphics.scene.add(this.group);graphics.scene.userData.water=this;

    // The far scene's water. The original draws the surface again in the far
    // scene, after the low-res terrain and in its fog (the sky's draw hands
    // the water's draw the far flag), out to the look's own FarSceneRange,
    // or else the sky's, when that reaches past the near
    // scene. Without it the far terrain's sea bed showed bare past the near
    // scene (both Kashyyyks). The same passes on a coarse grid, drawn only
    // from the eye's far camera (graphics.eyeLayer): mirrors leave it out.
    this.farReach = look.FarSceneRange?.[0] || 0;
    this.farRange=new Float64Array(2);
    this.farView = new THREE.PerspectiveCamera();
    this.farFrustum = new THREE.Frustum();
    this.farTint = this.surfaceAnchor(EFFECT.WATER_TINT,-20,true);
    this.farMirror = this.surfaceAnchor(EFFECT.WATER_REFLECTION,-19,true);
    this.farLow = this.lowPass ? this.surfaceAnchor(EFFECT.WATER_TEXTURED,-20,true) : null;
    this.farGroup = new THREE.Group();
    this.farGroup.name = "far-water";
    this.farGroup.add(...[this.farTint, this.farMirror, this.farLow].filter(Boolean));
    sceneProperty(this.farGroup,"visible",false);
    graphics.farScene?.add(this.farGroup);
    // Reflection selection still consumes the authored water bounds. This
    // collision/culling fact has no vertex layout or GPU resource ownership.
    this.bounds=new THREE.Box3();const point=new THREE.Vector3();
    for(const [i,j,patched]of data.cells){if(patched)continue;for(const a of [0,1])for(const b of [0,1])this.bounds.expandByPoint(point.set(Math.fround(i*data.cell-data.offset[0]+a*data.cell),Math.fround(data.height),Math.fround(j*data.cell-data.offset[1]+b*data.cell)));}
    for(const patch of data.patches||[])for(const p of patch.positions)this.bounds.expandByPoint(point.set(Math.fround(p[0]),Math.fround(p[1]),Math.fround(p[2])));
    this.bounds.min.y -= 1;
    this.bounds.max.y += 1;
    // Each water cell's own box: one box around a whole canal is "in view"
    // from most of the city, and with it the scene's second (mirrored)
    // render ran nearly every frame.
    this.cellBounds = data.cells.map(([i, j]) => new THREE.Box3(
      new THREE.Vector3(i * data.cell - data.offset[0], data.height - 1, j * data.cell - data.offset[1]),
      new THREE.Vector3((i + 1) * data.cell - data.offset[0], data.height + 1, (j + 1) * data.cell - data.offset[1])));

    // Rings.
    this.rings = this.ringMesh(texture("FXrippl1"));
    graphics.scene.add(this.rings);

    // The tint under the surface.
    const u = rgba(look.UnderwaterColor, [255, 255, 255, 0]);
    this.underwater = new THREE.Mesh(
      new THREE.PlaneGeometry(2, 2),
      ownedEffectMaterial('effects/water-underwater-tint', {
        uniforms: { color: { value: new THREE.Vector4(...u) } },
        transparent: true,
        depthTest: false,
        depthWrite: false,
      }),
    );
    sceneProperty(this.underwater,"frustumCulled",false);
    this.underwater.material.userData.bfeNoShadows = true;
    sceneProperty(this.underwater.material,"visible",true);
    this.underwaterScene = new THREE.Scene();
    this.underwaterScene.add(this.underwater);
    this.passCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);

    // Enhanced submerged views are composited inside the scene, after other
    // transparency and before the final AA. Capture here even when the surface
    // is out of view (e.g. looking at the riverbed), so depth never goes stale.
    const surfaceUniforms = this.surfaceUniforms;
    this.underwaterFog = new THREE.Mesh(
      new THREE.PlaneGeometry(2, 2),
      ownedEffectMaterial('effects/water-underwater-fog', {
        uniforms: {
          sceneColor: surfaceUniforms.sceneColor, sceneDepth: surfaceUniforms.sceneDepth,
          cameraRange: surfaceUniforms.cameraRange, tint: surfaceUniforms.tint, time: surfaceUniforms.time,
        },
        transparent: true, blending: THREE.NoBlending, depthTest: false, depthWrite: false,
      }),
    );
    sceneProperty(this.underwaterFog,"frustumCulled",false);
    sceneProperty(this.underwaterFog,"renderOrder",1e8);
    sceneProperty(this.underwaterFog,"visible",false);
    this.underwaterFog.onBeforeRender = (renderer, scene, camera) => this.captureSurface(renderer, camera);
    graphics.scene.add(this.underwaterFog);

    this.frustum = new THREE.Frustum();
    this.matrix = new THREE.Matrix4();
    this.forward = new THREE.Vector3();
    this.reflectionFar = null;
    this.surfaceBuffer = null;
    this.tintPass.onBeforeRender = (renderer, scene, camera) => this.captureSurface(renderer, camera);
    this.oceanSurface = ocean ? new OceanSurface({ data, texture, graphics, weather, surfaceHost: this.host, waterOwner:this }) : null;
    this.oceanAlways = oceanLook;
    this.applyQuality();
    graphics.settings.onChange((key) => { if (key === "waterQuality" || key === "oceanEffects") this.applyQuality(); });
  }

  // Native water descriptors are independent of the scene node. Far snapshots
  // use the same typed values with their own camera and view-range facts.
  surfaceAnchor(row,order,far=false) {
    const mesh=new THREE.Object3D();sceneProperty(mesh,"renderOrder",order);sceneProperty(mesh,"frustumCulled",false);
    const source={family:'water',nativeKind:row.ID,row,owner:this,mesh,far,center:null,values:new Float64Array(row.LENGTH)};
    mesh.userData.effectModel=source;if(far){mesh.name='far-water';sceneLayer(mesh.layers,"set",this.graphics.eyeLayer??4);}
    source.prepare=camera=>this.prepareSurface(source,camera);this.nativeSources.push(source);return mesh;
  }
  // Raw authored bytes enter once. Only Rust knows surface topology, catalog
  // selection, blend/depth policy or the meaning of sampler binding slots.
  prepareResources(core) {
    this.host.refresh();if(this.resourceCore)return;this.resourceCore=core;
    const facts=core.rust.water_source(JSON.stringify(this.data),Boolean(this.oceanSurface));this.sourceId=facts[0];
    const sample=t=>core.texture(core.resources.staticSample(t));
    this.normalFallback=core.texture(core.resources.gpu.water_fallback(true));this.maskFallback=core.texture(core.resources.gpu.water_fallback(false));
    this.normalIds=this.normals?Int32Array.from(this.normals.textures,sample):null;
    this.bumpIds=this.bumps?Int32Array.from(this.bumps.textures,sample):null;
    this.maskIds=this.masks?Int32Array.from(this.masks.textures,sample):null;
    this.mainId=this.mainMap?sample(this.mainMap):-1;this.reflectionId=-1;this.surfaceIds=null;
  }
  prepareSurface(source,camera) {
    const v=source.values,row=source.row,r=this.host.result,core=this.resourceCore;
    if(source.nativeKind===EFFECT.WATER_TINT.ID)this.captureSurface(this.graphics.renderer,camera);
    v.set(camera.projectionMatrix.elements,row.PROJECTION_MATRIX);v.set(camera.matrixWorldInverse.elements,row.VIEW_MATRIX);
    const c=camera.matrixWorld.elements;v[row.CAMERA_POSITION]=c[12];v[row.CAMERA_POSITION+1]=c[13];v[row.CAMERA_POSITION+2]=c[14];
    v[row.TIME]=this.time;v[row.CAMERA_RANGE]=camera.near;v[row.CAMERA_RANGE+1]=camera.far;
    if(source.nativeKind===EFFECT.WATER_TINT.ID){
      v[row.QUALITY]=this.quality.scale>0?1:0;v[row.UNDERWATER]=camera.position.y<this.level?1:0;
      const tint=this.surfaceUniforms.tint.value;v[row.TINT]=tint.x;v[row.TINT+1]=tint.y;v[row.TINT+2]=tint.z;
    }else{
      for(let i=0;i<3;i++)v[row.TO_SUN+i]=r[RESULT.SUN+i];
      if(source.nativeKind===EFFECT.WATER_TEXTURED.ID){
        v[row.MAIN_MAP]=this.mainId;v[row.MASK_A]=this.maskIds?.[r[RESULT.MASK_A]]??this.maskFallback;
        v[row.MASK_B]=this.maskIds?.[r[RESULT.MASK_A+1]]??this.maskFallback;v[row.MASK_MIX]=r[RESULT.MASK_A+2];
      }else{
        if(this.reflectionId<0){core.resources.targetId(this.target);this.reflectionId=core.texture(this.target.texture);}
        v[row.REFLECTION]=this.reflectionId;v[row.QUALITY]=this.quality.scale>0?1:0;v[row.SCREEN_SPACE]=this.quality.scale&&!this.quality.planarNear?1:0;
        lighting.sunColor.value.toArray(v,row.SUN_COLOR);v[row.DISTORT]=this.distort;v[row.VIEW_YAW]=r[RESULT.YAW];v.set(this.reflectionMatrix.elements,row.REFLECTION_MATRIX);
        v[row.NORMAL_A]=this.normalIds?.[r[RESULT.NORMAL_A]]??this.normalFallback;v[row.NORMAL_B]=this.normalIds?.[r[RESULT.NORMAL_A+1]]??this.normalFallback;v[row.NORMAL_MIX]=r[RESULT.NORMAL_A+2];
        v[row.BUMP_A]=this.bumpIds?.[r[RESULT.BUMP_A]]??this.normalFallback;v[row.BUMP_B]=this.bumpIds?.[r[RESULT.BUMP_A+1]]??this.normalFallback;v[row.BUMP_MIX]=r[RESULT.BUMP_A+2];
      }
    }
    if(row.WADERS!==undefined)for(let i=0;i<32;i++)v[row.WADERS+i]=r[RESULT.WADERS+i];
    if(row.SCENE_COLOR!==undefined){v[row.SCENE_COLOR]=this.surfaceIds?.[0]??-1;v[row.SCENE_DEPTH]=this.surfaceIds?.[1]??-1;
      const size=this.surfaceUniforms.resolution.value;v[row.RESOLUTION]=size.x;v[row.RESOLUTION+1]=size.y;}
    if(source.far){const span=this.farSpan(camera);v[row.SURFACE_RANGE]=span[0];v[row.SURFACE_RANGE+1]=span[1];lighting.fogColor.value.toArray(v,row.FOG_COLOR);lighting.fogRange.value.toArray(v,row.FOG_RANGE);}
  }

  // Where the far water draws, in view depth: from the near scene's end out
  // to the look's FarSceneRange (scaled with the view distance, as the far
  // scene's own reach is) or else the far scene's reach. None when the far
  // scene doesn't reach past the near scene's fade start, as the sky's draw
  // decides for the low-res terrain and the water together (Yavin 4:
  // Temple, both Rhen Vars, Jabba's Palace).
  farSpan(farCamera = this.graphics.farCamera) {
    const end = farCamera?.far || 0;
    const fadeStart=lighting.fade.value.x,fadeEnd=lighting.fade.value.y;
    if (!(end > fadeStart)){this.farRange[0]=fadeEnd;this.farRange[1]=0;return this.farRange;}
    const reach = this.graphics.farReach?.[1];
    const scale = reach > 0 ? end / reach : 1;
    this.farRange[0]=fadeEnd;this.farRange[1]=this.farReach>0?this.farReach*scale:end;return this.farRange;
  }

  // Whether any water cell lies in the eye's view between the far span's ends.
  farInView(camera) {
    const [start, end] = this.farSpan();
    if (!(end > start)) return false;
    const view = this.farView;
    view.fov = camera.fov;
    view.aspect = camera.aspect;
    view.near = Math.max(camera.near, 0.1);
    view.far = end;
    view.updateProjectionMatrix();
    this.farFrustum.setFromProjectionMatrix(sceneTransform(this.matrix.multiplyMatrices(view.projectionMatrix, camera.matrixWorldInverse)));
    if (!this.farFrustum.intersectsBox(this.bounds)) return false;
    return this.cellBounds.some((box) => this.farFrustum.intersectsBox(box));
  }

  // The ocean drawn in place of the flat surface: always where the look
  // asks for one, and on an Enhanced sea while ocean effects are enhanced.
  get ocean() {
    const surface = this.oceanSurface;
    if (!surface) return null;
    return this.oceanAlways || this.graphics.settings.get("oceanEffects") === "enhanced" ? surface : null;
  }

  applyQuality() {
    const name = this.graphics.settings.get("waterQuality");
    this.quality = WATER_QUALITIES[name] || WATER_QUALITIES.high;
    if (this.oceanSurface) {
      const active = Boolean(this.ocean);
      sceneProperty(this.oceanSurface.near,"visible",sceneProperty(this.oceanSurface.far,"visible",active));
      if (active) return; // textured ocean does not need planar reflection targets
    }
    const enhanced = this.quality.scale > 0;
    if (this.lowPass) sceneProperty(this.lowPass,"visible",!enhanced);
    sceneProperty(this.tintPass,"visible",enhanced || !this.lowPass);
    sceneProperty(this.mirrorPass,"visible",enhanced || !this.lowPass);
    if (!enhanced) sceneProperty(this.underwaterFog,"visible",false);
    // The far scene's passes follow (their quality uniforms are these).
    if (this.farLow) sceneProperty(this.farLow,"visible",this.lowPass.visible);
    sceneProperty(this.farTint,"visible",this.tintPass.visible);
    sceneProperty(this.farMirror,"visible",this.mirrorPass.visible);
    if (enhanced && !this.surfaceBuffer) {
      this.surfaceBuffer = new THREE.WebGLRenderTarget(1, 1, {
        depthTexture: new THREE.DepthTexture(1, 1, THREE.UnsignedIntType),
      });
      this.surfaceUniforms.sceneColor.value = this.surfaceBuffer.texture;
      this.surfaceUniforms.sceneDepth.value = this.surfaceBuffer.depthTexture;
    }
    if (!enhanced && this.surfaceBuffer) {
      this.surfaceBuffer.dispose();
      this.surfaceBuffer = null;this.surfaceIds=null;
      this.surfaceUniforms.sceneColor.value = null;
      this.surfaceUniforms.sceneDepth.value = null;
    }
    this.distort = this.bumps ? (enhanced ? DISTORT * 0.18 : DISTORT) : 0;
    const planar = enhanced && Boolean(this.quality.planarNear);
    if (planar && !this.reflectionFar) this.reflectionFar = new THREE.WebGLRenderTarget(1, 1);
    if (!planar && this.reflectionFar) {
      this.reflectionFar.dispose();
      this.reflectionFar = null;
    }
    this.resizeReflection();
  }

  captureSurface(renderer, camera) {
    if (!this.surfaceBuffer) return;
    const source = renderer.getRenderTarget();
    if (source !== this.graphics.sceneTarget) return;
    const target = this.surfaceBuffer;
    target.setSize(source.width, source.height);
    // Colour and depth are resolved together before the first water reader.
    // The Rust bank preserves the source's multisample framebuffer binding.
    renderer.resources.copy(source,target,COLOR_BUFFER_BIT | DEPTH_BUFFER_BIT);
    if(this.resourceCore&&!this.surfaceIds)this.surfaceIds=Int32Array.of(this.resourceCore.texture(target.texture),this.resourceCore.texture(target.depthTexture));
    const u = this.surfaceUniforms;
    u.resolution.value.set(source.width, source.height);
    u.cameraRange.value.set(camera.near, camera.far);
  }

  resizeReflection() {
    const { renderer } = this.graphics;
    // The 3D frame's resolution (smaller than the canvas under an upscaler).
    const size = this.graphics.renderSize?.clone() ?? renderer.getDrawingBufferSize(new THREE.Vector2());
    const q = this.quality;
    const scale = Math.min(q.scale, q.maxSize / Math.max(size.x, size.y));
    const width = q.scale ? Math.max(1, Math.round(size.x * scale)) : REFLECTION_SIZE[0];
    const height = q.scale ? Math.max(1, Math.round(size.y * scale)) : REFLECTION_SIZE[1];
    this.graphics.renderer.resources.resizeEffectTarget(this.target,width,height);
    this.reflectionFar?.setSize(width, height);
  }

  ringMesh(map) {
    const row=EFFECT.WATER_RINGS,mesh=new THREE.Object3D();sceneProperty(mesh,"renderOrder",-18);sceneProperty(mesh,"frustumCulled",false);
    const source={mesh,nativeKind:row.ID,texture:map,positions:this.host.positions,arrays:{position:this.host.positions,uv:this.host.uvs,alpha:this.host.alpha},
      values:new Float64Array(row.LENGTH),visibility:'anchor',vertex:0,index:0,indexCapacity:RIPPLE_MAX*RIPPLE_SEGMENTS*6};
    source.prepare=camera=>{source.values.set(camera.projectionMatrix.elements,row.PROJECTION_MATRIX);source.values.set(camera.matrixWorldInverse.elements,row.MODEL_VIEW_MATRIX);};
    this.host.bindArray(source,'positions','positions');
    this.host.bindArray(source.arrays,'position','positions');this.host.bindArray(source.arrays,'uv','uvs');this.host.bindArray(source.arrays,'alpha','alpha');
    mesh.userData.effectMesh=source;return mesh;
  }

  // The gameplay step reports each soldier in the water every tick; note the
  // wet ones for the surface's contact foam and ripples (High/Ultra).
  soldierStep(dt, contact, feet, head, gun, velocity, splash) {
    const damage = super.soldierStep(dt, contact, feet, head, gun, velocity, splash);
    if (this.level > feet.y && this.over(feet.x, feet.z)) this.host.wader(contact, feet, velocity, this.level - feet.y);
    return damage;
  }

  // Water.ripple's birth is a scalar command; the phase owns its lifetime,
  // oldest-ring replacement, wake shape and all ring vertex construction.
  ripple(position, life, radius, velocity = null) {
    if (this.over(position.x, position.z)) this.host.ripple(position, life, radius, velocity);
  }

  prepareFrame(dt, camera) {
    this.host.prepare(dt, camera);
    const f = this.host.frame, sun = lighting.sunDirection.value;
    f[FRAME.LEVEL] = this.level;
    f[FRAME.SUN] = sun.x; f[FRAME.SUN + 1] = sun.y; f[FRAME.SUN + 2] = sun.z;
    f[FRAME.OCEAN] = this.ocean ? 1 : 0;
    f[FRAME.STORM] = this.ocean?.enhanced && this.ocean.waves.storm?.enabled ? 1 : 0;
  }

  // Only resource publication remains from Water.update and updateRings.
  // Texture indices, mixing, sun/yaw, contacts and vertices are native outputs.
  publishFrame(camera) {
    this.host.refresh();
    const r = this.host.result;
    this.time = r[RESULT.TIME]; this.wave = r[RESULT.WAVE];
    sceneComponent(this.group.position,"y","=",sceneComponent(this.farGroup.position,"y","=",this.wave));
    this.surfaceUniforms.time.value=this.time;
    const quads=r[RESULT.QUADS],rings=this.rings.userData.effectMesh;rings.vertex=quads*4;rings.index=quads*6;
    if (this.ocean) this.ocean.publishFrame(r, camera);
  }

  // The native frame owns reflection eligibility, cadence and pass ordering.
  // Isolated resource fixtures publish through the same frame contract.
  reflect(camera){this.graphics.rust.frame.mirror(this,camera);}
  drawUnderwater(renderer,camera){this.graphics.rust.frame.underwater(this,camera);}
}
