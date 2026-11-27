-- Generated differential program 234 (generate.py, seed 503).
-- globals
do
  gx = 74
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
  print(pcall(string.format, "[%0i]", 2147483647))
  print(pcall(string.format, "[%5.2s]", "hello"))
  print(pcall(string.format, "[%+0d]", 2147483647))
  print(pcall(string.format, "[%14g]", 784))
  print(pcall(string.format, "[%q]", "nul\0byte"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
