// Fetches the toolchain the desktop app uses to prepare game data
// (tools/prepare.py): standalone CPython with the tools' wheels, a static
// ffmpeg and KTX-Software's `ktx`. electron-builder packs
// toolchain/<os>-<arch>/ as resources/toolchain (electron-builder.yml).
//
//   node build/fetch-toolchain.mjs [linux|mac|win] [x64|arm64] ...
//
// With no arguments, the host's platform and architecture. Wheels are
// installed for the target with the host's pip (--platform), so one runner
// can fetch several architectures; that pip comes from a standalone CPython
// for the host. Needs tar, unzip (Linux, macOS), and on the hosts that unpack
// them, 7z (Windows installer) or pkgutil (macOS pkg).
import { execFileSync } from "node:child_process";
import { createWriteStream, existsSync, promises as fs } from "node:fs";
import path from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const cache = path.join(root, "toolchain", ".downloads");

const PYTHON = { version: "3.13.16", release: "20261009" };
const KTX = "4.4.2";
// The wheels the tools import: Pillow for the HUD and shell atlases; the rest
// for tools/enhance_audio.py (pinned as in tools/requirements-audio.txt).
const CORE_WHEELS = ["pillow>=10"];
const AUDIO_WHEELS = ["numpy==2.5.3", "scipy==1.18.1", "soundfile==0.14.0", "soxr==1.1.0"];

const TARGETS = {
  "linux-x64": {
    python: "x86_64-unknown-linux-gnu",
    wheels: ["manylinux_2_28_x86_64", "manylinux_2_17_x86_64", "manylinux2014_x86_64"],
    ffmpeg: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-linux64-gpl-8.1.tar.xz",
    ktx: `https://github.com/KhronosGroup/KTX-Software/releases/download/v${KTX}/KTX-Software-${KTX}-Linux-x86_64.tar.bz2`,
  },
  "linux-arm64": {
    python: "aarch64-unknown-linux-gnu",
    wheels: ["manylinux_2_28_aarch64", "manylinux_2_17_aarch64", "manylinux2014_aarch64"],
    ffmpeg: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-linuxarm64-gpl-8.1.tar.xz",
    ktx: `https://github.com/KhronosGroup/KTX-Software/releases/download/v${KTX}/KTX-Software-${KTX}-Linux-arm64.tar.bz2`,
  },
  "win-x64": {
    python: "x86_64-pc-windows-msvc",
    wheels: ["win_amd64"],
    ffmpeg: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-gpl-8.1.zip",
    ktx: `https://github.com/KhronosGroup/KTX-Software/releases/download/v${KTX}/KTX-Software-${KTX}-Windows-x64.exe`,
  },
  "mac-x64": {
    python: "x86_64-apple-darwin",
    wheels: ["macosx_10_13_x86_64", "macosx_10_15_x86_64", "macosx_11_0_x86_64", "macosx_12_0_x86_64", "macosx_13_0_x86_64", "macosx_14_0_x86_64"],
    ffmpeg: "https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip",
    ktx: `https://github.com/KhronosGroup/KTX-Software/releases/download/v${KTX}/KTX-Software-${KTX}-Darwin-x86_64.pkg`,
  },
  "mac-arm64": {
    python: "aarch64-apple-darwin",
    wheels: ["macosx_11_0_arm64", "macosx_12_0_arm64", "macosx_13_0_arm64", "macosx_14_0_arm64", "macosx_15_0_arm64"],
    ffmpeg: "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip",
    ktx: `https://github.com/KhronosGroup/KTX-Software/releases/download/v${KTX}/KTX-Software-${KTX}-Darwin-arm64.pkg`,
  },
};

const run = (command, args, options = {}) => execFileSync(command, args, { stdio: "inherit", ...options });
// Windows' own bsdtar: Git Bash's GNU tar reads "D:\\..." as a remote host.
const tar = process.platform === "win32" ? path.join(process.env.SystemRoot || "C:\\Windows", "System32", "tar.exe") : "tar";
const pythonArchive = (triple) => `cpython-${PYTHON.version}+${PYTHON.release}-${triple}-install_only_stripped.tar.gz`;
const pythonUrl = (triple) => `https://github.com/astral-sh/python-build-standalone/releases/download/${PYTHON.release}/${encodeURIComponent(pythonArchive(triple))}`;

// A standalone CPython for this host, whose pip installs the targets' wheels.
let hostPython = null;
async function ensureHostPython() {
  if (hostPython) return hostPython;
  const triple = {
    "linux-x64": "x86_64-unknown-linux-gnu", "linux-arm64": "aarch64-unknown-linux-gnu",
    "win32-x64": "x86_64-pc-windows-msvc", "darwin-x64": "x86_64-apple-darwin", "darwin-arm64": "aarch64-apple-darwin",
  }[`${process.platform}-${process.arch}`];
  if (!triple) throw new Error(`no standalone Python for this host (${process.platform}-${process.arch})`);
  const dir = path.join(root, "toolchain", ".host");
  const exe = process.platform === "win32" ? path.join(dir, "python", "python.exe") : path.join(dir, "python", "bin", "python3");
  if (!existsSync(exe)) {
    await fs.mkdir(dir, { recursive: true });
    run(tar, ["-xzf", await download(pythonUrl(triple)), "-C", dir]);
  }
  return (hostPython = exe);
}

// Downloads into the cache, resuming a partial file and retrying when a
// connection stalls (a read that waits a minute is abandoned and resumed).
async function download(url, attempts = 6) {
  await fs.mkdir(cache, { recursive: true });
  const file = path.join(cache, url.replace(/^https?:\/\//, "").replace(/[^\w.-]+/g, "_"));
  if (existsSync(file)) return file;
  const part = file + ".part";
  for (let attempt = 1; ; attempt++) {
    const have = existsSync(part) ? (await fs.stat(part)).size : 0;
    const controller = new AbortController();
    let idle = setTimeout(() => controller.abort(), 60000);
    try {
      console.log(`download ${url}${have ? ` (resuming at ${have} bytes)` : ""}`);
      const response = await fetch(url, { redirect: "follow", signal: controller.signal, headers: have ? { Range: `bytes=${have}-` } : {} });
      if (response.status === 416) break; // already complete
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const append = have > 0 && response.status === 206;
      const body = Readable.fromWeb(response.body);
      body.on("data", () => {
        clearTimeout(idle);
        idle = setTimeout(() => controller.abort(), 60000);
      });
      await pipeline(body, createWriteStream(part, { flags: append ? "a" : "w" }));
      clearTimeout(idle);
      break;
    } catch (error) {
      clearTimeout(idle);
      if (attempt >= attempts) throw new Error(`${url}: ${error.message}`);
      console.warn(`download interrupted (${error.message}); retrying`);
      await new Promise((resolve) => setTimeout(resolve, 2000 * attempt));
    }
  }
  await fs.rename(part, file);
  return file;
}

async function findFile(dir, name) {
  for (const entry of await fs.readdir(dir, { withFileTypes: true, recursive: true })) {
    if (entry.isFile() && entry.name === name) return path.join(entry.parentPath ?? entry.path, entry.name);
  }
  return null;
}

async function scratch(name) {
  const dir = path.join(root, "toolchain", ".work", name);
  await fs.rm(dir, { recursive: true, force: true });
  await fs.mkdir(dir, { recursive: true });
  return dir;
}

function unpack(archive, into) {
  if (archive.endsWith(".zip") && process.platform !== "win32") run("unzip", ["-q", "-o", archive, "-d", into]);
  else run(tar, ["-xf", archive, "-C", into]);
}

async function fetchPython(target, out) {
  const archive = await download(pythonUrl(target.python));
  run(tar, ["-xzf", archive, "-C", out]);
  const host = await ensureHostPython();
  const windows = target.python.includes("windows");
  const site = windows ? path.join(out, "python", "Lib", "site-packages") : path.join(out, "python", "lib", `python${PYTHON.version.split(".").slice(0, 2).join(".")}`, "site-packages");
  const pip = (packages) => run(host, ["-m", "pip", "install", "--quiet", "--disable-pip-version-check", "--no-compile",
    "--target", site, "--upgrade", "--only-binary=:all:", "--implementation", "cp",
    "--python-version", PYTHON.version.split(".").slice(0, 2).join("."),
    ...target.wheels.flatMap((tag) => ["--platform", tag]), ...packages]);
  pip(CORE_WHEELS);
  try {
    pip(AUDIO_WHEELS);
  } catch {
    // tools/prepare.py skips the Enhanced sound when these are missing.
    console.warn(`warning: no audio wheels for ${target.python}; Enhanced sound preparation will be unavailable`);
  }
  await prunePython(path.join(out, "python"));
}

// What the tools never import: test suites, Tk/IDLE and byte-code caches.
async function prunePython(dir) {
  const drop = new Set(["tests", "__pycache__", "idlelib", "tkinter", "turtledemo", "ensurepip"]);
  const stdlibTest = /[\\/]lib[\\/]python3\.\d+[\\/]test$|[\\/]Lib[\\/]test$/;
  const walk = async (at) => {
    for (const entry of await fs.readdir(at, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue;
      const child = path.join(at, entry.name);
      if (drop.has(entry.name) || stdlibTest.test(child)) await fs.rm(child, { recursive: true, force: true });
      else await walk(child);
    }
  };
  await walk(dir);
}

async function fetchFfmpeg(target, out, key) {
  const archive = await download(target.ffmpeg);
  const work = await scratch(`ffmpeg-${key}`);
  unpack(archive, work);
  const exe = key.startsWith("win") ? "ffmpeg.exe" : "ffmpeg";
  const found = await findFile(work, exe);
  if (!found) throw new Error(`no ${exe} in ${target.ffmpeg}`);
  await fs.mkdir(path.join(out, "bin"), { recursive: true });
  await fs.copyFile(found, path.join(out, "bin", exe));
  await fs.chmod(path.join(out, "bin", exe), 0o755);
}

// KTX-Software's ktx and its library, laid out as <out>/ktx/{bin,lib}.
async function fetchKtx(target, out, key) {
  const archive = await download(target.ktx);
  const work = await scratch(`ktx-${key}`);
  const dest = path.join(out, "ktx");
  if (archive.endsWith(".tar.bz2")) {
    run(tar, ["-xjf", archive, "-C", work]);
  } else if (archive.endsWith(".exe")) {
    run("7z", ["x", "-y", `-o${work}`, archive], { stdio: "ignore" });
  } else if (archive.endsWith(".pkg")) {
    run("pkgutil", ["--expand-full", archive, path.join(work, "pkg")]);
  }
  const exe = key.startsWith("win") ? "ktx.exe" : "ktx";
  const binary = await findFile(work, exe);
  if (!binary) throw new Error(`no ${exe} in ${target.ktx}`);
  const base = path.dirname(path.dirname(binary));
  await fs.mkdir(path.join(dest, "bin"), { recursive: true });
  await fs.copyFile(binary, path.join(dest, "bin", exe));
  await fs.chmod(path.join(dest, "bin", exe), 0o755);
  // The shared library sits in lib/ (Linux, macOS) or beside the exe (Windows).
  for (const dir of [path.join(base, "lib"), path.dirname(binary)]) {
    if (!existsSync(dir)) continue;
    for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
      if (!/^(lib)?ktx(\.|-?\d)/i.test(entry.name) || /jni/i.test(entry.name)) continue;
      const into = key.startsWith("win") ? path.join(dest, "bin") : path.join(dest, "lib");
      await fs.mkdir(into, { recursive: true });
      await fs.cp(path.join(dir, entry.name), path.join(into, entry.name), { verbatimSymlinks: true });
    }
  }
}

async function fetchTarget(key) {
  const target = TARGETS[key];
  if (!target) throw new Error(`unknown target ${key}; one of ${Object.keys(TARGETS).join(", ")}`);
  const out = path.join(root, "toolchain", key);
  await fs.rm(out, { recursive: true, force: true });
  await fs.mkdir(out, { recursive: true });
  await fetchPython(target, out);
  await fetchFfmpeg(target, out, key);
  try {
    await fetchKtx(target, out, key);
  } catch (error) {
    // tools/ktx2.py only warns without an encoder: the game loads the PNGs.
    console.warn(`warning: KTX-Software unavailable for ${key}: ${error.message}`);
  }
  await fs.writeFile(path.join(out, "toolchain.json"), JSON.stringify({ python: PYTHON, ktx: KTX, ffmpeg: target.ffmpeg }, null, 2));
  console.log(`toolchain ${key} ready in ${path.relative(root, out)}`);
}

const hostOs = { linux: "linux", darwin: "mac", win32: "win" }[process.platform];
const args = process.argv.slice(2);
const os = ["linux", "mac", "win"].includes(args[0]) ? args.shift() : hostOs;
const arches = args.length ? args : [process.arch];
for (const arch of arches) await fetchTarget(`${os}-${arch}`);
