-- Generated differential program 007 (generate.py, seed 503).
-- closures
do
  local fs = {}
  for i = 1, 4 do fs[i] = function() return i * 4 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(0), counter(12)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
-- arith
do
  do local v = ((((181) * (-161)) * (3)) / (((4) + (0.13)) + (653.42103)))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = ((((-18.871) + (8)) * ((-519) + (25e30))) * (93e16))
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (((1e38) / ((5) * (0.1))) - (-664))
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((16777216) / (2^23))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((((0.04176899484) * (0.888174)) / ((0.6882105220) - (-27.222))) - (91e1))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(80e-1))
  print('ceil', math.ceil(7))
  print('abs', math.abs(34e37))
  print('sqrt', math.sqrt(16777217))
  do local m = math.mod(-16.325, 7) if m == m then print('mod', m) end end
  print('minmax', math.min(4, -25.137, 30e25), math.max(714.48603, 0.547))
  print('frexp', math.frexp(1e-40))
  print('ldexp', math.ldexp(0.75, -16))
  print('pow', 0.5 ^ -2)
end
-- trig
do
  print('deg', math.deg(2))
  print('sin', math.sin(0.5))
  print('log', math.log(0.5))
  print('rad', math.rad(0.001))
  print('atan2', math.atan2(0.5, -3))
  print('pi', math.pi, math.pi * 2)
end
-- compare
do
  print(2^-29 < 2^13, 2^-29 <= 2^13, 2^-29 == 2^13)
  print(2^-22 < 695.2582, 2^-22 <= 695.2582, 2^-22 == 695.2582)
  print(630.59312 < 721.73658, 630.59312 <= 721.73658, 630.59312 == 721.73658)
  print(2^-14 < 35e-22, 2^-14 <= 35e-22, 2^-14 == 35e-22)
  print(1 < 0.76958999, 1 <= 0.76958999, 1 == 0.76958999)
  print(59e-18 < 8388609, 59e-18 <= 8388609, 59e-18 == 8388609)
  print("_00" < "a", "_00" <= "a", "_00" == "a")
  print("" < "_bBB", "" <= "_bBB", "" == "_bBB")
  print("AAb_" < "AAB", "AAb_" <= "AAB", "AAb_" == "AAB")
end
