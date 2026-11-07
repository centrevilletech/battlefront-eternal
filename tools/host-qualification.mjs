#!/usr/bin/env node
// Backend choice belongs to this qualification executable only. Production
// selects its one backend from the validated bundle, without a user switch.
import {resolve,join} from 'node:path';
import {tmpdir} from 'node:os';
import {mkdtemp} from 'node:fs/promises';
import {options,output} from './host-qualification/common.mjs';
import {enterProbeNamespace} from './net-probe-shaping.mjs';
const [command,...args]=process.argv.slice(2),config=options(args);
if(!['addon','lifecycle','workers','service','network'].includes(command))throw Error('Use addon, lifecycle, workers, service or network');
if(!config.out)throw Error('An explicit --out=FILE is required');
config.engine=resolve(config.engine??'.host-runtime/engine');
config.data=resolve(config.data??process.env.BFE_CONTENT_DIR??'public');
config['raw-dir']=resolve(config['raw-dir']??await mkdtemp(join(tmpdir(),'bfe-host-qualification-')));
if(command==='network'){
  // The existing helper creates and removes a disposable namespace. A plain
  // host invocation cannot attach impairment to the developer's interfaces.
  if(!args.some(a=>a.startsWith('--shape-up-kbps=')))throw Error('Network qualification requires --shape-up-kbps=1000');
  const forwarded=process.argv.slice(2);
  // sudo resets the content environment, so carry its resolved path as a
  // harness argument into the disposable namespace.
  if(!args.some(a=>a.startsWith('--data=')))forwarded.push(`--data=${config.data}`);
  await enterProbeNamespace(forwarded);
}
let result;
try{
  if(command==='addon')result={passed:true,...await (await import('./host-qualification/addon.mjs')).qualifyAddon(resolve(config.addon??'.host-probe/bfe-node.node'))};
  else if(command==='workers')result={passed:true,backend:config.backend,workers:await (await import('./host-qualification/lifecycle.mjs')).qualifyWorkers(config)};
  else if(command==='lifecycle')result={passed:true,...await (await import('./host-qualification/lifecycle.mjs')).qualifyLifecycle(config)};
  else result=await (await import('./host-qualification/service.mjs')).qualifyService({...config,mode:command==='network'?'network':'memory'});
}catch(error){result={...error.qualification,passed:false,backend:config.backend,error:String(error.stack??error).slice(0,8000)};}
await output(config.out,result);
console.log(JSON.stringify({command,passed:result.passed,out:config.out,capacity:result.capacity}));
if(!result.passed)process.exitCode=1;
