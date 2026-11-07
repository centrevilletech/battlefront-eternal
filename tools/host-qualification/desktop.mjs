// The real Electron executable runs the embedded server. Production clients
// authenticate and receive snapshots before the detached host is stopped.
import assert from 'node:assert/strict';
import {readFile,writeFile,mkdtemp,rm} from 'node:fs/promises';
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
import {execFileSync} from 'node:child_process';
import {createHostController} from '../../electron/host.js';
import {clientFactory} from './clients.mjs';
import {until,sleep} from './common.mjs';
import {run,requireRun} from './process.mjs';
const config=JSON.parse(await readFile(process.argv[2],'utf8')),raw=join(config.raw,config.job.id),engine=config.engine,server=resolve(engine,'../server');
const state=await mkdtemp(join(tmpdir(),'host-desktop-')),clients=[];
const executable=resolve(config.electron??'node_modules/electron/dist/electron');
const version=execFileSync(executable,['-p','JSON.stringify(process.versions)'],{env:{...process.env,ELECTRON_RUN_AS_NODE:'1'},encoding:'utf8'});
const versions=JSON.parse(version);assert.equal(versions.electron.split('.')[0],'44');
const host=createHostController({executable,serverRoot:server,engineRoot:engine,contentRoot:()=>config.data,stateRoot:state,consent:async()=>false});
try{
  const info=await host('start',{name:'Host qualification',mode:'lan',map:'nab2',side:'rep',maxPlayers:8,relay:'never'});
  const connect=await clientFactory(engine,server,state);for(let i=0;i<2;i++)clients.push(await connect(i,info));
  await until(()=>{for(const c of clients)assert.deepEqual(c.errors,[]);return clients.every(c=>c.complete>0&&c.tick>0);},'Electron world snapshots');
  const result={versions,clients:clients.map(c=>({tick:c.tick,complete:c.complete,errors:c.errors})),nodes:[]};
  for(const binary of [config.node22,config.node]){
    assert.ok(binary,'Both Node 22 and 24 executable paths required');
    const r=requireRun(await run(binary,['tools/host-qualification/host-node.mjs',process.argv[2]],{log:join(raw,`node-${result.nodes.length}.log`)}));
    result.nodes.push(JSON.parse(r.tail.trim().split('\n').at(-1)));
  }
  await writeFile(join(raw,'desktop.json'),JSON.stringify(result));
}finally{
  for(const c of clients)await c.close();if((await host('status')).active)await host('stop');
  await until(async()=>!(await host('status')).active,'Electron host shutdown');await sleep(200);await rm(state,{recursive:true,force:true});
}
