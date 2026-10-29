import * as THREE from 'three';
import { lighting } from './materials.js';
import { EnhancedOcean } from './ocean-storm.js';

// Whether the Enhanced ocean storms: when the map's weather rains (any
// Precipitation but Type("Quads"), Hoth's snow, as game.js reads it). Kamino
// rains on its sea; Kashyyyk: Islands has no weather, and keeps its sky.
export function storms(weather) {
  const type=weather?.precipitation?.Type?.[0];
  return Boolean(weather?.precipitation)&&String(type??'').toLowerCase()!=='quads';
}

// Fixed-size, camera-following ocean: detailed nearby waves and a flat outer
// skirt. The authored water mask still determines where water exists.
export function oceanGeometry(reach = 4096) {
  const grid = new THREE.PlaneGeometry(512, 512, 64, 64).rotateX(-Math.PI / 2);
  const positions = Array.from(grid.attributes.position.array), indices = Array.from(grid.index.array);
  const inner = [[-256,-256],[256,-256],[256,256],[-256,256]];
  const outer = [[-reach,-reach],[reach,-reach],[reach,reach],[-reach,reach]];
  for (let i=0;i<4;i++) {
    const j=(i+1)%4, base=positions.length/3;
    for(const p of [inner[i],inner[j],outer[j],outer[i]])positions.push(p[0],0,p[1]);
    indices.push(base,base+2,base+1,base,base+3,base+2);
  }
  grid.dispose();
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(positions,3));geometry.setIndex(indices);
  return geometry;
}
const WAVES = /* glsl */ `
  float wave(vec2 p) {
    vec2 across = vec2(-wind.y, wind.x);
    return sin(dot(p, wind) * 0.18 - time * 1.7) * 0.65
      + sin(dot(p, across) * 0.29 + time * 1.3) * 0.24
      + sin(dot(p, normalize(wind + across)) * 0.43 - time * 2.0) * 0.11;
  }
`;
const VERTEX = /* glsl */ `
  uniform vec2 wind;
  uniform float time, amplitude;
  varying vec3 vWorld;
  varying float vDepth;
  ${WAVES}
  void main() {
    vec4 world = modelMatrix * vec4(position,1.0);
    float detail = 1.0 - smoothstep(100.0, 250.0, max(abs(position.x),abs(position.z)));
    world.y += wave(world.xz) * amplitude * detail;
    vWorld = world.xyz;
    vec4 view = viewMatrix * world; vDepth = -view.z;
    gl_Position = projectionMatrix * view;
  }
`;
const FRAGMENT = /* glsl */ `
  uniform sampler2D waterMap, foamMap, mask;
  uniform vec2 tile, foamTile, velocity, wind, gridOffset, fogRange;
  uniform float time, amplitude, cell, gridWidth;
  uniform vec3 fogColor, sunColor, sunDirection;
  varying vec3 vWorld;
  varying float vDepth;
  ${WAVES}
  void main() {
    vec2 maskUv = (vWorld.xz + gridOffset) / gridWidth;
    if(any(lessThan(maskUv,vec2(0.0))) || any(greaterThanEqual(maskUv,vec2(1.0))) || texture2D(mask,maskUv).r < 0.5) discard;
    vec2 uv = vWorld.xz / cell;
    vec2 drift = velocity * time;
    vec3 base = texture2D(waterMap,uv * tile + drift).rgb;
    base = mix(base,texture2D(waterMap,uv * tile * 1.63 - drift * 0.73).rgb,0.35);
    float h = wave(vWorld.xz);
    vec2 slope = vec2(wave(vWorld.xz + vec2(.2,0.0))-h,wave(vWorld.xz+vec2(0.0,.2))-h) * amplitude / .2;
    vec3 N = normalize(vec3(-slope.x,1.0,-slope.y));
    vec3 V = normalize(cameraPosition-vWorld), L = normalize(-sunDirection);
    float fresnel = pow(1.0-max(dot(N,V),0.0),4.0);
    vec3 color = base * (0.8 + 0.4 * max(N.y,0.0)) + vec3(.065,.085,.11) * fresnel;
    float crest = smoothstep(.58,.93,h) * .16;
    vec4 foam = texture2D(foamMap,uv * foamTile - drift);
    color = mix(color,foam.rgb,crest);
    color += sunColor * pow(max(dot(N,normalize(V+L)),0.0),48.0) * .2;
    float fog = smoothstep(fogRange.x,fogRange.y,vDepth);
    gl_FragColor = vec4(mix(color,fogColor,fog),1.0);
  }
`;
export class OceanSurface {
  constructor({data,texture,graphics,weather=null}) {
    this.graphics=graphics;this.height=data.height;this.time=0;this.look=data.look||{};this.stormy=storms(weather);
    const look=data.look;
    const cells=new Uint8Array(data.grid*data.grid);
    for(const [x,z] of data.cells)cells[z*data.grid+x]=255;
    this.mask=new THREE.DataTexture(cells,data.grid,data.grid,THREE.RedFormat);this.mask.needsUpdate=true;
    this.mask.minFilter=this.mask.magFilter=THREE.NearestFilter;
    const wind=new THREE.Vector2(...(look.WindDirection || [1,0])).normalize();if(!wind.lengthSq())wind.set(1,0);
    this.material=new THREE.ShaderMaterial({vertexShader:VERTEX,fragmentShader:FRAGMENT,side:THREE.DoubleSide,
      uniforms:{waterMap:{value:texture(look.MainTexture?.[0])},foamMap:{value:texture(look.FoamTexture?.[0]) || texture(look.MainTexture?.[0])},mask:{value:this.mask},
        tile:{value:new THREE.Vector2(...(look.Tile || [1,1]))},foamTile:{value:new THREE.Vector2(...(look.FoamTile || [1,1]))},
        velocity:{value:new THREE.Vector2(...(look.Velocity || [0,0]))},wind:{value:wind},time:{value:0},
        amplitude:{value:Math.min(2,Math.max(.15,(look.WindSpeed?.[0] || 0)*.05))},
        cell:{value:data.cell},gridWidth:{value:data.cell*data.grid},gridOffset:{value:new THREE.Vector2(...data.offset)},
        fogRange:lighting.fogRange,fogColor:lighting.fogColor,sunColor:lighting.sunColor,sunDirection:lighting.sunDirection}});
    this.classicMaterial=this.material;
    this.geometry=oceanGeometry();this.classicGeometry=this.geometry;
    this.near=new THREE.Mesh(this.geometry,this.material);this.far=new THREE.Mesh(this.geometry,this.material);
    for(const mesh of [this.near,this.far]){mesh.name='native-ocean';mesh.frustumCulled=false;mesh.position.y=this.height;}
    graphics.scene.add(this.near);graphics.farScene.add(this.far);
    this.applyMode();
    this.unsubscribe=graphics.settings?.onChange(key=>{if(key==='oceanEffects')this.applyMode();});
  }
  applyMode() {
    this.enhanced=this.graphics.settings?.get('oceanEffects')==='enhanced';
    if(this.enhanced&&!this.waves){
      this.waves=new EnhancedOcean(this.classicMaterial.uniforms,this.graphics,{storm:this.stormy,look:this.look});
      this.near.add(this.waves.spray);if(this.waves.haze)this.far.add(this.waves.haze);
    }
    this.geometry=this.enhanced?this.waves.geometry:this.classicGeometry;
    this.material=this.enhanced?this.waves.material:this.classicMaterial;
    for(const mesh of [this.near,this.far]){mesh.geometry=this.geometry;mesh.material=this.material;}
    this.waves?.setEnabled(this.enhanced);
  }
  update(dt,camera) {
    this.time+=dt;this.material.uniforms.time.value=this.time;
    const x=Math.floor(camera.position.x/8)*8,z=Math.floor(camera.position.z/8)*8;
    for(const mesh of [this.near,this.far])mesh.position.set(x,this.height,z);
    if(this.enhanced)this.waves.update(camera,this.height);
  }
  dispose(){this.near.removeFromParent();this.far.removeFromParent();this.unsubscribe?.();this.waves?.dispose();this.classicGeometry.dispose();this.classicMaterial.dispose();this.mask.dispose();}
}
