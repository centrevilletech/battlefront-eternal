// Qualification inputs never enter the application's settings or backend
// selection. A missing native capability is an error, never a WASM retry.
import {readFile,writeFile,mkdir} from 'node:fs/promises';
import {dirname,resolve} from 'node:path';
import {pathToFileURL} from 'node:url';
export const sleep=ms=>new Promise(r=>setTimeout(r,ms));
export function number(value,name,min,max,integer=false){
  const n=Number(value);
  if(!Number.isFinite(n)||n<min||n>max||(integer&&!Number.isInteger(n)))throw Error(`Invalid ${name}`);
  return n;
}
export function options(args){
  const values={},allowed=new Set('backend addon engine data out raw-dir backend-module world-worker seconds battles clients concurrency ticks stress-ticks map side rotate rotation blackout-seconds shape-up-kbps shape-down-kbps'.split(' '));
  for(const arg of args){const match=/^--([a-z][a-z-]*)=(.*)$/.exec(arg);if(!match)throw Error(`Expected --name=value: ${arg}`);if(!allowed.has(match[1])||Object.hasOwn(values,match[1]))throw Error(`Unknown or duplicate option: ${match[1]}`);values[match[1]]=match[2];}
  const backend=values.backend??'wasm';if(!['wasm','native'].includes(backend))throw Error('Invalid backend');
  return {...values,backend,seconds:number(values.seconds??1800,'seconds',0.01,86400),
    battles:number(values.battles??24,'battles',1,1000,true),clients:number(values.clients??32,'clients',1,64,true),
    concurrency:number(values.concurrency??1,'concurrency',1,4,true),
    'stress-ticks':number(values['stress-ticks']??12000,'stress-ticks',1,10800000,true),
    ticks:number(values.ticks??120,'ticks',1,10800000,true),rotate:number(values.rotate??1,'rotate',0,32,true),
    'blackout-seconds':number(values['blackout-seconds']??3,'blackout-seconds',0.1,30),
    'shape-up-kbps':number(values['shape-up-kbps']??1000,'shape-up-kbps',1,1000000),
    'shape-down-kbps':number(values['shape-down-kbps']??2000,'shape-down-kbps',1,1000000)};
}
export async function output(path,value){
  if(!path)throw Error('An explicit output path is required');
  await mkdir(dirname(resolve(path)),{recursive:true});await writeFile(path,JSON.stringify(value,null,2)+'\n');
}
export async function until(predicate,label,timeout=60000){
  const end=performance.now()+timeout;
  for(;;){const value=await predicate();if(value)return value;if(performance.now()>end)throw Error(`Timeout: ${label}`);await sleep(10);}
}
export async function collect(references,timeout=10000){
  if(typeof global.gc!=='function')throw Error('GC qualification requires --expose-gc');
  // Do not retain dereferenced objects across a collection turn. WeakRef keeps
  // a target alive through the current job, so every poll first yields.
  const end=performance.now()+timeout;
  for(;;){await sleep(10);global.gc();await sleep(10);if(references.every(ref=>ref.deref()===undefined))return true;if(performance.now()>end)return false;}
}
export async function backendProvider({backend,module,engine,manifest}){
  if(backend==='wasm')return (await import('./wasm.mjs')).wasmProvider({engine,manifest});
  if(!module)throw Error('Native qualification requires an explicit harness backend module');
  const provider=await (await import(pathToFileURL(resolve(module)))).createQualificationBackend({engine,manifest});
  if(provider?.capabilities?.nativeWorld!==true||provider.capabilities.nativePublication!==true)throw Error('Backend does not expose native world and publication');
  if(typeof provider.open!=='function')throw Error('Invalid qualification backend');
  return provider;
}
export async function manifest(engine){
  const value=JSON.parse(await readFile(resolve(engine,'host-runtime.json'),'utf8'));
  if(!/^[a-f0-9]{40}$/.test(value.engineRevision)||!/^[a-f0-9]{40}$/.test(value.serverRevision))throw Error('Qualification requires a pinned host bundle');
  return value;
}
