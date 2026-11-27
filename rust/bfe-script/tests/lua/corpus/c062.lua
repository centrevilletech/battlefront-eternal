-- Generated differential program 062 (generate.py, seed 503).
-- globals
do
  gx = 78
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
  print('exp', math.exp(10))
  print('log', math.log(3))
  print('log10', math.log10(0.25))
  print('tan', math.tan(2))
  print('atan2', math.atan2(0.5, -3))
  print('pi', math.pi, math.pi * 2)
end
