"""Where the tools read the retail game from.

By default, the Steam installation copied whole into data/game/install
(gitignored). BFE_GAME_DIR reads an installation in place instead: the folder
that holds GameData, such as Steam's "Star Wars Battlefront (Classic 2004)".
BFE_CONTENT_DIR moves the output, public/ by default, to another folder
(the desktop app's game data folder). The desktop app sets both.
"""

import os
from contextlib import contextmanager
from pathlib import Path
from uuid import uuid4

import zero_family

FAMILY = zero_family.current()
PROFILE = zero_family.profile(FAMILY)
ROOT = Path(__file__).resolve().parents[1]
INSTALL = (Path(os.environ["BFE_GAME_DIR"]) if os.environ.get("BFE_GAME_DIR") else ROOT.joinpath(*PROFILE.default_install_subpath)) / "GameData"
GAME = INSTALL.joinpath(*PROFILE.data_subpath)
PUBLIC = Path(os.environ["BFE_CONTENT_DIR"]) if os.environ.get("BFE_CONTENT_DIR") else ROOT / "public"


def write_text_lf(path, text):
    """Write prepared text (JSON) with LF bytes on every platform: default
    text mode would translate newlines to CRLF on Windows, and a Windows
    preparation must produce the same bytes as a Linux one."""
    path.write_text(text, encoding="utf-8", newline="\n")


def atomic_write(path, data, label, *, sync=False):
    """Publish a complete file with ordinary creation permissions, including the caller's umask."""
    part = path.with_name(f".bfe-{label}-{uuid4().hex}.part")
    handle = part.open("xb")
    try:
        with handle:
            handle.write(data)
            if sync:
                handle.flush()
                os.fsync(handle.fileno())
        part.replace(path)
    finally:
        part.unlink(missing_ok=True)


def child(base, *parts):
    """base/parts, each part matched without regard to case where the exact
    spelling isn't there, and spelled as on disk; None when one is missing.
    On disk because outputs record these paths (zero_addon.source_name):
    Windows finds "_LVL_PC" and "SIDE" in AddOn/Tat3, which spells them
    "_lvl_pc" and "Side", and must record what Linux does, or the content
    fingerprint differs."""
    at = Path(base)
    for part in parts:
        try:
            names = [entry.name for entry in at.iterdir()]
        except OSError:
            return None
        name = part if part in names else next((name for name in names if name.lower() == part.lower()), None)
        if name is None:
            return None
        at = at / name
    return at


if FAMILY == zero_family.SWBF2:
    parent = INSTALL.parent.resolve()
    # Helpers start in the project directory. Give them the caller's exact
    # installation and output locations before the working directory changes.
    if os.environ.get("BFE_GAME_DIR"):
        os.environ["BFE_GAME_DIR"] = str(parent)
    if os.environ.get("BFE_CONTENT_DIR"):
        PUBLIC = PUBLIC.absolute()
        os.environ["BFE_CONTENT_DIR"] = str(PUBLIC)
    INSTALL = child(parent, "GameData") or parent / "GameData"
    GAME = child(INSTALL, *PROFILE.data_subpath) or INSTALL.joinpath(*PROFILE.data_subpath)


@contextmanager
def shared_lock(name):
    """Holds <PUBLIC>/.locks/<name>.lock exclusively across processes, so
    parallel extractions (tools/prepare.py) take turns writing what they
    share: a side's units, common/, the audio index and streams."""
    path = PUBLIC / ".locks" / f"{name}.lock"
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a+b") as handle:
        if os.name == "nt":
            import msvcrt
            import time
            handle.seek(0)
            while True:
                try:
                    msvcrt.locking(handle.fileno(), msvcrt.LK_NBLCK, 1)
                    break
                except OSError:
                    time.sleep(0.2)
            try:
                yield
            finally:
                handle.seek(0)
                msvcrt.locking(handle.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl
            fcntl.flock(handle.fileno(), fcntl.LOCK_EX)
            try:
                yield
            finally:
                fcntl.flock(handle.fileno(), fcntl.LOCK_UN)
