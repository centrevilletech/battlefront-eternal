"""Paths recorded in prepared files keep POSIX spellings on every platform.

A relative path spelled through str(Path), os.fspath, os.path.join,
os.path.relpath or os.sep carries backslashes on Windows, and the readers
of prepared files (for example the interface catalog check) reject those,
so a copy prepared on Windows would not load. This scan reads the syntax
tree of every preparation tool module that writes or serializes a prepared
file (the list is derived from the tools below, never hand-listed, and
extends to the helper modules those writers import) and fails on any such
host spelling that flows into a recorded value: an assignment, a dict or
container literal or comprehension, a subscript store, a dict() call, a
container-recording call (append, update, ...), or a returned or yielded
value a caller records. Command lines (subprocess calls and command, cmd
or argv lists), sys.path and child-process environments name this machine
in its own spelling and are not recorded values; f-strings are not scanned,
because a path interpolation cannot be told from text without types, so
the Windows-versus-Linux preparation pair remains the check for those.
Machine-local absolute installation records are the one intentional
exception class: each live site is listed in ALLOWED with its reason, and
an entry that no longer matches a site fails the test.
"""
import ast
import unittest
from pathlib import Path

TOOLS = Path(__file__).resolve().parents[1] / "tools"

PATH_NAMES = {
    "archive", "dest", "directory", "file", "folder", "game", "install",
    "location", "out", "p", "path", "paths", "rel", "root", "source",
    "target",
}
PATH_METHODS = {
    "absolute", "expanduser", "joinpath", "relative_to", "resolve",
    "with_name", "with_stem", "with_suffix",
}
PATH_TYPES = {"Path", "PurePath", "PurePosixPath", "PureWindowsPath"}
WRITE_CALLS = {
    "publish", "write_bytes", "write_if_changed", "write_json",
    "write_text", "write_text_lf", "writelines",
}
RECORDERS = {"add", "append", "extend", "insert", "setdefault", "update"}
COMMAND_VARS = {"argv", "cmd", "command"}
LITERALS = (
    ast.Dict, ast.List, ast.Tuple, ast.Set,
    ast.ListComp, ast.SetComp, ast.DictComp, ast.GeneratorExp,
)

# (module, text on the site's source line) -> why the host spelling is
# correct there. Every entry must match a live site; see the test.
ALLOWED = {
    ("enhance_audio", "loop, str(OUT), digest)"):
        "a worker-process job tuple naming this machine's absolute output "
        "folder; the worker opens it as a path and it is never written "
        "into a prepared file",
    ("enhance_audio", "kind, loop, str(out)"):
        "arguments for the sample encoder job, naming this machine's "
        "output folder for the worker to open",
    ("enhance_audio", "loop, str(OUT))"):
        "a worker-process job tuple naming this machine's absolute output "
        "folder; the worker opens it as a path and it is never written "
        "into a prepared file",
    ("ktx2", "self.path = str(path)"):
        "the texture encoder executable's path on this machine, kept for "
        "its command line",
    ("ktx2", "str(Path(path).resolve().relative_to(ROOT))"):
        "shown() spells paths for log lines only",
    ("ktx2", "return str(path)"):
        "shown() spells paths for log lines only",
    ("prepare", '"game": str(GAME)'):
        "the preparation state records this machine's absolute game "
        "folder, compared only when resuming on the same machine",
    ("prepare", 'files["@installation"] = [str(game)'):
        "the health inventory records this machine's absolute install "
        "folder and is read back on the same machine",
    ("prepare", 'files["@missionFiles"] = {str(p):'):
        "the health inventory is keyed by this machine's absolute install "
        "file paths and is read back on the same machine",
    ("prepare_check", '"game": str(install_root(game))'):
        "the check report identifies this machine's absolute install to "
        "the person running the check",
    ("zero_fp2", "cache_key = str(archive_path)"):
        "the key of the two-entry archive cache inside one preparation run; "
        "it is never written into a prepared file",
    ("zero_outputs2", "else str(game)"):
        "a fresh receipt records this machine's absolute game folder; a "
        "resumed one keeps the spelling it recorded, compared only by "
        "same-file when resuming on the same machine",
    ("zero_paths", 'os.environ["BFE_GAME_DIR"] = str(parent)'):
        "a child-process environment value naming a folder on this "
        "machine in its own spelling",
    ("zero_paths", 'os.environ["BFE_CONTENT_DIR"] = str(PUBLIC)'):
        "a child-process environment value naming a folder on this "
        "machine in its own spelling",
    ("zero_shell", "archive = str(source)"):
        "the fallback for a movie archive outside the install: its "
        "absolute path on this machine, not a prepared relative path",
    ("zero_shell", "missing = [str(p) for p in paths.values()"):
        "install paths printed in a missing-file diagnostic only",
    ("zero_sky", "key = (str(path),"):
        "an in-memory lookup key only, never written out",
    ("zero_sound_health", "result[str(path)] ="):
        "the health inventory is keyed by this machine's absolute install "
        "file paths and is read back on the same machine",
}


def _is_path_name(name):
    name = name.lower()
    return (
        name in PATH_NAMES
        or name.endswith(("_path", "_paths", "_dir", "_file", "_root", "_folder"))
        or name.startswith(("path_", "dir_", "file_"))
    )


def _final_name(func):
    if isinstance(func, ast.Name):
        return func.id
    if isinstance(func, ast.Attribute):
        return func.attr
    return None


def _dotted(expr):
    parts = []
    while isinstance(expr, ast.Attribute):
        parts.append(expr.attr)
        expr = expr.value
    if isinstance(expr, ast.Name):
        parts.append(expr.id)
    return ".".join(reversed(parts))


def _is_pathlike(expr, tainted):
    """True when expr structurally holds a filesystem path."""
    if isinstance(expr, ast.Constant):
        return False
    if isinstance(expr, ast.Call):
        if isinstance(expr.func, ast.Attribute) and expr.func.attr == "as_posix":
            return False  # already a POSIX string
        name = _final_name(expr.func)
        if name in PATH_TYPES:
            return True
        if isinstance(expr.func, ast.Attribute) and expr.func.attr in PATH_METHODS:
            return True
        if _dotted(expr.func).startswith("os.path."):
            return True
        return bool(name) and _is_path_name(name)
    if isinstance(expr, ast.Attribute):
        return expr.attr == "parent" or _is_path_name(expr.attr)
    if isinstance(expr, ast.Name):
        return expr.id in tainted or _is_path_name(expr.id)
    if isinstance(expr, ast.Subscript):
        return (
            isinstance(expr.slice, ast.Constant)
            and isinstance(expr.slice.value, str)
            and _is_path_name(expr.slice.value)
        )
    if isinstance(expr, ast.BinOp) and isinstance(expr.op, ast.Div):
        return True  # the path-joining operator
    if isinstance(expr, ast.IfExp):
        return _is_pathlike(expr.body, tainted) or _is_pathlike(expr.orelse, tainted)
    return False


def _tainted_names(tree):
    """Names assigned a path-like value anywhere in the module."""
    tainted = set()
    assigns = [
        node for node in ast.walk(tree)
        if isinstance(node, (ast.Assign, ast.AnnAssign))
    ]
    changed = True
    while changed:
        changed = False
        for node in assigns:
            if node.value is None or not _is_pathlike(node.value, tainted):
                continue
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            for target in targets:
                for part in ast.walk(target):
                    if isinstance(part, ast.Name) and part.id not in tainted:
                        tainted.add(part.id)
                        changed = True
    return tainted


def _imported_host_functions(tree):
    """Local names bound to os.path.join, os.path.relpath, os.fspath, os.sep."""
    joins, fspaths, seps = set(), set(), set()
    for node in ast.walk(tree):
        if not isinstance(node, ast.ImportFrom):
            continue
        for alias in node.names:
            local = alias.asname or alias.name
            if node.module == "os.path" and alias.name in ("join", "relpath"):
                joins.add(local)
            if node.module == "os" and alias.name == "fspath":
                fspaths.add(local)
            if node.module in ("os", "os.path") and alias.name == "sep":
                seps.add(local)
    return joins, fspaths, seps


def _recorded_host_spellings(tree):
    """Host path spellings that flow into a recorded value.

    Returns (line number, kind, source text) triples, outermost first.
    """
    tainted = _tainted_names(tree)
    joins, fspaths, seps = _imported_host_functions(tree)
    hits = []

    def host_kind(node):
        if isinstance(node, ast.Call):
            if (
                isinstance(node.func, ast.Name)
                and node.func.id == "str"
                and len(node.args) == 1
                and _is_pathlike(node.args[0], tainted)
            ):
                return "str()"
            dotted = _dotted(node.func)
            if (dotted == "os.fspath" or _final_name(node.func) in fspaths) \
                    and node.args and _is_pathlike(node.args[0], tainted):
                return "os.fspath"
            if dotted in ("os.path.join", "os.path.relpath") \
                    or _final_name(node.func) in joins:
                return dotted if dotted.startswith("os.") else "os.path.join/relpath"
        if isinstance(node, ast.Attribute) and _dotted(node) in ("os.sep", "os.path.sep"):
            return "os.sep"
        if isinstance(node, ast.Name) and node.id in seps:
            return "os.sep"
        return None

    def visit(node, recorded):
        if node is None:
            return
        if recorded:
            kind = host_kind(node)
            if kind:
                hits.append((node.lineno, kind, ast.unparse(node)))
                return
        if isinstance(node, ast.Assign):
            if any(
                isinstance(target, ast.Name) and target.id in COMMAND_VARS
                for target in node.targets
            ):
                visit(node.value, False)
                return
            for target in node.targets:
                if isinstance(target, ast.Subscript):
                    visit(target.value, False)
                    visit(target.slice, True)
                else:
                    visit(target, False)
            visit(node.value, True)
            return
        if isinstance(node, ast.AnnAssign):
            visit(node.target, False)
            visit(node.value, True)
            return
        if isinstance(node, ast.AugAssign):
            visit(node.target, False)
            visit(node.value, True)
            return
        if isinstance(node, (ast.Return, ast.Yield)):
            visit(node.value, True)
            return
        if isinstance(node, ast.Dict):
            for key, value in zip(node.keys, node.values):
                visit(key, recorded)
                visit(value, recorded)
            return
        if isinstance(node, (ast.List, ast.Tuple, ast.Set)):
            for element in node.elts:
                visit(element, recorded)
            return
        if isinstance(node, ast.JoinedStr):
            return
        if isinstance(node, (ast.ListComp, ast.SetComp, ast.GeneratorExp)):
            visit(node.elt, recorded)
            for generator in node.generators:
                visit(generator.iter, False)
                for condition in generator.ifs:
                    visit(condition, False)
            return
        if isinstance(node, ast.DictComp):
            visit(node.key, recorded)
            visit(node.value, recorded)
            for generator in node.generators:
                visit(generator.iter, False)
                for condition in generator.ifs:
                    visit(condition, False)
            return
        if isinstance(node, ast.Subscript) and isinstance(node.ctx, ast.Load):
            visit(node.value, recorded)
            visit(node.slice, False)
            return
        if isinstance(node, ast.Call):
            dotted = _dotted(node.func)
            visit(node.func, False)
            if dotted.startswith("subprocess.") or dotted.startswith("sys.path."):
                arguments_recorded = False
            elif _final_name(node.func) == "dict" and node.args \
                    and _dotted(node.args[0]) == "os.environ":
                arguments_recorded = False  # a child-process environment
            elif _final_name(node.func) == "dict" \
                    or _final_name(node.func) in RECORDERS:
                arguments_recorded = True
            else:
                arguments_recorded = None  # literals record, the rest does not
            for argument in node.args:
                if arguments_recorded is None:
                    visit(argument, isinstance(argument, LITERALS))
                else:
                    visit(argument, arguments_recorded)
            for keyword in node.keywords:
                visit(keyword.value, bool(arguments_recorded))
            return
        for child in ast.iter_child_nodes(node):
            visit(child, recorded)

    visit(tree, False)
    return hits


def _writes_prepared_file(tree):
    """True when the module writes a file or serializes data for one."""
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        name = _final_name(node.func)
        if name in WRITE_CALLS:
            return True
        if isinstance(node.func, ast.Attribute) and node.func.attr in ("dump", "dumps"):
            return True
        if name == "open":
            modes = []
            if isinstance(node.func, ast.Name) and len(node.args) >= 2:
                modes.append(node.args[1])
            if isinstance(node.func, ast.Attribute) and node.args:
                modes.append(node.args[0])
            if any(
                isinstance(mode, ast.Constant)
                and isinstance(mode.value, str)
                and any(letter in mode.value for letter in "wxa")
                for mode in modes
            ):
                return True
    return False


def _local_imports(tree, modules):
    imported = set()
    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            imported.update(alias.name.split(".")[0] for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.module:
            imported.add(node.module.split(".")[0])
    return imported & modules


def _covered_modules():
    """Every tool module that writes a prepared file, plus its helpers.

    Derived from the tools themselves: importable modules under tools/
    whose syntax tree writes or serializes, extended with the local
    modules those writers import directly (helpers hand recorded paths
    to writers without writing themselves).
    """
    modules = {
        path.stem: path for path in TOOLS.glob("*.py") if path.stem.isidentifier()
    }
    writers = set()
    helpers = set()
    for stem, path in modules.items():
        tree = ast.parse(path.read_text(encoding="utf-8"))
        if _writes_prepared_file(tree):
            writers.add(stem)
            helpers.update(_local_imports(tree, set(modules)))
    return {stem: modules[stem] for stem in sorted(writers | helpers)}


class PathSpellingTests(unittest.TestCase):
    def test_no_recorded_path_is_spelled_by_the_host(self):
        covered = _covered_modules()
        # The derived list must keep covering the writers and helpers a
        # hand-written list once missed.
        for stem in (
            "zero_fx2", "zero_scripts2", "zero_mission2", "zero_vehicle2",
            "zero_ai", "prepare_check", "zero_addon",
        ):
            self.assertIn(stem, covered)
        matched = set()
        problems = []
        for stem, path in covered.items():
            source = path.read_text(encoding="utf-8")
            lines = source.splitlines()
            for lineno, kind, text in _recorded_host_spellings(ast.parse(source)):
                line = lines[lineno - 1] if lineno <= len(lines) else ""
                entry = next(
                    (
                        key for key in ALLOWED
                        if key[0] == stem and key[1] in line
                    ),
                    None,
                )
                if entry is None:
                    problems.append(
                        f"{stem}:{lineno}: recorded {kind} host spelling: {text}"
                    )
                else:
                    matched.add(entry)
        for stem, snippet in sorted(set(ALLOWED) - matched):
            problems.append(
                f"ALLOWED entry matches no site: ({stem!r}, {snippet!r})"
            )
        self.assertEqual(problems, [], "\n".join(problems))

    def test_battle_catalogue_paths_are_posix(self):
        source = (TOOLS / "zero_battle_ui2.py").read_text(encoding="utf-8")
        self.assertEqual(source.count("path.as_posix()"), 5)
        self.assertNotIn("str(path)", source)


if __name__ == "__main__":
    unittest.main()
