"""Per-user verification records for prepared sound. No audio dependencies."""

import errno
import hashlib
import json
import os
from pathlib import Path
import re
import tempfile
import shutil
import sys
import time

from zero_paths import atomic_write

VERSION = 2
TEMPORARY_FAILURE = 75


def permission_failure(error):
    return (isinstance(error, PermissionError)
            or isinstance(error, OSError) and error.errno in (errno.EACCES, errno.EPERM, errno.EROFS))


def retryable_io(error):
    return (isinstance(error, OSError) and not permission_failure(error)
            and not isinstance(error, (FileNotFoundError, NotADirectoryError, IsADirectoryError)))


def extraction_status(main):
    """Tell the scheduling process which extraction errors can clear on their own."""
    if os.environ.get("BFE_SOUND_REPAIR") != "1":
        return main()
    try:
        return main()
    except OSError as error:
        if not retryable_io(error):
            raise
        print(error, file=sys.stderr)
        return TEMPORARY_FAILURE


def record_path(root):
    cache = Path(os.environ.get("XDG_CACHE_HOME") or os.environ.get("LOCALAPPDATA") or Path.home() / ".cache")
    key = hashlib.sha256(os.fsencode(os.path.normcase(str(root.resolve())))).hexdigest()
    return cache / "battlefront-eternal" / "sound" / (key + ".json")


def load(root):
    try:
        record = json.loads(record_path(root).read_bytes())
        if isinstance(record, dict) and record.get("version") == VERSION:
            return record
    except (OSError, ValueError):
        pass
    return {"version": VERSION}


def can_record(root):
    """Check only when work is pending. Skip expensive verification if its result cannot be saved."""
    folder = record_path(root).parent
    try:
        folder.mkdir(parents=True, exist_ok=True)
        # A replacement and the previous verdicts must fit side by side.
        if shutil.disk_usage(folder).free < max(4 << 20, stamp(record_path(root))[0] or 0):
            raise OSError("insufficient free space for the sound verification record")
        with tempfile.TemporaryFile(dir=folder) as probe:
            probe.write(b"1")
            probe.flush()
        return True
    except OSError as error:
        print(f"Sound maintenance cannot save verification results in {folder}: {error}")
        return False


def retry_due(saved, audio, preparing=False, key="retryEnhanced"):
    retry = saved.get(key)
    if not isinstance(retry, dict):
        return False
    return (preparing or time.time() >= retry["after"]
            or shutil.disk_usage(audio).free > retry["free"] + (1 << 20))


def retry_later(audio, reason):
    return {"after": time.time() + 300, "free": shutil.disk_usage(audio).free, "reason": str(reason)}


def save(root, record):
    path = record_path(root)
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        atomic_write(path, json.dumps(record, separators=(",", ":")).encode(), "verdict")
    except OSError as error:
        print(f"Could not remember sound verification: {error}")


def stamp(path):
    try:
        info = os.stat(path)
        return [info.st_size, info.st_mtime_ns]
    except OSError:
        return [None, None]


def archive_inputs(game):
    """Watch the installation's source files only while an extraction is blocked."""
    from zero_addon import add_ons
    result = {}
    for root in [game, *(pack.root for pack in add_ons(game))]:
        for directory, _, files in os.walk(root):
            for name in files:
                if name.lower().endswith((".lvl", ".bnk")):
                    path = Path(directory) / name
                    result[str(path)] = [*stamp(path), os.access(path, os.R_OK)]
    return result


def inputs(audio, enhanced=False):
    """File metadata only. Include source banks so restoring one permits another repair."""
    paths = [audio / "index.json", *audio.glob("*/sound.json"), *audio.glob("*/bank.bin")]
    if enhanced:
        paths.extend((audio / "streams").glob("*"))
    result = {p.relative_to(audio).as_posix(): stamp(p) for p in paths}
    if not enhanced:
        # A killed writer can leave a sibling without changing the published file.
        result["@folders"] = {p.relative_to(audio).as_posix(): [*stamp(p), os.access(p, os.W_OK)]
                              for p in (audio, *audio.iterdir())
                              if p.is_dir() and p.name != "enhanced" and not p.is_symlink()}
    if enhanced:
        result["@sampleTreatments"] = 2
        for directory, _, files in os.walk(audio / "enhanced"):
            prefix = Path(directory).relative_to(audio).as_posix() + "/"
            if not os.access(directory, os.W_OK):
                result[prefix + "@writable"] = False
            for name in files:
                result[prefix + name] = stamp(os.path.join(directory, name))
    return result


def original_part(rel):
    return (rel == "audio/index.json.part"
            or re.fullmatch(r"audio/streams/[^/]+\.(?:bin|json)\.[0-9]+\.part", rel)
            or re.fullmatch(r"audio/[^/]+/\.bfe-bank-[a-z0-9_]{8}\.part", rel)
            or re.fullmatch(r"audio/[^/]+/\.bfe-(?:bank|manifest)-[0-9a-f]{32}\.part", rel))


def old_manifest_directory(rel):
    return re.fullmatch(r"audio/[^/]+/tmp[a-z0-9_]{8}", rel)


def clean_original_temporaries(audio):
    """Remove only staging files in the original banks, with the preparation lock held."""
    if audio.is_symlink():
        return
    for directory, children, files in os.walk(audio):
        folder = Path(directory)
        if folder == audio:
            children[:] = [name for name in children if name != "enhanced"]
        else:
            for name in children:
                path = folder / name
                if not old_manifest_directory(path.relative_to(audio.parent).as_posix()) or path.is_symlink():
                    continue
                try:
                    entries = list(path.iterdir())
                    if all(p.name == "sound.json" and p.is_file() and not p.is_symlink() for p in entries):
                        for p in entries:
                            p.unlink()
                        path.rmdir()
                except OSError as error:
                    print(f"Could not remove interrupted sound manifest {path}: {error}")
            children.clear()
        for name in files:
            path = folder / name
            if original_part(path.relative_to(audio.parent).as_posix()) and not path.is_symlink() and path.is_file():
                try:
                    path.unlink()
                except OSError as error:
                    print(f"Could not remove interrupted sound file {path}: {error}")


def clean_temporaries(enhanced):
    """Called under the preparation lock before starting any encoders."""
    if enhanced.is_symlink():
        return
    for directory, _, files in os.walk(enhanced):
        for name in files:
            if (re.fullmatch(r"\.[0-9a-f]{32}\.(?:part|opus)", name)
                    or re.fullmatch(r"[^/]+\.json\.[a-z0-9_]{8}\.part", name)):
                try:
                    (Path(directory) / name).unlink(missing_ok=True)
                except OSError as error:
                    print(f"Could not remove interrupted sound encode {name}: {error}")
