-- Generated differential program 060 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%-5X]", 255))
  print(pcall(string.format, "[%10g]", 17e10))
  print(pcall(string.format, "[%q]", "line1\nline2"))
  print(pcall(string.format, "[%q]", "a\"b"))
  print(pcall(string.format, "[%- .7e]", 1e38))
  print(pcall(string.format, "[%# 4x]", 255))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- globals
do
  gx = 15
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- trig
do
  print('deg', math.deg(0.5))
  print('rad', math.rad(3))
  print('log10', math.log10(3))
  print('exp', math.exp(0.001))
  print('atan2', math.atan2(1, 1))
  print('pi', math.pi, math.pi * 2)
end
