// The GPU as Chromium uses it (chrome://gpu), and the switches this run
// starts Chromium's GPU process with. The page shows the report in VIDEO
// OPTIONS (src/gpu-info.js): a GPU Chromium blocks for its driver draws the
// game in software, or composites the window in software and reads every
// frame back, which only a small window makes bearable.

// On a laptop with an integrated and a discrete GPU, the GPU process (which
// draws the whole window, the game included) runs on the discrete one, as a
// game would. Windows: elsewhere the system's own choice applies (PRIME
// offload on Linux).
export function gpuSwitches({ platform = process.platform } = {}) {
  return platform === "win32" ? [["force_high_performance_gpu"]] : [];
}

const FEATURES = ["webgl", "webgl2", "gpu_compositing", "rasterization"];

// { status: the features that decide the frame rate, devices: the GPUs
// (the active one marked), switches }. electronApp: Electron's app.
export async function gpuReport({ electronApp, platform = process.platform }) {
  const all = electronApp.getGPUFeatureStatus?.() || {};
  const status = Object.fromEntries(FEATURES.filter((name) => typeof all[name] === "string").map((name) => [name, all[name]]));
  let devices = [];
  try {
    const info = await electronApp.getGPUInfo("basic");
    devices = (Array.isArray(info?.gpuDevice) ? info.gpuDevice : []).slice(0, 4).map((device) => ({
      vendorId: Number(device.vendorId) || 0, deviceId: Number(device.deviceId) || 0, active: Boolean(device.active),
      driverVersion: typeof device.driverVersion === "string" ? device.driverVersion.slice(0, 64) : null,
    }));
  } catch {
    // No GPU at all (the promise rejects): the status says so.
  }
  return { status, devices, switches: gpuSwitches({ platform }).map(([name]) => name) };
}
