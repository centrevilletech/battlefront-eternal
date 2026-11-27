-- Generated differential program 039 (generate.py, seed 503).
-- globals
do
  gx = 27
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
  do local v = ((((0.2) + (4)) + ((-13.665) + (-11.906))) + (666.906))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((-427) - (8)) + ((1) * (44e6))) / (((2^-26) * (6)) + ((8) / (-46.791))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((44e-22) / (33554433)) / ((0.05) / (21.13815))) - ((3) + ((2147483647) / (3))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (((5) / ((2147483647) + (-2.406))) * (((-212) * (0.2)) * ((2^19) + (9))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (((-44.230) * (0.107591)) + (((0.25) / (2^21)) * ((2^-26) / (6))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = (((2^22) / ((2^2) + (24e16))) + (((-636) + (0.59)) * ((6) + (1/3))))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  print('floor', math.floor(16777217))
  print('ceil', math.ceil(-31.779))
  print('abs', math.abs(256.89877))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(-427, 3) if m == m then print('mod', m) end end
  print('minmax', math.min(2^12, 48, 2^-9), math.max(-9.790, 124.77764))
  print('frexp', math.frexp(1))
  print('ldexp', math.ldexp(0.75, 12))
  print('pow', 1.5 ^ 3)
end
-- compare
do
  print(0.81274 < -30.99, 0.81274 <= -30.99, 0.81274 == -30.99)
  print(85e-28 < -583, 85e-28 <= -583, 85e-28 == -583)
  print(0.1 < 0.1, 0.1 <= 0.1, 0.1 == 0.1)
  print(545 < 2e28, 545 <= 2e28, 545 == 2e28)
  print(2147483647 < 70e23, 2147483647 <= 70e23, 2147483647 == 70e23)
  print("_A" < "aa0", "_A" <= "aa0", "_A" == "aa0")
  print("aA0" < "", "aA0" <= "", "aA0" == "")
  print("0BB_" < "b_Bb", "0BB_" <= "b_Bb", "0BB_" == "b_Bb")
end
