// The repeat reference defines an amplitude envelope at each channel. A noisy
// control never grants a blanket differing-pixel allowance to the candidate.
import fs from 'node:fs';
import { join } from 'node:path';
import { gunzipSync } from 'node:zlib';
import { pathToFileURL } from 'node:url';
export function compare(a,b,c) {
  if(a.length!==b.length || a.length!==c.length || a.length%4)throw Error('Incompatible stage buffers');
  let differing=0,control=0,residual=0,stableResidual=0,maxDelta=0,maxExcess=0;
  for(let i=0;i<a.length;i+=4){let d=0,n=0,e=0;
    for(let j=0;j<4;j++){const delta=Math.abs(a[i+j]-b[i+j]),noise=Math.abs(a[i+j]-c[i+j]);d=Math.max(d,delta);n=Math.max(n,noise);e=Math.max(e,delta-noise);}
    if(d)differing++;if(n)control++;if(e>0){residual++;if(!n)stableResidual++;maxExcess=Math.max(maxExcess,e);}maxDelta=Math.max(maxDelta,d);
  }
  return {pixels:a.length/4,differing,control,residual,stableResidual,maxDelta,maxExcess};
}
export function compareCase(out,id,expected,bound=null) {
  if(expected!==undefined)return compareFrameCase(out,id,expected,bound);
  const roles=['reference','candidate','control'];
  const runs=Object.fromEntries(roles.map(role=>[role,JSON.parse(fs.readFileSync(`${out}/${id}-${role}/capture.json`))]));
  const report={id,valid:roles.every(r=>runs[r].valid),complete:roles.every(r=>runs[r].points.length===4),points:[],runs,shaderPrograms:runs.candidate.shaderPrograms || []};
  for(let k=0;k<runs.reference.points.length;k++){
    const points=roles.map(r=>runs[r].points[k]);
    const point={k,valid:points.every(Boolean) && points.every(p=>p.tick===points[0].tick),stages:{}};
    for(const name of Object.keys(points[0].stages)){
      const stages=points.map(p=>p?.stages[name]);
      if(stages.some(s=>!s) || stages.some(s=>s.w!==stages[0].w || s.h!==stages[0].h || s.bytes!==stages[0].bytes)){point.valid=false;point.stages[name]={error:'stage layout differs'};continue;}
      const buffers=roles.map((r,i)=>{const raw=fs.readFileSync(stages[i].file),buf=stages[i].compression==="gzip"?gunzipSync(raw):raw;return stages[i].bytes===2?new Uint16Array(buf.buffer,buf.byteOffset,buf.length/2):buf;});
      point.stages[name]=compare(...buffers);
    }
    report.points.push(point);
  }
  report.valid &&= report.points.every(p=>p.valid);
  report.frameExact=report.valid && report.points.every(p=>Object.values(p.stages).every(s=>s.differing===0));
  report.baselinePass=report.valid && report.complete && report.points.every(p=>Object.values(p.stages).every(s=>s.residual===0));
  report.pairs=runs.candidate.points.flatMap(p=>p.replay || []);
  report.selfPairs=runs.reference.points.flatMap(p=>p.selfReplay || []);
  report.controlPairs=runs.control.points.flatMap(p=>p.replay || []);
  report.pass=report.valid && report.complete && report.pairs.length>0 && report.pairs.length===report.selfPairs.length && report.pairs.length===report.controlPairs.length && [...report.pairs,...report.selfPairs,...report.controlPairs].every(p=>p.differing===0);
  fs.writeFileSync(`${out}/${id}.json`,JSON.stringify(report,null,2));
  // Retail-derived buffers are transient; keep only compact metrics.
  for(const role of roles)for(const point of runs[role].points){
    for(const stage of Object.values(point.stages))fs.unlinkSync(stage.file);
    if(point.replayFile){const payload=JSON.parse(fs.readFileSync(point.replayFile));for(const blob of Object.values(payload.blobs))fs.unlinkSync(blob.file);fs.unlinkSync(point.replayFile);}
  }
  return report;
}
if(process.argv[1] && import.meta.url===pathToFileURL(process.argv[1]).href){const report=compareCase(process.argv[2],process.argv[3]);console.log(JSON.stringify({id:report.id,valid:report.valid,frameExact:report.frameExact,pass:report.pass}));process.exitCode=report.pass?0:3;}

export function comparePixels(reference, candidate) {
  if(reference.length!==candidate.length)throw Error('Pixel buffer sizes differ');
  let differing=0,maxDelta=0;
  for(let i=0;i<reference.length;i+=4){let delta=0;for(let k=0;k<4;k++)delta=Math.max(delta,Math.abs(reference[i+k]-candidate[i+k]));if(delta){differing++;maxDelta=Math.max(maxDelta,delta);}}
  return {differing,maxDelta,pixels:reference.length/4};
}

// Reference/control are separate application sessions. A noisy control is a
// failed gate, never a tolerance that can conceal a candidate regression.
// A case may name a measured bound (expectations.json). It applies to that
// candidate only; exceeding its pixel count or channel delta still fails.
export function compareFrameCase(directory,id,expected,bound=null){
  const roles=['reference','control','candidate'];
  const reports=Object.fromEntries(roles.map(role=>[role,JSON.parse(fs.readFileSync(join(directory,role,`${id}.json`)))]));
  const points=[];
  for(const role of roles)if(!reports[role].complete||!reports[role].pass||reports[role].points.length!==expected)throw Error(`${role}: incomplete capture: ${reports[role].error||''}`);
  // Metadata must agree before reading pixels: a visually similar image from
  // the wrong simulation tick is not a valid reference/control/candidate point.
  // The control gets the same exact comparison policy as the candidate.
  for(let k=0;k<expected;k++){
    const meta=roles.map(role=>reports[role].points[k]);
    if(meta.some(m=>m.w!==meta[0].w||m.h!==meta[0].h||m.tick!==meta[0].tick))throw Error(`Point ${k}: dimensions or ticks differ`);
    if(meta.some(m=>m.renderFrame!==meta[0].renderFrame||m.rendererFrame!==meta[0].rendererFrame))throw Error(`Point ${k}: render phases differ`);
    const buffers=roles.map((role,i)=>gunzipSync(fs.readFileSync(join(directory,role,meta[i].file))));
    const control=comparePixels(buffers[0],buffers[1]),candidate=comparePixels(buffers[0],buffers[2]);
    const bounded=Boolean(bound)&&candidate.differing>0&&candidate.differing<=bound.maxPixels&&candidate.maxDelta<=bound.maxDelta;
    points.push({k,tick:meta[0].tick,control,candidate,bounded,pass:control.differing===0&&(candidate.differing===0||bounded)});
  }
  return {id,complete:points.length===expected,pass:points.every(p=>p.pass),points,restore:reports.candidate.restore,materials:reports.candidate.materials};
}
