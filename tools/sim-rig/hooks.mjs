// Module hook for the rig recorder (tools/sim-rig/record.mjs registers it):
// as the game's modules load, the rider pose's three functions pass through
// globalThis.__bfeRig when the recorder has set it, and the Vehicle
// constructor tells it each hull it has built. Nothing else changes: with
// the recorder absent the functions run as written. Every anchor must be
// found exactly once in its file, or the load fails: a moved line is fixed
// here, never guessed at.
const through = (name, params) => ({
  anchor: `export function ${name}(${params}) {\n`,
  text: `export function ${name}(${params}) {\n  return globalThis.__bfeRig ? globalThis.__bfeRig.${name}(${name}Of, ${params}) : ${name}Of(${params});\n}\nfunction ${name}Of(${params}) {\n`,
});

export const EDITS = {
  '/src/rider-pose.js': [
    through('riderPoints', 'animator, sample, out'),
    through('riderHitShapes', 'points, frame, scale, shapes'),
    through('riderDropPoint', 'points, out'),
  ],
  '/src/vehicles.js': [
    { anchor: '    this.root.visible = false;\n    this.body.enabled = false;\n    system.scene.add(this.root);\n  }\n',
      text: '    this.root.visible = false;\n    this.body.enabled = false;\n    system.scene.add(this.root);\n    globalThis.__bfeRig?.hullBuilt(this, template);\n  }\n' },
  ],
};

export function edit(source, edits, file) {
  for (const { anchor, text } of edits) {
    const at = source.indexOf(anchor);
    if (at < 0 || source.indexOf(anchor, at + 1) >= 0) throw Error(`sim-rig: ${file}: ${at < 0 ? 'no' : 'more than one'} ${JSON.stringify(anchor.slice(0, 80))}: update tools/sim-rig/hooks.mjs`);
    source = source.slice(0, at) + text + source.slice(at + anchor.length);
  }
  return source;
}

export async function load(url, context, nextLoad) {
  const result = await nextLoad(url, context);
  if (!url.startsWith('file:')) return result;
  const file = Object.keys(EDITS).find((suffix) => url.endsWith(suffix));
  if (!file) return result;
  return { ...result, source: edit(String(result.source), EDITS[file], file) };
}
