import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {spawnSync} from 'node:child_process';
import {WorldCorpus} from '../tools/world-corpus.mjs';

// A synthetic child verifies the corpus protocol without claiming that the
// unavailable native World or its lifecycle scenarios have been exercised.
test('native corpus transport retains observation order, numeric spelling and empty packet bytes',t=>{
  const root=fs.mkdtempSync(path.join(os.tmpdir(),'world-native-'));
  t.after(()=>fs.rmSync(root,{recursive:true,force:true}));
  const directory=path.join(root,'case'),driver=path.join(root,'driver.mjs');
  const corpus=new WorldCorpus({directory});
  corpus.readAsset('class.json',()=>Buffer.from('17'));
  corpus.begin({battle:0,index:0,now:1,input:{},fire:null,mount:[],acks:[]});
  corpus.finishFrame();corpus.finish({ticks:1});
  fs.writeFileSync(driver,`import readline from 'node:readline';
let admission;
for await(const line of readline.createInterface({input:process.stdin})) {
  const row=JSON.parse(line);
  if(row.kind==='admit'){admission=row;continue;}
  if(admission.assets[0].hex!=='3137')throw Error('cold bytes changed');
  console.log(JSON.stringify({tick:1,stateJson:'{"z":1.0,"a":-0.0}',digestJson:'{"units":[1.0,null],"hulls":[],"rounds":[]}',
    packets:[{channel:'state',bytes:'AQID'},{channel:'events',text:'✓'},{channel:'empty',bytes:''}]}));
}
`,{mode:0o755});
  const run=spawnSync(process.execPath,['tools/world-native.mjs','--corpus',directory,'--driver',process.execPath,'--driver-arg',driver],{encoding:'utf8'});
  assert.equal(run.status,0,run.stderr);
  assert.equal(run.stdout,'1 5e8b531544380ddb bc6d97ae2241e719 165c6b1d71344dfd\n');
});
