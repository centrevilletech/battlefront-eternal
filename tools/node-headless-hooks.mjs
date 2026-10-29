// Module hook for running the game's headless world under plain Node: Vite-only
// specifiers (asset URLs, wasm) resolve to an empty module. Registered by
// tools/headless-world.mjs; not loaded by the game.
export async function resolve(specifier, context, next) {
  if (/\?(url|raw)$/.test(specifier) || /\.(wasm|css|glsl|png|jpg|ktx2)$/.test(specifier.split('?')[0]))
    return { url: 'data:text/javascript,export default "stub"', shortCircuit: true };
  return next(specifier, context);
}
