// Emit declared browser layouts and the Rust registry from the renderer's source manifests.
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { createHash } from 'node:crypto';
import { generate as staticLayout } from './render-static-layout.mjs';
import { generate as interfaceLayout } from './render-interface-layout.mjs';
import { generate as postLayout } from './render-post-layout.mjs';
const root = fileURLToPath(new URL('../', import.meta.url));
export function generate(check = false) {
  interfaceLayout(check);
  staticLayout(check);
  postLayout(check);
  const base = root + 'rust/bfe-render/shaders/';
  const sources = {}, rows = [], twins = [];
  for (const family of readdirSync(base).sort()) {
    const manifest = JSON.parse(readFileSync(base + family + '/manifest.json'));
    if (manifest.version !== 1) throw Error('Unsupported shader manifest');
    const entries = new Map(manifest.programs.map(q => [q.key, q]));
    for (const p of manifest.programs) {
      if (!p.key.startsWith(family + '/') || sources[p.key]) throw Error('Invalid shader key ' + p.key);
      // A source path may be an ordered array of chunk files (the actor family's
      // shared chunks, tools/actor-shader-chunks.py), concatenated in order.
      const parts = path => Array.isArray(path) ? path : [path];
      const read = path => parts(path).map(file => readFileSync(base + family + '/' + file, 'utf8')).join('');
      // A shadowed twin stores no source of its own: it is its unshadowed base with
      // the two shadow defines inserted at the recorded line, as three emits them.
      const twin = p.shadowOf, own = twin ? entries.get(twin.key) : p;
      if (!own?.vertex || own.shadowOf) throw Error('Invalid shadow base for ' + p.key);
      const withShadow = (text, line) => twin ? text.split('\n').toSpliced(line, 0, '#define USE_SHADOWMAP', '#define SHADOWMAP_TYPE_PCF_SOFT').join('\n') : text;
      const vertex = (p.vertexPrefix || '') + withShadow(read(own.vertex), twin?.vertexLine);
      const fragment = (p.fragmentPrefix || '') + withShadow(read(own.fragment), twin?.fragmentLine);
      const kind = p.kind ?? 'Plain';
      const source = p.source ?? 'body';
      const lamps = p.lamps ?? false;
      if (!['Plain', 'World', 'Skinned', 'Rigid', 'Effect', 'Motion'].includes(kind)) throw Error('Unsupported shader kind ' + kind);
      if (!['body', 'expanded'].includes(source)) throw Error('Unsupported shader source ' + source);
      if (typeof lamps !== 'boolean' || (lamps && !['World', 'Skinned', 'Rigid'].includes(kind))) throw Error('Invalid lamp mode');
      if (/#[ \t]*include/.test(vertex + fragment)) throw Error('Source must be self contained');
      if (source === 'body' && kind !== 'Plain') throw Error('Rewritten programs require expanded source');
      if (source === 'body' && /#[ \t]*version/.test(vertex + fragment)) throw Error('Body declares version');
      if (source === 'expanded' && ![vertex, fragment].every(s => s.startsWith('#version 300 es\n'))) throw Error('Expanded source must declare GLSL ES 3.00');
      const attributes = Object.entries(p.attributes);
      const uniforms = Object.entries(p.uniforms).map(([name, value]) => {
        const type = typeof value === 'number' ? value : value.type;
        const size = typeof value === 'number' ? 1 : value.size;
        if (![1,2,3,4,5,9,16,32,33,34,35].includes(type) || !Number.isInteger(size) || size < 1) throw Error('Invalid uniform ' + name);
        return { name, type, size };
      });
      const hash = createHash('sha256').update(JSON.stringify({ vertex, fragment, attributes, uniforms, kind, source, lamps })).digest('hex');
      // The page picks a shadowed program through its base's shadowTwin, never by name.
      if (twin) twins.push([twin.key, p.key]);
      sources[p.key] = { attributes, uniforms, hash, kind, source, lamps, ...(p.derived ? { derived: true } : {}), ...(twin ? { shadowOf: twin.key } : {}), ...(p.material ? {material:p.material} : {}), ...(p.selector ? { selector: p.selector, inputs: p.inputs } : {}) };
      const pairs = (xs) => xs.map(([n,v]) => `(${JSON.stringify(n)}, ${v})`).join(', ');
      const files = (path) => `&[${parts(path).map(file => `include_str!("../shaders/${family}/${file}")`).join(", ")}]`;
      rows.push(`    Spec { key: ${JSON.stringify(p.key)}, vertex: ${files(own.vertex)}, fragment: ${files(own.fragment)}, vertex_prefix: ${JSON.stringify(p.vertexPrefix || "")}, fragment_prefix: ${JSON.stringify(p.fragmentPrefix || "")}, kind: Kind::${kind}, expanded: ${source === "expanded"}, lamps: ${lamps}, shadow_at: ${twin ? `Some((${twin.vertexLine}, ${twin.fragmentLine}))` : 'None'}, attributes: &[${pairs(attributes)}], uniforms: &[${pairs(uniforms.map(u => [u.name, u.type]))}], uniform_sizes: &[${pairs(uniforms.filter(u => u.size > 1).map(u => [u.name, u.size]))}] },`);
    }
  }
  for (const [from, key] of twins) {
    if (!sources[from] || sources[from].shadowTwin) throw Error('Invalid shadow twin for ' + key);
    sources[from].shadowTwin = key;
  }
  const outputs = {
    'rust/bfe-render/src/world-selectors.json': JSON.stringify(Object.entries(sources).filter(([key, p]) => key.startsWith('world/') && p.selector).map(([key, p]) => ({key, ...p.selector}))) + '\n',
    'src/render-programs.js': '// Generated by tools/render-shaders.mjs; edit rust/bfe-render/shaders instead.\nexport const programLayouts = ' + JSON.stringify(sources, null, 2) + ';\n',
    'rust/bfe-render/src/program_catalog.rs': '// Generated by tools/render-shaders.mjs; edit the shader manifests instead.\nuse super::Spec;\nuse crate::shader::Kind;\npub static PROGRAMS: &[Spec] = &[\n' + rows.join('\n') + '\n];\n',
  };
  for (const [path, text] of Object.entries(outputs)) {
    if (check) { if (readFileSync(root + path, 'utf8') !== text) throw Error('Stale ' + path); }
    else writeFileSync(root + path, text);
  }
}
if (process.argv[1] === fileURLToPath(import.meta.url)) generate(process.argv.includes('--check'));
