-- Generated differential program 194 (generate.py, seed 503).
-- trig
do
  print('exp', math.exp(0.5))
  print('sin', math.sin(3))
  print('atan', math.atan(3))
  print('rad', math.rad(1))
  print('atan2', math.atan2(-1, 2))
  print('pi', math.pi, math.pi * 2)
end
-- format
do
  print(pcall(string.format, "[%05.10G]", -38.717))
  print(pcall(string.format, "[%6c]", 65))
  print(pcall(string.format, "[%-+X]", -98765))
  print(pcall(string.format, "[%012i]", -1))
  print(pcall(string.format, "[% X]", 123456))
  print(pcall(string.format, "[%12x]", -98765))
  print(pcall(string.format, "[%#-14i]", 7))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- globals
do
  gx = 21
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
  do local v = ((((982.56975) + (561.80166)) / (0.7)) + ((4) * ((2^3) / (96e15))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((6) / (-44.462))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (89e-5)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((292.18374) - (((0.2) + (0.5455459)) - ((562) / (0.5))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((-369) * (564)) - ((0.1) + (2^-6))) - (((737) * (249.80335)) * ((2147483647) - (63e-31))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = (((707.42150) - ((6) - (87e35))) * (1))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = ((((0.3) * (507.30181)) + ((593.38751) * (2^30))) * ((-703) + ((8) + (7))))
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(16777215))
  print('ceil', math.ceil(9))
  print('abs', math.abs(16777217))
  print('sqrt', math.sqrt(2))
  do local m = math.mod(0.660, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(0.393269753, 4e-13, 51), math.max(818, 2^23))
  print('frexp', math.frexp(0.3))
  print('ldexp', math.ldexp(1, 16))
  print('pow', 1.5 ^ -2)
end
