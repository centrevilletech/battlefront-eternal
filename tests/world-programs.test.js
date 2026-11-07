import test from 'node:test';
import assert from 'node:assert/strict';
import { programLayouts } from '../src/render-programs.js';
import { ownedWorldProgram } from '../src/rust-render/world-programs.js';

function fixture(s) {
  const lights = [...Array.from({length:s.points},()=>({isPointLight:true,layers:{test:()=>true}})),
    ...Array.from({length:s.shadows},()=>({isDirectionalLight:true,castShadow:true,layers:{test:()=>true}}))];
  const scene={traverseVisible(fn){for(const light of lights)fn(light);}},camera={layers:{}};
  const material={defines:Object.fromEntries(s.defines),side:s.side,map:s.map?{}:null,version:0,
    userData:{bfeFabric:s.fabric,bfeTree:s.tree}};
  const calls=[];
  const core={graphics:{scene,camera,csm:s.csm?{}:null,settings:{get:()=>s.lamps}},renderer:{shadowMap:{enabled:true}},
    rust:{add_owned_program(key){calls.push(key);return calls.length;}}};
  return {core,material,calls};
}

test('every declared world feature set selects its exact variant and caches in its context',()=>{
  for(const [key,layout] of Object.entries(programLayouts)) {
    if(!key.startsWith('world/') || !layout.selector)continue;
    const {core,material,calls}=fixture(layout.selector);
    const entry=ownedWorldProgram(core,material);
    assert.equal(calls[0],key);
    assert.equal(ownedWorldProgram(core,material),entry);
    assert.equal(calls.length,1);
    assert.deepEqual(entry.attributes,layout.attributes);
    const second={...core};delete second.ownedPrograms;delete second.worldMaterialPrograms;
    ownedWorldProgram(second,material);
    assert.equal(calls.length,2,'another context registers its own program');
  }
});

test('an uncensused feature combination fails instead of selecting a near match',()=>{
  const layout=Object.entries(programLayouts).find(([key,p])=>key.startsWith('world/')&&p.selector)[1];
  const {core,material,calls}=fixture({...layout.selector,points:1000});
  assert.throws(()=>ownedWorldProgram(core,material),/Unowned world variant/);
  assert.deepEqual(calls,[]);
});


test('a far material selects the new shadow setup without a material version change',()=>{
  const entries=Object.entries(programLayouts).filter(([,p])=>p.selector);
  const [key,layout]=entries.find(([,p])=>p.selector.csm && !p.selector.shadows && entries.some(([,q])=>JSON.stringify(q.selector)===JSON.stringify({...p.selector,csm:false})));
  const [next]=entries.find(([,p])=>JSON.stringify(p.selector)===JSON.stringify({...layout.selector,csm:false}));
  const {core,material,calls}=fixture(layout.selector);
  ownedWorldProgram(core,material);core.graphics.csm=null;ownedWorldProgram(core,material);
  assert.deepEqual(calls,[key,next]);
});
