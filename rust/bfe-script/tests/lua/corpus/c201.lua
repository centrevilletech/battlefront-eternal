-- Generated differential program 201 (generate.py, seed 503).
-- strings
do
  local s = "eecYeY"
  print(string.sub(s, 6, -2), string.sub(s, 6), string.byte(s, -2))
  print(string.sub(s, -1, -13), string.sub(s, -1), string.byte(s, -13))
  print(string.sub(s, 1, 3), string.sub(s, 1), string.byte(s, 3))
  print(string.sub(s, 2, -7), string.sub(s, 2), string.byte(s, -7))
  print(string.sub(s, -1, -10), string.sub(s, -1), string.byte(s, -10))
  print(string.sub(s, -8, -12), string.sub(s, -8), string.byte(s, -12))
  print(string.sub(s, -9, 10), string.sub(s, -9), string.byte(s, 10))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 2))
  print(string.char())
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, nil))
  print(pcall(error))
  local function lvl1() error('one', 1) end local function lvl2() error('two', 2) end
  print(pcall(lvl1)) print(pcall(lvl2))
  print(pcall(function() local t = nil return t.x end))
  print(pcall(function() return undefined_global_fn() end))
  print(pcall(function() local t = {} return t.a.b end))
  print(pcall(function() return 1 < 'x' end))
  print(pcall(function() return {} .. 'x' end))
  print(pcall(function() return -{} end))
  print(pcall(function() local s = 'x' return s + 1 end))
  print(pcall(function() local t = setmetatable({}, {__index = function(t, k) error('deep ' .. k) end}) return t.key end))
  print(xpcall(function() error('boom') end, function(m) return 'handled: ' .. m end))
  print(pcall(error, setmetatable({}, {__tostring = function() return 'obj' end})) == false)
  print(pcall(assert, false, 'assert msg'))
  print(pcall(assert, nil))
  print(pcall(assert, 1, 2))
end
-- coerce
do
  print("0x10" + 0)
  print("5" + "0.25")
  print("  12  " + 0)
  print(1e100 .. "")
  print(" 3e2 " * 1)
  print("7" * "6")
  print("1e3" / 4)
  print(1/0 .. "")
  print("-inf", tonumber("-inf"))
  print("3x", tonumber("3x"))
  print(" 12 ", tonumber(" 12 "))
  print("12", tonumber("12"))
  print("1e", tonumber("1e"))
  print("1 2", tonumber("1 2"))
  print("-7.5", tonumber("-7.5"))
  print(19, tonumber("f", 19))
  print(18, tonumber("546", 18))
  print(21, tonumber("79", 21))
  print(8, tonumber("3664", 8))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- arith
do
  do local v = (-548)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((0.96326) - (38)) - ((546) + (236.35285))) / ((-110) * (11e-22)))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (0.1822328)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((2147483647) * (9)) + ((-36.638) * (0.1))) / ((2) + ((2^19) / (0.42550521148))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(0.0270245))
  print('ceil', math.ceil(4))
  print('abs', math.abs(2^-14))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(1, -3) if m == m then print('mod', m) end end
  print('minmax', math.min(-417, 0.2, 0.0), math.max(2^25, 0.505671586))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(0.75, 3))
  print('pow', 1.5 ^ 10)
end
