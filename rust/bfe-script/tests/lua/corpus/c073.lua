-- Generated differential program 073 (generate.py, seed 503).
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, true))
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
-- arith
do
  do local v = ((4e-3) * (8388609))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (7)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((6) / (8)) / ((661.97767) + (-7.852))) * (((2^18) * (-8.892)) / ((1e-45) * (7))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (8388609)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (0.1)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(-128))
  print('ceil', math.ceil(8))
  print('abs', math.abs(706.1770))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(2^2, 0.75) if m == m then print('mod', m) end end
  print('minmax', math.min(0.9, 0.1, 14), math.max(0.07, -48.673))
  print('frexp', math.frexp(0.3))
  print('ldexp', math.ldexp(1, -1))
  print('pow', 0.5 ^ 10)
end
-- coerce
do
  print(1e100 .. "")
  print(-1/0 .. "")
  print(".5" .. 1)
  print("10" + 5)
  print(1/0 .. "")
  print("5" + "0.25")
  print(123456789 .. "")
  print(16777217 .. "")
  print("-inf", tonumber("-inf"))
  print("3x", tonumber("3x"))
  print("0x1F", tonumber("0x1F"))
  print("+3", tonumber("+3"))
  print("1e", tonumber("1e"))
  print("  0x10  ", tonumber("  0x10  "))
  print("-7.5", tonumber("-7.5"))
  print(24, tonumber("2 9", 24))
  print(13, tonumber("cb", 13))
  print(21, tonumber("3ie", 21))
  print(7, tonumber("020", 7))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- varargs
do
  local function count(...) return arg.n end
  local function pass(...) return unpack(arg) end
  print(count(), count(nil), count(1, nil), count(nil, nil, 3))
  print(pass(1, 2, 3))
  print((pass(1, 2, 3)))
  local function tail(n, ...) if n == 0 then return arg.n end return tail(n - 1, n, unpack(arg)) end
  print(tail(2))
  local a, b, c = pass(7, 2, 6)
  print(a, b, c)
  local t = {pass(1, 2), pass(3, 4)} print(table.getn(t), t[1], t[2], t[3])
end
