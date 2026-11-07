// Repeat the browser reference without instrumentation for a differing case.
// Only a digest leaves the page; raw pixels stay on the execution host.
import {setupView} from './view.mjs';
import {connect,loadBattle} from '../renderer-corpus/battle.mjs';
import {expand,readManifest} from '../renderer-corpus/manifest.mjs';
const frames=Number(process.env.FRAMES||4),warm=Number(process.env.WARM||1);
const c=await connect(),test=expand(readManifest()).cases.find(c=>c.id===(process.env.CASE||'classic-nab2'));if(!test)throw Error('unknown case');
const load=await loadBattle(c,{settings:test.settings,query:test.query,warm});if(load.error)throw Error(load.error);await setupView(c,test);
const hash=await c.ev(`(async()=>{const b=__bf;for(let i=0;i<${frames};i++){b.controls.hold('KeyW');await b.lockstep(1/60,true);}b.controls.down.delete('KeyW');const gl=b.renderer.getContext(),pixels=new Uint8Array(gl.drawingBufferWidth*gl.drawingBufferHeight*4);gl.bindFramebuffer(gl.FRAMEBUFFER,null);gl.readPixels(0,0,gl.drawingBufferWidth,gl.drawingBufferHeight,gl.RGBA,gl.UNSIGNED_BYTE,pixels);return Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',pixels)),b=>b.toString(16).padStart(2,'0')).join('');})()`);
console.log(JSON.stringify({case:test.id,reference:hash}));c.close();
