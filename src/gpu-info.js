// The GPU the page draws with, and what keeps it from drawing fast: a
// software renderer standing in for a missing or blocked GPU, or (the
// desktop app's report of Chromium's feature status, chrome://gpu) the
// window composited in software, with every frame read back from the GPU.
// Both cost in proportion to the window's pixels, so a small window hides
// them. The VIDEO OPTIONS and the stats line say so.

const SOFTWARE = /SwiftShader|llvmpipe|softpipe|lavapipe|Software Rasterizer|Basic Render Driver|Microsoft Basic|GDI Generic/i;

// The renderer string: the unmasked one where the browser offers it.
export function rendererName(gl) {
  if (!gl?.getParameter) return "";
  const plain = String(gl.getParameter(gl.RENDERER) ?? "");
  if (plain && !/^WebKit WebGL$/i.test(plain)) return plain;
  const debug = gl.getExtension?.("WEBGL_debug_renderer_info");
  return String((debug && gl.getParameter(debug.UNMASKED_RENDERER_WEBGL)) || plain);
}

// "ANGLE (Intel, Intel(R) HD Graphics 3000 Direct3D11 vs_4_1 ps_4_1,
// D3D11-9.17.10.4459)" -> "Intel(R) HD Graphics 3000".
export function gpuLabel(renderer) {
  let name = String(renderer || "").trim();
  if (SOFTWARE.test(name)) return `Software (${(name.match(SOFTWARE) || ["renderer"])[0]})`;
  const angle = /^ANGLE \((.*)\)$/.exec(name);
  if (angle) {
    // Vendor, device (and backend), driver: split on the commas outside
    // parentheses.
    const parts = [];
    let depth = 0, start = 0;
    const inner = angle[1];
    for (let i = 0; i < inner.length; i++) {
      if (inner[i] === "(") depth++;
      else if (inner[i] === ")") depth--;
      else if (inner[i] === "," && depth === 0) { parts.push(inner.slice(start, i).trim()); start = i + 1; }
    }
    parts.push(inner.slice(start).trim());
    name = parts.length > 1 ? parts[1] : parts[0];
  }
  return name
    .replace(/\s*\(0x[0-9a-f]+\)/gi, "")
    .replace(/\s+vs_\S+\s+ps_\S+$/i, "")
    .replace(/\s+(Direct3D\S*|OpenGL\b.*|Vulkan\b.*|Metal\b.*)$/i, "")
    .trim() || "Unknown GPU";
}

// Intel GPUs older than Ivy Bridge (Sandy Bridge's HD 2000 and 3000,
// Ironlake, the GM45/G45 generation) have no BC7 (BPTC) sampling, but Mesa
// still offers it and unpacks every BC7 texture to RGBA8 on the CPU as it
// uploads: four times BC7's memory and bandwidth, and slow loads. They read
// BC1 and BC3 (DXT) themselves. Mesa names them by codename:
// "Mesa Intel(R) HD Graphics 2000 (SNB GT1)" (through ANGLE as well).
const UNPACKED_BC7 = /\bMesa\b.*\((SNB|ILK|ELK|CTG|CL|BW)( GT\d)?\)|\bMesa DRI Intel\(R\) (Sandybridge|Ironlake|G45|GM45|G41|Q45|Q43|B43|965|G35)/i;
export function unpacksBc7(renderer) {
  return UNPACKED_BC7.test(String(renderer || ""));
}

// GPUs the Minimum preset is for (settings.js PERFORMANCE_PRESETS): Intel's
// before Ivy Bridge, under Mesa (above) or Windows' own driver ("Intel(R) HD
// Graphics 3000", plain "HD Graphics" for Ironlake, the 4 Series). A first
// session on one starts on it (Settings adoptGpuPreset): the defaults are
// for current GPUs, and ran at a few frames a second on an HD 2000. Drawing
// in software is a driver to fix, which the VIDEO OPTIONS say instead.
const WINDOWS_OLD_INTEL = /\bIntel\(R\) (HD Graphics( 2000| 3000| Family)?|G41|G45|Q45|Q43|B43|4 Series)\b(?! \d)/i;
export function oldGpu(renderer) {
  const name = String(renderer || "");
  return !SOFTWARE.test(name) && (unpacksBc7(name) || WINDOWS_OLD_INTEL.test(name));
}

const INTEL = 0x8086, NVIDIA = 0x10de, AMD = 0x1002;

// What the page and the desktop app's report say: { renderer, label,
// software, softwareCompositing, readback, idleDiscrete, devices }.
// `report`: bfeDesktop.gpu()'s { status, devices } (electron/gpu.js), or
// none in a browser.
export function gpuState(renderer, report = null) {
  const status = report?.status || {};
  const devices = Array.isArray(report?.devices) ? report.devices : [];
  // Chromium's own words (chrome://gpu): "..._software" is drawn by the
  // CPU, "enabled_readback" drawn by the GPU and read back for a compositor
  // in software. Newer Chromium reports WebGL 2 under "webgl".
  const webgl = String(status.webgl2 ?? status.webgl ?? "");
  const active = devices.find((device) => device.active);
  return {
    renderer,
    label: gpuLabel(renderer),
    software: SOFTWARE.test(renderer) || /_software$/.test(webgl),
    softwareCompositing: /^(disabled|unavailable)/.test(String(status.gpu_compositing || "")),
    readback: webgl === "enabled_readback",
    // A laptop drawing on its Intel GPU while an NVIDIA or AMD one idles.
    idleDiscrete: active?.vendorId === INTEL && devices.some((device) => !device.active && (device.vendorId === NVIDIA || device.vendorId === AMD)),
    devices,
  };
}

// The warning to show, or null, with what to do about it: short, in the
// 2004 font's letters (the classic menu draws a note on one line).
export function gpuWarning(state) {
  if (!state) return null;
  if (state.software) return "DRAWN IN SOFTWARE: UPDATE THE GRAPHICS DRIVER";
  if (state.softwareCompositing || state.readback) return "COMPOSITED IN SOFTWARE: UPDATE THE GRAPHICS DRIVER";
  if (state.idleDiscrete) return "SECOND GPU IDLE: SET HIGH PERFORMANCE IN WINDOWS";
  return null;
}

// The QUALITY PRESET row's note: the GPU, and the warning if there is one.
export function gpuNote(state) {
  if (!state) return null;
  const warning = gpuWarning(state);
  if (!state.renderer && !warning) return null;
  // The name's first words, up to 26 letters.
  const words = state.label.toUpperCase().replace(/[^A-Z0-9 ()+.-]/g, "").split(/\s+/).filter(Boolean);
  let name = "";
  for (const word of words) {
    if ((name ? name.length + 1 : 0) + word.length > 26) break;
    name = name ? `${name} ${word}` : word;
  }
  name ||= (words[0] || "").slice(0, 26);
  return warning ? `GPU: ${name}, ${warning}` : `GPU: ${name}`;
}

// The page's GPU, looked up once and shared by everything that shows it:
// the battle passes its own context's renderer (Graphics); the front end,
// which may have none, reads a throwaway context's. The desktop app's report
// arrives a moment later; until then the renderer string alone decides.
let known = null;
export function pageGpu({ renderer = probeRenderer, desktop = globalThis.bfeDesktop } = {}) {
  if (known) return known.state;
  known = { state: gpuState(String((typeof renderer === "function" ? renderer() : renderer) || "")) };
  const entry = known;
  desktop?.gpu?.().then((report) => { if (report && known === entry) entry.state = gpuState(entry.state.renderer, report); }, () => {});
  return entry.state;
}

// For tests: forget what is known.
export function forgetGpu() { known = null; }

function probeRenderer() {
  if (typeof document === "undefined") return "";
  try {
    const gl = document.createElement("canvas").getContext("webgl2", { powerPreference: "high-performance" });
    const name = rendererName(gl);
    gl?.getExtension("WEBGL_lose_context")?.loseContext();
    return name;
  } catch {
    return "";
  }
}
