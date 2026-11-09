#!/usr/bin/env python3
"""Fetch a hash-verified libvpx release and configure its portable C decoder.

The archive and configuration live in Cargo's cached build output. Compilation
is owned by cc-rs so Windows uses MSVC, Linux uses its native compiler, and no
prebuilt binary or system codec is part of a release.
"""
import hashlib
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import urllib.request

VERSION = "1.17.0"
SHA256 = "1020f184046187baa2985dbde38e0691f49c44088bca7a1842b0236c6081dc0a"
out = Path(sys.argv[1]).resolve()
source = out / f"libvpx-{VERSION}"
build = out / "vpx-build"
if not source.exists():
    archive = out / "libvpx.tar.gz"
    urllib.request.urlretrieve(
        f"https://codeload.github.com/webmproject/libvpx/tar.gz/refs/tags/v{VERSION}", archive)
    if hashlib.sha256(archive.read_bytes()).hexdigest() != SHA256:
        raise RuntimeError("libvpx archive digest mismatch")
    with tarfile.open(archive) as tar:
        tar.extractall(out, filter="data")
build.mkdir(exist_ok=True)
if not (build / "libvpx_srcs.txt").exists():
    # Generic C has no OS-specific assembly or pthread ABI. cc-rs selects the
    # actual target compiler after configure has selected the decoder sources.
    configure = source / "configure"
    path = configure.as_posix()
    if os.name == "nt":
        path = "/" + path[0].lower() + path[2:]
    configure_env = os.environ.copy()
    if os.name == "nt":
        configure_env["CC"] = "clang"
    subprocess.run(["bash", path, "--target=generic-gnu", "--disable-examples",
                    "--disable-tools", "--disable-docs", "--disable-unit-tests",
                    "--disable-vp8", "--disable-vp9-encoder", "--disable-multithread",
                    "--disable-runtime-cpu-detect", "--disable-webm-io"], cwd=build, check=True, env=configure_env)
    subprocess.run(["make", "libvpx_srcs.txt"], cwd=build, check=True, env=configure_env)

if os.name == "nt":
    config = build / "vpx_config.h"
    text = config.read_text().replace("#define HAVE_UNISTD_H 1", "#define HAVE_UNISTD_H 0").replace("#define HAVE_PTHREAD_H 1", "#define HAVE_PTHREAD_H 0")
    config.write_text(text)
