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
