import test from "node:test";
import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import { loadRustRenderer } from "../src/rust-render/load.js";

test("safe mode never imports the renderer, even if loading it would fail", async () => {
  let imports = 0;
  const hooks = registerHooks({ resolve(specifier, context, nextResolve) {
    if (context.parentURL?.endsWith("/rust-render/load.js") && specifier === "./index.js") {
      imports++;
      throw Error("renderer unavailable");
    }
    return nextResolve(specifier, context);
  } });
  try {
    assert.equal(await loadRustRenderer({}, "?map=nab2&rustOff"), null);
    assert.equal(await loadRustRenderer({}, "?rustOff=1"), null);
    assert.equal(imports, 0);
    await assert.rejects(loadRustRenderer({}, "?rust"), /renderer unavailable/);
    assert.equal(imports, 1, "normal startup attempts the renderer");
  } finally { hooks.deregister(); }
});
