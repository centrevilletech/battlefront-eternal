-- Generated differential program 198 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%e]", 21e14))
  print(pcall(string.format, "[%.9e]", -37.867))
  print(pcall(string.format, "[%c]", 97))
  print(pcall(string.format, "[%q]", "back\\slash"))
  print(pcall(string.format, "[%+-7.1G]", 1/3))
  print(pcall(string.format, "[%-16E]", 2^-3))
  print(pcall(string.format, "[%0e]", 456.21089))
  print(pcall(string.format, "[%10x]", -98765))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- globals
do
  gx = 76
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
