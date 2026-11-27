-- Generated differential program 139 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 6 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(7), counter(18)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- arith
do
  do local v = (-39.503)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (5)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (1.17549435e-38)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((820.18994) + (3)) + ((2147483647) / (6))) * ((0.032804) * ((363.40457) * (47e25))))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((2/3) * (((0.86260990) / (67e-26)) + (-43.80)))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((4294967295) + (((-937) * (591.56800)) * ((-344) + (0.669))))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  do local v = (4294967295)
    if v == v then print(6, v, v * 2, -v) else print(6, 'not a number') end end
  print('floor', math.floor(928))
  print('ceil', math.ceil(-782))
  print('abs', math.abs(-10.148))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(1.17549435e-38, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(966.40132, 93e10, 92e-32), math.max(1.17549435e-38, 29e19))
  print('frexp', math.frexp(-5))
  print('ldexp', math.ldexp(-3, -3))
  print('pow', 10 ^ 3)
end
-- coerce
do
  print(0.1 .. "")
  print(2^24 + 1)
  print(".5" .. 1)
  print("5" + "0.25")
  print(-1/0 .. "")
  print(-"2")
  print(1e100 .. "")
  print(16777217 .. "")
  print("3x", tonumber("3x"))
  print("1e400", tonumber("1e400"))
  print("0x", tonumber("0x"))
  print("  0x10  ", tonumber("  0x10  "))
  print(".", tonumber("."))
  print("12", tonumber("12"))
  print(" 12 ", tonumber(" 12 "))
  print(28, tonumber("chm", 28))
  print(6, tonumber("143", 6))
  print(15, tonumber("b474", 15))
  print(10, tonumber("0 1112", 10))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
