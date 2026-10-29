import * as THREE from 'three';
import {foliageAtmosphere} from './foliage-material.js';

const GRASS_TURNS = [0,-.05,.1,-.15,.2,0,.05,-.1,.15,-.2];
const LEAF_TURNS = [0,-1,2,-3,4,0,1,-2,3,-4];
const ATLAS = [[0,.5,.5,1],[.5,.5,1,1],[0,0,.5,.5],[.5,0,1,.5]];
const CORNERS = [[-.5,-.5],[.5,-.5],[-.5,.5],[.5,.5]];

// One static set of authored cards per class, instanced at visible patches.
// Only patch selection/tint is uploaded when visibility changes. Orientation,
// wind and distance fade use uniforms, so looking around doesn't rebuild and
// upload every leaf vertex on the input thread.
export class FoliageGpuBatch {
  constructor(scene, map, cards, def, grass, capacity) {
    this.grass = grass; this.def = def; this.cardCount = cards.length;
    this.count = 0; this.selected = []; this.nextSelected = [];
    this.delta = new THREE.Vector3();
    const geometry = this.geometry = new THREE.InstancedBufferGeometry();
    const vertices = cards.length * 4;
    const positions = new Float32Array(vertices * 3), colors = new Float32Array(vertices * 4);
    const uvs = new Float32Array(vertices * 2), corners = new Float32Array(vertices * 2);
    const shapes = new Float32Array(vertices * 4), rates = new Float32Array(vertices);
    const indices = new Uint32Array(cards.length * 6);
    const scale = def.HeightScale ?? 1, atlas = grass ? (def.NumParts ?? 4) !== 1 : (def.NumParts ?? 4) === 4;
    cards.forEach((card, i) => {
      const [u0,v0,u1,v1] = atlas ? ATLAS[card.part] : [0,0,1,1];
      const mirror = !atlas && (card.part & 1) === 1;
      const left = mirror ? u1 : u0, right = mirror ? u0 : u1;
      for (let j = 0; j < 4; j++) {
        const k = i * 4 + j;
        positions.set(grass ? [card.x,card.y,card.z] : [card.p.x,card.p.y*scale,card.p.z],k*3);
        colors.set([card.grey,card.grey,card.grey,def.Alpha ?? .5],k*4);
        const bottom = grass && card.y < 0 ? v1 + (v0 - v1) * Math.min(1, -card.y / card.width) : v1;
        uvs.set([j%2 ? right : left,j<2 ? bottom : v0],k*2);
        corners.set(CORNERS[j],k*2);
        shapes.set(grass ? [card.width,card.height,GRASS_TURNS[i%10],card.phase]
          : [card.size,card.size*scale,LEAF_TURNS[i%10],card.phase],k*4);
        rates[k] = grass ? card.rate : def.WiggleSpeed ?? 1.2;
      }
      indices.set([i*4,i*4+2,i*4+1,i*4+1,i*4+2,i*4+3],i*6);
    });
    for (const [name,array,size] of [['position',positions,3],['color',colors,4],['uv',uvs,2],
      ['foliageCorner',corners,2],['foliageShape',shapes,4],['foliageRate',rates,1]]) {
      geometry.setAttribute(name,new THREE.BufferAttribute(array,size));
    }
    geometry.setIndex(new THREE.BufferAttribute(indices,1));
    this.patch = new THREE.InstancedBufferAttribute(new Float32Array(Math.max(1,capacity)*3),3).setUsage(THREE.DynamicDrawUsage);
    this.tint = new THREE.InstancedBufferAttribute(new Float32Array(Math.max(1,capacity)*3),3).setUsage(THREE.DynamicDrawUsage);
    geometry.setAttribute('foliagePatch',this.patch); geometry.setAttribute('foliageTint',this.tint);
    geometry.instanceCount = 0;
    this.axes = ['foliageAxisX', 'foliageAxisY', 'foliageAxisZ'].map(name => {
      const attribute = new THREE.InstancedBufferAttribute(new Float32Array(Math.max(1, capacity) * 3), 3).setUsage(THREE.DynamicDrawUsage);
      geometry.setAttribute(name, attribute);
      return attribute;
    });
    this.distance = new THREE.InstancedBufferAttribute(new Float32Array(Math.max(1, capacity)), 1).setUsage(THREE.DynamicDrawUsage);
    geometry.setAttribute('foliageDistance', this.distance);
    this.uniforms = {
      foliageEye:{value:new THREE.Vector3()}, foliageRight:{value:new THREE.Vector3()},
      foliageUp:{value:new THREE.Vector3()}, foliageBack:{value:new THREE.Vector3()},
      foliageTime:{value:0}, foliageEnd:{value:100}, foliageWiggle:{value:def.WiggleAmount ?? .11},
    };
    const material = this.material = foliageAtmosphere(new THREE.MeshBasicMaterial({map,vertexColors:true,alphaTest:.25,side:THREE.DoubleSide}));
    const atmosphere = material.onBeforeCompile;
    material.onBeforeCompile = shader => {
      atmosphere(shader);
      Object.assign(shader.uniforms,this.uniforms);
      shader.vertexShader = `
        attribute vec2 foliageCorner;
        attribute vec4 foliageShape;
        attribute float foliageRate;
        attribute vec3 foliagePatch;
        attribute vec3 foliageTint;
        attribute vec3 foliageAxisX, foliageAxisY, foliageAxisZ;
        attribute float foliageDistance;
        vec3 foliageWorld(vec3 p) {
          return foliagePatch + foliageAxisX * p.x + foliageAxisY * p.y + foliageAxisZ * p.z;
        }
        uniform vec3 foliageEye, foliageRight, foliageUp, foliageBack;
        uniform float foliageTime, foliageEnd, foliageWiggle;
      ` + shader.vertexShader;
      shader.vertexShader = shader.vertexShader.replace('#include <begin_vertex>', grass ? `
        float depth = -dot(foliagePatch - foliageEye, foliageBack);
        float maxDist = foliageDistance > 0.0 ? foliageDistance : foliageEnd;
        float fade = clamp((maxDist - depth) / 20.0, 0.0, 1.0);
        float sink = 1.0 - (1.0 - fade) * (1.0 - fade);
        vec3 width = (foliageRight * (1.1 * (1.0 - abs(foliageShape.z))) + foliageBack * (1.1 * foliageShape.z)) * foliageShape.x;
        vec3 lean = normalize(vec3(0.0,1.0,0.0) - foliageBack * (.7 * foliageBack.y));
        vec3 transformed = vec3(position.x,position.y*sink,position.z);
        transformed += width * foliageCorner.x;
        vec3 rise = lean * foliageShape.y * sink + width * (.08 * sin(foliageShape.w + foliageRate * foliageTime));
        if (foliageCorner.y > 0.0) transformed += rise;
        else if (position.y < 0.0) transformed += rise * min(1.0, -position.y / foliageShape.x);
        transformed = foliageWorld(transformed);
        vColor.rgb *= foliageTint;
      ` : `
        vec3 center = foliageWorld(position + foliageBack * .1);
        float depth = -dot(center - foliageEye, foliageBack);
        float maxDist = foliageDistance > 0.0 ? foliageDistance : foliageEnd;
        float fade = clamp((maxDist - depth) / (.4 * maxDist), 0.0, 1.0);
        vec3 width = normalize(foliageRight + foliageBack * (.3 * foliageShape.z)) * foliageShape.x;
        vec3 transformed = position + foliageBack * .1 + foliageUp * (foliageShape.y * foliageCorner.y) + width * foliageCorner.x;
        if (foliageCorner.y > 0.0) transformed += width * (foliageWiggle * sin(foliageShape.w + foliageRate * foliageTime));
        transformed = foliageWorld(transformed);
        vColor.rgb *= foliageTint;
        vColor.a *= fade;
      `);
      if (!grass) shader.vertexShader = shader.vertexShader.replace('#include <project_vertex>', `
        #include <project_vertex>
        // All four vertices share the card center/depth. Clip an out-of-range
        // leaf before rasterization, matching the CPU path's per-card cull.
        if (depth > maxDist) gl_Position = vec4(2.0,2.0,2.0,1.0);
      `);
    };
    material.customProgramCacheKey = () => `foliage-gpu-v1-${grass?'grass':'leaves'}`;
    const mesh = this.mesh = new THREE.Mesh(geometry,material);
    mesh.frustumCulled = false; mesh.castShadow = mesh.receiveShadow = false;
    scene.add(mesh);
  }

  update(instances, time, camera, right, up, back, frustum, nearEnd) {
    const u = this.uniforms;
    u.foliageTime.value=time; u.foliageEnd.value=nearEnd;
    u.foliageEye.value.copy(camera.position); u.foliageRight.value.copy(right);
    u.foliageUp.value.copy(up); u.foliageBack.value.copy(back);
    const selected=this.nextSelected; selected.length=0;
    for(let i=0;i<instances.length;i++) {
      const inst=instances[i];
      const maxDist=inst.maxDistance || nearEnd;
      if(inst.sphere && !frustum.intersectsSphere(inst.sphere))continue;
      if(Math.hypot(inst.position.x-camera.position.x,inst.position.z-camera.position.z)>inst.cullDistance)continue;
      const depth=-this.delta.subVectors(inst.position,camera.position).dot(back);
      if(this.grass ? depth>maxDist || depth< -5 : depth>maxDist+20 || depth< -25)continue;
      selected.push(i);
    }
    const changed=selected.length!==this.selected.length || selected.some((id,i)=>id!==this.selected[i]);
    if(changed) {
      for(let i=0;i<selected.length;i++) {
        const inst=instances[selected[i]];
        inst.position.toArray(this.patch.array,i*3); this.tint.array.set(inst.tint,i*3);
        const matrix = inst.matrix.elements;
        for (let axis = 0; axis < 3; axis++) this.axes[axis].array.set(matrix.slice(axis * 4, axis * 4 + 3), i * 3);
        this.distance.array[i] = inst.maxDistance || 0;
      }
      for(const attr of [this.patch,this.tint,...this.axes,this.distance]) {
        attr.clearUpdateRanges();
        if(selected.length) {attr.addUpdateRange(0,selected.length*attr.itemSize);attr.needsUpdate=true;}
      }
      this.nextSelected=this.selected; this.selected=selected;
    }
    this.geometry.instanceCount=selected.length;
    this.count=selected.length*this.cardCount;
    this.mesh.visible=this.count>0;
  }
}
