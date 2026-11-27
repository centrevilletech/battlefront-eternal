-- Generated differential program 232 (generate.py, seed 503).
-- compare
do
  print(0.9623820700 < 250.56988, 0.9623820700 <= 250.56988, 0.9623820700 == 250.56988)
  print(8 < 2^18, 8 <= 2^18, 8 == 2^18)
  print(1e38 < 97.91054, 1e38 <= 97.91054, 1e38 == 97.91054)
  print(16777216 < 2/3, 16777216 <= 2/3, 16777216 == 2/3)
  print(96e-7 < 0.2, 96e-7 <= 0.2, 96e-7 == 0.2)
  print(1e-45 < 63e21, 1e-45 <= 63e21, 1e-45 == 63e21)
  print("aAa" < "", "aAa" <= "", "aAa" == "")
  print("cA" < "AB0", "cA" <= "AB0", "cA" == "AB0")
  print("00" < "0b", "00" <= "0b", "00" == "0b")
end
-- arith
do
  do local v = ((((2^10) + (2/3)) + ((74.86842) * (2^-24))) / ((888) + ((37e-34) * (58e37))))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (((0.542) * ((227.37912) / (0.0616))) - (2))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((21e13) * (0.35)) + ((-25.675) / (8))) / (((0.975) + (-5.231)) * ((-24.305) * (0.87102620123))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (4)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (((16777215) * ((0.803) + (-373))) + (0.599))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = (-21.146)
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = (16777216)
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(429))
  print('ceil', math.ceil(4294967295))
  print('abs', math.abs(0.5934562))
  print('sqrt', math.sqrt(2))
  do local m = math.mod(2^3, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(47e-18, 0.2, 2^-28), math.max(9, -17.219))
  print('frexp', math.frexp(8))
  print('ldexp', math.ldexp(1, -1))
  print('pow', 0.5 ^ 0.5)
end
-- globals
do
  gx = 2
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
  print(pcall(string.format, "[%10o]", -98765))
  print(pcall(string.format, "[%E]", -15.92))
  print(pcall(string.format, "[%-10f]", 73e-6))
  print(pcall(string.format, "[%f]", 2147483647))
  print(pcall(string.format, "[%+017G]", 48e10))
  print(pcall(string.format, "[%q]", "back\\slash"))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
