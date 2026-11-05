import { gunzipSync } from 'node:zlib';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

// Compare all RGBA channels, including alpha and one-level MSAA edge changes.
// A pixel counts once even when several channels differ; maxDelta still keeps
// the largest channel difference. No image similarity threshold hides changes.
export function comparePixels(reference, candidate) {
  if(reference.length!==candidate.length)throw Error('Pixel buffer sizes differ');
  let differing=0,maxDelta=0;
  for(let i=0;i<reference.length;i+=4){let delta=0;for(let k=0;k<4;k++)delta=Math.max(delta,Math.abs(reference[i+k]-candidate[i+k]));if(delta){differing++;maxDelta=Math.max(maxDelta,delta);}}
  return {differing,maxDelta,pixels:reference.length/4};
}

// Reference/control are separate application sessions. A noisy control is a
// failed gate, never a tolerance that can conceal a candidate regression.
export function compareCase(directory,id,expected){
  const roles=['reference','control','candidate'];
  const reports=Object.fromEntries(roles.map(role=>[role,JSON.parse(readFileSync(join(directory,role,`${id}.json`)))]));
  const points=[];
  for(const role of roles)if(!reports[role].complete||!reports[role].pass||reports[role].points.length!==expected)throw Error(`${role}: incomplete capture: ${reports[role].error||''}`);
  // Metadata must agree before reading pixels: a visually similar image from
  // the wrong simulation tick is not a valid reference/control/candidate point.
  // The control gets the same exact comparison policy as the candidate.
  for(let k=0;k<expected;k++){
    const meta=roles.map(role=>reports[role].points[k]);
    if(meta.some(m=>m.w!==meta[0].w||m.h!==meta[0].h||m.tick!==meta[0].tick))throw Error(`Point ${k}: dimensions or ticks differ`);
    const buffers=roles.map((role,i)=>gunzipSync(readFileSync(join(directory,role,meta[i].file))));
    const control=comparePixels(buffers[0],buffers[1]),candidate=comparePixels(buffers[0],buffers[2]);
    points.push({k,tick:meta[0].tick,control,candidate,pass:control.differing===0&&candidate.differing===0});
  }
  return {id,complete:points.length===expected,pass:points.every(p=>p.pass),points,restore:reports.candidate.restore,materials:reports.candidate.materials};
}
