// Build one explicit Node-API target. Native world/publication capabilities are
// separate from addon availability; staging never guesses support from dlopen.
import { mkdirSync, copyFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, resolve } from 'node:path';
import { homedir } from 'node:os';
import { ROOT, engineBuild, schemaFingerprint } from './host-contract.mjs';
const target = 'x86_64-unknown-linux-gnu';
if (process.platform !== 'linux' || process.arch !== 'x64' || !process.report.getReport().header.glibcVersionRuntime) throw Error('Node host build requires linux-x64 glibc');
const build = engineBuild(), schema = schemaFingerprint();
const probe = process.argv.includes('--fixtures');
const result = spawnSync('cargo', ['build', '--release', '--locked', '-p', 'bfe-node', '--target', target, ...(probe ? ['--features', 'lifecycle-probe'] : [])], {
  cwd: join(ROOT, 'rust'), stdio: 'inherit',
  env: { ...process.env, BFE_ENGINE_BUILD: build, BFE_SCHEMA_FINGERPRINT: schema,
    RUSTFLAGS: `${process.env.RUSTFLAGS ?? ''} --remap-path-prefix=${ROOT}=engine/ --remap-path-prefix=${homedir()}/.cargo=/cargo` },
});
if (result.error || result.status) throw result.error ?? Error('Node addon build failed');
const output = resolve('.host-addon');
mkdirSync(output, { recursive: true });
copyFileSync(join(ROOT, 'rust/target', target, 'release/libbfe_node.so'), join(output, 'bfe-node.node'));
writeFileSync(join(output, 'target.json'), JSON.stringify({ target: 'linux-x64-gnu', triple: target, lifecycleProbe: probe, napiMinimum: 8, addonABI: 1, storeABI: 1,
  engineBuild: build, schema, nativeWorld: false, nativePublication: false }, null, 2) + '\n');
console.log('Staged Node host addon:', output);

// Native CI records generated world rows and the capabilities of the linked
// simulation, rather than assuming that exposing an addon means world readiness.
if (probe) {
  const {createRequire} = await import('node:module');
  const {readFileSync} = await import('node:fs');
  const world = await import('../src/sim/gen/world.js');
  const addon = createRequire(import.meta.url)(join(output, 'bfe-node.node'));
  const hello = addon.handshake();
  for (const name of ['stepInit', 'step', 'publish', 'readBank', 'close']) {
    if (typeof addon.NativeOwner.prototype[name] !== 'function') throw Error(`Missing native world boundary: ${name}`);
  }
  const metadata = JSON.parse(readFileSync(join(output, 'target.json'), 'utf8'));
  Object.assign(metadata, {addonABI: hello.addonAbi, operationTable: hello.operationTable,
    nativeWorld: hello.nativeWorld, nativePublication: hello.nativePublication});
  writeFileSync(join(output, 'target.json'), JSON.stringify(metadata, null, 2) + '\n');
  const setup = new Float64Array(world.WorldTickSetupLayout.words);
  world.putWorldTickSetup(null, {water: false, waterTime: 0, waterHeight: 0, waterBob: false}, setup, 0, []);
  const tick = new world.WorldTickInputMirror(); tick.dt = 1 / 60; tick.tick = 1; tick.at = 1 / 60; tick.mouseScale = 1;
  for (const key of Object.keys(tick.clock)) tick.clock[key] = 0;
  Object.assign(tick.clock, {hz: 60, dt: 1 / 60, maxSteps: 4, budget: 1, maxDebt: 8});
  const input = new Float64Array(world.WorldTickInputLayout.words);
  world.putWorldTickInput(null, tick, input, 0, []);
  writeFileSync(join(output, 'world-fixtures.json'), JSON.stringify({setup: [...setup], input: [...input], outputWords: world.WorldTickOutputLayout.words}, null, 2) + '\n');
}
