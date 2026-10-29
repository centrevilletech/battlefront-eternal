import * as THREE from 'three';
import { particleDepth } from './particle-depth.js';

// Short splashes where a character wades or lands in a basin, on the GPU; CPU
// work is the five fountain sources (shared with fountain-jets.js, which also
// throws the jets' own drops) and the eight wake slots.
export class FountainSpray {
  constructor(scene,wakes){
    const plane=new THREE.PlaneGeometry(1,1),geometry=new THREE.InstancedBufferGeometry();
    geometry.index=plane.index;geometry.attributes.position=plane.attributes.position;geometry.attributes.uv=plane.attributes.uv;
    const count=8*24,seed=new Float32Array(count*4);let state=137;
    const random=()=>((state=Math.imul(state,1664525)+1013904223>>>0)/4294967296);
    for(let i=0;i<count;i++)seed.set([Math.floor(i/24),random(),random(),random()],i*4);
    geometry.setAttribute('seed',new THREE.InstancedBufferAttribute(seed,4));geometry.instanceCount=count;
    const sources=Array.from({length:5},()=>new THREE.Vector4(0,0,0,0));
    const material=new THREE.ShaderMaterial({transparent:true,depthWrite:false,toneMapped:false,uniforms:{time:{value:0},sources:{value:sources},
      wakes:{value:wakes.points},motion:{value:wakes.motion},sceneDepth:particleDepth.map,depthResolution:particleDepth.resolution,
      cameraRange:particleDepth.cameraRange,hasDepth:{value:0}},vertexShader:/* glsl */`
      attribute vec4 seed;uniform float time;uniform vec4 sources[5],wakes[8],motion[8];
      varying vec2 vUv;varying float alpha,depth;
      void main(){
        float angle=seed.y*6.2831853;int i=int(seed.x);
        vec3 origin=wakes[i].xyz+vec3(0.,.15,0.);
        float age=time-wakes[i].w,life=.7,speed=(.7+seed.w*2.)*motion[i].w;
        vec3 velocity=vec3(cos(angle)*speed,2.+seed.z*2.,sin(angle)*speed);
        float size=.028+seed.w*.035;alpha=age<0.||age>life?0.:1.;
        // The water around the fountain's nozzle: Enhanced fills the inner
        // tier to its lip (TIER, fountain-jets.js), 4 m out, above the outer pool.
        bool wet=false;for(int j=0;j<5;j++)if(sources[j].w>.5&&distance(origin.xz,sources[j].xz)<6.3&&abs(origin.y-sources[j].y)<1.8){wet=true;origin.y=sources[j].y+(distance(origin.xz,sources[j].xz)<3.9?.58:-.25);}
        if(!wet)alpha=0.;
        vec3 world=origin+velocity*age+vec3(0.,-7.5*age*age,0.);
        vec4 eye=viewMatrix*vec4(world,1.);
        eye.xy+=position.xy*vec2(size,size*(1.+abs(velocity.y-15.*age)*.12));
        depth=-eye.z;vUv=uv;alpha*=smoothstep(0.,.10,age)*(1.-smoothstep(life*.8,life,age));
        if(alpha<=0.)eye=vec4(0.,0.,-1e6,1.);
        gl_Position=projectionMatrix*eye;
      }`,fragmentShader:/* glsl */`
      uniform sampler2D sceneDepth;uniform vec2 depthResolution,cameraRange;uniform float hasDepth;
      varying vec2 vUv;varying float alpha,depth;
      void main(){vec2 p=vUv*2.-1.;float r=dot(p,p);if(r>1.)discard;
        float a=alpha*(1.-smoothstep(.3,1.,r))*.55;
        if(hasDepth>.5){float z=texture2D(sceneDepth,gl_FragCoord.xy/depthResolution).r;
          float surface=cameraRange.x*cameraRange.y/(cameraRange.y-z*(cameraRange.y-cameraRange.x));a*=smoothstep(0.,.12,surface-depth);}
        gl_FragColor=vec4(mix(vec3(.66,.81,.86),vec3(.95,1.,1.),pow(max(0.,1.-length(p-vec2(-.3,.4))),5.)),a);
      }`});
    this.sources=sources;this.mesh=new THREE.Mesh(geometry,material);this.mesh.name='fountain-wake-spray';
    this.mesh.frustumCulled=false;this.mesh.visible=false;this.mesh.renderOrder=11;
    this.mesh.onBeforeRender=(r,s,c)=>{material.uniforms.hasDepth.value=particleDepth.capture(r,s,c,true)?1:0;material.uniformsNeedUpdate=true;};scene.add(this.mesh);
  }
  // Each fountain keeps its slot, so one leaving view doesn't reseed another.
  update(time,fountains){this.mesh.material.uniforms.time.value=time;this.mesh.visible=fountains.length>0;
    for(const s of this.sources)s.w=0;
    for(const f of fountains)this.sources[f.slot]?.set(f.position.x,f.position.y,f.position.z,1);}
  dispose(){this.mesh.removeFromParent();this.mesh.geometry.dispose();this.mesh.material.dispose();}
}
