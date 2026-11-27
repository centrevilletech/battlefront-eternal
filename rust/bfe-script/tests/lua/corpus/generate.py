#!/usr/bin/env python3
"""Write the differential corpus programs (cNNN.lua) next to this script.

Every program is standalone Lua 5.0 built from a fixed seed, so running this script
again writes the same files. The programs print only output that Lua 5.0.3 defines
the same way on every build: no table or function addresses, no traversal of a
table's hash part (its order is the build's own), no NaN (the reference prints the
sign of the hardware's NaN), no random numbers and no collector figures. A pattern
that ends in `%` is left out too: Lua 5.0.3 builds that error message from whatever
value lies below it on the C stack.

After the seeded programs come fixed ones (FIXED below), cases that a seeded mix
would rarely build exactly.

Usage: generate.py [COUNT]          write COUNT seeded programs (default 240) and FIXED
       generate.py --check [COUNT]  compare what it would write with the committed
                                    files, byte for byte; exit status 1 on any change
"""
import os
import random
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
R = random.Random(503)

ALPHA = "abcxyz019 ,.-_()[]%"


def lua_str(s):
    out = []
    for ch in s:
        o = ord(ch)
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif 32 <= o < 127:
            out.append(ch)
        else:
            out.append("\\%d" % o)
    return '"' + "".join(out) + '"'


def number_literal():
    kind = R.randrange(9)
    if kind == 0:
        return str(R.randrange(-1000, 1000))
    if kind == 1:
        return "%d.%d" % (R.randrange(0, 1000), R.randrange(0, 100000))
    if kind == 2:
        return "%de%d" % (R.randrange(1, 99), R.randrange(-40, 39))
    if kind == 3:
        return "2^%d" % R.randrange(-30, 31)
    if kind == 4:
        return str(R.choice([16777215, 16777216, 16777217, 33554433, 8388609, 2147483647, 4294967295]))
    if kind == 5:
        return "0.%s" % "".join(R.choice("0123456789") for _ in range(R.randrange(1, 12)))
    if kind == 6:
        return "-%d.%d" % (R.randrange(0, 50), R.randrange(0, 999))
    if kind == 7:
        return R.choice(["0.1", "0.2", "0.3", "1/3", "2/3", "1e-45", "1e38", "3.4e38", "1.17549435e-38"])
    return str(R.randrange(1, 10))


def arith_expr(depth=0):
    if depth > 2 or R.random() < 0.3:
        return "(" + number_literal() + ")"
    op = R.choice(["+", "-", "*", "/", "+", "*"])
    return "(%s %s %s)" % (arith_expr(depth + 1), op, arith_expr(depth + 1))


def sec_arith(n):
    lines = []
    for i in range(R.randrange(4, 9)):
        lines.append("do local v = %s" % arith_expr())
        lines.append("  if v == v then print(%d, v, v * 2, -v) else print(%d, 'not a number') end end" % (i, i))
    for name in ["floor", "ceil", "abs"]:
        lines.append("print('%s', math.%s(%s))" % (name, name, number_literal()))
    lines.append("print('sqrt', math.sqrt(%s))" % R.choice(["2", "0.5", "1e10", "16777217", "3"]))
    a, b = number_literal(), R.choice(["3", "-3", "0.75", "7", "2.5"])
    lines.append("do local m = math.mod(%s, %s) if m == m then print('mod', m) end end" % (a, b))
    lines.append("print('minmax', math.min(%s, %s, %s), math.max(%s, %s))" % (
        number_literal(), number_literal(), number_literal(), number_literal(), number_literal()))
    lines.append("print('frexp', math.frexp(%s))" % R.choice(["8", "0.3", "-5", "1e-40", "1"]))
    lines.append("print('ldexp', math.ldexp(%s, %d))" % (R.choice(["0.75", "1", "-3"]), R.randrange(-20, 20)))
    lines.append("print('pow', %s ^ %s)" % (R.choice(["2", "1.5", "10", "0.5"]), R.choice(["3", "0.5", "-2", "10"])))
    return lines


def sec_trig(n):
    lines = []
    for f in R.sample(["sin", "cos", "tan", "exp", "log", "log10", "atan", "deg", "rad"], 4):
        arg = R.choice(["0.5", "1", "2", "0.25", "3", "10", "0.001"])
        lines.append("print('%s', math.%s(%s))" % (f, f, arg))
    lines.append("print('atan2', math.atan2(%s, %s))" % (R.choice(["1", "-1", "0.5"]), R.choice(["2", "-3", "1"])))
    lines.append("print('pi', math.pi, math.pi * 2)")
    return lines


def sec_compare(n):
    lines = []
    for _ in range(R.randrange(4, 8)):
        a, b = number_literal(), number_literal()
        lines.append("print(%s < %s, %s <= %s, %s == %s)" % (a, b, a, b, a, b))
    for _ in range(3):
        s1 = lua_str("".join(R.choice("abcAB_09") for _ in range(R.randrange(0, 5))))
        s2 = lua_str("".join(R.choice("abcAB_09") for _ in range(R.randrange(0, 5))))
        lines.append("print(%s < %s, %s <= %s, %s == %s)" % (s1, s2, s1, s2, s1, s2))
    return lines


def sec_coerce(n):
    lines = []
    picks = R.sample([
        '"10" + 5', '"0x10" + 0', '" 3e2 " * 1', '".5" .. 1', '10 .. 20', '"7" * "6"',
        '-"2"', '"1e3" / 4', '2^24 + 1', '"  12  " + 0', '1e15 .. ""', '16777217 .. ""',
        '0.1 .. ""', '-0 .. ""', '1/0 .. ""', '-1/0 .. ""', '123456789 .. ""', '1e100 .. ""',
        '"5" + "0.25"', '3 .. "x"',
    ], 8)
    for p in picks:
        lines.append("print(%s)" % p)
    for s in R.sample(["12", " 12 ", "1e", "0x1F", "0x", "1 2", "-7.5", "inf", "-inf",
                       "  0x10  ", ".", "5.", ".5e1", "1e400", "+3", "0x1p4", "3x"], 7):
        lines.append("print(%s, tonumber(%s))" % (lua_str(s), lua_str(s)))
    for _ in range(4):
        base = R.randrange(2, 37)
        digits = "".join(R.choice("0123456789abcdefghijklmnopqrstuvwxyz"[:base] + " ") for _ in range(R.randrange(1, 7)))
        lines.append("print(%d, tonumber(%s, %d))" % (base, lua_str(digits.strip() or "0"), base))
    lines.append("print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))")
    return lines


def sec_format(n):
    lines = []
    for _ in range(R.randrange(5, 10)):
        conv = R.choice(["d", "i", "x", "X", "o", "c", "e", "E", "f", "g", "G", "s", "q", "5.2s", "u"])
        flags = "".join(R.sample("-0+ #", R.randrange(0, 3))) if conv not in ("s", "q", "c", "5.2s") else ""
        width = str(R.randrange(0, 20)) if R.random() < 0.6 and conv not in ("q", "5.2s") else ""
        prec = ("." + str(R.randrange(0, 15))) if R.random() < 0.5 and conv in "eEfgGs" else ""
        if conv in ("d", "i", "x", "X", "o", "u"):
            val = str(R.choice([0, 1, -1, 255, 65535, 123456, -98765, 2147483647, 7]))
        elif conv == "c":
            val = str(R.choice([65, 97, 48, 126]))
        elif conv in ("s", "5.2s"):
            val = lua_str(R.choice(["hello", "", "a b", "tab\there", "x" * 12]))
        elif conv == "q":
            val = lua_str(R.choice(['a"b', "line1\nline2", "back\\slash", "nul\0byte", "plain"]))
        else:
            val = number_literal()
        spec = "%" + flags + width + prec + (conv if conv != "5.2s" else "5.2s")
        lines.append("print(pcall(string.format, %s, %s))" % (lua_str("[" + spec + "]"), val))
    lines.append("print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))")
    lines.append("print(string.format('%s %s %d', 1, 2.5, 3))")
    return lines


def sec_strings(n):
    lines = []
    s = "".join(R.choice("abcdefXYZ019 ") for _ in range(R.randrange(3, 12)))
    lines.append("local s = %s" % lua_str(s))
    for _ in range(R.randrange(4, 8)):
        i = R.randrange(-15, 16)
        j = R.randrange(-15, 16)
        lines.append("print(string.sub(s, %d, %d), string.sub(s, %d), string.byte(s, %d))" % (i, j, i, j))
    lines.append("print(string.len(s), string.upper(s), string.lower(s))")
    lines.append("print(string.rep(%s, %d))" % (lua_str(R.choice(["ab", "-", "xyz", ""])), R.randrange(0, 6)))
    lines.append("print(string.char(%s))" % ", ".join(str(R.randrange(32, 127)) for _ in range(R.randrange(0, 6))))
    lines.append("print(pcall(string.char, 300))")
    lines.append("print(pcall(string.rep))")
    return lines


PATTERNS = ["%a+", "(%d+)", "^%s*(.-)%s*$", "%b()", "%f[%w]%w+", "[%a_][%w_]*", "(a*)", ".-b",
            "a?b", "[^,]+", "%x+", "(%a)(%d)", "()", "%s", "[%.%-]", "(.)%1", "^(%w+)", "z$",
            "[]]", "[^%s]+", "%p", "c-", "x*", "(b+)(c*)"]


def subject():
    return "".join(R.choice(ALPHA) for _ in range(R.randrange(0, 18)))


def sec_patterns(n):
    lines = []
    for _ in range(R.randrange(4, 8)):
        subj, pat = lua_str(subject()), lua_str(R.choice(PATTERNS))
        init = R.choice(["", ", 1", ", 3", ", -4", ", 20"])
        lines.append("print(pcall(string.find, %s, %s%s))" % (subj, pat, init))
    for _ in range(3):
        subj, pat = lua_str(subject()), lua_str(R.choice(PATTERNS))
        repl = R.choice(['"<%0>"', '"%1-"', '"#"', 'function(c) return nil end',
                         'function(c) return string.upper(c) end', 'function() return 7 end',
                         'function() return false end', '"%%"'])
        maxn = R.choice(["", ", 1", ", 2", ", 0"])
        lines.append("print(pcall(string.gsub, %s, %s, %s%s))" % (subj, pat, repl, maxn))
    subj, pat = lua_str(subject()), lua_str(R.choice(["%a+", "%d", "(%w)(%w)", "[,.]", "x*"]))
    lines.append("do local out = {} for a, b in string.gfind(%s, %s) do"
                 " table.insert(out, tostring(a) .. ':' .. tostring(b)) end"
                 " print(table.concat(out, ' ')) end" % (subj, pat))
    lines.append("print(pcall(string.gsub, 'abc', 'b', {}))")
    lines.append("print(string.find('a.b', '.', 1, true), string.find('a+b', '+', 1, 1))")
    return lines


def sec_tables(n):
    lines = []
    vals = [str(R.randrange(-50, 50)) for _ in range(R.randrange(3, 10))]
    lines.append("local t = {%s}" % ", ".join(vals))
    lines.append("print(table.getn(t), table.concat(t, ','))")
    for _ in range(R.randrange(2, 5)):
        op = R.randrange(4)
        if op == 0:
            lines.append("table.insert(t, %d)" % R.randrange(-9, 9))
        elif op == 1:
            lines.append("table.insert(t, math.max(1, math.min(%d, table.getn(t) + 1)), %d)" % (R.randrange(1, 5), R.randrange(100, 200)))
        elif op == 2:
            lines.append("if table.getn(t) > 0 then print('removed', table.remove(t)) end")
        else:
            lines.append("if table.getn(t) > 0 then print('removed', table.remove(t, math.min(%d, table.getn(t)))) end" % R.randrange(1, 4))
        lines.append("print(table.getn(t), table.concat(t, ' '))")
    lines.append("table.sort(t)")
    lines.append("print(table.concat(t, ' '))")
    lines.append("table.sort(t, function(a, b) return a > b end)")
    lines.append("print(table.concat(t, ' '))")
    lines.append("print(pcall(table.concat, t, '-', 2, 3))")
    lines.append("print(table.concat({}, 'x'), table.concat(t, '', 3, 2))")
    lines.append("local u = {n = 2, 'a', 'b', 'c'} print(table.getn(u))")
    lines.append("table.setn(u, 1) print(table.getn(u))")
    lines.append("print(unpack({1, 2, nil, 4}))")
    lines.append("table.foreachi(t, function(i, v) if i <= 3 then print(i, v) end end)")
    words = R.sample(["pear", "apple", "fig", "kiwi", "plum", "date", "lime", "Apple", "a", "b"], 6)
    lines.append("local w = {%s} table.sort(w) print(table.concat(w, ' '))" % ", ".join(lua_str(x) for x in words))
    lines.append("for i, v in ipairs({10, 20, nil, 40}) do print('ipairs', i, v) end")
    return lines


def sec_closures(n):
    lines = []
    lines.append("local fs = {}")
    lines.append("for i = 1, %d do fs[i] = function() return i * %d end end" % (R.randrange(2, 6), R.randrange(1, 9)))
    lines.append("for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end")
    lines.append("local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end")
    lines.append("local c1, c2 = counter(%d), counter(%d)" % (R.randrange(0, 10), R.randrange(10, 20)))
    lines.append("print(c1(), c1(2), c2(), c1(), c2(-5))")
    lines.append("local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end")
    lines.append("print(acc[1](), acc[2](), acc[3]())")
    lines.append("local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end")
    lines.append("local inc, get = shared() inc() inc() print('shared', get())")
    return lines


def sec_varargs(n):
    lines = []
    lines.append("local function count(...) return arg.n end")
    lines.append("local function pass(...) return unpack(arg) end")
    lines.append("print(count(), count(nil), count(1, nil), count(nil, nil, 3))")
    lines.append("print(pass(1, 2, 3))")
    lines.append("print((pass(1, 2, 3)))")
    lines.append("local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end")
    lines.append("print(tail(%d))" % R.randrange(1, 8))
    lines.append("local a, b, c = pass(%s)" % ", ".join(str(R.randrange(9)) for _ in range(R.randrange(0, 5))))
    lines.append("print(a, b, c)")
    lines.append("local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])")
    return lines


def sec_meta(n):
    lines = []
    lines.append("local mt = {}")
    lines.append("local function val(x) if type(x) == 'table' then return rawget(x, 'v') end return x end")
    lines.append("mt.__add = function(a, b) return 'add(' .. tostring(val(a)) .. ',' .. tostring(val(b)) .. ')' end")
    lines.append("mt.__index = function(t, k) return 'missing ' .. tostring(k) end")
    lines.append("mt.__newindex = function(t, k, v) rawset(t, k, v .. '!') end")
    lines.append("mt.__eq = function(a, b) return rawget(a, 'v') == rawget(b, 'v') end")
    lines.append("mt.__lt = function(a, b) return rawget(a, 'v') < rawget(b, 'v') end")
    lines.append("mt.__le = function(a, b) return rawget(a, 'v') <= rawget(b, 'v') end")
    lines.append("mt.__concat = function(a, b) return 'cat' end")
    lines.append("mt.__unm = function(a) return 'neg' end")
    lines.append("mt.__call = function(self, x) return 'called ' .. tostring(x) end")
    lines.append("local a = setmetatable({v = %d}, mt)" % R.randrange(0, 5))
    lines.append("local b = setmetatable({v = %d}, mt)" % R.randrange(0, 5))
    lines.append("print(a + b, a + 1, 2 + b)")
    lines.append("print(a.nothing, a[1])")
    lines.append("a.new = 'x' print(rawget(a, 'new'))")
    lines.append("print(a == b, a < b, a <= b, a > b, a >= b, a ~= b)")
    lines.append("print(a .. 'x', 'x' .. b, -a, a(5))")
    lines.append("local p = setmetatable({}, {__metatable = 'locked'})")
    lines.append("print(getmetatable(p), pcall(setmetatable, p, {}))")
    lines.append("local base = {greet = function(self) return 'hi ' .. self.name end}")
    lines.append("local obj = setmetatable({name = %s}, {__index = base})" % lua_str(R.choice(["ann", "bo", "cy"])))
    lines.append("print(obj:greet(), rawget(obj, 'greet'), rawequal(obj, obj))")
    return lines


def sec_errors(n):
    lines = []
    lines.append("print(pcall(error, 'plain'))")
    lines.append("print(pcall(error, %s))" % R.choice(["42", "nil", "true", "'s'", "-0.5"]))
    lines.append("print(pcall(error))")
    lines.append("local function lvl1() error('one', 1) end local function lvl2() error('two', 2) end")
    lines.append("print(pcall(lvl1)) print(pcall(lvl2))")
    lines.append("print(pcall(function() local t = nil return t.x end))")
    lines.append("print(pcall(function() return undefined_global_fn() end))")
    lines.append("print(pcall(function() local t = {} return t.a.b end))")
    lines.append("print(pcall(function() return 1 < 'x' end))")
    lines.append("print(pcall(function() return {} .. 'x' end))")
    lines.append("print(pcall(function() return -{} end))")
    lines.append("print(pcall(function() local s = 'x' return s + 1 end))")
    lines.append("print(pcall(function() local t = setmetatable({}, {__index = function(t, k) error('deep ' .. k) end}) return t.key end))")
    lines.append("print(xpcall(function() error('boom') end, function(m) return 'handled: ' .. m end))")
    lines.append("print(pcall(error, setmetatable({}, {__tostring = function() return 'obj' end})) == false)")
    lines.append("print(pcall(assert, false, 'assert msg'))")
    lines.append("print(pcall(assert, nil))")
    lines.append("print(pcall(assert, 1, 2))")
    return lines


def sec_recursion(n):
    lines = []
    lines.append("local function fib(k) if k < 2 then return k end return fib(k - 1) + fib(k - 2) end")
    lines.append("print('fib', fib(%d))" % R.randrange(5, 18))
    lines.append("local function down(k) if k == 0 then return 'bottom' end return down(k - 1) end")
    lines.append("print(down(%d))" % R.choice([100, 5000, 20000]))
    lines.append("local function depth(k) if k == 0 then return 0 end return 1 + depth(k - 1) end")
    lines.append("print(depth(%d))" % R.choice([10, 300, 1000, 2500]))
    lines.append("local function ack(m, k) if m == 0 then return k + 1 elseif k == 0 then return ack(m - 1, 1) end return ack(m - 1, ack(m, k - 1)) end")
    lines.append("print('ack', ack(2, %d))" % R.randrange(1, 5))
    return lines


def sec_loops(n):
    lines = []
    a, b, c = R.choice(["1", "0.5", "-2", "10"]), R.choice(["3", "2.6", "-5", "1"]), R.choice(["1", "0.25", "-1", "0.1", "2"])
    lines.append("do local n, last = 0, nil for i = %s, %s, %s do n = n + 1 last = i if n > 50 then break end end print('steps', n, last) end" % (a, b, c))
    lines.append("do local s = 0 for i = 1, %d do if math.mod(i, 3) == 0 then s = s + i end end print('sum', s) end" % R.randrange(5, 40))
    lines.append("do local k = 0 repeat k = k + 2 until k >= %d print('repeat', k) end" % R.randrange(1, 20))
    lines.append("do local x = 0.1 local m = 0 while x < 1 do x = x + 0.1 m = m + 1 end print('float loop', m, x) end")
    lines.append("do local t = {} for i = 1, 5 do t[i] = i * i end local s = '' for i, v in ipairs(t) do s = s .. v .. ';' end print(s) end")
    return lines


def sec_globals(n):
    lines = []
    lines.append("gx = %d" % R.randrange(100))
    lines.append("print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)")
    lines.append("setglobal_test = nil print(setglobal_test, _G['setglobal_test'])")
    lines.append("local env = setmetatable({}, {__index = _G})")
    lines.append("local f = function() gy = 5 return gx end")
    lines.append("setfenv(f, env) print(f(), rawget(env, 'gy'), gy)")
    lines.append("print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))")
    lines.append("print(tostring(nil), tostring(false), tostring(12), tostring('q'))")
    lines.append("print(next({}), rawequal('a', 'a'), rawequal({}, {}))")
    return lines


SECTIONS = [sec_arith, sec_trig, sec_compare, sec_coerce, sec_format, sec_strings, sec_patterns,
            sec_tables, sec_closures, sec_varargs, sec_meta, sec_errors, sec_recursion, sec_loops,
            sec_globals]


def program(n):
    lines = ["-- Generated differential program %03d (generate.py, seed 503)." % n]
    for sec in R.sample(SECTIONS, R.randrange(2, 5)):
        lines.append("-- " + sec.__name__[4:])
        lines.append("do")
        lines.extend("  " + line for line in sec(n))
        lines.append("end")
    return "\n".join(lines) + "\n"


# Fixed programs, written after the seeded ones in this order.
FIXED = [
    # Environment levels: a level that converts to 0 selects the running native
    # (`setfenv` itself) unless the argument is exactly 0; only then are the thread's
    # globals replaced, and `getfenv` reads them.
    """-- Fixed differential program: environment levels (generate.py).
local env = _G
for _, level in ipairs({0, 0.5, -0.5, "0.75", "0", -0, 0.25, "0x0", " 0 "}) do
  local ok, err = pcall(setfenv, level, env)
  print(level, ok, err)
end
print(getfenv(0) == _G, getfenv(0.5) == _G, getfenv("0.75") == _G, getfenv(-0) == _G)
print(pcall(getfenv, -1))
local ok, found = pcall(getfenv, -0.5)
print(ok, found == _G)
local f = function() return 1 end
print(pcall(setfenv, f, env) == true, getfenv(f) == env)
print(pcall(setfenv, print, env))
print(pcall(setfenv, 1.5, env))
""",
]


def programs(count):
    """(file name, text) of every program, seeded then fixed."""
    out = [("c%03d.lua" % n, program(n)) for n in range(count)]
    out += [("c%03d.lua" % (count + i), text) for i, text in enumerate(FIXED)]
    return out


def main():
    args = sys.argv[1:]
    check = bool(args) and args[0] == "--check"
    if check:
        args = args[1:]
    count = int(args[0]) if args else 240
    changed = []
    for name, text in programs(count):
        path = os.path.join(HERE, name)
        if check:
            try:
                with open(path, "rb") as f:
                    same = f.read() == text.encode()
            except FileNotFoundError:
                same = False
            if not same:
                changed.append(name)
        else:
            with open(path, "w", newline="\n") as f:
                f.write(text)
    if check:
        if changed:
            print("differs from the generator: " + " ".join(changed))
            sys.exit(1)
        print("all %d programs match the generator" % (count + len(FIXED)))


if __name__ == "__main__":
    main()
