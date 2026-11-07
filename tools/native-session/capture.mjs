// Full native session archive. Retail-derived bytes stay on the execution VM.
import fs from 'node:fs';
import {setupView} from './view.mjs';
import crypto from 'node:crypto';
import {execFileSync} from 'node:child_process';
import {connect,loadBattle} from '../renderer-corpus/battle.mjs';
import {expand,readManifest} from '../renderer-corpus/manifest.mjs';
const out=process.argv[2];if(!out)throw Error('usage: capture.mjs OUTPUT');fs.mkdirSync(out,{recursive:true});
const api=[JSON.parse(fs.readFileSync(new URL('./renderer-api.json',import.meta.url))),...JSON.parse(fs.readFileSync(new URL('./presentation-api.json',import.meta.url)))];
const resource=[['retain_sample',['u32']],['world_fallback',['bool']],['canopy_state',['u32','pixels']],['update_canopy_state',['u32','pixels']],['source',['&str']],['source_desc',['u32','&str']],['source_pixels',['u32','pixels']],['source_png',['u32','encoded','pixels']],['source_canvas',['u32','canvas']],['source_blocks',['u32','Vec<u8>','&str']],['release_source',['u32']],['sample',['u32','&str']],['sampling',['u32','&str']],['dispose_sample',['u32']],['upload',['u32']],['target',['&str']],['resize_target',['u32','&str']],['dispose_target',['u32']],['bind',['i32','u32','i32','i32','i32','i32','i32','i32','i32','i32','i32','bool']],['resolve',['u32']],['copy',['u32','u32','u32']],['mipmap',['u32']]];
api.push({class:'gpuresources',banks:[],methods:resource.map(([name,args])=>({name,args:args.map((t,i)=>['a'+i,t])}))});
api.push({class:'timeline',banks:[],methods:[['push',[]],['present',[]],['reset',['f64']],['remove',['f64']]].map(([name,args])=>({name,args:args.map((t,i)=>['a'+i,t])}))});
const schemaText=fs.readFileSync(process.env.NATIVE_SCHEMA,'utf8').trim();
const lengths=JSON.parse(schemaText);
const apiHash=crypto.createHash('sha256').update(fs.readFileSync(new URL('./renderer-api.json',import.meta.url))).update(fs.readFileSync(new URL('./presentation-api.json',import.meta.url))).update(schemaText).digest('hex');
const extraSrc=fs.readFileSync(new URL('./record.js',import.meta.url),'utf8').replace('__NATIVE_API__',JSON.stringify(api)).replace('__NATIVE_LENGTHS__',JSON.stringify(lengths));
const frames=Number(process.env.FRAMES||4),warm=Number(process.env.WARM||1);if(!Number.isInteger(frames)||frames<1||frames>240||!Number.isFinite(warm)||warm<1||warm>60)throw Error('invalid session span');
const c=await connect(),test=expand(readManifest()).cases.find(c=>c.id===(process.env.CASE||'classic-nab2'));if(!test)throw Error('unknown case');
const loaded=await loadBattle(c,{settings:test.settings,query:test.query,warm,extraSrc});console.log('loaded',loaded);if(loaded.error)throw Error(loaded.error);
const appliedSettings=await c.ev('({...__bf.settings.values})');
// Desktop builds read their profile file through IPC; localStorage alone does
// not select a corpus preset there. Never label a default frame as Enhanced.
for(const key of ['upscaler','anisotropy','bumpMapping','renderScale','antialiasing','shadows','textureCompression','waterQuality','terrainDetail','treeDetail','environmentEffects','softParticles','bloom','softFocus','vehicleMotionBlur'])if(JSON.stringify(appliedSettings[key])!==JSON.stringify(test.settings[key]))throw Error('Preset not applied: '+key+'; configure BFE_SETTINGS_FILE for the desktop profile');
await setupView(c,test);
const summary=await c.ev(`(async()=>{const s=__nativeSession,b=__bf,g=b.graphics,gl=b.renderer.getContext();s.calls.push({kind:'checkpoint',index:0});for(let i=0;i<${frames};i++){b.controls.hold('KeyW');await b.lockstep(1/60,true);}b.controls.down.delete('KeyW');s.calls.push({kind:'checkpoint',index:1});const pixels=new Uint8Array(gl.drawingBufferWidth*gl.drawingBufferHeight*4);gl.bindFramebuffer(gl.FRAMEBUFFER,null);gl.readPixels(0,0,gl.drawingBufferWidth,gl.drawingBufferHeight,gl.RGBA,gl.UNSIGNED_BYTE,pixels);s.blobs.push(pixels);const reference=s.blobs.length-1;const main=s.owners.find(o=>o.kind==='bferenderer'&&o.ptr===g.rust.rust.__wbg_ptr);if(!main)throw Error('main renderer not found');const requests=new Map();const request=(url,png)=>{if(!url)return;const path=new URL(url,location.href).pathname.slice(1);if(!['maps/','sides/','common/'].some(prefix=>path.startsWith(prefix)))throw Error('asset outside prepared content: '+path);requests.set(path,{path,png});};for(const entry of g.renderer.resources.samples){const texture=entry.texture.deref();if(!texture)continue;const image=g.renderer.resources.images.get(texture.source);request(image?.png?.url||texture.userData.png,!!image?.png);}for(const url of b.hud?.textures?.keys?.()||[])request(url,true);const result={...s.export(),textureRequests:[...requests.values()],context:main.context,renderer:main.id,size:[gl.drawingBufferWidth,gl.drawingBufferHeight],reference,blobLengths:s.blobs.map(b=>b.length)};s.packed=new Uint8Array(result.blobLengths.reduce((a,b)=>a+b,0));s.depth++;let at=0;for(const b of s.blobs){s.packed.set(b,at);at+=b.length;}s.depth--;s.blobs=[];return result;})()`);
if(summary.errors.length)console.log('coverage errors',summary.errors);
const fd=fs.openSync(out+'/blobs.bin','w'),spans=[];let offset=0;
for(const length of summary.blobLengths){spans.push([offset,length]);offset+=length;}

for(let at=0;at<offset;at+=4<<20){const encoded=await c.ev(`(()=>{const u=__nativeSession.packed.subarray(${at},${at+(4<<20)});let s='';for(let k=0;k<u.length;k+=32768)s+=String.fromCharCode.apply(null,u.subarray(k,k+32768));return btoa(s);})()`);fs.writeSync(fd,Buffer.from(encoded,'base64'));}
fs.closeSync(fd);
// Native geometry and decoded image bytes replace matching recorded arguments.
// Only argument blobs are candidates; expected framebuffers stay isolated.
const map=test.query.match(/map=([^&]+)/)?.[1];
const catalog=JSON.parse(execFileSync((process.env.BFE_CLIENT_BIN||'rust/target/debug')+'/session-assets',[process.env.BFE_CONTENT_DIR||(()=>{throw Error('BFE_CONTENT_DIR is required');})(),map,test.query,String(summary.capabilities?.[summary.context]||0),JSON.stringify(summary.textureRequests),String(appliedSettings.textureCompression)],{encoding:'utf8',maxBuffer:16<<20}));
const bytes=fs.readFileSync(out+'/blobs.bin');let nativeAssets=0,nativeBytes=0;
for(const call of summary.calls)if(call.kind==='call')for(const a of call.args){if(a&&typeof a==='object'&&Number.isInteger(a.blob)){const [at,len]=spans[a.blob],hash=crypto.createHash('sha256').update(bytes.subarray(at,at+len)).digest('hex');if(catalog[hash]){a.asset=catalog[hash];a.sha256=hash;delete a.blob;nativeAssets++;nativeBytes+=len;}}}
for(const v of summary.verification){const [at,len]=spans[v.data.blob];v.sha256=crypto.createHash('sha256').update(bytes.subarray(at,at+len)).digest('hex');delete v.data;}
summary.settings=appliedSettings;summary.nativeAssets=nativeAssets;summary.nativeBytes=nativeBytes;
delete summary.blobLengths;
summary.apiHash=apiHash;summary.blobs=spans;summary.map=test.query.match(/map=([^&]+)/)?.[1];summary.query=test.query;summary.case=test.id;
fs.writeFileSync(out+'/session.json',JSON.stringify(summary));
console.log(JSON.stringify({version:summary.version,battleFrames:summary.calls.filter(c=>c.kind==="call"&&c.owner===summary.renderer&&c.name==="battle_frame").length,case:summary.case,calls:summary.calls.length,owners:summary.owners.length,bytes:offset,context:summary.context,externalDraws:summary.externalDraws,errors:summary.errors,settings:summary.settings,passDraws:summary.passDraws[summary.renderer]}));
c.close();
