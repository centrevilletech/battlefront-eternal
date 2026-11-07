import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";

// The actor family's shared GLSL chunks are derived from complete sources
// (tools/actor-shader-chunks.py): joining them and factoring the result must
// give the checked-in files back, so an edit made by hand to one chunk, or a
// chunk left behind, fails here.
test("the actor shader chunks regenerate exactly from their joined sources", (t) => {
  const run = spawnSync("python3", ["-I", "tools/actor-shader-chunks.py", "--check"], { encoding: "utf8" });
  if (run.error) return t.skip("python3 is not available");
  assert.equal(run.status, 0, run.stdout + run.stderr);
});
