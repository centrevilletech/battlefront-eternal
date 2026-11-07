// Sample only at qualification boundaries. Native capacity fields remain null
// unless the backend supplies them; linear memory is not native heap usage.
import {readFile} from 'node:fs/promises';
import {join} from 'node:path';
export function smaps(text){
  const result={rss:0,pss:0};
  for(const [key,label] of [['rss','Rss'],['pss','Pss']]){const match=new RegExp(`^${label}:\\s+(\\d+) kB$`,'m').exec(text);if(!match)throw Error(`Missing ${label} in smaps`);result[key]=Number(match[1])*1024;}
  return result;
}
export function counters(text){return Object.fromEntries(text.trim().split('\n').filter(Boolean).map(line=>{const [key,value]=line.trim().split(/\s+/);if(!/^\d+$/.test(value))throw Error('Invalid cgroup counter');return [key,Number(value)];}));}
export async function processMemory(pid=process.pid){
  try{return {pid,...smaps(await readFile(`/proc/${pid}/smaps_rollup`,'utf8'))};}
  catch(error){if(error.code==='ENOENT')return {pid,exited:true};throw error;}
}
export async function cgroupMemory(path){
  const read=name=>readFile(join(path,name),'utf8');
  const [current,peak,maximum,events]=await Promise.all(['memory.current','memory.peak','memory.max','memory.events'].map(read));
  return {current:Number(current),peak:Number(peak),maximum:maximum.trim()==='max'?null:Number(maximum),events:counters(events)};
}
export function maximum(previous,sample){
  const next={...previous};for(const [key,value]of Object.entries(sample))if(typeof value==='number'&&Number.isFinite(value))next[key]=Math.max(next[key]??0,value);return next;
}
