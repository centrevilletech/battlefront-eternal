// Production addons are built against the release libc floor. Probe binaries
// may use the developer's compiler, but cannot enter a production bundle.
import { mkdirSync, copyFileSync, writeFileSync, readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { homedir } from 'node:os';
import { ROOT, engineBuild, schemaFingerprint } from './host-contract.mjs';
const target = 'x86_64-unknown-linux-gnu';
const builder = 'quay.io/pypa/manylinux_2_28_x86_64@sha256:9555ba6c479fa69a2aa80ad1dc57154a60d562b19df159361d9379a02beb1e36';
const probe = process.argv.includes('--fixtures');
const inside = process.argv.includes('--in-builder');
const glibc = process.report.getReport().header.glibcVersionRuntime;
if (process.platform !== 'linux' || process.arch !== 'x64' || !glibc) throw Error('Node host build requires linux-x64 glibc');
function run(command, args, options = {}) {
  const result = spawnSync(command, args, { stdio: 'inherit', ...options });
  if (result.error || result.status) throw result.error ?? Error(`${command} failed`);
}
if (!probe && !inside) {
  // Mount the pinned Rust toolchain and dependency cache, not host libraries.
  // Re-entry verifies libc too, so --in-builder cannot bypass the floor.
  const runtime = process.env.BFE_HOST_CONTAINER_RUNTIME ?? 'podman';
  const cargo = process.env.CARGO_HOME ?? join(homedir(), '.cargo');
  const rustup = process.env.RUSTUP_HOME ?? join(homedir(), '.rustup');
  run(runtime, ['run', '--rm', '--userns=keep-id', '--security-opt=label=disable',
    '-v', `${ROOT}:/engine`, '-v', `${cargo}:/cargo`, '-v', `${rustup}:/rustup`,
    '-v', `${process.execPath}:/node:ro`, '-w', '/engine',
    '-e', 'CARGO_HOME=/cargo', '-e', 'RUSTUP_HOME=/rustup', '-e', `CARGO_BUILD_JOBS=${process.env.CARGO_BUILD_JOBS ?? '8'}`,
    '-e', 'PATH=/cargo/bin:/usr/local/bin:/usr/bin:/bin',
    builder, '/node', 'tools/build-node-host.mjs', '--in-builder']);
} else {
  if (!probe && glibc !== '2.28') throw Error('Production Node addon requires the glibc 2.28 builder');
  const build = engineBuild(), schema = schemaFingerprint();
  run('cargo', ['build', '--release', '--locked', '-p', 'bfe-node', '--target', target, ...(probe ? ['--features', 'lifecycle-probe'] : [])], {
    cwd: join(ROOT, 'rust'),
    env: { ...process.env, ...(!probe ? { CARGO_TARGET_DIR: join(ROOT, 'rust/target/glibc228') } : {}), BFE_ENGINE_BUILD: build, BFE_SCHEMA_FINGERPRINT: schema,
      RUSTFLAGS: `${process.env.RUSTFLAGS ?? ''} --remap-path-prefix=${ROOT}=engine/ --remap-path-prefix=${homedir()}/.cargo=/cargo` },
  });
  const library = join(ROOT, probe ? 'rust/target' : 'rust/target/glibc228', target, 'release/libbfe_node.so');
  // Check the linked binary as well as the builder. A cached dependency must
  // not raise the floor despite compiling the final crate in the right libc.
  const symbols = spawnSync('readelf', ['--version-info', library], { encoding: 'utf8' });
  if (symbols.error || symbols.status) throw symbols.error ?? Error('Cannot inspect Node addon symbols');
  const versions = [...symbols.stdout.matchAll(/GLIBC_(\d+)\.(\d+)/g)].map(match => [Number(match[1]), Number(match[2])]);
  if (!probe && versions.some(([major, minor]) => major > 2 || (major === 2 && minor > 28))) throw Error('Node addon exceeds the glibc 2.28 floor');
  const output = resolve(ROOT, '.host-addon');
  mkdirSync(output, { recursive: true });
  const artifact = join(output, 'bfe-node.node');
  copyFileSync(library, artifact);
  const addon = createRequire(import.meta.url)(artifact);
  const hello = addon.handshake();
  // Metadata describes the actual binary. Staging independently compares it
  // with the required capabilities instead of trusting a build-time promise.
  writeFileSync(join(output, 'target.json'), JSON.stringify({ target: 'linux-x64-gnu', triple: target, lifecycleProbe: probe,
    glibcMinimum: probe ? glibc : '2.28', builder: probe ? null : builder,
    sha256: createHash('sha256').update(readFileSync(artifact)).digest('hex'),
    addonABI: hello.addonAbi, storeABI: hello.storeAbi, napiMinimum: hello.napiMinimum,
    engineBuild: hello.engineBuild, schema: hello.schema, operationTable: hello.operationTable ?? null,
    nativeWorld: hello.nativeWorld, nativePublication: hello.nativePublication }, null, 2) + '\n');
  console.log('Staged Node host addon');

  // Probe builds retain the generated world rows used by native CI. Boundary
  // availability is checked on the linked addon, not inferred from its file.
  if (probe) {
    const world = await import('../src/sim/gen/world.js');
    for (const name of ['stepInit', 'step', 'publish', 'readBank', 'close']) {
      if (typeof addon.NativeOwner.prototype[name] !== 'function') throw Error(`Missing native world boundary: ${name}`);
    }
    const setup = new Float64Array(world.WorldTickSetupLayout.words);
    world.putWorldTickSetup(null, {water: false, waterTime: 0, waterHeight: 0, waterBob: false}, setup, 0, []);
    const tick = new world.WorldTickInputMirror(); tick.dt = 1 / 60; tick.tick = 1; tick.at = 1 / 60; tick.mouseScale = 1;
    for (const key of Object.keys(tick.clock)) tick.clock[key] = 0;
    Object.assign(tick.clock, {hz: 60, dt: 1 / 60, maxSteps: 4, budget: 1, maxDebt: 8});
    const input = new Float64Array(world.WorldTickInputLayout.words);
    world.putWorldTickInput(null, tick, input, 0, []);
    writeFileSync(join(output, 'world-fixtures.json'), JSON.stringify({setup: [...setup], input: [...input], outputWords: world.WorldTickOutputLayout.words}, null, 2) + '\n');
  }

}
