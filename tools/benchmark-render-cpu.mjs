// Optional reference is a pre-change src directory with resolvable imports.
import { pathToFileURL } from 'node:url';
import path from 'node:path';
import * as THREE from 'three';
import { mergeGeometries } from 'three/addons/utils/BufferGeometryUtils.js';
const roots=[...(process.env.RENDER_CPU_REFERENCE?[process.env.RENDER_CPU_REFERENCE]:[]),path.resolve('src')];
globalThis.window=new EventTarget();globalThis.document=new EventTarget();
const suites=[];
for(const root of roots) {
  const load=name=>import(pathToFileURL(path.join(root,name)).href);
  const {Controls}=await load('input.js'),{LocalLights}=await load('local-lights.js'),{partitionShadowCaster}=await load('shadow-casters.js'),{TreeCanopies}=await load('tree-canopies.js');
  const controls=new Controls(new EventTarget());controls.hold('KeyW','KeyD','Mouse0');
  const lights=new LocalLights();for(let i=0;i<250;i++)lights.add([Math.sin(i)*15,1,Math.cos(i)*15],[1,.5,.2],5+i%15);
  const object=new THREE.Object3D();
  const geometry=mergeGeometries(Array.from({length:400},(_,i)=>new THREE.BoxGeometry(3,4,3).translate((i%20-10)*20,2,(Math.floor(i/20)-10)*20)));
  const mesh=new THREE.Mesh(geometry,new THREE.MeshBasicMaterial());
  const [proxy]=partitionShadowCaster(mesh,()=>{},1);mesh.updateMatrixWorld(true);
  const camera=new THREE.OrthographicCamera(-90,90,90,-90,1,1000);camera.position.set(0,80,100);camera.lookAt(0,0,0);camera.updateMatrixWorld(true);
  const graphics={scene:new THREE.Scene(),farScene:new THREE.Scene(),settings:{get:()=> 'enhanced'}};
  const trees=new TreeCanopies(graphics,{instances:Array.from({length:20},(_,i)=>({matrix:new THREE.Matrix4().makeTranslation((i%5-2)*25,0,-30-Math.floor(i/5)*40).toArray(),bounds:[[-4,0,-4],[4,12,4]]})),texture:'leaves'},()=>new THREE.Texture());
  const view=new THREE.PerspectiveCamera(75,1,.1,1000);view.position.set(0,6,0);view.updateMatrixWorld(true);
  suites.push({root,times:{},jobs:{
    input(){for(let i=0;i<100000;i++){controls.press('Mouse0');controls.take('firePrimary');controls.axis('forward');controls.axis('right');controls.axis('turn');controls.held('firePrimary');}},
    lamps(){for(let i=0;i<5000;i++){lights.now=i;object.position.x=Math.sin(i*.01)*5;object.updateMatrixWorld();lights.select(object);}},
    shadows(){for(let i=0;i<5000;i++){camera.position.x=Math.sin(i*.01)*30;camera.updateMatrixWorld();proxy.onBeforeShadow(null,null,null,camera);proxy.geometry.index.clearUpdateRanges();}},
    color(){for(let i=0;i<5000;i++){camera.position.x=Math.sin(i*.01)*30;camera.updateMatrixWorld();mesh.onBeforeRender(null,null,camera,mesh.geometry,mesh.material,null);mesh.geometry.index.clearUpdateRanges();}},
    canopy(){for(let i=0;i<2000;i++){view.rotation.y=Math.sin(i*.01)*.6;view.updateMatrixWorld();trees.update(view);for(const a of Object.values(trees.mesh.geometry.attributes))a.clearUpdateRanges();}},
  }});
}
for(let round=0;round<8;round++)for(const suite of (round%2?[...suites].reverse():suites))for(const [name,job] of Object.entries(suite.jobs)) {
  const start=performance.now();job();const ms=performance.now()-start;
  if(round)(suite.times[name]??=[]).push(ms);
}
for(const {root,times} of suites)console.log(JSON.stringify({root,measurements:Object.fromEntries(Object.entries(times).map(([key,values])=>{values.sort((a,b)=>a-b);return [key,{medianMs:values[3],minMs:values[0],maxMs:values.at(-1)}]}))}));
