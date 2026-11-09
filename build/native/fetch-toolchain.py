#!/usr/bin/env python3
"""Fetch the pinned standalone CPython + wheels, FFmpeg, and KTX-Software toolchain.

Used by the release workflow to populate `toolchain/<os>-<arch>` before
`build/native/stage.py` packs it into `resources/toolchain`.

Usage:
  python3 build/native/fetch-toolchain.py [linux|mac|win] [x64|arm64] ...
"""

from __future__ import annotations

import json
import os
from pathlib import Path
import platform as py_platform
import re
import shutil
import subprocess
import sys
import time
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / "toolchain" / ".downloads"

PYTHON = {"version": "3.13.16", "release": "20261009"}
KTX = "4.4.2"
CORE_WHEELS = ["pillow>=10"]
AUDIO_WHEELS = ["numpy==2.5.3", "scipy==1.18.1", "soundfile==0.14.0", "soxr==1.1.0"]

TARGETS = {
    "linux-x64": {
        "python": "x86_64-unknown-linux-gnu",
        "wheels": ["manylinux_2_28_x86_64", "manylinux_2_17_x86_64", "manylinux2014_x86_64"],
        "ffmpeg": "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-linux64-gpl-8.1.tar.xz",
        "ktx": f"https://github.com/KhronosGroup/KTX-Software/releases/download/v{KTX}/KTX-Software-{KTX}-Linux-x86_64.tar.bz2",
    },
    "linux-arm64": {
        "python": "aarch64-unknown-linux-gnu",
        "wheels": ["manylinux_2_28_aarch64", "manylinux_2_17_aarch64", "manylinux2014_aarch64"],
        "ffmpeg": "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-linuxarm64-gpl-8.1.tar.xz",
        "ktx": f"https://github.com/KhronosGroup/KTX-Software/releases/download/v{KTX}/KTX-Software-{KTX}-Linux-arm64.tar.bz2",
    },
    "win-x64": {
        "python": "x86_64-pc-windows-msvc",
        "wheels": ["win_amd64"],
        "ffmpeg": "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n8.1-latest-win64-gpl-8.1.zip",
        "ktx": f"https://github.com/KhronosGroup/KTX-Software/releases/download/v{KTX}/KTX-Software-{KTX}-Windows-x64.exe",
    },
    "mac-x64": {
        "python": "x86_64-apple-darwin",
        "wheels": [
            "macosx_10_13_x86_64",
            "macosx_10_15_x86_64",
            "macosx_11_0_x86_64",
            "macosx_12_0_x86_64",
            "macosx_13_0_x86_64",
            "macosx_14_0_x86_64",
        ],
        "ffmpeg": "https://ffmpeg.martin-riedl.de/redirect/latest/macos/amd64/release/ffmpeg.zip",
        "ktx": f"https://github.com/KhronosGroup/KTX-Software/releases/download/v{KTX}/KTX-Software-{KTX}-Darwin-x86_64.pkg",
    },
    "mac-arm64": {
        "python": "aarch64-apple-darwin",
        "wheels": [
            "macosx_11_0_arm64",
            "macosx_12_0_arm64",
            "macosx_13_0_arm64",
            "macosx_14_0_arm64",
            "macosx_15_0_arm64",
        ],
        "ffmpeg": "https://ffmpeg.martin-riedl.de/redirect/latest/macos/arm64/release/ffmpeg.zip",
        "ktx": f"https://github.com/KhronosGroup/KTX-Software/releases/download/v{KTX}/KTX-Software-{KTX}-Darwin-arm64.pkg",
    },
}


def tar_bin() -> str:
    if os.name == "nt":
        sys_root = os.environ.get("SystemRoot", r"C:\Windows")
        return str(Path(sys_root) / "System32" / "tar.exe")
    return "tar"


def run(command: str, args: list[str], quiet: bool = False) -> None:
    kwargs = {"stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL} if quiet else {}
    subprocess.run([command, *args], check=True, **kwargs)


def python_url(triple: str) -> str:
    archive = f"cpython-{PYTHON['version']}+{PYTHON['release']}-{triple}-install_only_stripped.tar.gz"
    return f"https://github.com/astral-sh/python-build-standalone/releases/download/{PYTHON['release']}/{urllib.parse.quote(archive)}"


def host_key() -> tuple[str, str]:
    sys_name = sys.platform
    os_part = "win" if sys_name.startswith("win") else ("mac" if sys_name == "darwin" else "linux")
    mach = py_platform.machine().lower()
    arch_part = "arm64" if mach in ("aarch64", "arm64") else "x64"
    return os_part, arch_part


def download(url: str, attempts: int = 6) -> Path:
    CACHE.mkdir(parents=True, exist_ok=True)
    safe_name = re.sub(r"[^\w.-]+", "_", re.sub(r"^https?://", "", url))
    dest = CACHE / safe_name
    if dest.is_file():
        return dest
    part = Path(str(dest) + ".part")
    for attempt in range(1, attempts + 1):
        have = part.stat().st_size if part.is_file() else 0
        try:
            print(f"download {url}" + (f" (resuming at {have} bytes)" if have else ""))
            headers = {"Range": f"bytes={have}-"} if have else {}
            req = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(req, timeout=60) as resp:
                status = getattr(resp, "status", 200)
                mode = "ab" if (have > 0 and status == 206) else "wb"
                with part.open(mode) as out_f:
                    while True:
                        chunk = resp.read(262144)
                        if not chunk:
                            break
                        out_f.write(chunk)
            break
        except urllib.error.HTTPError as err:
            if err.code == 416:
                break
            if attempt >= attempts:
                raise RuntimeError(f"{url}: HTTP {err.code}") from err
            print(f"download interrupted (HTTP {err.code}); retrying", file=sys.stderr)
            time.sleep(2 * attempt)
        except Exception as err:
            if attempt >= attempts:
                raise RuntimeError(f"{url}: {err}") from err
            print(f"download interrupted ({err}); retrying", file=sys.stderr)
            time.sleep(2 * attempt)
    part.replace(dest)
    return dest


_HOST_PYTHON: Path | None = None


def ensure_host_python() -> Path:
    global _HOST_PYTHON
    if _HOST_PYTHON is not None:
        return _HOST_PYTHON
    os_part, arch_part = host_key()
    triple_map = {
        ("linux", "x64"): "x86_64-unknown-linux-gnu",
        ("linux", "arm64"): "aarch64-unknown-linux-gnu",
        ("win", "x64"): "x86_64-pc-windows-msvc",
        ("mac", "x64"): "x86_64-apple-darwin",
        ("mac", "arm64"): "aarch64-apple-darwin",
    }
    triple = triple_map.get((os_part, arch_part))
    if not triple:
        raise RuntimeError(f"no standalone Python for this host ({os_part}-{arch_part})")
    host_dir = ROOT / "toolchain" / ".host"
    exe = host_dir / "python" / "python.exe" if os.name == "nt" else host_dir / "python" / "bin" / "python3"
    if not exe.is_file():
        host_dir.mkdir(parents=True, exist_ok=True)
        archive = download(python_url(triple))
        run(tar_bin(), ["-xzf", str(archive), "-C", str(host_dir)])
    _HOST_PYTHON = exe
    return exe


def find_file(root: Path, name: str) -> Path | None:
    for dirpath, _, filenames in os.walk(root):
        if name in filenames:
            return Path(dirpath) / name
    return None


def scratch(name: str) -> Path:
    work = ROOT / "toolchain" / ".work" / name
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True, exist_ok=True)
    return work


def unpack(archive: Path, into: Path) -> None:
    if str(archive).endswith(".zip") and os.name != "nt":
        run("unzip", ["-q", "-o", str(archive), "-d", str(into)])
    else:
        run(tar_bin(), ["-xf", str(archive), "-C", str(into)])


def prune_python(py_dir: Path) -> None:
    drop = {"tests", "__pycache__", "idlelib", "tkinter", "turtledemo", "ensurepip"}
    stdlib_test = re.compile(r"[/\\]lib[/\\]python3\.\d+[/\\]test$|[/\\]Lib[/\\]test$")
    for dirpath, dirnames, _ in os.walk(py_dir, topdown=True):
        keep = []
        for dname in dirnames:
            child = Path(dirpath) / dname
            if dname in drop or stdlib_test.search(str(child)):
                shutil.rmtree(child, ignore_errors=True)
            else:
                keep.append(dname)
        dirnames[:] = keep


def fetch_python(target: dict, out: Path) -> None:
    archive = download(python_url(target["python"]))
    run(tar_bin(), ["-xzf", str(archive), "-C", str(out)])
    host_py = ensure_host_python()
    is_win = "windows" in target["python"]
    py_ver = ".".join(PYTHON["version"].split(".")[:2])
    site = out / "python" / "Lib" / "site-packages" if is_win else out / "python" / "lib" / f"python{py_ver}" / "site-packages"

    def pip_install(packages: list[str]) -> None:
        plat_args = [arg for tag in target["wheels"] for arg in ("--platform", tag)]
        run(
            str(host_py),
            [
                "-m",
                "pip",
                "install",
                "--quiet",
                "--disable-pip-version-check",
                "--no-compile",
                "--target",
                str(site),
                "--upgrade",
                "--only-binary=:all:",
                "--implementation",
                "cp",
                "--python-version",
                py_ver,
                *plat_args,
                *packages,
            ],
        )

    pip_install(CORE_WHEELS)
    try:
        pip_install(AUDIO_WHEELS)
    except Exception:
        print(f"warning: no audio wheels for {target['python']}; Enhanced sound preparation will be unavailable", file=sys.stderr)
    prune_python(out / "python")


def fetch_ffmpeg(target: dict, out: Path, key: str) -> None:
    archive = download(target["ffmpeg"])
    work = scratch(f"ffmpeg-{key}")
    unpack(archive, work)
    exe = "ffmpeg.exe" if key.startswith("win") else "ffmpeg"
    found = find_file(work, exe)
    if not found:
        raise RuntimeError(f"no {exe} in {target['ffmpeg']}")
    bin_dir = out / "bin"
    bin_dir.mkdir(parents=True, exist_ok=True)
    dst = bin_dir / exe
    shutil.copy2(found, dst)
    dst.chmod(0o755)


def fetch_ktx(target: dict, out: Path, key: str) -> None:
    archive = download(target["ktx"])
    work = scratch(f"ktx-{key}")
    dest = out / "ktx"
    arch_str = str(archive)
    if arch_str.endswith(".tar.bz2"):
        run(tar_bin(), ["-xjf", arch_str, "-C", str(work)])
    elif arch_str.endswith(".exe"):
        run("7z", ["x", "-y", f"-o{work}", arch_str], quiet=True)
    elif arch_str.endswith(".pkg"):
        run("pkgutil", ["--expand-full", arch_str, str(work / "pkg")])
    exe = "ktx.exe" if key.startswith("win") else "ktx"
    binary = find_file(work, exe)
    if not binary:
        raise RuntimeError(f"no {exe} in {target['ktx']}")
    base = binary.parent.parent
    (dest / "bin").mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, dest / "bin" / exe)
    (dest / "bin" / exe).chmod(0o755)
    for lib_dir in (base / "lib", binary.parent):
        if not lib_dir.is_dir():
            continue
        for entry in lib_dir.iterdir():
            if not re.search(r"^(lib)?ktx(\.|-?\d)", entry.name, re.IGNORECASE) or re.search(r"jni", entry.name, re.IGNORECASE):
                continue
            into = dest / ("bin" if key.startswith("win") else "lib")
            into.mkdir(parents=True, exist_ok=True)
            dst_entry = into / entry.name
            if entry.is_symlink():
                dst_entry.unlink(missing_ok=True)
                dst_entry.symlink_to(os.readlink(entry))
            elif entry.is_file():
                shutil.copy2(entry, dst_entry)


def fetch_target(key: str) -> None:
    target = TARGETS.get(key)
    if not target:
        raise ValueError(f"unknown target {key}; one of {', '.join(TARGETS)}")
    out = ROOT / "toolchain" / key
    shutil.rmtree(out, ignore_errors=True)
    out.mkdir(parents=True, exist_ok=True)
    fetch_python(target, out)
    fetch_ffmpeg(target, out, key)
    try:
        fetch_ktx(target, out, key)
    except Exception as err:
        print(f"warning: KTX-Software unavailable for {key}: {err}", file=sys.stderr)
    (out / "toolchain.json").write_text(
        json.dumps({"python": PYTHON, "ktx": KTX, "ffmpeg": target["ffmpeg"]}, indent=2) + "\n",
        encoding="utf-8",
    )
    print(f"toolchain {key} ready in {out.relative_to(ROOT)}")


def main() -> None:
    default_os, default_arch = host_key()
    args = list(sys.argv[1:])
    os_name = args.pop(0) if (args and args[0] in ("linux", "mac", "win")) else default_os
    arches = args if args else [default_arch]
    for arch in arches:
        fetch_target(f"{os_name}-{arch}")


if __name__ == "__main__":
    main()
