// Index the native shader-catalog example's length-delimited records. Source
// bodies stay in Rust and external captures; the replay index contains hashes.
import fs from 'node:fs';
import { createHash } from 'node:crypto';
export function fingerprint(source) {
  let hash=2166136261;
  for(let i=0;i<source.length;i++)hash=Math.imul(hash^source.charCodeAt(i),16777619);
  return (hash>>>0).toString(16)+':'+source.length;
}
export function shaderIndex(bytes,instanceLayout) {
  const rows=[];
  let at=0;
  while(at<bytes.length){
    const end=bytes.indexOf(10,at);if(end<0)throw Error('Incomplete shader catalog header');
    const [key,kind,declared,vlen,flen,layout]=bytes.subarray(at,end).toString('utf8').split('\t');
    const v=Number(vlen),f=Number(flen);at=end+1;
    if(!Number.isInteger(v)||!Number.isInteger(f)||v<1||f<1||at+v+f>bytes.length)throw Error('Invalid shader catalog length');
    const vertex=bytes.subarray(at,at+v),fragment=bytes.subarray(at+v,at+v+f);at+=v+f;
    rows.push({key,kind,declared:declared==='true',fingerprint:fingerprint(vertex.toString('utf8'))+'/'+fingerprint(fragment.toString('utf8')),
      vertex:createHash('sha256').update(vertex).digest('hex'),fragment:createHash('sha256').update(fragment).digest('hex'),
      ...(declared==='true'&&['Motion','Skinned'].includes(kind)&&instanceLayout?{instanceLayout}:{}),
      attributes:layout.split(',').filter(Boolean).map(s=>{const [name,location]=s.split(':');return [name,Number(location)];})});
  }
  return rows;
}
if(process.argv[1] && new URL(import.meta.url).pathname===process.argv[1]){
  if(!process.argv[2] || !process.argv[3])throw Error('Supply native catalog output and index destination');
  fs.writeFileSync(process.argv[3],JSON.stringify(shaderIndex(fs.readFileSync(process.argv[2]),process.argv[4]?JSON.parse(fs.readFileSync(process.argv[4])):undefined),null,2));
}
