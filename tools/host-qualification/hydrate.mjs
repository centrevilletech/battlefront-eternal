// Called under the worker's bundle lock. Re-extracting over a running world
// can briefly remove dependency files, so validate once and never rewrite a
// complete matching bundle. A changed archive gets a fresh validation.
import {createHash} from 'node:crypto';
import {readFile,writeFile} from 'node:fs/promises';
import {resolve,dirname,join,basename} from 'node:path';
import {execFileSync} from 'node:child_process';
const [archive,engine,receipt]=process.argv.slice(2);
const hash=bytes=>createHash('sha256').update(bytes).digest('hex');
const digest=hash(await readFile(archive));
try{if((await readFile(receipt,'utf8')).trim()===digest)process.exit(0);}catch{}
let complete=true;
try{
  const expected=execFileSync('tar',['-xOf',archive,`${basename(dirname(engine))}/engine/host-runtime.json`],{maxBuffer:16777216});
  const actual=await readFile(join(engine,'host-runtime.json'));
  if(hash(actual)!==hash(expected))throw Error('Bundle identity differs');
  const manifest=JSON.parse(actual);
  for(const [file,digest] of Object.entries(manifest.artifacts))if(hash(await readFile(resolve(engine,'..',file)))!==digest){complete=false;break;}
}catch{complete=false;}
if(!complete)execFileSync('tar',['-xf',archive,'-C',dirname(dirname(engine))]);
await writeFile(receipt,digest+'\n');
