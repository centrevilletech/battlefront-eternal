#!/usr/bin/env python3
"""Run the release app's startup, menu and synthetic room on software OpenGL."""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--client-bin", required=True)
    parser.add_argument("--output-dir", required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    spec = importlib.util.spec_from_file_location("renderer_fixture", repo / "tools/renderer-smoke-native.py")
    fixture = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(fixture)
    output = Path(args.output_dir).resolve()
    output.mkdir(parents=True, exist_ok=True)
    # A repeated run cannot pass using an earlier process's captures or summary.
    for name in ("startup.png", "main-menu.png", "scene.png", "summary.json", "stdout.log", "stderr.log"):
        (output / name).unlink(missing_ok=True)
    client = Path(args.client_bin).resolve()
    if not client.is_file():
        raise RuntimeError(f"release client missing: {client}")
    # Keep desktop settings and fixture content isolated from the runner's profile.
    with tempfile.TemporaryDirectory(prefix="bfe-platform-smoke-") as temporary:
        root = Path(temporary)
        content = root / "content"
        fixture.write_synthetic_content(content)
        profile = root / "profile"
        profile.mkdir()
        (profile / "player-settings.json").write_text(json.dumps({
            "complete": True, "profileName": "Smoke", "privacyNoticeAccepted": True,
            "values": {"menuTheme": "eternal", "textureCompression": False},
        }), encoding="utf-8")
        env = dict(os.environ, BFE_USER_DATA_DIR=str(profile), XDG_CONFIG_HOME=str(root),
                   LIBGL_ALWAYS_SOFTWARE="1", GALLIUM_DRIVER="llvmpipe", LP_NUM_THREADS="2")
        command = [str(client), "--content-dir", str(content), "--no-audio", "--skip-update-check",
                   "--desktop-gl", "--width", "640", "--height", "360", "--script", "platform-smoke",
                   "--shots-dir", str(output)]
        if sys.platform.startswith("linux") and not env.get("DISPLAY"):
            xvfb = shutil.which("xvfb-run")
            if not xvfb:
                raise RuntimeError("xvfb-run is required for the Linux window smoke")
            command = [xvfb, "-a", "-s", "-screen 0 1280x720x24"] + command
        process = subprocess.Popen(command, cwd=repo, env=env, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, start_new_session=os.name == "posix")
        try:
            stdout, stderr = process.communicate(timeout=150)
        except subprocess.TimeoutExpired as error:
            # Xvfb and the app belong to this diagnostic's private process group.
            if os.name == "posix":
                os.killpg(process.pid, signal.SIGKILL)
            else:
                process.kill()
            stdout, stderr = process.communicate()
            (output / "stdout.log").write_bytes(stdout)
            (output / "stderr.log").write_bytes(stderr)
            raise RuntimeError("release app did not complete the platform smoke in 150 seconds") from error
        (output / "stdout.log").write_bytes(stdout)
        (output / "stderr.log").write_bytes(stderr)
        if process.returncode:
            raise RuntimeError(f"release app failed ({process.returncode}): {stderr.decode(errors='replace')}")
        summary = json.loads((output / "summary.json").read_text(encoding="utf-8"))
        if not summary["ok"] or summary["glErrors"] != 0 or "llvmpipe" not in summary["renderer"].lower():
            raise RuntimeError(f"software GL smoke failed: {summary}")
        if [f["stage"] for f in summary["frames"]] != ["startup", "main-menu", "scene"]:
            raise RuntimeError(f"app did not render every required stage: {summary}")
        for frame in summary["frames"]:
            fixture.verify_png_file(output / (frame["stage"] + ".png"), *frame["size"])
        print(json.dumps(summary))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as error:
        print(f"platform smoke: {error}", file=sys.stderr)
        sys.exit(1)
