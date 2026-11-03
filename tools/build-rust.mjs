// Builds the WebAssembly made from rust/ and writes it where the game loads
// it: the simulation's module (rust/sim: the collision's queries, the
// soldiers' movement, the pose sampler and the entity store) as base64 in
// src/sim-wasm.js, the audio worklets' DSP (rust/audio) as base64 in
// src/audio-dsp-wasm.js, and the battle renderer (rust/bfe-render, with
// wasm-bindgen's glue) in src/rust-pkg/. All are committed, so building and
// running the game needs no Rust; CI rebuilds them with --check and fails
// when a byte differs (docs/development.md, "The Rust parts").
//
//   node tools/build-rust.mjs [sim] [audio] [renderer]
//                                                  build (all by default)
//                                                  and write them into src/
//   node tools/build-rust.mjs --check [...]        build and compare with
//                                                  src/; exit 1 if any differ
//
// The simulation's module includes the entity store's code generated from
// rust/sim/schema/ (tools/sim-schema.mjs): building writes it first, and
// --check fails when it is stale as when a byte of the module differs.
//
// The same sources give the same bytes on any machine: the build takes the
// Rust that rust/rust-toolchain.toml pins (rustup reads it), Cargo.lock as
// it is (--locked), the wasm-bindgen CLI of the version Cargo.lock locks
// (cargo install wasm-bindgen-cli --version <it> --locked), and writes
// Cargo's home folder as /cargo in the file names panic messages carry.
// What it builds is staged in rust/target/generated/ (laid out as src/).
// CARGO_JOBS=n limits cargo's jobs.
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { homedir } from 'node:os';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { main as simSchema } from './sim-schema.mjs';

const ROOT = fileURLToPath(new URL('../', import.meta.url));
const RUST = join(ROOT, 'rust');
const TARGET = join(RUST, 'target');
const STAGE = join(TARGET, 'generated');
const SRC = join(ROOT, 'src');

// Every file below a folder, '/'-separated, sorted.
function files(dir, base = dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true })
    .flatMap((entry) => entry.isDirectory() ? files(join(dir, entry.name), base) : [relative(base, join(dir, entry.name)).replaceAll('\\', '/')])
    .sort();
}

function run(command, args, options) {
  const result = spawnSync(command, args, { cwd: RUST, ...options });
  if (result.error?.code === 'ENOENT') throw Error(`${command} is not installed (docs/development.md, "The Rust parts")`);
  if (result.error || result.status) throw result.error || Error(`${command} ${args.join(' ')} failed (${result.status})`);
  return result.stdout;
}

// Nothing from the environment may change the bytes: rust-toolchain.toml
// picks the toolchain, Cargo.toml's release profile the code, and these
// flags are the only ones. The modules abort on a panic (the profile's
// native builds unwind, so a crate's tests and bins share one build).
function cargoEnv() {
  const env = { ...process.env };
  for (const name of Object.keys(env)) {
    if (['RUSTUP_TOOLCHAIN', 'RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS', 'CARGO_INCREMENTAL'].includes(name) || name.startsWith('CARGO_PROFILE_')) delete env[name];
  }
  env.CARGO_ENCODED_RUSTFLAGS = `--remap-path-prefix=${process.env.CARGO_HOME || join(homedir(), '.cargo')}=/cargo`;
  env.CARGO_PROFILE_RELEASE_PANIC = 'abort';
  return env;
}

// The rustc that builds them, checked against the pin (another install of
// Rust, or an override, would build other bytes).
function rustcVersion(env) {
  const pinned = /^channel\s*=\s*"([^"]+)"/m.exec(readFileSync(join(RUST, 'rust-toolchain.toml'), 'utf8'))[1];
  const version = run('rustc', ['--version'], { env, encoding: 'utf8' }).trim();
  if (version.split(' ')[1] !== pinned) throw Error(`rust/rust-toolchain.toml pins Rust ${pinned}, but this rustc is ${version}: build with rustup, which installs the pinned one`);
  return version;
}

function checkBindgen() {
  const locked = /name = "wasm-bindgen"\r?\nversion = "([^"]+)"/.exec(readFileSync(join(RUST, 'Cargo.lock'), 'utf8'))[1];
  const found = spawnSync('wasm-bindgen', ['--version'], { encoding: 'utf8' }).stdout?.trim();
  if (found !== `wasm-bindgen ${locked}`) {
    throw Error(`wasm-bindgen ${locked} is needed, the version rust/Cargo.lock locks (${found ? `found ${found}` : 'none found'}): cargo install wasm-bindgen-cli --version ${locked} --locked`);
  }
}

// The crate's .wasm, named after its library (`lib`, when the crate names
// it otherwise than itself).
function cargoBuild(crate, env, lib = crate) {
  run('cargo', ['build', '--release', '--locked', '--target', 'wasm32-unknown-unknown', '--target-dir', TARGET, '-p', crate,
    ...(process.env.CARGO_JOBS ? ['-j', process.env.CARGO_JOBS] : [])], { env, stdio: 'inherit' });
  return join(TARGET, 'wasm32-unknown-unknown', 'release', `${lib.replaceAll('-', '_')}.wasm`);
}

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

// Each artifact: the crate, what it puts in src/ (a file, or a folder), and
// how its staged copy is made.
const ARTIFACTS = {
  sim: {
    crate: 'sim',
    lib: 'bfe_sim',
    path: 'sim-wasm.js',
    // The module's bytes as base64, so the world worker (bundled by Vite),
    // the page and plain Node (the dedicated server, tools/world-hash.mjs)
    // all load it as a JavaScript import, with no wasm asset to serve,
    // unpack or stub.
    stage(wasm, rustc) {
      const bytes = readFileSync(wasm);
      new WebAssembly.Module(bytes);
      const lines = bytes.toString('base64').match(/.{1,120}/g);
      writeFileSync(join(STAGE, this.path), `// Generated by tools/build-rust.mjs from rust/sim (${rustc}); do not edit.
// ${bytes.length} bytes, sha256 ${sha256(bytes)}.
export const SIM_WASM =
${lines.map((line) => `  "${line}"`).join(' +\n')};
`);
    },
  },
  audio: {
    crate: 'audio',
    path: 'audio-dsp-wasm.js',
    // The module's bytes as base64, as sim's: the page compiles it and
    // hands it to the worklets (which cannot fetch), and plain Node loads
    // it the same way for the offline comparisons, so no wasm asset needs
    // serving here either.
    stage(wasm, rustc) {
      const bytes = readFileSync(wasm);
      new WebAssembly.Module(bytes);
      const lines = bytes.toString('base64').match(/.{1,120}/g);
      writeFileSync(join(STAGE, this.path), `// Generated by tools/build-rust.mjs from rust/audio (${rustc}); do not edit.
// ${bytes.length} bytes, sha256 ${sha256(bytes)}.
export const AUDIO_DSP_WASM =
${lines.map((line) => `  "${line}"`).join(' +\n')};
`);
    },
  },
  renderer: {
    crate: 'bfe-render',
    path: 'rust-pkg',
    folder: true,
    // wasm-bindgen's ES module and the .wasm it fetches (by a URL relative
    // to the module, which Vite turns into a hashed asset).
    stage(wasm) {
      checkBindgen();
      const out = join(STAGE, this.path);
      rmSync(out, { recursive: true, force: true });
      run('wasm-bindgen', ['--target', 'web', '--no-typescript', '--out-dir', out, wasm], { stdio: 'inherit' });
      new WebAssembly.Module(readFileSync(join(out, 'bfe_render_bg.wasm')));
    },
  },
};

// An artifact's files (its file, or every file in its folder) below a root:
// src/ or the stage.
const contents = (root, { path, folder }) => (folder ? files(join(root, path)).map((file) => `${path}/${file}`) : existsSync(join(root, path)) ? [path] : []);

export async function main(args) {
  const check = args.includes('--check');
  // (rays and anim, the crates rust/sim took in, name it too.)
  const names = [...new Set(args.filter((arg) => arg !== '--check').map((arg) => (arg === 'rays' || arg === 'anim' ? 'sim' : arg)))];
  for (const name of names) if (!ARTIFACTS[name]) throw Error(`usage: node tools/build-rust.mjs [--check] [${Object.keys(ARTIFACTS).join('] [')}]`);
  const env = cargoEnv();
  const rustc = rustcVersion(env);
  mkdirSync(STAGE, { recursive: true });
  const problems = [];
  for (const name of names.length ? names : Object.keys(ARTIFACTS)) {
    const artifact = ARTIFACTS[name];
    if (name === 'sim') {
      await simSchema(check ? ['--check'] : []);
      if (process.exitCode) problems.push("the store's generated code is stale: node tools/sim-schema.mjs");
    }
    artifact.stage(cargoBuild(artifact.crate, env, artifact.lib), rustc);
    const built = contents(STAGE, artifact);
    if (check) {
      const committed = contents(SRC, artifact);
      for (const file of new Set([...built, ...committed])) {
        if (!committed.includes(file)) problems.push(`src/${file} is missing (a rebuild makes it)`);
        else if (!built.includes(file)) problems.push(`src/${file} is not made by a rebuild`);
        else {
          const [a, b] = [readFileSync(join(SRC, file)), readFileSync(join(STAGE, file))];
          if (!a.equals(b)) problems.push(`src/${file} differs from a rebuild (committed ${a.length} bytes, sha256 ${sha256(a).slice(0, 16)}; rebuilt ${b.length} bytes, sha256 ${sha256(b).slice(0, 16)})`);
        }
      }
    } else {
      rmSync(join(SRC, artifact.path), { recursive: true, force: true });
      cpSync(join(STAGE, artifact.path), join(SRC, artifact.path), { recursive: true });
    }
    for (const file of built) {
      const bytes = readFileSync(join(STAGE, file));
      console.log(`${check ? 'rebuilt' : 'wrote'} src/${file}: ${bytes.length} bytes, sha256 ${sha256(bytes).slice(0, 16)}`);
    }
  }
  if (problems.length) {
    console.error(`\n${problems.join('\n')}\n\nThe committed WebAssembly does not match rust/ (${rustc}). Run \`npm run rust:build\` and commit what it writes, or copy in the rebuilt files from rust/target/generated/ (docs/development.md, "The Rust parts").`);
    process.exitCode = 1;
  } else if (check) console.log(`The committed WebAssembly matches a rebuild (${rustc}).`);
}

// Run from the command line: a failure prints its reason, not a stack.
export async function cli(args) {
  try {
    await main(args);
  } catch (error) {
    console.error(error.message || error);
    process.exitCode = 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) cli(process.argv.slice(2));
