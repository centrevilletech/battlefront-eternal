import test from 'node:test';
import assert from 'node:assert/strict';
import * as THREE from 'three';
import { stormGeometry } from '../src/ocean-storm.js';
import { OceanSurface, storms } from '../src/ocean-surface.js';
import { PROBE_LAYER } from '../src/sky-probe.js';
import { lighting } from '../src/materials.js';
import { buildWorld, SKY_ORDER } from '../src/world.js';

// Kamino's weather: rain (Type Streaks) on the sea, and with it the storm.
const RAIN={precipitation:{Type:['Streaks']}};

function fixture(){
 let mode='enhanced',listener;
 const settings={get:()=>mode,onChange:fn=>{listener=fn;return()=>{listener=null;}}};
 const graphics={scene:new THREE.Scene(),farScene:new THREE.Scene(),settings,sky:new THREE.Group()};
 graphics.farScene.add(graphics.sky);
 const texture=new THREE.Texture();
 const data={grid:4,cell:160,height:-8,offset:[320,320],cells:[[2,2]],look:{MainTexture:['sea'],WindSpeed:[25]}};
 const ocean=new OceanSurface({data,texture:()=>texture,graphics,weather:RAIN});
 return {ocean,graphics,setMode:m=>{mode=m;listener('oceanEffects');},getListener:()=>listener};
}

test('storm rings are a watertight bounded mesh with only the outer perimeter open',()=>{
 const g=stormGeometry(),p=g.attributes.position.array,index=g.index.array,edges=new Map();
 assert.ok(index.length/3<66000);assert.ok(p.length/3<33000);
 for(let i=0;i<index.length;i+=3){
   const [a,b,c]=index.slice(i,i+3);assert.ok(a!==b&&b!==c&&a!==c);
   const ux=p[b*3]-p[a*3],uz=p[b*3+2]-p[a*3+2],vx=p[c*3]-p[a*3],vz=p[c*3+2]-p[a*3+2];
   assert.ok(uz*vx-ux*vz>0,'all triangles face up');
   for(const [u,v] of [[a,b],[b,c],[c,a]]){const key=u<v?`${u}:${v}`:`${v}:${u}`;edges.set(key,(edges.get(key)||0)+1);}
 }
 const boundary=[...edges].filter(([,n])=>n===1);
 assert.equal(boundary.length,256);assert.ok([...edges.values()].every(n=>n===1||n===2));
 for(const [key] of boundary)for(const i of key.split(':').map(Number))assert.equal(Math.max(Math.abs(p[i*3]),Math.abs(p[i*3+2])),4096);
 g.dispose();
});

test('enhanced ocean switches without reallocating resources and restores original sky and lighting',()=>{
 const f=fixture(),{ocean,graphics}=f,camera=new THREE.PerspectiveCamera();camera.position.set(25,50,100);
 const waves=ocean.waves,geometry=ocean.geometry,sky=waves.storm;
 const ambient=lighting.ambient.value.clone();
 ocean.update(6.06,camera); // First flash begins when the scheduled strike starts.
 ocean.update(.06,camera);
 assert.ok(sky.uniforms.lightningFlash.value>.9);assert.ok(sky.bolt.visible);
 assert.ok(lighting.ambient.value.x>ambient.x);assert.equal(graphics.sky.visible,false);
 assert.ok(sky.boltGeometry.drawRange.count>300&&sky.boltGeometry.drawRange.count<=1536);
 assert.ok(sky.positions.slice(0,sky.boltGeometry.drawRange.count*3).every(Number.isFinite));
 assert.equal(waves.spray.geometry.instanceCount,384);assert.ok('STORM' in waves.material.defines);
 f.setMode('classic');assert.equal(graphics.sky.visible,true);assert.ok(lighting.ambient.value.equals(ambient));assert.equal(sky.bolt.visible,false);
 assert.equal(waves.spray.visible,false);assert.equal(sky.sky.visible,false);
 f.setMode('enhanced');ocean.update(.6,camera);
 assert.equal(ocean.geometry,geometry);assert.equal(ocean.waves,waves);assert.equal(sky.bolt.visible,false);
 assert.ok(lighting.ambient.value.equals(ambient));
 ocean.dispose();assert.equal(f.getListener(),null);assert.equal(graphics.scene.children.length,0);
 assert.deepEqual(graphics.farScene.children,[graphics.sky]);assert.equal(graphics.sky.visible,true);
});

// Kamino's sky as tools/extract.py writes it: the dome in two parts, then the
// city on its horizon (blended), and an additive model after it.
function stormSky(){
 let mode='enhanced',listener;
 const settings={get:()=>mode,onChange:fn=>{listener=fn;return()=>{listener=null;}}};
 const graphics={scene:new THREE.Scene(),farScene:new THREE.Scene(),settings,skyLayers:[],localLights:{register(){}}};
 const quad=[[-1,0,-1],[1,0,-1],[1,0,1],[-1,0,1]],verts=[],indices=[];
 const meshes=[['SKY_kam1',0,0],['SKY_kam1',0,0],['kam1_city',4,1],['kam1_glow',0x84,2]].map(([texture,flags,layer])=>{
   const record={texture,flags,attributes:['position','normal','uv'],stride:8,vertexOffset:verts.length*4,vertexCount:4,indexOffset:indices.length*4,indexCount:6,sky:true};
   for(const [x,y,z] of quad)verts.push(x,y,z,0,1,0,0,0);
   indices.push(0,1,2,0,2,3);
   if(layer){record.skyLayer=layer;record.skyFollow=[.995,80];}
   return record;
 });
 const texture=new THREE.Texture();
 buildWorld(graphics,{name:'kam1',meshes},new Float32Array(verts).buffer,new Uint32Array(indices).buffer,()=>texture);
 const imported=graphics.skyLayers.map(({mesh})=>mesh.material);
 const data={grid:4,cell:160,height:-8,offset:[320,320],cells:[[2,2]],look:{MainTexture:['sea'],WindSpeed:[25]}};
 const ocean=new OceanSurface({data,texture:()=>texture,graphics,weather:RAIN});
 return {ocean,graphics,imported,setMode:m=>{mode=m;listener('oceanEffects');}};
}

test('the storm replaces only the dome: the models on its horizon draw over the panorama in the storm light',()=>{
 const {ocean,graphics,imported,setMode}=stormSky(),sky=ocean.waves.storm;
 const domes=graphics.skyLayers.filter(l=>!l.layer).map(l=>l.mesh),horizon=graphics.skyLayers.filter(l=>l.layer).map(l=>l.mesh);
 assert.equal(domes.length,2);assert.equal(horizon.length,2);
 const originals=imported.slice(2);
 assert.ok(originals.every(material=>material.name==='zero:sky'));
 // Enhanced: every part of the dome hidden, the horizon models kept and drawn
 // after the panorama. All of them are opaque-queue draws, so render order
 // alone sorts them: dome, panorama, city, glow.
 assert.ok(domes.every(mesh=>!mesh.visible));assert.ok(horizon.every(mesh=>mesh.visible));
 assert.ok(sky.sky.visible);
 for(const mesh of [...domes,sky.sky,...horizon])assert.equal(mesh.material.transparent,false);
 assert.ok(Math.max(...domes.map(m=>m.renderOrder))<sky.sky.renderOrder);
 assert.ok(sky.sky.renderOrder<Math.min(...horizon.map(m=>m.renderOrder)));
 assert.equal(domes[0].renderOrder,SKY_ORDER);
 const [city,glow]=horizon.map(mesh=>mesh.material);
 for(const [material,original] of [[city,originals[0]],[glow,originals[1]]]){
   assert.equal(material.name,'storm-horizon');
   assert.equal(material.depthTest,false);assert.equal(material.depthWrite,false);
   assert.equal(material.blending,THREE.CustomBlending);assert.equal(material.blendSrc,original.blendSrc);assert.equal(material.blendDst,original.blendDst);
   assert.equal(material.side,original.side);
   // The same texture as the sky draws, and the storm's own haze and flash.
   assert.equal(material.uniforms.map,original.uniforms.map);
   assert.equal(material.uniforms.hazeColor,sky.uniforms.hazeColor);
   assert.equal(material.uniforms.lightningFlash,sky.uniforms.lightningFlash);
   assert.equal(material.uniforms.lightningPosition,sky.uniforms.lightningPosition);
 }
 assert.ok('BLENDED' in city.defines);assert.ok(!('ADDITIVE' in city.defines));assert.equal(city.blendDst,THREE.OneMinusSrcAlphaFactor);
 assert.ok('ADDITIVE' in glow.defines);assert.equal(glow.blendDst,THREE.OneFactor);
 // A strike lights them: the flash reaches their uniforms.
 const camera=new THREE.PerspectiveCamera();camera.position.set(-12,47,166);
 ocean.update(6.06,camera);ocean.update(.06,camera);
 assert.ok(city.uniforms.lightningFlash.value>.9);
 // Classic: the dome and the horizon models' own materials come back.
 setMode('classic');
 assert.ok(domes.every(mesh=>mesh.visible));assert.ok(!sky.sky.visible);
 assert.ok(horizon.every((mesh,i)=>mesh.material===originals[i]));
 // Back to Enhanced: the same storm materials, nothing allocated again.
 setMode('enhanced');
 assert.equal(horizon[0].material,city);assert.equal(horizon[1].material,glow);
 assert.ok(domes.every(mesh=>!mesh.visible));
 // Disposal leaves the imported sky as it was and frees the storm materials.
 let freed=0;for(const material of [city,glow])material.addEventListener('dispose',()=>freed++);
 ocean.dispose();
 assert.equal(freed,2);assert.ok(horizon.every((mesh,i)=>mesh.material===originals[i]));
 assert.ok([...domes,...horizon].every(mesh=>mesh.visible));
 assert.ok(graphics.skyLayers.every(({mesh})=>graphics.farScene.children.includes(mesh)));
});

test('a dome hidden before the storm stays hidden after it',()=>{
 const {ocean,graphics,setMode}=stormSky();
 const dome=graphics.skyLayers[0].mesh;
 setMode('classic');dome.visible=false;setMode('enhanced');setMode('classic');
 assert.equal(dome.visible,false);assert.equal(graphics.skyLayers[1].mesh.visible,true);
 ocean.dispose();
});

test('the storm comes with rain: Kamino storms; snow, or a sea under a dry sky, does not',()=>{
 assert.equal(storms(RAIN),true);
 assert.equal(storms({precipitation:{}}),true,'rain unless it says snow, as game.js reads it');
 assert.equal(storms({precipitation:{Type:['Quads']}}),false,"Hoth's snow");
 assert.equal(storms(null),false,'Kashyyyk: Islands has no weather');
 assert.equal(storms({wind:[1,.3]}),false);
});

// Kashyyyk: Islands' sky and sea: the dome, the mountains on its horizon (in
// two parts), and a sea out to every edge of the grid. The map has no weather.
function calmSea(){
 let mode='enhanced',listener;
 const settings={get:()=>mode,onChange:fn=>{listener=fn;return()=>{listener=null;}}};
 const graphics={scene:new THREE.Scene(),farScene:new THREE.Scene(),settings,skyLayers:[],localLights:{register(){}}};
 const quad=[[-1,0,-1],[1,0,-1],[1,0,1],[-1,0,1]],verts=[],indices=[];
 const meshes=[['SKY_kashyyyk1',0,0],['kas_mountains',0,1],['kas_mountains',4,1]].map(([texture,flags,layer])=>{
   const record={texture,flags,attributes:['position','normal','uv'],stride:8,vertexOffset:verts.length*4,vertexCount:4,indexOffset:indices.length*4,indexCount:6,sky:true};
   for(const [x,y,z] of quad)verts.push(x,y,z,0,1,0,0,0);
   indices.push(0,1,2,0,2,3);
   if(layer){record.skyLayer=layer;record.skyFollow=[.995,-40];}
   return record;
 });
 const texture=new THREE.Texture();
 buildWorld(graphics,{name:'kas1',meshes},new Float32Array(verts).buffer,new Uint32Array(indices).buffer,()=>texture);
 const imported=graphics.skyLayers.map(({mesh})=>mesh.material);
 const data={grid:8,cell:256,height:-45,offset:[1024,1024],cells:[[0,0],[7,7],[0,7],[7,0]],
   look:{MainTexture:['kas1_water'],RefractionColor:[45,53,74,255],ReflectionColor:[191,191,191,191],OceanEnable:[0],WindSpeed:[0]}};
 const ocean=new OceanSurface({data,texture:()=>texture,graphics,weather:null});
 return {ocean,graphics,imported,setMode:m=>{mode=m;listener('oceanEffects');}};
}

test('a sea under a dry sky takes the waves alone and keeps the map sky as it is',()=>{
 const {ocean,graphics,imported,setMode}=calmSea(),waves=ocean.waves;
 // No storm: no panorama, lightning, veil or rain rings, and every model of
 // the sky shown in its own material.
 assert.equal(waves.storm,null);assert.equal(waves.haze,null);
 for(const name of ['storm-volume-clouds','storm-lightning','ocean-horizon-haze'])assert.equal(graphics.farScene.getObjectByName(name),undefined);
 assert.ok(graphics.skyLayers.every(({mesh},i)=>mesh.visible&&mesh.material===imported[i]));
 assert.equal(waves.uniforms.rainRipples.value,0);
 // The waves, in the map's colours: the same rings, without the storm's palette.
 assert.equal(ocean.geometry,waves.geometry);assert.equal(ocean.material,waves.material);
 assert.ok(!('STORM' in waves.material.defines));assert.ok(!('STORM' in waves.spray.material.defines));
 assert.deepEqual(waves.uniforms.waterColor.value.toArray().map(v=>Math.round(v*255)),[45,53,74]);
 assert.equal(waves.uniforms.reflectAmount.value,191/255);
 assert.equal(waves.uniforms.ambient,lighting.ambient);
 // What it reflects is the sky, from the probe, which sees only the sky.
 assert.equal(waves.uniforms.skyProbe.value,waves.probe.texture);
 assert.ok(graphics.skyLayers.every(({mesh})=>mesh.layers.isEnabled(PROBE_LAYER)));
 assert.ok(!ocean.far.layers.isEnabled(PROBE_LAYER));
 assert.ok(waves.probe.camera.children.every(face=>face.layers.mask===1<<PROBE_LAYER));
 // The map's own light: nothing flashes.
 const ambient=lighting.ambient.value.clone(),camera=new THREE.PerspectiveCamera();camera.position.set(167,-22,147);
 ocean.update(20,camera);assert.ok(lighting.ambient.value.equals(ambient));
 // Classic and back: the same waves, the probe idle in Classic.
 setMode('classic');assert.equal(ocean.material,ocean.classicMaterial);assert.equal(waves.spray.visible,false);assert.equal(waves.probe.enabled,false);
 assert.ok(graphics.skyLayers.every(({mesh},i)=>mesh.visible&&mesh.material===imported[i]));
 setMode('enhanced');assert.equal(ocean.waves,waves);assert.equal(waves.probe.enabled,true);assert.equal(waves.spray.visible,true);
 // Disposal takes the probe's layer off the sky and frees its cube.
 let freed=false;waves.probe.target.addEventListener('dispose',()=>{freed=true;});
 ocean.dispose();assert.ok(freed);
 assert.ok(graphics.skyLayers.every(({mesh},i)=>!mesh.layers.isEnabled(PROBE_LAYER)&&mesh.visible&&mesh.material===imported[i]));
});

test('the sky probe draws only the sky, as the eye sees it, once a second or when the eye climbs',()=>{
 const {ocean,graphics,setMode}=calmSea(),probe=ocean.waves.probe,drawn=[];
 const renderer={autoClear:false,shadowMap:{autoUpdate:true},xr:{enabled:false},coordinateSystem:THREE.WebGLCoordinateSystem,
   getRenderTarget:()=>null,getActiveCubeFace:()=>0,getActiveMipmapLevel:()=>0,setRenderTarget(){},
   render(scene,camera){const seen=[];scene.traverse(o=>{if(o.isMesh&&o.visible&&o.layers.test(camera.layers))seen.push(o.name);});drawn.push({seen,autoClear:this.autoClear,shadows:this.shadowMap.autoUpdate});}};
 graphics.renderer=renderer;
 const camera=new THREE.PerspectiveCamera();camera.position.set(167,-22,147);
 ocean.update(0,camera);
 assert.equal(probe.captures,1);assert.equal(drawn.length,6,'six faces');
 const sky=graphics.skyLayers.map(({mesh})=>mesh.name).join();
 assert.ok(drawn.every(face=>face.seen.join()===sky&&face.autoClear&&!face.shadows));
 assert.equal(renderer.autoClear,false);assert.equal(renderer.shadowMap.autoUpdate,true);
 // Around the eye as graphics.render() stands it: the mountains by their MovementScale.
 assert.deepEqual(probe.camera.position.toArray(),[167,-22,147]);
 assert.ok(Math.abs(graphics.skyLayers[1].mesh.position.y-(.995*-22+.005*-40))<1e-9);
 ocean.update(.5,camera);assert.equal(probe.captures,1,'held within the second');
 camera.position.y+=5;ocean.update(0,camera);assert.equal(probe.captures,2,'the eye climbed');
 ocean.update(1,camera);assert.equal(probe.captures,3,'a second on');
 setMode('classic');ocean.update(5,camera);assert.equal(probe.captures,3,'not in Classic');
 ocean.dispose();
});
