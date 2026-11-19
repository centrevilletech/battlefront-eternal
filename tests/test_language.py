"""tools/zero_language.py: which language's files a copy of the game has, and
which the tools read. Synthetic folder trees only: the archives are a header
and filler, the text is made here, nothing retail."""
import os
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "tools"))
sys.path.insert(0, str(HERE))
import prepare_check  # noqa: E402
import zero_addon  # noqa: E402
import zero_language  # noqa: E402
from test_addon_import import addme, chunk, ucfb, write  # noqa: E402
from test_prepare_check import Install  # noqa: E402
from zero_hash import fnv1a  # noqa: E402
from zero_locl import languages, localize  # noqa: E402

EXACT_NAMES = os.path.normcase("A") != os.path.normcase("a") and not Path(__file__.upper()).exists()


def archive(size=0):
    """An archive of about size bytes: one complete, or one picture."""
    return ucfb(chunk(b"tex_", bytes(size)))


def text(**tables):
    """core.lvl's text in several languages: language -> {key: text}."""
    def body(texts):
        return b"".join(struct.pack("<IH", fnv1a(key), 6 + 2 * len(words) + 2) + (words + "\0").encode("utf-16-le")
                        for key, words in texts.items())
    return ucfb(*(chunk(b"Locl", chunk(b"NAME", name.encode() + b"\0") + chunk(b"BODY", body(texts))) for name, texts in tables.items()))


class Tree(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.game = Path(self.temp.name) / "GameData" / "Data" / "_LVL_PC"
        self.game.mkdir(parents=True)

    def put(self, name, size=0):
        return write(self.game / name, archive(size))

    def chosen(self):
        return zero_language.common_archive(self.game), zero_language.shell_archive(self.game)


class Folders(Tree):
    def test_english_is_read_where_a_copy_has_it(self):
        # As the Steam copy is laid out: English complete, the others one picture each.
        for folder, size in (("ENG", 4000), ("FRE", 40), ("GER", 40), ("ITA", 40), ("SPA", 40)):
            self.put(f"Common/{folder}/common.lvl", size)
            self.put(f"Shell/{folder}/shell.lvl", size)
        self.assertEqual(self.chosen(), (self.game / "Common/ENG/common.lvl", self.game / "Shell/ENG/shell.lvl"))
        # English even when another language's archive is the larger.
        self.put("Common/GER/common.lvl", 9000)
        self.assertEqual(self.chosen()[0], self.game / "Common/ENG/common.lvl")
        self.assertEqual(zero_language.found(self.game)["message"], "This copy's game files are in English.")

    def test_a_copy_in_one_other_language(self):
        self.put("Common/FRE/common.lvl", 4000)
        self.put("Shell/FRE/shell.lvl", 4000)
        self.assertEqual(self.chosen(), (self.game / "Common/FRE/common.lvl", self.game / "Shell/FRE/shell.lvl"))
        found = zero_language.found(self.game)
        self.assertEqual((found["common"], found["shell"]), ("FRE", "FRE"))
        self.assertEqual(found["message"], "This copy's game files are in French (Common/FRE and Shell/FRE).")
        self.assertEqual(zero_addon.source_name(self.chosen()[0], self.game), "Common/FRE/common.lvl")

    def test_several_languages_and_no_english_reads_the_complete_one(self):
        for folder, size in (("FRE", 40), ("GER", 4000), ("SPA", 4000)):
            self.put(f"Common/{folder}/common.lvl", size)
        self.put("Shell/ITA/shell.lvl", 4000)
        self.put("Shell/SPA/shell.lvl", 4000)
        common, shell = self.chosen()
        self.assertEqual(common, self.game / "Common/GER/common.lvl", "the largest, and the first by name of two as large")
        self.assertEqual(shell, self.game / "Shell/ITA/shell.lvl", "each folder by itself")
        self.assertEqual(zero_language.found(self.game)["message"],
                         "This copy's game files are in German (Common/GER) and Italian (Shell/ITA).")

    def test_a_half_renamed_copy(self):
        # A French copy whose Common folder was renamed to ENG and whose Shell folder was not.
        self.put("Common/ENG/common.lvl", 4000)
        self.put("Shell/FRE/shell.lvl", 4000)
        self.assertEqual(self.chosen(), (self.game / "Common/ENG/common.lvl", self.game / "Shell/FRE/shell.lvl"))
        self.assertEqual(zero_language.found(self.game)["message"],
                         "This copy's game files are in English (Common/ENG) and French (Shell/FRE).")

    def test_names_match_without_regard_to_case(self):
        self.put("Common/eng/COMMON.LVL", 40)
        self.put("Common/Fre/common.lvl", 4000)
        self.put("Shell/fre/Shell.lvl", 4000)
        common, shell = self.chosen()
        self.assertTrue(common.is_file() and shell.is_file())
        self.assertEqual(common.parent.name.upper(), "ENG", "English first, however it is spelled")
        self.assertEqual(zero_language.recorded(common), "Common/ENG/common.lvl", "recorded as every copy records it")
        self.assertEqual(zero_addon.source_name(common, self.game), "Common/ENG/common.lvl")
        self.assertEqual((shell.parent.name, shell.name), ("fre", "Shell.lvl"), "spelled as on disk")
        self.assertEqual(zero_language.recorded(shell), "Shell/FRE/shell.lvl")
        self.assertEqual(zero_language.found(self.game)["message"],
                         "This copy's game files are in English (Common/eng) and French (Shell/fre)."
                         if EXACT_NAMES else "This copy's game files are in English (Common/ENG) and French (Shell/fre).")

    def test_a_folder_of_any_name_is_a_language(self):
        self.put("Common/POL/common.lvl", 4000)
        self.put("Shell/POL/shell.lvl", 4000)
        write(self.game / "Common" / "readme.txt")
        (self.game / "Common" / "empty").mkdir()
        self.assertEqual(self.chosen()[0], self.game / "Common/POL/common.lvl")
        self.assertEqual(zero_language.found(self.game)["message"], "This copy's game files are in POL (Common/POL and Shell/POL).")

    def test_no_language_folder(self):
        write(self.game / "Common" / "common.lvl", archive())  # the 2005 game's layout
        self.assertIsNone(zero_language.archive(self.game / "Common", "common.lvl"))
        self.assertIsNone(zero_language.archive(self.game / "Shell", "shell.lvl"), "no folder at all")
        self.assertIsNone(zero_language.archive(None, "shell.lvl"))
        self.assertEqual(self.chosen()[0], self.game / "Common/ENG/common.lvl", "the English path, for the message")
        found = zero_language.found(self.game)
        self.assertEqual((found["common"], found["shell"], found["text"]), (None, None, None))


class Text(Tree):
    KEY = "common.yes"

    def read(self):
        return localize(zero_addon.localization(self.game), self.KEY)

    def test_english_text_is_read_where_the_copy_has_it(self):
        self.put("Common/FRE/common.lvl")
        self.put("Shell/FRE/shell.lvl")
        core = write(self.game / "core.lvl", text(french={self.KEY: "OUI"}, english={self.KEY: "YES"}, german={self.KEY: "JA"}))
        self.assertEqual(languages(core.read_bytes()), ["french", "english", "german"])
        self.assertEqual(self.read(), "YES")
        found = zero_language.found(self.game)
        self.assertEqual(found["text"], "english")
        self.assertEqual(found["message"], "This copy's game files are in French (Common/FRE and Shell/FRE). "
                                           "Menus and messages are shown in English.")

    def test_a_copy_without_english_text_is_read_in_its_own_language(self):
        self.put("Common/FRE/common.lvl")
        self.put("Shell/FRE/shell.lvl")
        write(self.game / "core.lvl", text(german={self.KEY: "JA"}, french={self.KEY: "OUI"}))
        self.assertEqual(self.read(), "OUI")
        self.assertEqual(zero_language.found(self.game)["message"], "This copy's game files are in French (Common/FRE and Shell/FRE).")
        # A language this module has no name for: the first the archive has.
        (self.game / "Common" / "FRE").rename(self.game / "Common" / "POL")
        self.assertEqual(self.read(), "JA")

    def test_an_add_on_is_read_in_the_games_language(self):
        self.put("Common/FRE/common.lvl")
        write(self.game / "core.lvl", text(french={self.KEY: "OUI"}))
        root = self.game.parents[1] / "AddOn" / "Abc"
        write(root / "addme.script", addme("ABC1", "ABC: TEST", "r"))
        write(root / "Data" / "_lvl_pc" / "core.lvl", text(english={"level.abc1.cp1": "THRONE ROOM"}, french={"level.abc1.cp1": "SALLE DU TRONE"}))
        self.assertEqual(localize(zero_addon.localization(self.game), "level.abc1.cp1"), "SALLE DU TRONE")

    def test_an_add_ons_movie_config_is_chosen_like_the_games(self):
        root = self.game.parents[1] / "AddOn" / "Abc"
        write(root / "addme.script", addme("ABC1", "ABC: TEST", "r"))
        lvl = root / "Data" / "_lvl_pc"
        movies = write(lvl / "Movies" / "abc.mvs")
        self.assertEqual(zero_addon.movie_archives(self.game), [(movies, None)])
        empty = write(lvl / "Shell" / "ENG" / "shell.lvl", ucfb())
        config = write(lvl / "Shell" / "FRE" / "shell.lvl", archive(40))
        self.assertEqual(zero_addon.movie_archives(self.game), [(movies, empty)], "English where the add-on has it")
        empty.unlink()
        self.assertEqual(zero_addon.movie_archives(self.game), [(movies, config)])


class Check(unittest.TestCase):
    """The installation's check (tools/prepare_check.py) over a copy in
    another language."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.install = Install(Path(self.temp.name) / "Star Wars Battlefront")
        self.game = self.install.game
        patcher = mock.patch("zero_foliage.placement_table", lambda *_: (0.0,) * 256)
        patcher.start()
        self.addCleanup(patcher.stop)

    def rename(self, folder, name):
        (self.game / folder / "ENG").rename(self.game / folder / name)

    def test_a_copy_in_another_language_passes_and_is_named(self):
        self.rename("Common", "FRE")
        self.rename("Shell", "FRE")
        report = prepare_check.check(self.game)
        self.assertEqual((report["ok"], report["problems"], report["maps"]), (True, [], ["geo1"]))
        self.assertEqual(report["language"], {"common": "FRE", "shell": "FRE", "text": "english", "message":
                                              "This copy's game files are in French (Common/FRE and Shell/FRE). "
                                              "Menus and messages are shown in English."})

    def test_a_half_renamed_copy_in_small_letters_passes(self):
        self.rename("Shell", "fre")
        report = prepare_check.check(self.game)
        self.assertEqual((report["ok"], report["problems"]), (True, []))
        self.assertEqual((report["language"]["common"], report["language"]["shell"]), ("ENG", "fre"))

    def test_an_english_copy_says_so(self):
        report = prepare_check.check(self.game)
        self.assertTrue(report["ok"])
        self.assertEqual(report["language"]["message"], "This copy's game files are in English.")

    def test_no_language_folder_is_the_problem(self):
        (self.game / "Common" / "ENG" / "common.lvl").rename(self.game / "Common" / "common.lvl")
        found = prepare_check.check(self.game)["problems"]
        self.assertEqual([(entry["code"], entry["path"]) for entry in found], [("language", "GameData/Data/_LVL_PC/Common")])
        self.assertEqual(found[0]["message"], "GameData/Data/_LVL_PC/Common has no language folder (such as ENG or FRE) with common.lvl in it.")

    def test_a_damaged_archive_in_the_copys_language_is_named(self):
        self.rename("Shell", "GER")
        write(self.game / "Shell" / "GER" / "shell.lvl", b"not an archive")
        found = prepare_check.check(self.game)["problems"]
        self.assertEqual([(entry["code"], entry["path"]) for entry in found], [("damaged", "GameData/Data/_LVL_PC/Shell/GER/shell.lvl")])

    @unittest.skipUnless(EXACT_NAMES, "the file system matches names without regard to case")
    def test_a_common_folder_spelled_otherwise_is_named_itself(self):
        self.rename("Common", "FRE")
        (self.game / "Common").rename(self.game / "common")
        found = prepare_check.check(self.game)["problems"]
        self.assertEqual([(entry["code"], entry["path"]) for entry in found], [("misnamed", "GameData/Data/_LVL_PC/Common")])
        self.assertIn("is spelled common in this copy, where it must be Common", found[0]["message"])


if __name__ == "__main__":
    unittest.main()
