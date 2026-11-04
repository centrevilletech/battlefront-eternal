// The repeat reference defines an amplitude envelope at each channel. A noisy
// control never grants a blanket differing-pixel allowance to the candidate.
import fs from 'node:fs';
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
export function compareCase(out,id) {
  const roles=['reference','candidate','control'];
  const runs=Object.fromEntries(roles.map(role=>[role,JSON.parse(fs.readFileSync(`${out}/${id}-${role}/capture.json`))]));
  const report={id,valid:roles.every(r=>runs[r].valid),complete:roles.every(r=>runs[r].points.length===4),points:[],runs,shaderPrograms:runs.candidate.shaderPrograms || []};
  for(let k=0;k<runs.reference.points.length;k++){
    const points=roles.map(r=>runs[r].points[k]);
    const point={k,valid:points.every(Boolean) && points.every(p=>p.tick===points[0].tick),stages:{}};
    for(const name of Object.keys(points[0].stages)){
      const stages=points.map(p=>p?.stages[name]);
      if(stages.some(s=>!s) || stages.some(s=>s.w!==stages[0].w || s.h!==stages[0].h || s.bytes!==stages[0].bytes)){point.valid=false;point.stages[name]={error:'stage layout differs'};continue;}
      const buffers=roles.map((r,i)=>{const buf=fs.readFileSync(stages[i].file);return stages[i].bytes===2?new Uint16Array(buf.buffer,buf.byteOffset,buf.length/2):buf;});
      point.stages[name]=compare(...buffers);
    }
    report.points.push(point);
  }
  report.valid &&= report.points.every(p=>p.valid);
  report.exact=report.valid && report.points.every(p=>Object.values(p.stages).every(s=>s.differing===0));
  report.pass=report.valid && report.complete && report.points.every(p=>Object.values(p.stages).every(s=>s.residual===0));
  fs.writeFileSync(`${out}/${id}.json`,JSON.stringify(report,null,2));
  // Retail-derived buffers are transient; keep only compact metrics.
  for(const role of roles)for(const point of runs[role].points)for(const stage of Object.values(point.stages))fs.unlinkSync(stage.file);
  return report;
}
if(process.argv[1] && import.meta.url===pathToFileURL(process.argv[1]).href){const report=compareCase(process.argv[2],process.argv[3]);console.log(JSON.stringify({id:report.id,valid:report.valid,exact:report.exact,pass:report.pass}));process.exitCode=report.pass?0:3;}
