import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";

// The actor variants no battle has been seen to need (the rest of the closed
// material space) are derived from the observed ones by
// tools/actor-shader-compose.py; the checked-in derived rows must be exactly
// what it derives, so a hand edit or a changed observed source fails here.
test("the derived actor variants are what the observed ones derive", (t) => {
  const run = spawnSync("python3", ["-I", "tools/actor-shader-compose.py", "--check"], { encoding: "utf8" });
  if (run.error) return t.skip("python3 is not available");
  assert.equal(run.status, 0, run.stdout + run.stderr);
});
