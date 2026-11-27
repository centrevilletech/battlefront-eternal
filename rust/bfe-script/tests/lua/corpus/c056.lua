-- Generated differential program 056 (generate.py, seed 503).
-- globals
do
  gx = 46
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- format
do
  print(pcall(string.format, "[%#+i]", -1))
  print(pcall(string.format, "[%3.3e]", 2^15))
  print(pcall(string.format, "[%f]", 595))
  print(pcall(string.format, "[%+ .1f]", -348))
  print(pcall(string.format, "[%1.6G]", 817))
  print(pcall(string.format, "[%5.2s]", "xxxxxxxxxxxx"))
  print(pcall(string.format, "[%18.2s]", ""))
  print(pcall(string.format, "[%+X]", 1))
  print(pcall(string.format, "[%d]", 255))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- arith
do
  do local v = ((((26e19) + (55e5)) - ((58e-20) + (16777216))) * (((53e-16) * (-41.839)) + ((2147483647) * (42e-15))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((-48.809) - (16777216))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((501) + ((-38.386) / (-669))) / ((0.3) * ((33554433) / (1))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (2^23)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((0.319) + (4294967295)) / (0.1)) + ((2^-1) / ((-427) - (794))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(0.0287668531))
  print('ceil', math.ceil(3))
  print('abs', math.abs(96.68971))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(2^-13, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(34e11, -10.190, 4294967295), math.max(8, 94e-9))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(-3, -13))
  print('pow', 2 ^ 0.5)
end
