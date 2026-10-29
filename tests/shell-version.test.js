// The main menu's version line (src/shell/version.js).
import test from "node:test";
import assert from "node:assert/strict";
import { versionLine, versionText } from "../src/shell/version.js";

const wanted = { maps: 12, sides: 2, common: 2 };

test("a desktop build shows its preview, simulation and game data generations", () => {
  const about = { version: "0.1.0-preview.279", wanted, prepared: { maps: 12, sides: 2, common: 2, current: true } };
  assert.equal(versionLine(about, "bfe-sim-25"), "PREVIEW 279   SIMULATION 25   GAME DATA M12 S2 C2");
  assert.equal(versionLine({ ...about, version: "0.1.0~preview.280" }, "bfe-sim-25").split("   ")[0], "PREVIEW 280");
});

test("stale game data shows what this build needs", () => {
  const about = { version: "0.1.0-preview.279", wanted, prepared: { maps: 8, sides: 2, common: null, current: false } };
  assert.equal(versionLine(about, "bfe-sim-25"), "PREVIEW 279   SIMULATION 25   GAME DATA M8 S2 C? (NEEDS M12 S2 C2)");
  assert.equal(versionLine({ version: "0.1.0-preview.279", wanted, prepared: null }, "bfe-sim-25"), "PREVIEW 279   SIMULATION 25   GAME DATA NOT PREPARED");
});

test("a web build shows its simulation, and the desktop's answer once it comes", async () => {
  assert.equal(versionLine(null, "bfe-sim-25"), "WEB BUILD   SIMULATION 25");
  const text = versionText({ about: async () => ({ version: "0.1.0-preview.279", wanted, prepared: { ...wanted, current: true } }) });
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.match(text(), /^PREVIEW 279   SIMULATION \d+   GAME DATA M12 S2 C2$/);
});
