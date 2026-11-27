-- Generated differential program 102 (generate.py, seed 503).
-- arith
do
  do local v = ((1) + (((9) / (9)) - ((4294967295) / (33554433))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (((-313) + ((2^1) + (33554433))) * (((1.17549435e-38) + (98e-17)) / ((0.925) * (1/3))))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (6)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (76e-37)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (4)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((((3.4e38) + (2^9)) - (0.2)) + (((0.3) / (-641)) + ((1.17549435e-38) + (0.82))))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = (694)
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(0.1))
  print('ceil', math.ceil(-416))
  print('abs', math.abs(9))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(0.178904, -3) if m == m then print('mod', m) end end
  print('minmax', math.min(975, -49.480, 2^-2), math.max(93e36, 3.4e38))
  print('frexp', math.frexp(0.3))
  print('ldexp', math.ldexp(1, 6))
  print('pow', 2 ^ 10)
end
-- strings
do
  local s = "baf a"
  print(string.sub(s, -4, -7), string.sub(s, -4), string.byte(s, -7))
  print(string.sub(s, -7, 4), string.sub(s, -7), string.byte(s, 4))
  print(string.sub(s, 1, 1), string.sub(s, 1), string.byte(s, 1))
  print(string.sub(s, 7, 10), string.sub(s, 7), string.byte(s, 10))
  print(string.sub(s, -7, -3), string.sub(s, -7), string.byte(s, -3))
  print(string.sub(s, -15, -6), string.sub(s, -15), string.byte(s, -6))
  print(string.sub(s, -10, -7), string.sub(s, -10), string.byte(s, -7))
  print(string.len(s), string.upper(s), string.lower(s))
  print(string.rep("ab", 4))
  print(string.char(43, 46, 43, 113))
  print(pcall(string.char, 300))
  print(pcall(string.rep))
end
-- globals
do
  gx = 24
  print(gx, _G.gx, rawget(_G, 'gx'), getfenv(1) == _G)
  setglobal_test = nil print(setglobal_test, _G['setglobal_test'])
  local env = setmetatable({}, {__index = _G})
  local f = function() gy = 5 return gx end
  setfenv(f, env) print(f(), rawget(env, 'gy'), gy)
  print(type(print), type(nil), type({}), type('s'), type(2), type(true), type(f))
  print(tostring(nil), tostring(false), tostring(12), tostring('q'))
  print(next({}), rawequal('a', 'a'), rawequal({}, {}))
end
