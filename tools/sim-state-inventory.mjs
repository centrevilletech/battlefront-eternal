// Audit lifetime roots and host seams before changing simulation ownership.
// This reads source text only; it does not instantiate a simulation module.
import fs from 'node:fs';
import crypto from 'node:crypto';
const root = new URL('../', import.meta.url);
function files(dir) {
  return fs.readdirSync(new URL(dir, root), { withFileTypes: true }).flatMap((entry) => {
    const name = `${dir}/${entry.name}`;
    return entry.isDirectory() ? files(name) : entry.name.endsWith('.rs') ? [name] : [];
  });
}
const patterns = {
  state: /\bstatic\s+(?:mut\s+)?[A-Za-z_]\w*\s*:|thread_local!|lazy_static!|OnceLock|OnceCell|LazyLock|UnsafeCell/,
  heap: /Box::into_raw/,
  target: /target_arch\s*=\s*"wasm32"/,
  imports: /import_name\s*\(|import_\w+\s*\(/,
};
const seams = Object.fromEntries(Object.keys(patterns).map((key) => [key, []]));
for (const file of files('rust/sim').sort()) {
  const test = file.includes('/tests/');
  fs.readFileSync(new URL(file, root), 'utf8').split('\n').forEach((text, index) => {
    for (const [kind, pattern] of Object.entries(patterns)) {
      if (pattern.test(text)) seams[kind].push({ file, line: index + 1, test, text: text.trim() });
    }
  });
}
const artifacts = ['tools/world-hash-matrix.txt', 'rust/rust-toolchain.toml', 'build/host-runtime.json', 'src/sim-wasm.js'];
const hashes = Object.fromEntries(artifacts.map((file) => [file, crypto.createHash('sha256').update(fs.readFileSync(new URL(file, root))).digest('hex')]));
console.log(JSON.stringify({ hashes, seams }, null, 2));
