import {sceneComponent,sceneLayer,sceneProperty,sceneTransform} from "./rust-render/scene-changes.js";
import {EnvironmentRecipe as RECIPE} from "./environment/layout.js";
import { environmentRecipe } from './atmosphere.js';
import { EFFECT, EFFECT_TARGET } from './rust-render/effect-layout.js';
import { ownedEffectMaterial } from './rust-render/effect-material.js';
import { COLOR_BUFFER_BIT, DEPTH_BUFFER_BIT } from "./render-resources.js";
import { WaterState } from "./runtime/water-state.js";
import * as THREE from "three";
import { lighting } from "./materials.js";
import { WATER_QUALITIES } from "./settings.js";
import { OceanSurface } from "./ocean-surface.js";
import { SurfaceHost } from "./environment/surfaces.js";
import { Surface as LIMITS, SurfaceConfig as CONFIG, SurfaceFrame as FRAME, SurfaceResult as RESULT, WaterView as VIEW, WaterViewResult as POLICY } from "./environment/layout.js";

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
// Water.isSea is a cold content query, shared with the native surface recipe.
export function isSea(data) { return environmentRecipe(RECIPE.WATER,data).sea; }

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
    this.host.config[CONFIG.BOBBING] = this.bobbing ? 1 : 0;
    // The original draws an ocean where the look asks (OceanEnable). A sea
    // the look leaves flat (Kashyyyk's: water out to every edge of its
    // grid) takes the Enhanced ocean too, while Enhanced ocean effects are
    // on; Classic keeps the flat layer the 2004 game drew there.
    const recipe = environmentRecipe(RECIPE.WATER,data,surfaceOwner);
    this.host.refresh();
    const oceanLook = recipe.oceanAlways, ocean = recipe.ocean;
    this.height = data.height;
    this.cell = data.cell;
    this.offset = data.offset;
    this.grid = data.grid;
    this.time = 0;
    this.wave = 0;
    this.frames = (key) => {
      const set = recipe.frames[key];
      if (!set) return null;
      const textures = set.names.map(name=>texture(name)).filter(Boolean);
      return textures.length ? { textures, fps: set.fps } : null;
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
    const c0=recipe.tint;
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
    this.policySize=new THREE.Vector2();this.policyBuffer=null;
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
    this.bounds=new THREE.Box3();
    if(!recipe.empty){this.bounds.min.fromArray(recipe.bounds[0]);this.bounds.max.fromArray(recipe.bounds[1]);}
    this.cellBounds=recipe.cells.map(box=>new THREE.Box3(new THREE.Vector3().fromArray(box),new THREE.Vector3().fromArray(box,3)));

    // Rings.
    this.rings = this.ringMesh(texture("FXrippl1"));
    graphics.scene.add(this.rings);

    // The tint under the surface.
    const u = recipe.underwater;
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
      v[row.QUALITY]=this.quality.scale>0?1:0;v[row.UNDERWATER]=this.surfacePolicy(camera)[POLICY.UNDERWATER];
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
  // One native per-view query owns range, projection/cell visibility and
  // target dimensions. Rebind its fixed blocks only after memory growth.
  surfacePolicy(camera=this.graphics.camera,farCamera=this.graphics.farCamera) {
    const x=this.host.owner.x,buffer=x.memory.buffer;
    if(this.policyBuffer!==buffer){
      this.policyBuffer=buffer;
      this.policyInput=new Float64Array(buffer,x.water_view_input(),VIEW.STRIDE);
      this.policyResult=new Float64Array(buffer,x.water_view_output(),POLICY.STRIDE);
    }
    const v=this.policyInput,q=this.quality;
    v[VIEW.FAR]=farCamera?.far||0;lighting.fade.value.toArray(v,VIEW.FADE);
    v[VIEW.AUTHORED_FAR]=this.graphics.farReach?.[1]||0;v[VIEW.WATER_FAR]=this.farReach;
    v[VIEW.NEAR]=camera?.near??.1;v[VIEW.FOV]=camera?.fov??50;v[VIEW.ASPECT]=camera?.aspect??1;
    if(camera)v.set(camera.matrixWorldInverse.elements,VIEW.INVERSE);
    v[VIEW.SCALE]=q?.scale??0;v[VIEW.MAX_SIZE]=q?.maxSize??0;
    const size=this.graphics.renderSize||this.graphics.renderer.getDrawingBufferSize(this.policySize);
    size.toArray(v,VIEW.SIZE);v[VIEW.LEVEL]=this.level;v[VIEW.EYE_Y]=camera?.position.y??0;
    v[VIEW.HAS_BUMPS]=this.bumps?1:0;v[VIEW.PLANAR]=q?.planarNear?1:0;
    x.water_view();return this.policyResult;
  }
  farSpan(farCamera=this.graphics.farCamera) {
    const r=this.surfacePolicy(this.graphics.camera,farCamera);
    this.farRange[0]=r[POLICY.RANGE];this.farRange[1]=r[POLICY.RANGE+1];return this.farRange;
  }
  farInView(camera) {return this.surfacePolicy(camera)[POLICY.VISIBLE]!==0;}

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
    const policy=this.surfacePolicy();
    const enhanced = policy[POLICY.ENHANCED]!==0;
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
    this.distort = policy[POLICY.DISTORT];
    const planar = policy[POLICY.PLANAR]!==0;
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
    const r=this.surfacePolicy(),width=r[POLICY.DIMENSIONS],height=r[POLICY.DIMENSIONS+1];
    this.graphics.renderer.resources.resizeEffectTarget(this.target,width,height);
    this.reflectionFar?.setSize(width,height);
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
