import test from "node:test";
import assert from "node:assert/strict";
import { forgetGpu, gpuLabel, gpuNote, gpuState, gpuWarning, pageGpu, rendererName } from "../src/gpu-info.js";
import { gpuReport, gpuSwitches } from "../electron/gpu.js";

test("a renderer string becomes the GPU's name", () => {
  assert.equal(gpuLabel("ANGLE (Intel, Intel(R) HD Graphics 3000 Direct3D11 vs_4_1 ps_4_1, D3D11-9.17.10.4459)"), "Intel(R) HD Graphics 3000");
  assert.equal(gpuLabel("ANGLE (NVIDIA, NVIDIA GeForce GTX 750 Ti (0x00001380) Direct3D11 vs_5_0 ps_5_0, D3D11)"), "NVIDIA GeForce GTX 750 Ti");
  assert.equal(gpuLabel("ANGLE (Intel, Mesa Intel(R) HD Graphics 4000 (IVB GT2), OpenGL 4.2)"), "Mesa Intel(R) HD Graphics 4000 (IVB GT2)");
  assert.equal(gpuLabel("ANGLE (AMD, AMD Radeon HD 6450 Direct3D11 vs_5_0 ps_5_0)"), "AMD Radeon HD 6450");
  assert.equal(gpuLabel("ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)"), "Software (SwiftShader)");
  assert.equal(gpuLabel("llvmpipe (LLVM 15.0.7, 256 bits)"), "Software (llvmpipe)");
  assert.equal(gpuLabel("Intel(R) HD Graphics 3000"), "Intel(R) HD Graphics 3000");
  assert.equal(gpuLabel(""), "Unknown GPU");
});

test("the renderer is read unmasked where only the debug extension has it", () => {
  const debug = { UNMASKED_RENDERER_WEBGL: 37446 };
  const gl = { RENDERER: 7937, getParameter: (name) => (name === 7937 ? "WebKit WebGL" : name === 37446 ? "ANGLE (Intel, Intel(R) HD Graphics 3000 Direct3D11 vs_4_1 ps_4_1, D3D11)" : null), getExtension: () => debug };
  assert.match(rendererName(gl), /HD Graphics 3000/);
  const firefox = { RENDERER: 7937, getParameter: () => "Intel(R) HD Graphics 400", getExtension: () => null };
  assert.equal(rendererName(firefox), "Intel(R) HD Graphics 400");
  assert.equal(rendererName(null), "");
});

test("software drawing and software compositing are told apart, with what to do", () => {
  const hd = "ANGLE (Intel, Intel(R) HD Graphics 3000 Direct3D11 vs_4_1 ps_4_1, D3D11)";
  const fine = gpuState(hd, { status: { webgl2: "enabled", gpu_compositing: "enabled" } });
  assert.equal(gpuWarning(fine), null);
  assert.equal(gpuNote(fine), "GPU: INTEL(R) HD GRAPHICS 3000");
  const readback = gpuState(hd, { status: { webgl2: "enabled_readback", gpu_compositing: "disabled_software" } });
  assert.ok(readback.readback && readback.softwareCompositing && !readback.software);
  assert.match(gpuWarning(readback), /COMPOSITED IN SOFTWARE/);
  assert.equal(gpuNote(readback), "GPU: INTEL(R) HD GRAPHICS 3000, COMPOSITED IN SOFTWARE: UPDATE THE GRAPHICS DRIVER");
  const swiftshader = gpuState("ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)");
  assert.ok(swiftshader.software);
  assert.equal(gpuNote(swiftshader), "GPU: SOFTWARE (SWIFTSHADER), DRAWN IN SOFTWARE: UPDATE THE GRAPHICS DRIVER");
  assert.ok(gpuState(hd, { status: { webgl2: "unavailable_software" } }).software);
  // Electron 44's status has no webgl2 entry: "webgl" stands for both.
  assert.ok(gpuState(hd, { status: { webgl: "disabled_software" } }).software);
  assert.ok(gpuState(hd, { status: { webgl: "enabled_readback", gpu_compositing: "disabled_software" } }).readback);
  assert.equal(gpuState(hd, { status: { webgl: "enabled", gpu_compositing: "enabled" } }).software, false);
  // The 2004 font's letters only, and short enough for the classic menu's
  // one line (the longest note it had: 79 characters).
  assert.match(gpuNote(gpuState("ANGLE (NVIDIA, NVIDIA GeForce GTX 1050 Ti with Max-Q Design (0x00001C8C) Direct3D11 vs_5_0 ps_5_0, D3D11)")), /^GPU: NVIDIA GEFORCE GTX 1050 TI$/, "cut at a word");
  const long = gpuState("ANGLE (NVIDIA, NVIDIA GeForce GTX 1050 Ti with Max-Q Design (0x00001C8C) Direct3D11 vs_5_0 ps_5_0, D3D11)", { status: { gpu_compositing: "disabled_software" } });
  for (const state of [fine, readback, swiftshader, long]) {
    assert.match(gpuNote(state), /^[A-Z0-9 ()+.,:-]+$/);
    assert.ok(gpuNote(state).length <= 84, gpuNote(state));
  }
  // A laptop drawing on its Intel GPU while its NVIDIA one idles.
  const laptop = gpuState("ANGLE (Intel, Intel(R) HD Graphics 4000 Direct3D11 vs_5_0 ps_5_0, D3D11)", { status: { webgl2: "enabled", gpu_compositing: "enabled" },
    devices: [{ vendorId: 0x8086, deviceId: 0x166, active: true }, { vendorId: 0x10de, deviceId: 0x1140, active: false }] });
  assert.ok(laptop.idleDiscrete);
  assert.equal(gpuNote(laptop), "GPU: INTEL(R) HD GRAPHICS 4000, SECOND GPU IDLE: SET HIGH PERFORMANCE IN WINDOWS");
  assert.ok(gpuNote(laptop).length <= 84);
  const onNvidia = gpuState("ANGLE (NVIDIA, NVIDIA GeForce GT 620M Direct3D11 vs_5_0 ps_5_0, D3D11)", { status: {},
    devices: [{ vendorId: 0x8086, deviceId: 0x166, active: false }, { vendorId: 0x10de, deviceId: 0x1140, active: true }] });
  assert.equal(onNvidia.idleDiscrete, false);
  assert.equal(gpuWarning(onNvidia), null);
  // Nothing known, nothing said.
  assert.equal(gpuNote(gpuState("")), null);
});

test("the page looks its GPU up once, then takes the desktop app's report", async () => {
  forgetGpu();
  let asked = 0, resolve;
  const desktop = { gpu: () => { asked++; return new Promise((r) => { resolve = r; }); } };
  const first = pageGpu({ renderer: () => "ANGLE (Intel, Intel(R) HD Graphics 3000 Direct3D11 vs_4_1 ps_4_1, D3D11)", desktop });
  assert.equal(first.label, "Intel(R) HD Graphics 3000");
  assert.equal(first.softwareCompositing, false);
  assert.equal(pageGpu({ renderer: () => "other", desktop }), first, "looked up once");
  resolve({ status: { webgl2: "enabled_readback", gpu_compositing: "disabled_software" } });
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(pageGpu().softwareCompositing, true);
  assert.equal(pageGpu().label, "Intel(R) HD Graphics 3000");
  assert.equal(asked, 1);
  forgetGpu();
  // A browser: no desktop app, no probe outside a page.
  assert.equal(pageGpu({ desktop: null }).renderer, "");
  forgetGpu();
});

test("the desktop app asks for the discrete GPU on Windows and reports Chromium's view", async () => {
  assert.deepEqual(gpuSwitches({ platform: "win32" }), [["force_high_performance_gpu"]]);
  assert.deepEqual(gpuSwitches({ platform: "linux" }), []);
  const electronApp = {
    getGPUFeatureStatus: () => ({ webgl2: "enabled_readback", gpu_compositing: "disabled_software", video_decode: "enabled", "2d_canvas": "enabled" }),
    getGPUInfo: async () => ({ gpuDevice: [{ vendorId: 32902, deviceId: 294, active: true, driverVersion: "9.17.10.4459" }, { vendorId: 4318, deviceId: 4576, active: false }] }),
  };
  const report = await gpuReport({ electronApp, platform: "win32" });
  assert.deepEqual(report.status, { webgl2: "enabled_readback", gpu_compositing: "disabled_software" });
  assert.deepEqual(report.devices, [
    { vendorId: 32902, deviceId: 294, active: true, driverVersion: "9.17.10.4459" },
    { vendorId: 4318, deviceId: 4576, active: false, driverVersion: null },
  ]);
  assert.deepEqual(report.switches, ["force_high_performance_gpu"]);
  // No GPU at all: getGPUInfo rejects, the status still comes.
  const none = await gpuReport({ electronApp: { getGPUFeatureStatus: () => ({ webgl2: "unavailable_off" }), getGPUInfo: () => Promise.reject(new Error("GPU access is disabled")) }, platform: "linux" });
  assert.deepEqual(none, { status: { webgl2: "unavailable_off" }, devices: [], switches: [] });
});
