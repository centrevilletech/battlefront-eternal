import assert from "node:assert/strict";
import { test } from "node:test";
import { outputSize, sharpnessOf } from "../src/shell/video-sharpen.js";

test("the canvas keeps the video's shape at the screen pixels it covers", () => {
  // 800x600 into a 1920x1080 box: contain fits the height, cover the width.
  assert.deepEqual(outputSize(800, 600, 1920, 1080, "contain"), [1440, 1080]);
  assert.deepEqual(outputSize(800, 600, 1920, 1080, "cover"), [1920, 1440]);
  assert.deepEqual(outputSize(800, 600, 1920, 1080, "fill"), [1920, 1080]);
  assert.deepEqual(outputSize(800, 600, 1280, 800, "contain", 2), [2133, 1600]);
  // Capped so a huge display can't ask for an unbounded buffer.
  assert.deepEqual(outputSize(800, 600, 8000, 6000, "contain", 2, 4096), [4096, 3072]);
  assert.deepEqual(outputSize(0, 0, 100, 100, "contain"), [1, 1]);
});

test("strength maps onto RCAS sharpness", () => {
  assert.equal(sharpnessOf(0), 0);
  assert.equal(sharpnessOf(1), 1);
  assert.equal(sharpnessOf(0.5), 0.5);
  assert.equal(sharpnessOf(2), 1);
});
