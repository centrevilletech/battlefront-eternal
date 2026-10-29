import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';

// Sparse, resolved wisps over the volumetric body retain the original steam's
// small curls. Their motion is local to each vent, including the angled pipes.
export class SteamWisps {
  constructor(scene,wakes,texture){
    const plane=new THREE.PlaneGeometry(1,1),geometry=new THREE.InstancedBufferGeometry();
    geometry.index=plane.index;geometry.attributes.position=plane.attributes.position;geometry.attributes.uv=plane.attributes.uv;
    const count=9*16,seeds=new Float32Array(count*4);let state=317;
    const random=()=>((state=Math.imul(state,1664525)+1013904223>>>0)/4294967296);
    for(let i=0;i<count;i++)seeds.set([Math.floor(i/16),random(),random(),random()],i*4);
    geometry.setAttribute('seed',new THREE.InstancedBufferAttribute(seeds,4));geometry.instanceCount=count;
    this.sources=Array.from({length:9},()=>new THREE.Vector4());this.directions=Array.from({length:9},()=>new THREE.Vector4());
    const material=new THREE.ShaderMaterial({transparent:true,depthWrite:false,toneMapped:false,
      uniforms:{map:{value:texture},time:{value:0},sources:{value:this.sources},directions:{value:this.directions},
        wakes:{value:wakes.points},motion:{value:wakes.motion},sceneDepth:particleDepth.map,depthResolution:particleDepth.resolution,
        cameraRange:particleDepth.cameraRange,hasDepth:{value:0}},vertexShader:/* glsl */`
      attribute vec4 seed;uniform float time;uniform vec4 sources[9],directions[9];
      uniform vec4 wakes[8],motion[8];varying vec2 vUv;varying vec3 world;varying float alpha,depth,turn;
      void main(){
        int i=int(seed.x);vec3 up=normalize(directions[i].xyz+vec3(0.,1e-6,0.));
        vec3 right=cross(up,vec3(0.,0.,1.));if(dot(right,right)<.01)right=cross(up,vec3(1.,0.,0.));
        right=normalize(right);vec3 forward=cross(right,up);
        bool pipe=directions[i].w>.5;float life=pipe?.72:3.6,age=mod(time+seed.y*life,life),t=age/life;
        float angle=seed.z*6.2831853;
        float r=pipe?.07:(.15+seed.w*.9);
        vec3 origin=sources[i].xyz+right*cos(angle)*r+forward*sin(angle)*r;
        world=origin+up*age*(pipe?4.2:1.25+seed.w*.45);
        world+=(right*sin(time*.45+angle+t*2.)+forward*cos(time*.37+angle+t*2.))*t*t*(pipe?.15:.75);
        for(int j=0;j<8;j++){
          float a=time-wakes[j].w;if(a>=0.&&a<2.){
            vec3 delta=world-wakes[j].xyz;delta.y-=clamp(delta.y,.1,1.6);
            float force=exp(-dot(delta,delta)*1.5)*exp(-a*1.5)*motion[j].w;
            world+=motion[j].xyz*force*.12;
          }
        }
        float size=pipe?mix(.18,.85,t):mix(.65,2.2,t);
        size*=.8+seed.w*.35;
        vec3 cameraRight=vec3(viewMatrix[0][0],viewMatrix[1][0],viewMatrix[2][0]);
        vec3 cameraUp=vec3(viewMatrix[0][1],viewMatrix[1][1],viewMatrix[2][1]);
        world+=cameraRight*position.x*size+cameraUp*position.y*size;
        vec4 eye=viewMatrix*vec4(world,1.);depth=-eye.z;vUv=uv;turn=angle+t*.4;
        alpha=sources[i].w*smoothstep(0.,.12,t)*(1.-smoothstep(.35,1.,t))*(pipe?.16:.20);
        if(alpha<=0.)eye=vec4(0.,0.,-1e6,1.);gl_Position=projectionMatrix*eye;
      }`,fragmentShader:/* glsl */`
      uniform sampler2D map,sceneDepth;uniform float time,hasDepth;uniform vec2 depthResolution,cameraRange;
      uniform vec4 wakes[8],motion[8];varying vec2 vUv;varying vec3 world;varying float alpha,depth,turn;
      void main(){
        vec2 p=vUv-.5;vec2 uv=mat2(cos(turn),-sin(turn),sin(turn),cos(turn))*p+.5;
        vec4 smoke=texture2D(map,clamp(uv,0.,1.));
        float a=smoke.a*alpha*(1.-smoothstep(.12,.25,dot(p,p)));
        for(int j=0;j<8;j++){
          float age=time-wakes[j].w;if(age>=0.&&age<2.){
            vec3 d=world-wakes[j].xyz-motion[j].xyz*age*.18;d.y-=clamp(d.y,.15,1.65);
            a*=1.-clamp(exp(-dot(d,d)/pow(.55+age*.42,2.))*motion[j].w*exp(-age*1.8)*.94,0.,.98);
          }
        }
        if(hasDepth>.5){float z=texture2D(sceneDepth,gl_FragCoord.xy/depthResolution).r;
          float surface=cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));a*=smoothstep(0.,.35,surface-depth);}
        if(a<.002)discard;gl_FragColor=vec4(mix(vec3(.38,.44,.51),vec3(.82,.87,.92),smoke.r),a);
      }`});
    this.mesh=new THREE.Mesh(geometry,material);this.mesh.name='chamber-steam-wisps';this.mesh.frustumCulled=false;
    this.mesh.visible=false;this.mesh.renderOrder=11;
    this.mesh.onBeforeRender=(r,s,c)=>{material.uniforms.hasDepth.value=particleDepth.capture(r,s,c,true)?1:0;material.uniformsNeedUpdate=true;};scene.add(this.mesh);
  }
  update(time,volumes){
    this.mesh.material.uniforms.time.value=time;this.mesh.visible=volumes.length>0;
    for(let i=0;i<9;i++){const v=volumes[i];if(v){this.sources[i].set(v.position.x,v.position.y,v.position.z,1);this.directions[i].set(v.direction.x,v.direction.y,v.direction.z,v.kind);}else this.sources[i].w=0;}
  }
  dispose(){this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();}
}
