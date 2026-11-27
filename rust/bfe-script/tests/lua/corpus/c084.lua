-- Generated differential program 084 (generate.py, seed 503).
-- globals
do
  gx = 95
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
-- arith
do
  do local v = (709.70533)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (2^7)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (16777215)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (((28e7) * ((0.2) * (9))) * ((24e29) + ((7) / (2^-13))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  print('floor', math.floor(193))
  print('ceil', math.ceil(156.42013))
  print('abs', math.abs(863.42126))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(-895, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(604.46122, 9, 18e25), math.max(366, -15.787))
  print('frexp', math.frexp(1e-40))
  print('ldexp', math.ldexp(1, -11))
  print('pow', 2 ^ 0.5)
end
