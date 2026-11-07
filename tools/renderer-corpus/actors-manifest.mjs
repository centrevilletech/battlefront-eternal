// Material source coverage crosses quality with every cascade configuration.
// The extra class cases exercise first-person rigs and real AI hero bodies.
export function actorManifest(base, programs) {
 const manifest=structuredClone(base);manifest.family='actors';
 manifest.cases=manifest.cases.filter(c=>!c.contextLoss);delete manifest.contextLossOwner;
 manifest.cases=manifest.cases.map(c=>({...c,query:c.query.replace('heroes=0','heroes=1')}));
 for(const preset of ['classic','balanced','high'])for(const c of manifest.cases.filter(c=>c.id.startsWith(preset+'-'))){
  for(const shadows of ['off','low','medium','high','ultra']){
   if(shadows===manifest.presets[preset].shadows)continue;
   manifest.cases.push({...c,id:c.id+'-shadows-'+shadows,settings:{...c.settings,shadows},tags:[...c.tags,'shadow-configuration']});
  }
 }
 const heroes=new Set(['rep_inf_macewindu','cis_inf_countdooku','imp_inf_darthvader','all_inf_lukeskywalker']);
 for(const preset of ['classic','balanced','high'])for(const [map,className] of [['nab2','cis_inf_droideka'],['end1','imp_inf_dark_trooper'],['nab2','rep_inf_macewindu'],['nab2','cis_inf_countdooku'],['end1','imp_inf_darthvader'],['end1','all_inf_lukeskywalker']]){
  const c=manifest.cases.find(c=>c.id===preset+'-'+map),side=className.split('_')[0];
  const q=new URLSearchParams(c.query),hero=heroes.has(className);
  q.set('side',side);q.set('era',['cis','rep'].includes(side)?'cw':'gcw');
  q.set('unit',hero?({rep:'rep_inf_clone_trooper',cis:'cis_inf_droideka',imp:'imp_inf_storm_trooper',all:'all_inf_soldierjungle'})[side]:className);
  if(hero)q.set('view','3rd');
  manifest.cases.push({...c,id:preset+'-'+map+'-'+className,query:q.toString(),tags:[...new Set([...c.tags.filter(tag=>!hero || tag!=='first-person'),'actor-class',hero?'hero':'first-person'])],...(hero?{actorFocus:className}:{})});
 }
 for(const preset of ['classic','balanced','high'])for(const shadows of ['off','low','medium','high','ultra']){
  const c=manifest.cases.find(c=>c.id===preset+'-nab2-cis_inf_droideka'),q=new URLSearchParams(c.query);q.set('view','3rd');
  manifest.cases.push({...c,id:preset+'-nab2-dissolve-shadows-'+shadows,query:q.toString(),settings:{...c.settings,shadows},tags:[...c.tags.filter(t=>t!=='first-person'),'dissolve'],actorDissolve:true,actorPieces:true});
 }
 for(const preset of ['classic','balanced','high'])for(const shadows of ['off','low','medium','high','ultra']){
  const c=manifest.cases.find(c=>c.id===preset+'-nab2');
  manifest.cases.push({...c,id:preset+'-nab2-wreck-shadows-'+shadows,settings:{...c.settings,shadows},tags:[...c.tags,'prop-wreck'],actorWreck:true});
 }
 for(const preset of ['classic','balanced','high']){
  const c=manifest.cases.find(c=>c.id===preset+'-nab2'),q=new URLSearchParams(c.query);q.set('view','3rd');
  manifest.cases.push({...c,id:preset+'-nab2-shock',query:q.toString(),tags:[...c.tags.filter(t=>t!=='first-person'),'shock'],actorShock:true});
 }
 for(const preset of ['classic','balanced','high'])manifest.cases.find(c=>c.id===preset+'-nab2').actorFlash=true;
 for(const id of ['classic-nab2-shadows-low','balanced-nab2-shadows-medium','high-nab2'])manifest.cases.find(c=>c.id===id).actorSkinnedCutout=true;
 manifest.programs=programs.filter(key=>key.startsWith('actors/') || key.startsWith('post-hud/'));
 return manifest;
}

// The programs a battle has been seen to link. Derived variants (tools/actor-shader-compose.py) are for
// materials no census saw: the smoke fixture links them, but no replay can draw what no battle contains.
export function observedPrograms(layouts) {
 return Object.entries(layouts).filter(([,layout])=>!layout.derived).map(([key])=>key);
}

// `node tools/renderer-corpus/actors-manifest.mjs` rewrites actors-manifest.json from the shared corpus manifest and
// the generated program layouts (tests/actor-corpus.test.js fails when the checked-in file is stale).
import { fileURLToPath, pathToFileURL } from 'node:url';
if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
 const { readFileSync, writeFileSync } = await import('node:fs');
 const { programLayouts } = await import('../../src/render-programs.js');
 const here = new URL('.', import.meta.url);
 const base = JSON.parse(readFileSync(new URL('manifest.json', here)));
 writeFileSync(fileURLToPath(new URL('actors-manifest.json', here)), JSON.stringify(actorManifest(base, observedPrograms(programLayouts)), null, 2) + '\n');
}
