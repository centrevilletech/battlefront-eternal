// Generated textures and a two-bone preview exercise menu/spawn ownership with
// no game files. Exact readback after restoration checks the whole retained
// program/geometry/sampler path, including a holomap-style offscreen batch.
import * as THREE from 'three';
import {actorAsset,actorNode,releaseActorSkin} from '../src/actor-resources.js';
import {canvasImage,releaseImage} from '../src/interface/images.js';
import { Batch, loadInterfaceBatch } from '../src/interface/batch.js';
import { interfaceMaterial } from '../src/interface/models.js';
import { renderInterface } from '../src/rust-render/draw-guard.js';
import { requireRustDraws } from '../src/rust-render/draw-guard.js';
import { createRustRender } from '../src/rust-render/index.js';
import { InterfaceSurface } from '../src/interface/surface.js';

export function interfaceFixture(g, assert) {
  const renderer = g.renderer, gl = renderer.getContext();
  const image = rgba => {
    const canvas=document.createElement('canvas');canvas.width=canvas.height=1;
    const context=canvas.getContext('2d');context.putImageData(new ImageData(new Uint8ClampedArray(rgba),1,1),0,0);
    return canvasImage(canvas);
  };
  const texture=image([80,160,240,255]),mask=image([255,255,255,128]);
  const batch = new Batch(), scene = new THREE.Scene();
  const camera = new THREE.OrthographicCamera(0,64,64,0,-10,10);
  const matrix = new THREE.Matrix4();
  batch.begin();
  batch.quad(matrix,2,2,30,30,texture,[0,0,1,1],[1,1,1,1]);
  batch.quad(matrix,34,2,62,30,texture,[0,0,1,1],[1,1,1,1],0,mask);
  batch.end(); scene.add(batch.mesh);
  const target = renderer.resources.interfaceTarget(64);
  const preview = new THREE.Scene();
  const geometry=actorAsset({kind:'rig',positions:[-10,10,0,10,10,0,-10,-10,0,10,-10,0],normals:[0,0,1,0,0,1,0,0,1,0,0,1],uv:[0,1,1,1,0,0,1,0],joints:[0,1,0,0,0,1,0,0,0,1,0,0,0,1,0,0],weights:[.25,.75,0,0,.25,.75,0,0,.25,.75,0,0,.25,.75,0,0],indices:[0,2,1,2,3,1]});
  const model=actorNode(geometry,interfaceMaterial({map:texture,lit:false,blend:false,cutoff:-1}),true);
  const bone = new THREE.Bone(), child = new THREE.Bone();
  bone.add(child); model.add(bone); model.bind(new THREE.Skeleton([bone,child]));
  model.position.set(32,46,0); model.frustumCulled = false;
  child.position.x = 4; preview.add(model);
  const pixels = () => {
    const previous = renderer.getRenderTarget();
    const clear = renderer.getClearColor(new THREE.Color()), alpha = renderer.getClearAlpha();
    renderer.setRenderTarget(target); renderer.setClearColor(0,0); renderer.clear();
    renderInterface(renderer,scene,camera,'spawn-holomap');
    renderInterface(renderer,preview,camera,'classic-spawn');
    g.rust.settle();
    const data = new Uint8Array(64*64*4);
    renderer.readRenderTargetPixels(target,0,0,64,64,data);
    renderer.setRenderTarget(previous);
    renderer.setClearColor(clear,alpha);
    const error = gl.getError(); if(error)throw Error(`Interface GL error ${error}`);
    return data;
  };
  const before = pixels();
  const at = (data,x,y) => [...data.slice((y*64+x)*4,(y*64+x)*4+4)];
  assert('Rust interface draws textured quads into an offscreen target',JSON.stringify(at(before,12,12))===JSON.stringify([80,160,240,255]),at(before,12,12));
  assert('Rust interface applies a separate alpha mask',at(before,44,12).every((v,i)=>Math.abs(v-[40,80,120,128][i])<=1),at(before,44,12));
  assert('Rust interface draws a skinned preview',at(before,36,46).slice(0,3).every((v,i)=>v===[80,160,240][i]),at(before,36,46));
  // A changed quad writes the existing GPU buffer and its recovery description.
  // Fewer quads on the next frame must not replay the previous group's tail.
  batch.begin(); batch.quad(matrix,4,4,30,30,texture,[0,0,1,1],[.5,1,1,1]); batch.end();
  const changed = pixels();
  assert('Rust interface updates changed vertices and drops removed groups',at(changed,12,12)[0]===40&&at(changed,44,12)[3]===0);
  return { pixels, before: changed, dispose() { renderer.resources.disposeEffectTarget(target); releaseActorSkin(g.rust,model.skeleton); releaseImage(texture); releaseImage(mask); } };
}

// The front end constructs its renderer without a battle or its preparation
// hooks. Exercise that lifecycle separately: rebuilding the main battle context
// alone would miss a shell-only dependency on a missing graphics field.
export async function shellInterfaceFixture(assert) {
  await loadInterfaceBatch();
  const renderer = new THREE.WebGLRenderer({alpha:true,antialias:true});
  renderer.setSize(64,64); document.body.append(renderer.domElement);
  const core = await createRustRender({renderer,scene:new THREE.Scene(),warmup(){}});
  requireRustDraws(renderer);
  const surface = new InterfaceSurface(renderer);
  const matrix = surface.anchor(0,0,false);
  const pixels = () => {
    renderer.setClearColor(0,0); renderer.clear();
    surface.drawFrame(batch=>batch.quad(matrix,0,0,1024,surface.height,surface.solid,[0,0,1,1],[.25,.5,.75,1]));
    core.settle();
    const gl=renderer.getContext(),data=new Uint8Array(64*64*4);
    gl.readPixels(0,0,64,64,gl.RGBA,gl.UNSIGNED_BYTE,data);
    if(gl.getError())throw Error('Shell fixture GL error');
    return data;
  };
  try {
    const before=pixels(),center=(32*64+32)*4;
    assert('Standalone Rust shell draws menu quads',before.slice(center,center+4).every((v,i)=>Math.abs(v-[64,128,191,255][i])<=1));
    const canvas=renderer.domElement,loss=renderer.getContext().getExtension('WEBGL_lose_context');
    const event=name=>new Promise((resolve,reject)=>{
      const timer=setTimeout(()=>reject(Error(`Shell ${name} timed out`)),30000);
      canvas.addEventListener(name,()=>{clearTimeout(timer);resolve();},{once:true});
    });
    const lost=event('webglcontextlost');loss.loseContext();await lost;
    await new Promise(resolve=>setTimeout(resolve,100));
    const restored=event('webglcontextrestored');loss.restoreContext();await restored;
    if(core.failure)throw core.failure;
    const after=pixels();
    assert('Standalone shell rebuild preserves exact menu pixels',before.every((v,i)=>v===after[i]));
    assert('Standalone shell never draws through Three',renderer.battleThreeCalls===0);
  } finally { renderer.dispose(); renderer.domElement.remove(); }
}
