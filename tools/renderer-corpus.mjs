// Run immutable builds in separate seeded sessions, including a reference /
// reference noise control. The runner owns browser launch and slot leasing.
import { readFileSync, mkdirSync, writeFileSync, unlinkSync } from 'node:fs';
import { resolve } from 'node:path';
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { compareCase } from './renderer-corpus/compare.mjs';
// Fingerprint the case definitions separately from either build. Captures
// from different manifests must not accidentally be presented as one matrix.
const bytes=readFileSync(new URL('./renderer-corpus/manifest.json',import.meta.url)),manifest=JSON.parse(bytes);
// Named, explained per-case bounds. Every case absent from this file is exact.
const expectationBytes=readFileSync(new URL('./renderer-corpus/expectations.json',import.meta.url)),expectations=JSON.parse(expectationBytes).bounded;
for(const id of Object.keys(expectations))if(!manifest.cases.some(c=>c.id===id))throw Error(`Bounded case ${id} is not in the corpus`);
const args=process.argv.slice(2),arg=(name,fallback)=>args.includes(name)?args[args.indexOf(name)+1]:fallback;
const runner=arg('--runner',process.env.BFE_CORPUS_RUNNER),against=arg('--against');
if(!runner||!against)throw Error('Supply --runner executable and --against frozen reference dist');
const out=resolve(arg('--out','renderer-corpus-out')),dist=resolve(arg('--dist','dist-desktop'));
const jobs=Number(arg('--jobs','1'));
if(!Number.isInteger(jobs)||jobs<1)throw Error('--jobs must be a positive integer');
const selected=arg('--cases','').split(',').filter(Boolean),cases=manifest.cases.filter(c=>!selected.length||selected.includes(c.id));
if(!cases.length||selected.some(id=>!cases.some(c=>c.id===id)))throw Error('Unknown corpus case');
mkdirSync(out,{recursive:true});
const results=[];let next=0;
// The executable owns three independent application sessions and slot leasing.
// Its exit status is part of the result even if old capture files are present.
async function worker(){for(;;){const c=cases[next++];if(!c)return;const directory=resolve(out,c.id);mkdirSync(directory,{recursive:true});
 const rc=await new Promise(done=>{const child=spawn(runner,[dist,c.id,directory,resolve(against)],{stdio:'inherit',env:{...process.env,BFE_CORPUS_CASE:c.id}});child.on('error',e=>{console.error(e);done(1);});child.on('exit',code=>done(code??1));});
 let result;try{result=compareCase(directory,c.id,manifest.captures,expectations[c.id]);}catch(error){result={id:c.id,pass:false,complete:false,error:String(error.message)};}
 result.rc=rc;result.pass&&=rc===0;results.push(result);writeFileSync(resolve(directory,'comparison.json'),JSON.stringify(result,null,2));console.log(JSON.stringify(result));
 // Only discard pixels after recording a successful exact comparison. Failed
 // cases retain their evidence for diagnosis; metadata and hashes always stay.
 if(result.pass && args.includes('--discard-raw'))for(const role of ['reference','control','candidate']){
  const report=JSON.parse(readFileSync(resolve(directory,role,`${c.id}.json`)));
  for(const point of report.points){
   if(point.file!==`${c.id}-${report.points.indexOf(point)}.rgba.gz`)throw Error('Unexpected capture filename');
   unlinkSync(resolve(directory,role,point.file));
  }
 }

}}
await Promise.all(Array.from({length:Math.min(jobs,cases.length)},worker));results.sort((a,b)=>a.id.localeCompare(b.id));
// A selected subset can pass without claiming the full corpus is complete.
// Persist both facts so a partial diagnostic run cannot stand in for a gate.
const summary={version:2,corpusHash:createHash('sha256').update(bytes).digest('hex'),expectationsHash:createHash('sha256').update(expectationBytes).digest('hex'),dist,against:resolve(against),complete:cases.length===manifest.cases.length,pass:results.length===cases.length&&results.every(r=>r.pass),cases:results};
writeFileSync(resolve(out,'summary.json'),JSON.stringify(summary,null,2));process.exitCode=summary.pass?0:1;
