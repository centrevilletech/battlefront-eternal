import test from "node:test";
import assert from "node:assert/strict";
import { clockText, barText, cssColor } from "../src/spawn-screen/page/view.js";

test("the countdown reads minutes and seconds, seconds alone under a minute", () => {
  assert.equal(clockText(65), "1:05");
  assert.equal(clockText(600), "10:00");
  assert.equal(clockText(45.9), "45");
  assert.equal(clockText(-3), "0");
});

test("a panel's label drops the original's colon", () => {
  assert.equal(barText("OBJECTIVES:"), "OBJECTIVES");
  assert.equal(barText(" VEHICLES: "), "VEHICLES");
  assert.equal(barText(null), "");
});

test("the look's colours read from CSS as 0..1 values", () => {
  assert.deepEqual(cssColor("rgb(255, 0, 51)"), [1, 0, 0.2, 1]);
  assert.deepEqual(cssColor("rgba(0, 0, 0, .5)"), [0, 0, 0, 0.5]);
  assert.deepEqual(cssColor("rgb(255 255 255 / 50%)"), [1, 1, 1, 0.5]);
  assert.deepEqual(cssColor("#ff0000"), [1, 0, 0, 1]);
  assert.equal(cssColor("tomato"), null);
  assert.equal(cssColor(""), null);
});
