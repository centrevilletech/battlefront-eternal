// Replay previously frozen GPU draw fixtures in a fresh context. This is also
// useful for diagnosing serialization without loading another battle.
import fs from 'node:fs';
import { createServer } from 'node:http';
import { gunzipSync } from 'node:zlib';
import { connect } from './battle.mjs';
import { installGLTracking, installGLReplay } from './gl-replay.mjs';
const [input,role,destination]=process.argv.slice(2);
if(!input||!['reference','candidate','control'].includes(role)||!destination)throw Error('Supply fixture JSON, role and result destination');
const payload=JSON.parse(fs.readFileSync(input)),c=await connect();
const result={role,input,pairs:[],valid:false};
// SHA-256 verification requires a secure context. Loopback is trustworthy
// while an isolated about:blank page does not expose SubtleCrypto.
const server=createServer((req,res)=>{res.setHeader('Content-Type','text/html');res.end('<!doctype html><title>Shader fixture</title>');});
await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
try{
  await c.psend('Page.navigate',{url:`http://127.0.0.1:${server.address().port}/`});
  await c.ev(`(${installGLTracking.toString()})([])`);
  await c.ev(`(()=>{const canvas=document.createElement('canvas');canvas.width=1600;canvas.height=900;document.body.append(canvas);const gl=canvas.getContext('webgl2',{antialias:false});if(!gl)throw Error('No fixture WebGL2');gl.getExtension('EXT_color_buffer_float');gl.getExtension('OES_texture_float_linear');gl.getExtension('EXT_texture_filter_anisotropic');for(const name of ['WEBGL_compressed_texture_s3tc','WEBGL_compressed_texture_s3tc_srgb','EXT_texture_compression_bptc'])gl.getExtension(name);window.__bf={graphics:{renderer:{getContext:()=>gl},rust:{}}};window.__corpus={want:false,referenceBuild:${role!=='candidate'},blobs:{}};return 1;})()`);
  if(role==='candidate'){
    const root=new URL('../../src/rust-pkg/',import.meta.url),js=fs.readFileSync(new URL('bfe_render.js',root),'base64'),wasm=fs.readFileSync(new URL('bfe_render_bg.wasm',root),'base64');
    await c.ev(`(async()=>{const m=await import('data:text/javascript;base64,${js}');m.initSync({module:Uint8Array.from(atob('${wasm}'),c=>c.charCodeAt(0))});__bf.graphics.rust.rust=new m.BfeRenderer(__bf.graphics.renderer.getContext());return 1;})()`);
  }
  await c.ev(`(${installGLReplay.toString()})()`);
  for(const [key,blob]of Object.entries(payload.blobs)){
    const raw=fs.readFileSync(blob.file),bytes=(blob.compression==='gzip'?gunzipSync(raw):raw).toString('base64');
    await c.ev(`(()=>{const bytes=Uint8Array.from(atob('${bytes}'),c=>c.charCodeAt(0));window.__corpus.blobs[${JSON.stringify(key)}]=new window[${JSON.stringify(blob.type)}](bytes.buffer);return 1;})()`);
  }
  for(const row of payload.records.filter(row=>row.kind==='gl-effect'))result.pairs.push(await c.ev(`window.__corpus.replayGL(${JSON.stringify(row)},window.__corpus.blobs)`));
  result.valid=result.pairs.length>0&&result.pairs.every(p=>p.differing===0);
}catch(e){result.error=String(e.stack||e);}finally{fs.writeFileSync(destination,JSON.stringify(result,null,2));c.close();server.close();}
console.log(JSON.stringify(result));process.exitCode=result.valid?0:2;
