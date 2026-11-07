// Preload only for qualification subprocesses. A native adapter must supply
// the same battle factory as the packaged host; missing support fails closed.
import {resolve} from 'node:path';
import {pathToFileURL,fileURLToPath} from 'node:url';
import {engineBuild} from '../host-contract.mjs';
import {backendProvider,manifest} from './common.mjs';
const engine=resolve(process.env.BFE_QUALIFICATION_ENGINE);
const backend=process.env.BFE_QUALIFICATION_BACKEND;
if(!['wasm','native'].includes(backend))throw Error('Qualification backend is required');
const identity=await manifest(engine);
let factory;
if(backend==='wasm'){
  const {loadHostRuntime}=await import(pathToFileURL(resolve(engine,'../server/src/host-runtime.js')));
  await loadHostRuntime(engine);
  // The capture constructs source-tree entities. Its bridge must import the
  // same classes, rather than a second module graph from the bundle path.
  const source=fileURLToPath(new URL('../../',import.meta.url));
  if(engineBuild(source)!==identity.engineBuild)throw Error('Capture source differs from the paired bundle');
  factory=(await import('../../src/sim/wasm/backend.js')).loadWasmBackend;
}else{
  const provider=await backendProvider({backend,module:process.env.BFE_QUALIFICATION_MODULE,engine,manifest:identity,sourceRoot:fileURLToPath(new URL('../../',import.meta.url))});
  factory=provider.simulationFactory;
}
if(typeof factory!=='function')throw Error('Backend must expose simulationFactory for matrix and publication replay');
globalThis[Symbol.for('bfe.qualification.simulationFactory')]=factory;
