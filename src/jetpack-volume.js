import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';
const ZERO = new THREE.Vector3();

// A small, advected 3D density field. Linear hardware filtering replaces dozens
// of procedural noise operations at each integration step. Shared by both jets.
function noiseTexture() {
  const size = 32, data = new Uint8Array(size ** 3);
  let seed = 0x6d2b79f5;
  for (let i = 0; i < data.length; i++) {
    seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5;
    data[i] = seed >>> 24;
  }
  const texture = new THREE.Data3DTexture(data, size, size, size);
  texture.format = THREE.RedFormat;
  texture.minFilter = texture.magFilter = THREE.LinearFilter;
  texture.wrapS = texture.wrapT = texture.wrapR = THREE.RepeatWrapping;
  texture.unpackAlignment = 1; texture.needsUpdate = true;
  return texture;
}

const VERTEX = /* glsl */ `
  attribute vec3 nozzle, direction, bend;
  attribute float nozzleRadius, phase;
  uniform float time, power;
  varying vec3 boxPoint, rayOrigin, axisRight, axisUp, axisForward, origin;
  varying vec2 extent, curvature;
  varying float radius, lengthJet, seed;
  void main() {
    vec3 forward = normalize(direction);
    vec3 right = cross(forward, vec3(0.0,1.0,0.0));
    if (dot(right,right) < 0.01) right = cross(forward,vec3(0.0,0.0,1.0));
    right = normalize(right);
    vec3 up = cross(forward,right);
    curvature = vec2(dot(bend,right),dot(bend,up));
    radius = nozzleRadius;
    lengthJet = (0.61 + 0.025 * sin(time * 19.0 + phase)) * power;
    extent = vec2(radius * 3.8) + abs(curvature);
    // Proxy extends just inside the nozzle lip. Density itself starts at z=0.
    boxPoint = vec3(position.xy * 2.0, position.z + 0.5);
    vec3 eye = cameraPosition - nozzle;
    rayOrigin = vec3(dot(eye,right)/extent.x,dot(eye,up)/extent.y,dot(eye,forward)/lengthJet);
    vec3 world = nozzle + right * boxPoint.x * extent.x + up * boxPoint.y * extent.y + forward * boxPoint.z * lengthJet;
    axisRight=right;axisUp=up;axisForward=forward;origin=nozzle;seed=phase;
    gl_Position = projectionMatrix * viewMatrix * vec4(world,1.0);
  }
`;
const FRAGMENT = /* glsl */ `
  uniform highp sampler3D noiseField;
  uniform sampler2D sceneDepth;
  uniform vec2 depthResolution, cameraRange;
  uniform float time, hasDepth;
  varying vec3 boxPoint, rayOrigin, axisRight, axisUp, axisForward, origin;
  varying vec2 extent, curvature;
  varying float radius, lengthJet, seed;
  vec3 worldAt(vec3 p) { return origin+axisRight*p.x*extent.x+axisUp*p.y*extent.y+axisForward*p.z*lengthJet; }
  float noise3(vec3 p) { return texture(noiseField,p/32.0).r; }
  void main() {
    bool within = all(greaterThan(rayOrigin,vec3(-1.0,-1.0,0.0))) && all(lessThan(rayOrigin,vec3(1.0)));
    if ((!within && !gl_FrontFacing) || (within && gl_FrontFacing)) discard;
    vec3 ray = normalize(boxPoint-rayOrigin);
    vec3 inv = 1.0 / (ray + vec3(1e-7));
    vec3 a = (vec3(-1.0,-1.0,0.0)-rayOrigin)*inv;
    vec3 b = (vec3(1.0)-rayOrigin)*inv;
    vec3 lo = min(a,b), hi=max(a,b);
    float start = max(0.0,max(lo.x,max(lo.y,lo.z)));
    float finish = min(hi.x,min(hi.y,hi.z));
    if (finish <= start) discard;
    float surface = 1e6;
    if (hasDepth > 0.5) {
      float z=texture2D(sceneDepth,gl_FragCoord.xy/depthResolution).x;
      surface=cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));
    }
    // Fixed midpoint samples: no screen-space random jitter or temporal shimmer.
    float stepSize=(finish-start)/28.0;
    float meters=length(vec3(ray.xy*extent,ray.z*lengthJet))*stepSize;
    vec3 emission=vec3(0.0); float transmittance=1.0;
    for(int i=0;i<28;i++) {
      vec3 p=rayOrigin+ray*(start+(float(i)+0.5)*stepSize);
      float depth=-(viewMatrix*vec4(worldAt(p),1.0)).z;
      if (depth>surface) break;
      float z=clamp(p.z,0.0,1.0), spread=smoothstep(0.025,0.30,z);
      vec2 center=curvature*z*z;
      // Advected eddies become larger and less coherent downstream.
      vec3 field=vec3(p.xy*extent*42.0,z*9.0-time*28.0+seed);
      float coarse=noise3(field);
      float fine=noise3(field*2.31+vec3(7.0,13.0,-time*3.0));
      center+=vec2(sin(z*13.0-time*5.0+seed),cos(z*17.0-time*4.0+seed))*radius*0.20*spread*z;
      float r=radius*(0.90+spread*1.1)*(1.0-0.66*smoothstep(0.48,1.0,z));
      float radial=length(p.xy*extent-center)/max(r,0.001);
      float breakup=(coarse-0.46)*1.55+(fine-0.5)*0.50;
      float body=1.0-smoothstep(0.45,1.12,radial+breakup*spread);
      float tail=1.0-smoothstep(0.53+coarse*0.18,0.89+fine*0.10,z);
      float lip=smoothstep(0.0,0.012,z);
      float core=exp(-radial*radial*3.2)*(1.0-smoothstep(0.26,0.76,z));
      core*=0.84+0.16*cos(z*51.0-time*12.0+seed);
      // Dense hot root; separate, cooler billowing combustion envelope.
      float density=(body*(4.0+coarse*8.0)*tail+core*42.0)*lip;
      density*=smoothstep(0.0,0.014,surface-depth);
      float alpha=1.0-exp(-density*meters);
      vec3 color=mix(vec3(1.5,0.32+fine*0.24,0.018),vec3(2.5,2.15,1.6),core);
      color+=vec3(0.12,0.08,0.02)*body*coarse;
      // Radiance from the dense nozzle core shines through the translucent
      // combustion layer instead of being tinted into an opaque orange torch.
      emission+=transmittance*(alpha*color+vec3(1.0,0.88,0.63)*core*lip*meters*14.0);
      transmittance*=1.0-alpha;
      if(transmittance<0.02)break;
    }
    float opacity=1.0-transmittance;
    if(opacity<0.003)discard;
    gl_FragColor=vec4(emission,opacity);
  }
`;

export class JetpackVolume {
  constructor({scene,count}) {
    const box=new THREE.BoxGeometry(1,1,1),geometry=new THREE.InstancedBufferGeometry();
    geometry.index=box.index;geometry.attributes.position=box.attributes.position;
    for(const [name,size] of Object.entries({nozzle:3,direction:3,bend:3,nozzleRadius:1,phase:1}))
      geometry.setAttribute(name,new THREE.InstancedBufferAttribute(new Float32Array(count*size),size).setUsage(THREE.DynamicDrawUsage));
    for(let i=0;i<count;i++)geometry.attributes.phase.array[i]=i*17.31;
    this.noise=noiseTexture();
    const material=new THREE.ShaderMaterial({uniforms:{time:{value:0},power:{value:1},noiseField:{value:this.noise},
      sceneDepth:particleDepth.map,depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange,hasDepth:{value:0}},
      vertexShader:VERTEX,fragmentShader:FRAGMENT,side:THREE.DoubleSide,forceSinglePass:true,transparent:true,premultipliedAlpha:true,depthWrite:false,toneMapped:false});
    this.mesh=new THREE.Mesh(geometry,material);this.mesh.name='jetpack-volumetric-combustion';
    this.mesh.frustumCulled=false;this.mesh.renderOrder=10;this.mesh.visible=false;geometry.instanceCount=0;
    this.mesh.onBeforeRender=(renderer,scene,camera)=>{
      material.uniforms.hasDepth.value=particleDepth.capture(renderer,scene,camera,true)?1:0;
      material.uniformsNeedUpdate=true;
    };
    scene.add(this.mesh);
  }
  update(time,power,nozzles) {
    const mesh=this.mesh,a=mesh.geometry.attributes;
    mesh.visible=power>0.015;mesh.geometry.instanceCount=mesh.visible?nozzles.length:0;
    if(!mesh.visible)return;
    mesh.material.uniforms.time.value=time;mesh.material.uniforms.power.value=Math.max(power,0.025);
    for(let i=0;i<nozzles.length;i++) {
      const n=nozzles[i];n.position.toArray(a.nozzle.array,i*3);n.direction.toArray(a.direction.array,i*3);
      (n.bendWorld||ZERO).toArray(a.bend.array,i*3);a.nozzleRadius.array[i]=n.radius||0.034;
    }
    for(const [name,attribute] of Object.entries(a))if(attribute.isInstancedBufferAttribute&&name!=='phase')attribute.needsUpdate=true;
  }
  clear(){this.mesh.visible=false;this.mesh.geometry.instanceCount=0;}
  dispose(){this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();this.noise.dispose();}
}
