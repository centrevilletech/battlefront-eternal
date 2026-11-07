// The bundle chooses one factory before createGame attaches any kernels.
// An advertised native target must be ready; a refusal ends construction.
import { loadWasmBackend } from './wasm/backend.js';
import { loadNativeSim, nativeIdentity } from './native-backend.js';
import { HOST_SCHEMA } from './host-schema.js';
export function hostSimulation(manifest, target, addon) {
  if (manifest.schema !== HOST_SCHEMA) throw Error('Host schema mismatch');
  const entry=manifest.targets?.find(entry=>entry.target===target);
  if (!entry) return loadWasmBackend;
  if(!addon)throw Error('Native host addon missing');
  const identity=Object.freeze({schema:manifest.schema,engineBuild:manifest.engineBuild,
    addonABI:entry.addonABI??manifest.addonABI,storeABI:manifest.storeABI,
    napiMinimum:entry.napiMinimum??manifest.napiMinimum,operationTable:manifest.operationTable});
  const hello=nativeIdentity(addon,identity);
  if(hello.addonAbi!==2 || hello.nativeWorld!==true || hello.nativePublication!==true || !hello.operationTable)throw Error('Native world operations unavailable');
  if(hello.operationTable!==manifest.operationTable)throw Error('Native operation table mismatch');
  const reserve=Object.freeze({inputWords:8192,outputWords:8192});
  return ()=>loadNativeSim(addon,reserve,identity);
}
