"""Which language's files a copy of the game has, and which the tools read.

Two archives come per language, each in a folder named for it: the shared
one, Common/<language>/common.lvl, and the front end's,
Shell/<language>/shell.lvl (an add-on's movie config is its own
Shell/<language>/shell.lvl). Everything else is one file whatever the
language: the text (core.lvl holds a table per language; the Steam copy's
has english, french, german, italian, spanish and uk_english), the fonts
(core.lvl's, which have the accented letters), the sound and voices
(Sound/), the movies (Movies/), the loading pictures, worlds and armies.

The Steam copy holds five folders: ENG, FRE, GER, ITA and SPA. ENG's
archives are complete. Each of the others holds one picture: the victory
and defeat banner in Common, the legal screen in Shell (neither is used
here). Copies sold in one language are reported to hold that language's
folder alone, complete: FRE and no ENG.

So the tools read English where a copy has it, and nothing changes for
such a copy. Otherwise they read the folder whose archive is the largest (a
complete one rather than a one-picture one), the first by name among
equals. Common and Shell are chosen each by itself, so a copy with one
folder renamed to ENG and the other not prepares too. Folder and archive
names match without regard to case (child in tools/zero_paths.py). What is
prepared doesn't say which folder it came from (recorded), so the same
archives give the same content whatever their folder is called.

The text is read in English as well where core.lvl has it, as the Steam
copy's does: the bots' names and the labels are part of what players must
share to meet online (the content fingerprint, docs/desktop.md). A core.lvl
without English text is read in the copy's own language, else in the first
it has.
"""

from pathlib import Path

import zero_family
from zero_locl import languages
from zero_paths import GAME, child

ENGLISH = "ENG"
# A language's folder and its text's name in core.lvl, for the five the Steam
# copy has. Any other folder is read just the same, and named as spelled.
TEXT = {"ENG": "english", "FRE": "french", "GER": "german", "ITA": "italian", "SPA": "spanish"}


def size(path):
    try:
        return path.stat().st_size
    except OSError:
        return 0


def archive(folder, name):
    """folder/<language>/name for the language the tools read (English, else
    the largest), spelled as on disk; None when no language folder there
    holds one."""
    if folder is None:
        return None
    # As the tools spell it first, so a copy that has it reads as it always
    # did, on a file system that matches names exactly or not.
    english = Path(folder) / ENGLISH / name
    if english.is_file():
        return english
    try:
        entries = sorted((entry for entry in Path(folder).iterdir() if entry.is_dir()), key=lambda entry: entry.name.lower())
    except OSError:
        return None
    found = [path for path in (child(entry, name) for entry in entries) if path and path.is_file()]
    return next((path for path in found if path.parent.name.upper() == ENGLISH), max(found, key=size, default=None))


def common_archive(game=GAME, family=None):
    """The shared archive. For swbf1, in the language folder (English when
    none, for the message); for swbf2, directly under _lvl_pc."""
    if not zero_family.profile(family).language_folders:
        return Path(game) / "common.lvl"
    return archive(Path(game) / "Common", "common.lvl") or Path(game) / "Common" / ENGLISH / "common.lvl"


def shell_archive(game=GAME, family=None):
    """The front end's archive, as common_archive."""
    if not zero_family.profile(family).language_folders:
        return Path(game) / "shell.lvl"
    return archive(Path(game) / "Shell", "shell.lvl") or Path(game) / "Shell" / ENGLISH / "shell.lvl"


def recorded(path, family=None):
    """A language archive as outputs record it: under the English folder's
    name whatever the copy's is and however it is spelled
    ("Common/ENG/common.lvl" for Common/FRE/common.lvl and for
    Common/eng/COMMON.LVL). Families without language folders record the
    archive's name directly ("common.lvl"). The record is in the content
    fingerprint, so the same archive must give the same record: a folder's
    name is not content, and the archive's checksum beside the name says
    which it is."""
    path = Path(path)
    if not zero_family.profile(family).language_folders:
        return path.name.lower()
    return f"{path.parent.parent.name}/{ENGLISH}/{path.name.lower()}"


def text(core, game=GAME, family=None):
    """The language the tools read core.lvl's text in (core: its bytes):
    english where it has it, else the language of the copy's Common folder,
    else the first it has."""
    have = languages(core)
    if not zero_family.profile(family).language_folders:
        return "english" if "english" in have else (have[0] if have else "english")
    own = TEXT.get(common_archive(game, family=family).parent.name.upper())
    return next((name for name in ("english", own) if name in have), have[0] if have else "english")


def named(code):
    """A language as a player reads it: "French" for FRE or french, any
    other as it is spelled."""
    name = TEXT.get(code.upper(), code)
    return name.replace("_", " ").capitalize() if name.islower() else name


def found(game=GAME, family=None):
    """What a copy has and the tools read, for the check's report and the
    run's log: the folders chosen in Common and Shell (None when there is
    none), the text's language, and the sentence saying so."""
    game = Path(game)
    if not zero_family.profile(family).language_folders:
        try:
            words = text((game / "core.lvl").read_bytes(), game, family=family)
        except OSError:
            words = None
        message = (f"This copy's game files are in {named(words)}." if words
                   else "This copy's game files have no language folders.")
        return {"common": None, "shell": None, "text": words, "message": message}
    common, shell = archive(game / "Common", "common.lvl"), archive(game / "Shell", "shell.lvl")
    try:
        words = text((game / "core.lvl").read_bytes(), game, family=family)
    except OSError:
        words = None
    parts = [(named(path.parent.name), f"{part}/{path.parent.name}") for part, path in (("Common", common), ("Shell", shell)) if path]
    names = list(dict.fromkeys(name for name, _ in parts))
    if not parts:
        message = "This copy has no language folder with the game's shared files in it."
    elif names == [named(ENGLISH)]:
        message = "This copy's game files are in English."
    elif len(names) == 1:
        message = f"This copy's game files are in {names[0]} ({' and '.join(folder for _, folder in parts)})."
    else:
        message = f"This copy's game files are in {' and '.join(f'{name} ({folder})' for name, folder in parts)}."
    if parts and words and named(words) not in names:
        message += f" Menus and messages are shown in {named(words)}."
    return {"common": common.parent.name if common else None, "shell": shell.parent.name if shell else None, "text": words,
            "message": message}
