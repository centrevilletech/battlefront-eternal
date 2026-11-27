-- Standard library semantics regression tests (pow, sort, tonumber, format, print, patterns).
local F = string.format
local L = string.len
local P = pcall

-- C99 / Lua 5.0 pow special cases
print("pow", math.pow(1, 0/0), 1^(1/0), (-1)^(-1/0), (0/0)^0)

-- table.sort preserves compared values through swap even if comparator mutates table
local t1 = {3, 1}
table.sort(t1, function(a, b) t1[1] = 99; return a < b end)
print("sort1", unpack(t1))

local t2 = {3, 1}
table.sort(t2, function() t2[1] = 99; return true end)
print("sort2", unpack(t2))

local t3 = {4, 2, 5, 1, 3}
table.sort(t3, function(a, b) t3[3] = 77; return a < b end)
print("sort3", unpack(t3))

-- tonumber base 10: hex floats, nan/inf, NUL truncation, round-trip NaN
print("tn_hex", tonumber("0x1p1"), tonumber("-0x1.8p1"), tonumber("0x.8p0"), tonumber("0x1.p2"))
print("tn_spec", tonumber("nan") ~= nil, tonumber("NaN(7)") ~= nil, tonumber("inf"), tonumber("-INFINITY"))
print("tn_nul", tonumber("2\000oops"), tonumber(tostring(0/0)) ~= nil)

-- tonumber with radix: signs, 0x prefix, NUL truncation
print("tn_rad", tonumber("+f", 16), tonumber("0x10", 16), tonumber("11\000q", 2))

-- string.format: %c NUL, %s NUL vs len>=100 bypass, %d 32-bit int
print("fmt_nul", L(F("%c", 0)), L(F("%3c", 0)), L(F("%-3c", 0)), L(F("%s", "a\000b")))
print("fmt_long", L(F("%2s", "\000" .. string.rep("a", 99))))
print("fmt_int", F("%d", 2147483648), F("%d", -2147483648), F("%u", -1), F("%x", -1))

-- print: NUL truncation and immediate per-argument flush on error
print("ab\000cd", 9)
P(print, "a", setmetatable({}, {__tostring = function() error(false) end}))
print("b")

-- pattern matching: C-string NUL stop in pattern, plain find length-aware, gsub trailing %
print("find_nul", string.find("a", ".\000x"))
print("find_plain_nul", string.find("a\000.", "\000."))
local g_acc = {}
for x in string.gfind("a", ".\000x") do
    table.insert(g_acc, x)
end
print("gfind_nul", table.concat(g_acc, ","))
print("gsub_nul", string.gsub("ab", ".\000x", "z"))
local gs = string.gsub("a", "a", "%")
print("gsub_pct", L(gs), string.byte(gs))
