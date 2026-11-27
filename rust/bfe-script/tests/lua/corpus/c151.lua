-- Generated differential program 151 (generate.py, seed 503).
-- format
do
  print(pcall(string.format, "[%q]", "nul\0byte"))
  print(pcall(string.format, "[%-E]", 69e-34))
  print(pcall(string.format, "[%+ .8G]", 6))
  print(pcall(string.format, "[% 08.14E]", 763))
  print(pcall(string.format, "[%0#10f]", 2^-20))
  print(pcall(string.format, "[%i]", 65535))
  print(string.format('%5.1f|%-8.3e|%g', 3.14159, 1234.5, 0.0001))
  print(string.format('%s %s %d', 1, 2.5, 3))
end
-- compare
do
  print(344.61997 < 0.7, 344.61997 <= 0.7, 344.61997 == 0.7)
  print(-13.198 < 209.60428, -13.198 <= 209.60428, -13.198 == 209.60428)
  print(992.88822 < 2^16, 992.88822 <= 2^16, 992.88822 == 2^16)
  print(-27.202 < -17.833, -27.202 <= -17.833, -27.202 == -17.833)
  print(525.59500 < 479.68260, 525.59500 <= 479.68260, 525.59500 == 479.68260)
  print("AbcB" < "acA", "AbcB" <= "acA", "AbcB" == "acA")
  print("" < "099_", "" <= "099_", "" == "099_")
  print("AA9" < "ba", "AA9" <= "ba", "AA9" == "ba")
end
-- errors
do
  print(pcall(error, 'plain'))
  print(pcall(error, 42))
  print(pcall(error))
  local function lvl1() error('one', 1) end local function lvl2() error('two', 2) end
  print(pcall(lvl1)) print(pcall(lvl2))
  print(pcall(function() local t = nil return t.x end))
  print(pcall(function() return undefined_global_fn() end))
  print(pcall(function() local t = {} return t.a.b end))
  print(pcall(function() return 1 < 'x' end))
  print(pcall(function() return {} .. 'x' end))
  print(pcall(function() return -{} end))
  print(pcall(function() local s = 'x' return s + 1 end))
  print(pcall(function() local t = setmetatable({}, {__index = function(t, k) error('deep ' .. k) end}) return t.key end))
  print(xpcall(function() error('boom') end, function(m) return 'handled: ' .. m end))
  print(pcall(error, setmetatable({}, {__tostring = function() return 'obj' end})) == false)
  print(pcall(assert, false, 'assert msg'))
  print(pcall(assert, nil))
  print(pcall(assert, 1, 2))
end
-- arith
do
  do local v = ((995.11091) - ((1e38) + (2^-23)))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (-20.538)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = ((((0.333790229) - (-721)) + ((2^-5) / (4294967295))) / (-44.991))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (31e-1)
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (5)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((((69e5) / (255.48388)) / ((319.73775) + (-41.921))) * (4294967295))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = (2/3)
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(7))
  print('ceil', math.ceil(0.2))
  print('abs', math.abs(-21.441))
  print('sqrt', math.sqrt(0.5))
  do local m = math.mod(2147483647, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(0.610, -24.937, -17.962), math.max(0.30678990782, 1.17549435e-38))
  print('frexp', math.frexp(1e-40))
  print('ldexp', math.ldexp(-3, -13))
  print('pow', 10 ^ -2)
end
