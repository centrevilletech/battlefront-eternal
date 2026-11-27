-- Generated differential program 075 (generate.py, seed 503).
-- globals
do
  gx = 63
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- closures
do
  local fs = {}
  for i = 1, 2 do fs[i] = function() return i * 7 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(8), counter(10)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- format
do
  print(pcall(string.format, "[% i]", -98765))
  print(pcall(string.format, "[% #x]", 65535))
  print(pcall(string.format, "[%- 16i]", 0))
  print(pcall(string.format, "[%4.12f]", 4))
  print(pcall(string.format, "[%q]", "a\"b"))
  print(pcall(string.format, "[% 14X]", 65535))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- arith
do
  do local v = ((16777215) * (((8388609) / (129.55036)) + (26e13)))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (4294967295)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((0.38114964) * (8)) / (0.1)) + ((-17.266) + ((85e-14) * (2/3))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((0.1) + (((41e22) / (4)) * ((85.74905) * (5))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((5) / (1e38)) * ((3) + (0.2))) + ((3) * ((5) + (0.5004))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((((1e-45) * (2147483647)) - ((2^2) / (0.8140177449))) / ((-854) + ((3) / (-5.81))))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = (-21.491)
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  do local v = (((6) + (16777216)) / (((511.5842) + (1e38)) + ((183.18054) * (84e9))))
    if v == v then print(7, v, v * 2, -v) else print(7, 'not a number') end end
  print('floor', math.floor(0.883))
  print('ceil', math.ceil(2147483647))
  print('abs', math.abs(38e-6))
  print('sqrt', math.sqrt(3))
  do local m = math.mod(-25.169, 0.75) if m == m then print('mod', m) end end
  print('minmax', math.min(386.63945, 5, 84e38), math.max(-27.520, -34.807))
  print('frexp', math.frexp(0.3))
  print('ldexp', math.ldexp(-3, -17))
  print('pow', 2 ^ 10)
end
