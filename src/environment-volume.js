import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';

export const WAKE_COUNT = 8;
// Fixed storage shared by water and fog. Sampling actor motion never allocates
// in the render loop, and stationary actors do not continuously excavate fog.
export class EnvironmentWakes {
  constructor() {
    this.points = Array.from({length: WAKE_COUNT}, () => new THREE.Vector4(0, 0, 0, -100));
    this.motion = Array.from({length: WAKE_COUNT}, () => new THREE.Vector4());
    this.states = new WeakMap(); this.cursor = 0; this.time = 0;
  }
  add(position, velocity, strength = 1) {
    const i = this.cursor++ % WAKE_COUNT;
    this.points[i].set(position.x, position.y, position.z, this.time);
    this.motion[i].set(velocity.x, velocity.y, velocity.z, strength);
  }
  update(dt, actors, relevant) {
    this.time += dt;
    for (const actor of actors) {
      if (!actor?.position) continue;
      let s = this.states.get(actor);
      if (!s) { s = {position: actor.position.clone(), velocity: new THREE.Vector3(), gap: 0}; this.states.set(actor, s); }
      s.velocity.copy(actor.position).sub(s.position).divideScalar(Math.max(dt, 1e-5));
      const speed = s.velocity.length();
      s.gap += dt;
      if (dt > 0 && s.gap >= .10 && speed > .3 && speed < 60 && relevant(actor.position)) {
        this.add(actor.position, s.velocity, Math.min(1.6, .4 + speed * .1)); s.gap = 0;
      }
      s.position.copy(actor.position);
    }
  }
  clear() { for (const p of this.points) p.w = -100; this.states = new WeakMap(); }
}

// Periodic, correlated fractal density. White noise averaged along a ray
// becomes an almost uniform haze; this field retains visible rolling eddies.
function noiseTexture() {
  const n=32,bytes=new Uint8Array(n**3);let seed=145231;
  const random=()=>{seed^=seed<<13;seed^=seed>>>17;seed^=seed<<5;return(seed>>>0)/4294967296;};
  const layers=[8,16,32].map(size=>({size,data:Float32Array.from({length:size**3},random)}));
  const smooth=t=>t*t*(3-2*t);
  function sample(layer,x,y,z){
    const size=layer.size,scale=size/n;x*=scale;y*=scale;z*=scale;
    const ix=Math.floor(x),iy=Math.floor(y),iz=Math.floor(z),fx=smooth(x-ix),fy=smooth(y-iy),fz=smooth(z-iz);
    let v=0;
    for(let k=0;k<2;k++)for(let j=0;j<2;j++)for(let i=0;i<2;i++)
      v+=layer.data[((iz+k)%size*size+(iy+j)%size)*size+(ix+i)%size]*(i?fx:1-fx)*(j?fy:1-fy)*(k?fz:1-fz);
    return v;
  }
  for(let z=0;z<n;z++)for(let y=0;y<n;y++)for(let x=0;x<n;x++){
    const v=.60*sample(layers[0],x,y,z)+.30*sample(layers[1],x,y,z)+.10*sample(layers[2],x,y,z);
    bytes[(z*n+y)*n+x]=Math.round(255*THREE.MathUtils.clamp(.5+(v-.5)*1.65,0,1));
  }
  const t=new THREE.Data3DTexture(bytes,n,n,n);t.format=THREE.RedFormat;
  t.minFilter=t.magFilter=THREE.LinearFilter;t.wrapS=t.wrapT=t.wrapR=THREE.RepeatWrapping;
  t.unpackAlignment=1;t.needsUpdate=true;return t;
}
const vertexShader = /* glsl */ `
attribute vec3 origin, direction, extent;
attribute float kind;
varying vec3 point, eye, base, rightAxis, upAxis, forwardAxis, size;
varying float type;
void main() {
  vec3 up=normalize(direction),right=cross(up,vec3(0.,0.,1.));
  if(dot(right,right)<.01)right=cross(up,vec3(1.,0.,0.));
  right=normalize(right);vec3 forward=cross(right,up);
  point=position+vec3(0.,.5,0.);size=extent;base=origin;type=kind;
  rightAxis=right;upAxis=up;forwardAxis=forward;
  vec3 relative=cameraPosition-base;
  eye=vec3(dot(relative,right),dot(relative,up),dot(relative,forward))/size;
  vec3 world=base+right*point.x*size.x+up*point.y*size.y+forward*point.z*size.z;
  gl_Position=projectionMatrix*viewMatrix*vec4(world,1.);
}`;
const fragmentShader = /* glsl */ `
uniform highp sampler3D noiseField;
uniform sampler2D sceneDepth, reflection;
uniform mat4 reflectionMatrix;
uniform vec2 volumeResolution,cameraRange;
uniform float time,hasDepth,hasReflection;
uniform vec4 wakes[8], motion[8];
varying vec3 point,eye,base,rightAxis,upAxis,forwardAxis,size;
varying float type;
vec3 worldAt(vec3 p){return base+rightAxis*p.x*size.x+upAxis*p.y*size.y+forwardAxis*p.z*size.z;}
float noise3(vec3 p){return texture(noiseField,p/32.).r;}
void main(){
  vec3 ray=normalize(point-eye),inv=1./(ray+vec3(1e-7));
  vec3 a=(vec3(-.5,0.,-.5)-eye)*inv,b=(vec3(.5,1.,.5)-eye)*inv;
  vec3 lo=min(a,b),hi=max(a,b);
  float start=max(0.,max(lo.x,max(lo.y,lo.z))),finish=min(hi.x,min(hi.y,hi.z));
  float depthRate=-(viewMatrix*vec4(worldAt(eye+ray)-cameraPosition,0.)).z;
  start=max(start,cameraRange.x/max(depthRate,1e-5));
  if(finish<=start)discard;
  float surface=1e6;
  if(hasDepth>.5){float z=texture2D(sceneDepth,gl_FragCoord.xy/volumeResolution).r;
    surface=cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));}
  float samples=type>2.5?24.:(distance(cameraPosition,base)>35.?12.:18.);
  float stepSize=(finish-start)/samples,meters=length(ray*size)*stepSize,trans=1.;vec3 color=vec3(0.);
  for(int i=0;i<24;i++){
    if(float(i)>=samples)break;
    vec3 p=eye+ray*(start+(float(i)+.5)*stepSize),world=worldAt(p);
    float depth=-(viewMatrix*vec4(world,1.)).z;if(depth>surface)break;
    vec3 local=p*size;
    float height=p.y;
    vec2 bend=vec2(sin(local.y*.9-time*.7+base.x),cos(local.y*.7-time*.6+base.z))*local.y*.13;
    float radius=type<.5?size.x*(.10+.25*height):mix(.09,.75,height);
    if(type>1.5)radius=size.x*.37;
    float radial=length(local.xz-bend)/max(radius,.05);
    // Noise is in [0,1], so even maximal erosion cannot put density here.
    // Skip texture fetches and character-wake work outside the actual plume.
    if(radial>=1.81)continue;
    vec3 flow=world*2.4-upAxis*time*(type<.5?1.15:3.6)+vec3(time*.16,0.,time*.11);
    float n=noise3(flow),fine=noise3(flow*2.07+vec3(11.,7.,3.));
    float edge=1.-smoothstep(.35,1.25,radial+(n-.5)*1.1);
    float breakup=smoothstep(.28,.66,n*.72+fine*.28);
    float density=edge*breakup*smoothstep(0.,.055,height)*(1.-smoothstep(.45,1.,height));
    density*=type<.5?.65:type<1.5?1.7:.16;
    if(density<=0.)continue;
    float light=clamp(.65+(noise3(flow+vec3(0.,2.,0.))-n)*1.2,.45,1.);
    // Cool, translucent steam. A restrained warm contribution stays close
    // to the grates instead of tinting the entire cloud tan.
    vec3 tint=vec3(.78,.84,.91)*light;
    if(type<.5)tint+=vec3(.14,.045,0.)*exp(-local.y*1.8);
    if(type<2.5){
      for(int j=0;j<8;j++){
        float age=time-wakes[j].w;
        if(age>=0.&&age<2.){
          vec3 center=wakes[j].xyz+motion[j].xyz*age*.18;
          vec3 delta=world-center;delta.y-=clamp(delta.y,.15,1.65);
          float r=length(delta),radius=.55+age*.42;
          float strength=motion[j].w*exp(-age*1.8);
          density*=1.-clamp(exp(-r*r/(radius*radius))*strength*.94,0.,.98);
          // Displaced steam curls around the wake instead of simply vanishing.
          density*=1.+.35*strength*exp(-pow((r-radius)*3.,2.))*n;
        }
      }
    }
    density*=smoothstep(0.,.12,surface-depth);
    float alpha=1.-exp(-density*meters);color+=trans*alpha*tint;trans*=1.-alpha;
    if(trans<.035)break;
  }
  if(1.-trans<.002)discard;gl_FragColor=vec4(color,1.-trans);
}`;

const compositeFragment = /* glsl */ `
uniform sampler2D volumeMap,sceneDepth;
uniform vec2 volumeResolution,depthResolution,cameraRange;
uniform float enabled;
float linearDepth(float z){return cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));}
void main(){
  if(enabled<.5)discard;
  vec2 uv=gl_FragCoord.xy/depthResolution,pixel=uv*volumeResolution-.5,base=floor(pixel),f=fract(pixel);
  float depth=linearDepth(texture2D(sceneDepth,uv).r),total=0.;vec4 color=vec4(0.);
  for(int y=0;y<2;y++)for(int x=0;x<2;x++){
    vec2 offset=vec2(float(x),float(y)),at=(base+offset+.5)/volumeResolution;
    float neighbor=linearDepth(texture2D(sceneDepth,at).r);
    vec2 weight=mix(1.-f,f,offset);
    // Reject samples from the far side of silhouettes. Ordinary bilinear
    // filtering would smear the steam onto foreground actors and railings.
    float w=weight.x*weight.y*exp(-abs(depth-neighbor)*4./max(1.,depth*.02));
    color+=texture2D(volumeMap,at)*w;total+=w;
  }
  if(total<.001)discard;gl_FragColor=color/total;
}`;

export class EnvironmentVolume {
  constructor(scene, capacity, wakes, reflectionUniforms = {}) {
    const box = new THREE.BoxGeometry(1, 1, 1), geometry = new THREE.InstancedBufferGeometry();
    geometry.index = box.index; geometry.attributes.position = box.attributes.position;
    for (const [key, size] of Object.entries({origin:3,direction:3,extent:3,kind:1}))
      geometry.setAttribute(key, new THREE.InstancedBufferAttribute(new Float32Array(capacity * size), size).setUsage(THREE.DynamicDrawUsage));
    this.noise = noiseTexture();
    this.target = new THREE.WebGLRenderTarget(1,1,{depthBuffer:false,minFilter:THREE.NearestFilter,magFilter:THREE.NearestFilter});
    this.resolution={value:new THREE.Vector2(1,1)};
    const material = new THREE.ShaderMaterial({vertexShader,fragmentShader,
      uniforms:{noiseField:{value:this.noise},time:{value:0},sceneDepth:particleDepth.map,
        volumeResolution:this.resolution,cameraRange:particleDepth.cameraRange,hasDepth:{value:1},
        wakes:{value:wakes.points},motion:{value:wakes.motion},reflection:{value:null},reflectionMatrix:{value:new THREE.Matrix4()},hasReflection:{value:0},...reflectionUniforms},
      transparent:true,depthWrite:false,depthTest:false,side:THREE.BackSide,forceSinglePass:true,premultipliedAlpha:true,toneMapped:false});
    this.volumeMesh = new THREE.Mesh(geometry, material);this.volumeMesh.frustumCulled=false;
    this.volumeScene=new THREE.Scene();this.volumeScene.add(this.volumeMesh);
    this.mesh=new THREE.Mesh(new THREE.PlaneGeometry(2,2),new THREE.ShaderMaterial({
      vertexShader:'void main(){gl_Position=vec4(position.xy,0.,1.);}',fragmentShader:compositeFragment,
      uniforms:{volumeMap:{value:this.target.texture},sceneDepth:particleDepth.map,volumeResolution:this.resolution,
        depthResolution:particleDepth.resolution,cameraRange:particleDepth.cameraRange,enabled:{value:0}},
      transparent:true,premultipliedAlpha:true,depthWrite:false,depthTest:false,toneMapped:false}));
    this.mesh.name='environment-volumes';this.mesh.frustumCulled=false;this.mesh.renderOrder=10;this.mesh.visible=false;
    this.clearColor=new THREE.Color();this.scale=.5;
    this.mesh.onBeforeRender=(renderer,scene,camera)=>this.renderVolumes(renderer,scene,camera);
    scene.add(this.mesh);
  }
  renderVolumes(renderer,scene,camera){
    const composite=this.mesh.material;
    const captured=particleDepth.capture(renderer,scene,camera,true);
    composite.uniforms.enabled.value=captured?1:0;composite.uniformsNeedUpdate=true;
    if(!captured)return;
    const source=renderer.getRenderTarget(),width=Math.max(1,Math.ceil(source.width*this.scale)),height=Math.max(1,Math.ceil(source.height*this.scale));
    this.target.setSize(width,height);this.resolution.value.set(width,height);
    renderer.getClearColor(this.clearColor);const alpha=renderer.getClearAlpha(),autoClear=renderer.autoClear;
    try{
      renderer.autoClear=true;renderer.setClearColor(0,0);renderer.setRenderTarget(this.target);
      renderer.render(this.volumeScene,camera);
    }finally{
      renderer.setRenderTarget(source);renderer.setClearColor(this.clearColor,alpha);renderer.autoClear=autoClear;
    }
  }
  update(time, volumes) {
    this.mesh.visible=volumes.length>0;this.volumeMesh.geometry.instanceCount=volumes.length;
    this.volumeMesh.material.uniforms.time.value=time;
    const attributes=this.volumeMesh.geometry.attributes;
    for(let i=0;i<volumes.length;i++){
      const v=volumes[i];v.position.toArray(attributes.origin.array,i*3);
      v.direction.toArray(attributes.direction.array,i*3);v.extent.toArray(attributes.extent.array,i*3);
      attributes.kind.array[i]=v.kind;
    }
    if(volumes.length)for(const a of Object.values(attributes))if(a.isInstancedBufferAttribute)a.needsUpdate=true;
  }
  dispose(){
    this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();
    this.volumeMesh.geometry.dispose();this.volumeMesh.material.dispose();this.noise.dispose();this.target.dispose();
  }
}
