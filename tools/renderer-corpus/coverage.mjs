// Compare expanded battle census sources with a source-owned family. Unknown
// effect variants remain explicit gaps; a linked program is never evidence of
// ownership merely because another variant of its material was captured.
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
const digest=source=>createHash('sha256').update(source).digest('hex');
export function coverage(directory,familyDirectory) {
  const manifest=JSON.parse(fs.readFileSync(path.join(familyDirectory,'manifest.json'))),owned=new Map(),cases=[],gaps=new Map(),seen=new Map(),other=new Map();
  for(const p of manifest.programs){const hash=digest(fs.readFileSync(path.join(familyDirectory,p.vertex)))+'/'+digest(fs.readFileSync(path.join(familyDirectory,p.fragment)));if(!owned.has(hash))owned.set(hash,[]);owned.get(hash).push(p.key);}
  const marker=/^(particle-depth-copier|fountain-frame-copy)$/;
  const effectsObject=/^(death-trace|destruction-(hot|smoke|arcs)|droideka-(shield|rolling-dust)|arc-(lightning|shock)|jetpack-|rain-(sheets|ground-mist|surface-impacts)|map-(snow|precipitation)|chamber-steam|fountain-|environment-volumes|native-ocean|ocean-|storm-)/;
  const effectsUniform=/^(volumeTime|jetTime|flameTime|wakeTime|trailTime|shockTime|traceTime|sprayTime|wispTime|rainTime|impactTime|flakeCount|stormStrength|cloudMap|waterTime|waterSunDirection|waderRipples|currentViewProjection|previousViewProjection)$/;
  for(const censusDirectory of [].concat(directory))for(const filename of fs.readdirSync(censusDirectory).filter(f=>f.endsWith('.json'))){
    const run=JSON.parse(fs.readFileSync(path.join(censusDirectory,filename)));if(!Array.isArray(run.programs))continue;cases.push({id:run.id,valid:run.valid,error:run.error});
    for(const p of run.programs){const stages=new Map(p.stages.map(s=>[s.stage,s.hash])),hash=stages.get(35633)+'/'+stages.get(35632),keys=owned.get(hash),materials=p.materials || [];
      if(keys){const row=seen.get(hash)||{keys,cases:[]};if(!row.cases.includes(run.id))row.cases.push(run.id);seen.set(hash,row);continue;}
      if(materials.length&&materials.every(m=>marker.test(m.object)))continue;
      const names=p.uniforms.map(u=>u.name.replace(/\[0\]$/,'')),attributes=p.attributes.map(a=>a.name);
      // The post-hud motion compositor shares the view-projection names with
      // object velocities. Its depth reconstruction inputs identify its owner.
      const motionComposite=['inverseJittered','depthMap','velocityMap','currentViewProjection','previousViewProjection'].every(n=>names.includes(n));
      const basicColor=materials.some(m=>m.type==='MeshBasicMaterial'&&!m.skinned&&!m.instanced)&&attributes.includes('color')&&!attributes.some(n=>n.startsWith('foliage'));
      const basicMapped=['diffuse','opacity','map','mapTransform'].every(n=>names.includes(n))&&names.every(n=>['modelMatrix','modelViewMatrix','projectionMatrix','viewMatrix','diffuse','opacity','map','mapTransform','rotation','center','isOrthographic'].includes(n))&&attributes.every(n=>['position','uv','color'].includes(n));
      const water=names.includes('cameraRange')&&(names.includes('tint')||names.includes('reflection')||names.includes('surfaceRange'))||names.includes('mainMap')&&names.includes('maskMix');
      const effect=!motionComposite&&(basicColor||basicMapped||water||materials.some(m=>effectsObject.test(m.object)||m.material==='zero:sky'||m.material==='sky-flyer'||m.material==='sky-flyer-fading')||names.some(n=>effectsUniform.test(n))||attributes.some(n=>['spawnMeta','iShape','iHead','iTail'].includes(n)));
      const row={hash,materials,uniforms:names,attributes,cases:[]};const table=effect?gaps:other,entry=table.get(hash)||row;if(!entry.cases.includes(run.id))entry.cases.push(run.id);table.set(hash,entry);
    }
  }
  return {cases,ownedVariants:manifest.programs.length,ownedSources:owned.size,observedSources:seen.size,observed:[...seen.values()],unobserved:[...owned].filter(([hash])=>!seen.has(hash)).flatMap(([,keys])=>keys),gaps:[...gaps.values()],other:[...other.values()],valid:cases.length>0&&cases.every(c=>c.valid&&!c.error)&&gaps.size===0};
}
if(process.argv[1]&&path.resolve(process.argv[1])===new URL(import.meta.url).pathname){
  const [directory,family,destination]=process.argv.slice(2);if(!directory||!family||!destination)throw Error('Supply census directory, family directory and summary destination');const result=coverage(directory,family);fs.writeFileSync(destination,JSON.stringify(result,null,2));console.log(JSON.stringify({valid:result.valid,cases:result.cases.length,owned:result.ownedVariants,observed:result.observedSources,gaps:result.gaps.length,unobserved:result.unobserved.length}));process.exitCode=result.valid?0:2;
}
