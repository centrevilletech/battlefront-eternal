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

ROOT = Path(__file__).resolve().parents[1]
INSTALL = (Path(os.environ["BFE_GAME_DIR"]) if os.environ.get("BFE_GAME_DIR") else ROOT / "data" / "game" / "install") / "GameData"
GAME = INSTALL / "Data" / "_LVL_PC"
PUBLIC = Path(os.environ["BFE_CONTENT_DIR"]) if os.environ.get("BFE_CONTENT_DIR") else ROOT / "public"


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
