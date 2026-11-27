-- Generated differential program 195 (generate.py, seed 503).
-- arith
do
  do local v = ((-265) + (((16777217) * (4)) + (-20.152)))
    if v == v then print(0, v, v * 2, -v) else print(0, 'not a number') end end
  do local v = (-789)
    if v == v then print(1, v, v * 2, -v) else print(1, 'not a number') end end
  do local v = (16777217)
    if v == v then print(2, v, v * 2, -v) else print(2, 'not a number') end end
  do local v = ((((548.26163) * (2/3)) + ((2^-3) * (63e-21))) / ((8) - (0.8990246)))
    if v == v then print(3, v, v * 2, -v) else print(3, 'not a number') end end
  do local v = ((42e5) * (((61e2) / (-28.466)) - ((71e-11) * (471))))
    if v == v then print(4, v, v * 2, -v) else print(4, 'not a number') end end
  print('floor', math.floor(748))
  print('ceil', math.ceil(2^1))
  print('abs', math.abs(2^-28))
  print('sqrt', math.sqrt(2))
  do local m = math.mod(90e-11, 0.75) if m == m then print('mod', m) end end
  print('minmax', math.min(-16.610, 516.89790, 11e9), math.max(0.25779553867, 1.17549435e-38))
  print('frexp', math.frexp(1e-40))
  print('ldexp', math.ldexp(0.75, -13))
  print('pow', 1.5 ^ 3)
end
-- compare
do
  print(-41.243 < 1.17549435e-38, -41.243 <= 1.17549435e-38, -41.243 == 1.17549435e-38)
  print(-451 < 33554433, -451 <= 33554433, -451 == 33554433)
  print(501.78216 < 1e-45, 501.78216 <= 1e-45, 501.78216 == 1e-45)
  print(155.81296 < 5, 155.81296 <= 5, 155.81296 == 5)
  print(883.31122 < 53.37332, 883.31122 <= 53.37332, 883.31122 == 53.37332)
  print(33554433 < 2^20, 33554433 <= 2^20, 33554433 == 2^20)
  print(16 < -33.212, 16 <= -33.212, 16 == -33.212)
  print("0ba" < "B9B0", "0ba" <= "B9B0", "0ba" == "B9B0")
  print("09" < "9", "09" <= "9", "09" == "9")
  print("c_" < "0a_", "c_" <= "0a_", "c_" == "0a_")
end
-- closures
do
  local fs = {}
  for i = 1, 3 do fs[i] = function() return i * 8 end end
  for i = 1, table.getn(fs) do print('closure', i, fs[i]()) end
  local function counter(start) local c = start return function(d) c = c + (d or 1) return c end end
  local c1, c2 = counter(1), counter(19)
  print(c1(), c1(2), c2(), c1(), c2(-5))
  local acc = {} local k = 0 while k < 3 do k = k + 1 local j = k * 10 acc[k] = function() return j + k end end
  print(acc[1](), acc[2](), acc[3]())
  local function shared() local x = 0 local function inc() x = x + 1 end local function get() return x end return inc, get end
  local inc, get = shared() inc() inc() print('shared', get())
end
