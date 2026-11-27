-- Generated differential program 002 (generate.py, seed 503).
-- coerce
do
  print(-1/0 .. "")
  print(".5" .. 1)
  print(1e15 .. "")
  print("10" + 5)
  print("5" + "0.25")
  print("1e3" / 4)
  print("0x10" + 0)
  print(10 .. 20)
  print("+3", tonumber("+3"))
  print("0x", tonumber("0x"))
  print("1e", tonumber("1e"))
  print("-7.5", tonumber("-7.5"))
  print("3x", tonumber("3x"))
  print("0x1F", tonumber("0x1F"))
  print("1 2", tonumber("1 2"))
  print(19, tonumber("g14", 19))
  print(3, tonumber("20 1", 3))
  print(3, tonumber("222", 3))
  print(11, tonumber("492837", 11))
  print(tonumber('-ff', 16), tonumber('7fffffff', 16), tonumber('zz', 36))
end
-- closures
do
  local fs = {}
  for i = 1, 5 do fs[i] = function() return i * 2 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(1), counter(15)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- trig
do
  print('rad', math.rad(0.5))
  print('tan', math.tan(1))
  print('log10', math.log10(0.001))
  print('sin', math.sin(0.25))
  print('atan2', math.atan2(0.5, 2))
  print('pi', math.pi, math.pi * 2)
end
-- arith
do
  do local v = (-41.901)
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (-34.61)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((313) + (0.553400524)) + ((2/3) - ((0.1) + (16777216))))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = (((0.770) * (0.58337)) + (677.98497))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = (16777216)
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  do local v = ((((-25.46) + (2^-1)) * ((6) * (2^-6))) - (((0.033504825) * (2^3)) * ((-49.505) + (135.69527))))
    if v == v then print(5, v, v * 2, -v) else print(5, 'not a number') end end
  print('floor', math.floor(-995))
  print('ceil', math.ceil(1.17549435e-38))
  print('abs', math.abs(2^-8))
  print('sqrt', math.sqrt(2))
  do local m = math.mod(-47.993, 2.5) if m == m then print('mod', m) end end
  print('minmax', math.min(849, 741.51212, -144), math.max(4294967295, 89e18))
  print('frexp', math.frexp(-5))
  print('ldexp', math.ldexp(-3, 3))
  print('pow', 1.5 ^ -2)
end
