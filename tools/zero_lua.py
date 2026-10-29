"""Mission scripts: Lua 4.0 bytecode in mission.lvl.

The 2004 game's mission scripts are compiled Lua 4.0 chunks (header version
byte 0x40, 4-byte floats). A mission's setup is straight-line code in its
ScriptInit function: calls such as SetTeamName(1, "Republic"). Executing that
code symbolically recovers each call with its literal arguments, which is all
the port needs (teams, unit classes, counts, bleed thresholds, death regions).
"""

import struct

from ucfb import child, chunks, cstr_at

OPS = (
    "END", "RETURN", "CALL", "TAILCALL", "PUSHNIL", "POP", "PUSHINT", "PUSHSTRING", "PUSHNUM",
    "PUSHNEGNUM", "PUSHUPVALUE", "GETLOCAL", "GETGLOBAL", "GETTABLE", "GETDOTTED", "GETINDEXED",
    "PUSHSELF", "CREATETABLE", "SETLOCAL", "SETGLOBAL", "SETTABLE", "SETLIST", "SETMAP", "ADD",
    "ADDI", "SUB", "MULT", "DIV", "POW", "CONCAT", "MINUS", "NOT", "JMPNE", "JMPEQ", "JMPLT",
    "JMPLE", "JMPGT", "JMPGE", "JMPT", "JMPF", "JMPONT", "JMPONF", "JMP", "PUSHNILJMP", "FORPREP",
    "FORLOOP", "LFORPREP", "LFORLOOP", "CLOSURE",
)
SIZE_OP = 6
SIZE_B = 9
MAXARG_S = ((1 << (32 - SIZE_OP)) - 1) >> 1
POS_A = SIZE_OP + SIZE_B


class _Reader:
    def __init__(self, data):
        self.data = data
        self.at = 0

    def take(self, fmt):
        value = struct.unpack_from(fmt, self.data, self.at)[0]
        self.at += struct.calcsize(fmt)
        return value

    def string(self):
        n = self.take("<I")
        if n == 0:
            return None
        text = self.data[self.at:self.at + n - 1].decode("latin1")
        self.at += n
        return text


class Function:
    """One compiled Lua function: its constants, nested functions, and code."""

    def __init__(self, r):
        self.source = r.string()
        r.take("<i")  # line defined
        self.params = r.take("<i")
        r.take("<B")  # is_vararg
        r.take("<i")  # max stack
        self.locals = [(r.string(), r.take("<i"), r.take("<i")) for _ in range(r.take("<i"))]
        [r.take("<i") for _ in range(r.take("<i"))]  # line info
        self.strings = [r.string() for _ in range(r.take("<i"))]
        self.numbers = [r.take("<f") for _ in range(r.take("<i"))]
        self.functions = [Function(r) for _ in range(r.take("<i"))]
        self.code = [r.take("<I") for _ in range(r.take("<i"))]


def load(chunk):
    if chunk[:4] != b"\x1bLua" or chunk[4] != 0x40:
        raise ValueError("not a Lua 4.0 chunk")
    r = _Reader(chunk)
    r.at = 6 + 7  # signature, version, endianness, seven type sizes
    r.take("<f")  # the test number
    return Function(r)


class Global(str):
    """A global variable's name on the symbolic stack."""


def calls(function):
    """Every call in straight-line code, as (name, [args]), recursing into
    nested functions. Literal arguments come back as str or float; anything
    computed comes back as None."""
    out = []
    stack = []
    for word in function.code:
        op = OPS[word & ((1 << SIZE_OP) - 1)] if (word & 63) < len(OPS) else None
        u = word >> SIZE_OP
        s = u - MAXARG_S
        a = word >> POS_A
        b = (word >> SIZE_OP) & ((1 << SIZE_B) - 1)
        try:
            if op == "END":
                break
            elif op in ("CALL", "TAILCALL"):
                name = stack[a] if a < len(stack) else None
                args = [None if isinstance(v, Global) else v for v in stack[a + 1:]]
                out.append((str(name) if isinstance(name, Global) else None, args))
                del stack[a:]
                if op == "CALL" and b not in (0,):
                    stack.extend([None] * (1 if b == 255 else b))
            elif op == "PUSHNIL":
                stack.extend([None] * u)
            elif op == "POP":
                del stack[len(stack) - u:]
            elif op == "PUSHINT":
                stack.append(float(s))
            elif op == "PUSHSTRING":
                stack.append(function.strings[u])
            elif op == "PUSHNUM":
                stack.append(function.numbers[u])
            elif op == "PUSHNEGNUM":
                stack.append(-function.numbers[u])
            elif op == "GETLOCAL":
                stack.append(stack[u] if u < len(stack) else None)
            elif op == "GETGLOBAL":
                stack.append(Global(function.strings[u]))
            elif op in ("SETLOCAL",):
                value = stack.pop()
                if u < len(stack):
                    stack[u] = value
            elif op in ("SETGLOBAL",):
                stack.pop()
            elif op in ("GETTABLE", "GETINDEXED", "ADD", "SUB", "MULT", "DIV", "POW"):
                if op in ("GETTABLE", "ADD", "SUB", "MULT", "DIV", "POW"):
                    stack.pop()
                stack[-1] = None
            elif op in ("GETDOTTED", "PUSHSELF", "ADDI", "MINUS", "NOT"):
                stack[-1] = None
            elif op == "CONCAT":
                del stack[len(stack) - u + 1:]
                stack[-1] = None
            elif op == "CREATETABLE":
                stack.append(None)
            elif op in ("SETTABLE",):
                del stack[len(stack) - b:]
            elif op in ("SETLIST",):
                del stack[len(stack) - b:]
            elif op in ("SETMAP",):
                del stack[len(stack) - 2 * u:]
            elif op == "CLOSURE":
                del stack[len(stack) - b:]
                stack.append(None)
            elif op in ("JMPNE", "JMPEQ", "JMPLT", "JMPLE", "JMPGT", "JMPGE"):
                del stack[-2:]
            elif op in ("JMPT", "JMPF", "JMPONT", "JMPONF"):
                stack.pop()
            elif op == "PUSHNILJMP":
                stack.append(None)
        except IndexError:
            stack.clear()
    for nested in function.functions:
        out.extend(calls(nested))
    return out


LFIELDS_PER_FLUSH = 64  # list items per SETLIST block


def _number(v):
    return isinstance(v, float)


def _lists(value):
    """Tables keyed 1..n become lists, the rest dicts, all the way down."""
    if not isinstance(value, dict):
        return value
    out = {k: _lists(v) for k, v in value.items()}
    if out and all(isinstance(k, float) for k in out) and sorted(out) == [float(i) for i in range(1, len(out) + 1)]:
        return [out[float(i)] for i in range(1, len(out) + 1)]
    return {(str(int(k)) if isinstance(k, float) and k.is_integer() else str(k)): v for k, v in out.items()}


def getn(table=None, *_):
    """Lua 4's getn on a table here: how many of its keys run 1, 2, 3..."""
    n = 0
    while isinstance(table, dict) and float(n + 1) in table:
        n += 1
    return float(n)


def global_tables(function, env=None, builtins=None):
    """The globals a chunk's top-level code sets from literals and table
    constructors, as Python values (tables keyed 1..n as lists). It runs the
    straight-line code up to the first branch; anything computed some other
    way comes back as None. env seeds globals the chunk adds to (the shell's
    lists an add-on's addme script appends its maps to), and builtins are
    the functions it may call by name ({"getn": getn}), each given the
    arguments as found."""
    env = dict(env or {})
    builtins = builtins or {}
    stack = []
    arith = {"ADD": lambda x, y: x + y, "SUB": lambda x, y: x - y, "MULT": lambda x, y: x * y,
             "DIV": lambda x, y: x / y if y else None, "POW": lambda x, y: x ** y}
    for word in function.code:
        op = OPS[word & ((1 << SIZE_OP) - 1)] if (word & 63) < len(OPS) else None
        u = word >> SIZE_OP
        s = u - MAXARG_S
        a = word >> POS_A
        b = (word >> SIZE_OP) & ((1 << SIZE_B) - 1)
        try:
            if op == "END" or op is None or op.startswith("JMP") or op in ("PUSHNILJMP", "FORPREP", "FORLOOP", "LFORPREP", "LFORLOOP"):
                break
            elif op == "PUSHNIL":
                stack.extend([None] * u)
            elif op == "POP":
                del stack[len(stack) - u:]
            elif op == "PUSHINT":
                stack.append(float(s))
            elif op == "PUSHSTRING":
                stack.append(function.strings[u])
            elif op == "PUSHNUM":
                stack.append(function.numbers[u])
            elif op == "PUSHNEGNUM":
                stack.append(-function.numbers[u])
            elif op == "GETLOCAL":
                stack.append(stack[u])
            elif op == "SETLOCAL":
                stack[u] = stack.pop()
            elif op == "GETGLOBAL":
                name = function.strings[u]
                stack.append(env[name] if name in env else builtins.get(name))
            elif op == "SETGLOBAL":
                env[function.strings[u]] = stack.pop()
            elif op == "GETDOTTED":
                t = stack[-1]
                stack[-1] = t.get(function.strings[u]) if isinstance(t, dict) else None
            elif op == "GETTABLE":
                key = stack.pop()
                t = stack[-1]
                stack[-1] = t.get(key) if isinstance(t, dict) else None
            elif op == "GETINDEXED":
                t = stack[-1]
                stack[-1] = t.get(stack[u]) if isinstance(t, dict) else None
            elif op == "CREATETABLE":
                stack.append({})
            elif op == "SETTABLE":
                t, key, v = stack[-a], stack[-a + 1], stack[-1]
                if isinstance(t, dict):
                    t[key] = v
                del stack[len(stack) - b:]
            elif op == "SETLIST":
                t = stack[-b - 1]
                for i in range(b):
                    t[float(a * LFIELDS_PER_FLUSH + i + 1)] = stack[-b + i]
                del stack[len(stack) - b:]
            elif op == "SETMAP":
                t = stack[-2 * u - 1]
                for i in range(u):
                    t[stack[-2 * u + 2 * i]] = stack[-2 * u + 2 * i + 1]
                del stack[len(stack) - 2 * u:]
            elif op in arith:
                y = stack.pop()
                x = stack[-1]
                stack[-1] = arith[op](x, y) if _number(x) and _number(y) else None
            elif op == "ADDI":
                stack[-1] = stack[-1] + s if _number(stack[-1]) else None
            elif op == "MINUS":
                stack[-1] = -stack[-1] if _number(stack[-1]) else None
            elif op == "NOT":
                stack[-1] = None
            elif op == "CONCAT":
                del stack[len(stack) - u + 1:]
                stack[-1] = None
            elif op in ("CALL", "TAILCALL"):
                fn, args = (stack[a] if a < len(stack) else None), stack[a + 1:]
                del stack[a:]
                if op == "CALL" and b not in (0, 255):
                    results = [fn(*args)] if callable(fn) else []
                    stack.extend((results + [None] * b)[:b])
            elif op == "CLOSURE":
                del stack[len(stack) - b:]
                stack.append(None)
            elif op in ("PUSHUPVALUE", "PUSHSELF"):
                stack.append(None)
        except (IndexError, TypeError):
            break
    return {name: _lists(value) for name, value in env.items() if not callable(value)}


def mission_scripts(data):
    """Script chunks in an archive, by name: name -> compiled chunk bytes."""
    found = {}

    def walk(start, end):
        for tag, b, e in chunks(data, start, end):
            if tag == b"lvl_":
                walk(b + 8, e)
                continue
            name = child(data, b, e, b"NAME")
            body = child(data, b, e, b"BODY")
            if name and body and data[body[0]:body[0] + 4] == b"\x1bLua":
                found[cstr_at(data, *name)] = data[body[0]:body[1]]

    for tag, b, e in chunks(data, 0, len(data)):
        walk(b, e)
    return found


def data_path(path):
    """A file a script reads, without the "dc:" an add-on's scripts put before
    their own files ("dc:SIDE\\gam.lvl" is the add-on's SIDE\\gam.lvl;
    zero_addon.py finds it)."""
    return path[3:] if path[:3].lower() == "dc:" else path


def mission_setup(chunk):
    """The parts of a mission's setup the port uses."""
    setup = {"teams": {}, "attacker": None, "deathRegions": [], "sides": [], "spawnDelay": None}

    def team(n):
        return setup["teams"].setdefault(str(int(n)), {"classes": [], "bleed": []})

    for name, args in calls(load(chunk)):
        literal = [a for a in args]
        if name == "SetTeamName" and len(literal) >= 2 and literal[0] is not None:
            team(literal[0])["name"] = literal[1]
        elif name == "SetTeamIcon" and len(literal) >= 2 and literal[0] is not None:
            team(literal[0])["icon"] = literal[1]
        elif name == "AddUnitClass" and len(literal) >= 3 and literal[0] is not None:
            team(literal[0])["classes"].append([literal[1], int(literal[2] or 0)])
        elif name == "SetHeroClass" and len(literal) >= 2 and literal[0] is not None:
            team(literal[0])["hero"] = literal[1]
        elif name == "SetUnitCount" and len(literal) >= 2 and literal[0] is not None:
            team(literal[0])["units"] = int(literal[1])
        elif name == "SetReinforcementCount" and len(literal) >= 2 and literal[0] is not None:
            team(literal[0])["reinforcements"] = int(literal[1])
        elif name == "AddBleedThreshold" and len(literal) >= 3 and literal[0] is not None:
            team(literal[0])["bleed"].append([literal[1], literal[2]])
        elif name == "SetAttackingTeam" and literal and literal[0] is not None:
            setup["attacker"] = int(literal[0])
        elif name == "AddDeathRegion" and literal:
            setup["deathRegions"].append(literal[0])
        elif name == "ReadDataFile" and literal and isinstance(literal[0], str) and data_path(literal[0]).upper().startswith("SIDE\\"):
            setup["sides"].append(literal[0].split("\\")[-1].rsplit(".", 1)[0].lower())
        elif name == "SetSpawnDelay" and len(literal) >= 2:
            setup["spawnDelay"] = [literal[0], literal[1]]
        elif name == "SetMaxFlyHeight" and literal and literal[0] is not None:
            setup["maxFlyHeight"] = literal[0]
        elif name == "SetMemoryPoolSize" and len(literal) >= 2 and literal[0] in POOLS and literal[1] is not None:
            # How many power-ups and mines the world may hold at once.
            setup.setdefault("pools", {})[POOLS[literal[0]]] = int(literal[1])
    return setup


# Memory pools a mission can size, by the item base they hold.
POOLS = {"PowerupItem": "powerupitem", "EntityMine": "mine"}
