import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { checkHostRuntime } from './check-host-runtime.mjs';
import { operationTableFingerprint, schemaFingerprint } from '../tools/host-contract.mjs';
async function bundle(t,native=true) {
  const root = await mkdtemp(join(tmpdir(), 'bfe-package-host-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  const artifact = 'engine/native/linux-x64-gnu/bfe-node.node', bytes = 'synthetic library',identity='a'.repeat(64);
  await mkdir(join(root, artifact, '..'), { recursive: true });
  if(native)await writeFile(join(root, artifact), bytes);
  const packaged=JSON.stringify({type:'module',bfeHostBundle:2,bfeEngineBuild:identity});
  await writeFile(join(root,'engine/package.json'),packaged);
  await writeFile(join(root,'version.json'),JSON.stringify({engineBuild:identity}));
  const manifest = { format: 2, serverAdapterABI: 5, addonABI: 2, storeABI: 1, napiMinimum: 8, engineBuild:identity,operationTable: operationTableFingerprint(),
    targets: native?[{ target: 'linux-x64-gnu', artifact, addonABI: 2, napiMinimum: 8, glibcMinimum: '2.28' }]:[],
    artifacts: { 'engine/package.json':createHash('sha256').update(packaged).digest('hex'),...(native?{[artifact]:createHash('sha256').update(bytes).digest('hex')}:{}) } };
  const save = () => writeFile(join(root, 'engine/host-runtime.json'), JSON.stringify(manifest));
  await save();return {root,manifest,save,artifact,bytes};
}
test('packaged host permits only its advertised hashed native library', async t => {
  const {root,manifest,save,artifact,bytes}=await bundle(t);
  await checkHostRuntime(root);
  await writeFile(join(root, 'engine/extra.node'), bytes);
  await assert.rejects(checkHostRuntime(root), /Unexpected/);
  await rm(join(root, 'engine/extra.node'));
  await writeFile(join(root, artifact), 'corrupt');
  await assert.rejects(checkHostRuntime(root), /hash/);
  await rm(join(root, artifact));
  await assert.rejects(checkHostRuntime(root), /ENOENT/);
  manifest.targets = []; await save(); await checkHostRuntime(root);
});
for(const native of [false,true])for(const field of ['manifest','package','version','manifest and version','manifest and package','missing package identity','missing version identity','unhashed package'])test(`package verification rejects ${native?'native':'WASM'} ${field} build mismatch`,async t=>{
  const {root,manifest,save}=await bundle(t,native),wrong='d'.repeat(64);
  if(field.includes('manifest'))manifest.engineBuild=wrong;
  if(field.includes('package')&&field!=='unhashed package'){
    const pkg={type:'module',bfeHostBundle:2,...(field==='missing package identity'?{}:{bfeEngineBuild:wrong})},content=JSON.stringify(pkg);
    await writeFile(join(root,'engine/package.json'),content);
    manifest.artifacts['engine/package.json']=createHash('sha256').update(content).digest('hex');
  }
  if(field.includes('version'))await writeFile(join(root,'version.json'),JSON.stringify(field==='missing version identity'?{}:{engineBuild:wrong}));
  if(field==='unhashed package')delete manifest.artifacts['engine/package.json'];
  await save();
  await assert.rejects(checkHostRuntime(root),{message:field==='unhashed package'?'Packaged host package hash mismatch':'Packaged host engine build mismatch'});
});
test('operation fingerprint shares schema identity and changes with operation layouts', async t => {
  const root = await mkdtemp(join(tmpdir(), 'bfe-contract-'));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const path of ['tools/sim-schema.mjs', 'rust/sim/schema/operations.mjs', 'src/sim/gen/operations.js', 'rust/sim/src/gen/operations.rs']) {
    await mkdir(join(root, path, '..'), { recursive: true });
    await writeFile(join(root, path), 'original');
  }
  const original = operationTableFingerprint(root);
  assert.equal(original,schemaFingerprint(root));
  for (const path of ['tools/sim-schema.mjs', 'rust/sim/schema/operations.mjs', 'src/sim/gen/operations.js']) {
    await writeFile(join(root, path), 'changed');
    assert.notEqual(operationTableFingerprint(root), original);
    assert.equal(operationTableFingerprint(root),schemaFingerprint(root));
    await writeFile(join(root, path), 'original');
    assert.equal(operationTableFingerprint(root), original);
  }
});
