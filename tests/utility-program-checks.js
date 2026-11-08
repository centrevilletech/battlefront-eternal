// LineBasicMaterial's former draw inputs, without a game loader. The reference
// renderer exists only in this fixture; production links the owned catalog.
// Compare every supported define and topology, including dynamic index ranges,
// before and after a real context loss. A nonempty reference prevents blank parity.
import * as THREE from 'three';
import { Graphics } from '../src/graphics.js';
import { Settings } from '../src/settings.js';
import catalog from '../rust/bfe-render/shaders/utility/manifest.json';
import { ownedGeometryProgram } from '../src/rust-render/programs.js';
import { writeScreenStep } from '../src/rust-render/screen.js';
const defines=import.meta.glob('../rust/bfe-render/shaders/utility/defines-*.glsl',{query:'?raw',import:'default',eager:true});
import { ROLE, KIND } from '../src/rust-render/frame-layout.js';

export async function checkOwnedUtilityDraws() {
  const g=new Graphics(document.body,new Settings({shadows:'off',antialiasing:'off'}));
  const reference=new THREE.WebGLRenderer({antialias:false});
  reference.outputColorSpace=THREE.LinearSRGBColorSpace;reference.setSize(64,64);
  const scene=new THREE.Scene(),target=new THREE.WebGLRenderTarget(64,64),referenceTarget=target.clone();
  const results=[],fixtures=[];
  try {
    await g.rustReady;
    const renderer=g.renderer,core=g.rust,camera=g.camera;
    // The global scene hook packs lights through an explicit native owner.
    scene.onBeforeRender=()=>{};
    renderer.setSize(64,64);camera.aspect=1;camera.near=.1;camera.far=100;camera.position.set(0,0,0);camera.quaternion.identity();camera.updateProjectionMatrix();camera.updateMatrixWorld();
    g.warmup();core.frame.reserve();await core.prewarm(3,async()=>{});const programs=core.shaderPrograms.length;
    // Different colours, depths and non-unit W exercise interpolation and the
    // model-view/projection order. Loops are indexed with a shortened draw range.
    // Rust's catalog descriptors supply keys and defines. The fixture derives
    // reference material inputs from them; it never recreates a key selector.
    for(const descriptor of catalog.programs){
      const read=paths=>paths.filter(p=>p.startsWith('defines-')).map(p=>{const text=defines['../rust/bfe-render/shaders/utility/'+p];if(typeof text!=='string')throw Error(`Missing utility defines: ${p}`);return text;}).join('\n');
      const vertex=read(descriptor.vertex),fragment=read(descriptor.fragment);
      const color=vertex.includes('#define USE_COLOR_ALPHA')?4:vertex.includes('#define USE_COLOR')?3:0;
      const transparent=!fragment.includes('#define OPAQUE'),cutout=fragment.includes('#define USE_ALPHATEST'),premultipliedAlpha=fragment.includes('#define PREMULTIPLIED_ALPHA');
      for(const blending of [THREE.NormalBlending,THREE.NoBlending])for(const Type of [THREE.Line,THREE.LineSegments,THREE.LineLoop]){
      const geometry=new THREE.BufferGeometry().setAttribute('position',new THREE.Float32BufferAttribute([-.8,-.4,-2,.8,.5,-2.5,.2,-.6,-1.8],3));
      if(color)geometry.setAttribute('color',new THREE.Float32BufferAttribute(color===3?[1,.2,.4,.3,.6,1,.5,1,.2]:[1,.2,.4,.2,.3,.6,1,.8,.5,1,.2,.5],color));
      geometry.setIndex(Type===THREE.LineLoop?[0,1,2,0]:[0,1,2]);
      if(Type===THREE.LineLoop)geometry.setDrawRange(1,3);
      const material=new THREE.LineBasicMaterial({color:0xb7b5ab,blending,vertexColors:color!==0,transparent,alphaTest:cutout?.25:0,premultipliedAlpha,opacity:.7});
      // Explicit catalog OPAQUE also applies when the raster blend is disabled.
      if(!transparent&&blending===THREE.NoBlending)material.defines={OPAQUE:''};
      const object=new Type(geometry,material);object.frustumCulled=false;
      fixtures.push({object,key:descriptor.key,program:ownedGeometryProgram(core,descriptor.key)});
      }
    }
    const pixels=new Uint8Array(64*64*4),expected=new Uint8Array(pixels.length),clear=new Uint8Array(4);
    // Reuse native resources on replay. Explicit topology/range updates retain
    // all catalog variants, including ones no domain line source currently uses.
    const compare=(fixture,stage)=>{
      const {object,key,program}=fixture;
      scene.add(object);reference.setRenderTarget(referenceTarget);reference.setClearColor(0x173249,.65);reference.clear();reference.readRenderTargetPixels(referenceTarget,0,0,1,1,clear);
      // The frozen LinesPass packed alphaTest through writeScreenStep even
      // though Three's line-uniform refresher omits it. Replay that actual
      // historical draw input, including RGBA interpolation across the cutoff.
      reference.compile(scene,camera);
      const referenceUniforms=reference.properties.get(object.material).uniforms;
      if(referenceUniforms.alphaTest)referenceUniforms.alphaTest.value=object.material.alphaTest;
      reference.render(scene,camera);reference.readRenderTargetPixels(referenceTarget,0,0,64,64,expected);scene.remove(object);
      // Generic utility variants are shader fixtures, not domain line anchors.
      // Keep their native geometry ids across dynamic writes and restoration.
      const geometry=object.geometry,id=core.geometry(geometry,program),position=geometry.attributes.position.array;
      if(id===null)throw Error(`Utility geometry: ${key}`);
      core.rust.geometry_buffer_sub_data(id,0,0,new Uint8Array(position.buffer,position.byteOffset,position.byteLength));
      const mode=object.isLineSegments?renderer.getContext().LINES:object.isLineLoop?renderer.getContext().LINE_LOOP:renderer.getContext().LINE_STRIP;
      core.rust.geometry_primitive(id,mode,geometry.drawRange.start,true,1);
      renderer.setRenderTarget(target);core.frame.target(target);renderer.setClearColor(0x173249,.65);renderer.clear();
      if(!writeScreenStep(core,0,program,object.material,object,camera,{geometry:{id,count:Math.min(geometry.drawRange.count,geometry.index.count-geometry.drawRange.start)}}))throw Error(`Utility uniforms: ${key}`);
      core.frame.seal(ROLE.CLOUDS,KIND.SCREEN,{steps:1});
      renderer.readRenderTargetPixels(target,0,0,64,64,pixels);
      let differing=0,drawn=0;for(let i=0;i<pixels.length;i++){if(pixels[i]!==expected[i])differing++;if(i%4===0&&(expected[i]!==clear[0]||expected[i+1]!==clear[1]||expected[i+2]!==clear[2]||expected[i+3]!==clear[3]))drawn++;}
      results.push({key,blending:object.material.blending,topology:object.type,stage,differing,drawn,ok:differing===0&&drawn>0});
    };
    for(const fixture of fixtures)compare(fixture,'initial');
    // The same geometry identity must see its changed stream and shortened
    // range. This catches stale journal bytes as well as a source mismatch.
    for(const fixture of fixtures){const position=fixture.object.geometry.attributes.position;position.array[3]+=.13;position.needsUpdate=true;fixture.object.geometry.setDrawRange(fixture.object.geometry.index?1:0,2);compare(fixture,'dynamic');}
    const gl=renderer.getContext(),loss=gl.getExtension('WEBGL_lose_context');
    const wait=name=>new Promise((resolve,reject)=>{const timer=setTimeout(()=>reject(Error(`Utility ${name} timeout`)),30000);renderer.domElement.addEventListener(name,()=>{clearTimeout(timer);resolve();},{once:true});});
    const lost=wait('webglcontextlost');loss.loseContext();await lost;await new Promise(resolve=>setTimeout(resolve,50));
    const restored=wait('webglcontextrestored');loss.restoreContext();await restored;
    for(const fixture of fixtures)compare(fixture,'restored');
    results.push({stage:'ownership',ok:core.shaderPrograms.length===programs&&renderer.battleThreeCalls===0&&renderer.battleCompileCalls===0&&renderer.battleStateCalls===0&&!core.errors.length,programs:core.shaderPrograms.length,errors:core.errors});
    return {ok:results.every(row=>row.ok),results};
  } finally {reference.dispose();referenceTarget.dispose();target.dispose();g.renderer.dispose();g.renderer.domElement.remove();}
}
